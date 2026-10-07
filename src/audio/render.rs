use std::{ops::Range, sync::Arc};

use cpal::{FromSample, SizedSample};

use crate::{
    controls::{ControlState, Normalized},
    situations::{
        Registry, SituationIndex, Voice, VoiceFrame, VoiceLatencyFrames, VoicePreparation,
        VoicePreparationError,
    },
};

use super::{
    lease::EngineLease,
    timing::{MachNanos, WorldSeconds, presentation_time},
    transport::{ControlMailbox, ErrorCell, VisualRing, VisualWriter, WorldVisualState},
    world::WorldClock,
};

pub(crate) fn output_f32(
    lease: &EngineLease,
    data: &mut [f32],
    channels: usize,
    info: &cpal::OutputCallbackInfo,
    errors: &ErrorCell,
) {
    data.fill(0.0);
    let Some(mut guard) = lease.try_borrow() else {
        return;
    };
    let Some(playback) = callback_playback(info, errors) else {
        return;
    };
    guard.engine().render_f32(data, channels, playback, errors);
}

pub(crate) fn output_i16(
    lease: &EngineLease,
    data: &mut [i16],
    channels: usize,
    info: &cpal::OutputCallbackInfo,
    errors: &ErrorCell,
) {
    data.fill(0);
    let Some(mut guard) = lease.try_borrow() else {
        return;
    };
    let Some(playback) = callback_playback(info, errors) else {
        return;
    };
    guard.engine().render_i16(data, channels, playback, errors);
}

fn callback_playback(info: &cpal::OutputCallbackInfo, errors: &ErrorCell) -> Option<MachNanos> {
    match u64::try_from(info.timestamp().playback.as_nanos()) {
        Ok(nanos) => Some(MachNanos::new(nanos)),
        Err(_) => {
            errors.report_timing_fault();
            None
        }
    }
}

struct PreparedVoice {
    index: SituationIndex,
    knobs: Range<usize>,
    delay: VoiceLatencyFrames,
    voice: Box<dyn Voice>,
}

/// All allocations and node preparation occur on the output worker.
/// The callback receives exclusive access only through EngineLease.
pub(crate) struct PreparedEngine {
    voices: Box<[PreparedVoice]>,
    controls: ControlState,
    scratch: ControlState,
    mailbox: Arc<ControlMailbox>,
    visuals: Arc<VisualRing>,
    world: WorldClock,
    gains: Box<[f32]>,
    breath: Box<[f32]>,
    state_interval: u32,
    state_delay: VoiceLatencyFrames,
    until_state: u32,
    #[cfg(test)]
    drop_observer: Option<(std::thread::ThreadId, Arc<std::sync::atomic::AtomicU8>)>,
}

impl PreparedEngine {
    pub(crate) fn prepare(
        registry: Arc<Registry>,
        controls: ControlState,
        visuals: Arc<VisualRing>,
        sample_rate: u32,
        seed: u64,
    ) -> Result<Self, VoicePreparationError> {
        let count = registry.definitions().len();
        if controls.registry_count() != count || controls.knobs.len() != registry.knob_count() {
            return Err(VoicePreparationError::InvalidParameters);
        }
        let mut voices = Vec::with_capacity(count);
        let mut gains = vec![0.0; count];
        for (position, definition) in registry.definitions().iter().enumerate() {
            let index = SituationIndex::from_index(position, count)
                .ok_or(VoicePreparationError::InvalidDefinition)?;
            let mut voice = registry.prepare_voice(
                index,
                VoicePreparation {
                    sample_rate,
                    seed: seed.wrapping_add(position as u64),
                    grain_capacity: definition.grain_capacity,
                },
            )?;
            let delay = voice.prepared_delay()?;
            let contributing = index == controls.manual_target;
            voice.set_contributing(contributing);
            gains[position] = if contributing {
                Normalized::ONE
            } else {
                Normalized::ZERO
            }
            .get();
            voices.push(PreparedVoice {
                index,
                knobs: registry.knob_range(index),
                delay,
                voice,
            });
        }
        let scratch = controls.clone();
        let mailbox = Arc::new(ControlMailbox::new(&controls));
        Ok(Self {
            voices: voices.into_boxed_slice(),
            controls,
            scratch,
            mailbox,
            visuals,
            world: WorldClock::new(sample_rate)?,
            gains: gains.into_boxed_slice(),
            breath: vec![0.0; count].into_boxed_slice(),
            state_delay: VoiceLatencyFrames::new(0.0)
                .expect("aggregate state tracks generated samples without a voice dry path"),
            state_interval: sample_rate.div_ceil(30),
            until_state: 0,
            #[cfg(test)]
            drop_observer: None,
        })
    }

    pub(crate) fn control_mailbox(&self) -> &Arc<ControlMailbox> {
        &self.mailbox
    }

    pub(crate) fn world(&self) -> WorldSeconds {
        self.world.now()
    }

    /// The outer callback clears the entire buffer BEFORE attempting its lease.
    /// These methods only overwrite complete frames' first one/two channels.
    pub(crate) fn render_f32(
        &mut self,
        data: &mut [f32],
        channels: usize,
        playback: MachNanos,
        errors: &ErrorCell,
    ) {
        self.render_samples(data, channels, playback, errors);
    }

    pub(crate) fn render_i16(
        &mut self,
        data: &mut [i16],
        channels: usize,
        playback: MachNanos,
        errors: &ErrorCell,
    ) {
        self.render_samples(data, channels, playback, errors);
    }

    fn render_samples<T: SizedSample + FromSample<f32>>(
        &mut self,
        data: &mut [T],
        channels: usize,
        playback: MachNanos,
        errors: &ErrorCell,
    ) {
        if channels == 0 {
            errors.report_invalid_sample();
            return;
        }
        // An incomplete read may modify scratch, never committed controls.
        if self.mailbox.try_read_into(&mut self.scratch) {
            std::mem::swap(&mut self.controls, &mut self.scratch);
        }
        let rate = self.world.sample_rate();
        let delta_seconds = 1.0 / f64::from(rate);
        let gain = if self.controls.mute {
            0.0
        } else {
            self.controls.volume.get()
        };
        for (frame_offset, output) in data.chunks_exact_mut(channels).enumerate() {
            let frame = VoiceFrame {
                world: self.world.now(),
                delta_seconds,
                sample_rate: rate,
            };
            let mut stereo = [0.0_f32; 2];
            for (position, prepared) in self.voices.iter_mut().enumerate() {
                let Ok(present) = presentation_time(playback, frame_offset, prepared.delay, rate)
                else {
                    errors.report_timing_fault();
                    return;
                };
                let mut writer = VisualWriter::new(
                    &self.visuals,
                    errors.epoch(),
                    prepared.index,
                    frame.world,
                    present,
                );
                let contributing = self.gains[position] > 0.0;
                writer.set_contributing(contributing);
                prepared.voice.advance_world(
                    frame,
                    &self.controls.knobs[prepared.knobs.clone()],
                    &mut writer,
                );
                if writer.has_failed() {
                    errors.report_timing_fault();
                    return;
                }
                if contributing {
                    let sample = prepared.voice.render_sample();
                    stereo[0] += sample[0] * self.gains[position];
                    stereo[1] += sample[1] * self.gains[position];
                }
            }
            stereo[0] *= gain;
            stereo[1] *= gain;
            if stereo
                .iter()
                .any(|sample| !sample.is_finite() || sample.abs() > 1.0)
            {
                errors.report_invalid_sample();
                return;
            }
            if channels == 1 {
                output[0] = T::from_sample((stereo[0] + stereo[1]) * 0.5);
            } else {
                output[0] = T::from_sample(stereo[0]);
                output[1] = T::from_sample(stereo[1]);
            }
            if self.until_state == 0 {
                for (position, prepared) in self.voices.iter().enumerate() {
                    self.breath[position] = prepared.voice.breath_intensity().get();
                }
                let Ok(present) = presentation_time(playback, frame_offset, self.state_delay, rate)
                else {
                    errors.report_timing_fault();
                    return;
                };
                if !self.visuals.push_state(WorldVisualState {
                    epoch: errors.epoch(),
                    world: frame.world,
                    present,
                    phase: 0,
                    gains: &self.gains,
                    breath_intensity: &self.breath,
                }) {
                    errors.report_timing_fault();
                    return;
                }
                self.until_state = self.state_interval;
            }
            self.until_state -= 1;
            self.world.advance_sample();
        }
    }

    #[cfg(test)]
    pub(crate) fn observe_drop(
        &mut self,
        expected: std::thread::ThreadId,
        result: Arc<std::sync::atomic::AtomicU8>,
    ) {
        self.drop_observer = Some((expected, result));
    }
}

#[cfg(test)]
impl Drop for PreparedEngine {
    fn drop(&mut self) {
        if let Some((expected, result)) = &self.drop_observer {
            result.store(
                if std::thread::current().id() == *expected {
                    1
                } else {
                    2
                },
                std::sync::atomic::Ordering::SeqCst,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::audio::timing::StreamEpoch;
    use cpal::Sample;
    use std::sync::Arc;

    use super::*;
    use crate::audio::transport::EVENT_CAPACITY;

    fn prepared(seed: u64) -> PreparedEngine {
        let registry = Arc::new(Registry::new().unwrap());
        let controls = ControlState::new(&registry);
        let visuals = Arc::new(VisualRing::new(
            EVENT_CAPACITY,
            registry.definitions().len(),
        ));
        PreparedEngine::prepare(registry, controls, visuals, 48_000, seed).unwrap()
    }

    #[test]
    fn variable_frames_channel_mapping_and_remainders_follow_generated_samples() {
        let epoch = StreamEpoch::new(1).unwrap();
        for frames in [1, 127, 513] {
            let stereo = EngineLease::new(prepared(84), epoch);
            let mono = EngineLease::new(prepared(84), epoch);
            let surround = EngineLease::new(prepared(84), epoch);
            let errors = ErrorCell::new(epoch);
            let mut reference = vec![1.0; frames * 2];
            let mut average = vec![1.0; frames];
            let mut six = vec![1.0; frames * 6 + 3];
            let info = callback_info(50_000);
            let (_, heap) = crate::rt_alloc::measure(|| {
                output_f32(&stereo, &mut reference, 2, &info, &errors);
                output_f32(&mono, &mut average, 1, &info, &errors);
                output_f32(&surround, &mut six, 6, &info, &errors);
            });
            assert_eq!(heap, crate::rt_alloc::Counts::default());
            for index in 0..frames {
                assert_eq!(
                    average[index],
                    (reference[index * 2] + reference[index * 2 + 1]) * 0.5
                );
                assert_eq!(six[index * 6], reference[index * 2]);
                assert_eq!(six[index * 6 + 1], reference[index * 2 + 1]);
                assert_eq!(&six[index * 6 + 2..index * 6 + 6], &[0.0; 4]);
            }
            assert_eq!(&six[frames * 6..], &[0.0; 3]);
            let stereo = stereo.try_take().unwrap();
            assert_eq!((stereo.world().get() * 48_000.0).round() as usize, frames);
            assert_eq!(mono.try_take().unwrap().world(), stereo.world());
            assert_eq!(surround.try_take().unwrap().world(), stereo.world());
            assert_eq!(errors.take_bits(), 0);
        }
    }

    #[test]
    fn muted_output_keeps_the_same_world_and_unmuted_voice_history() {
        let epoch = StreamEpoch::new(1).unwrap();
        let errors = ErrorCell::new(epoch);
        let mut audible = prepared(91);
        let mut muted = prepared(91);
        let mut controls = muted.controls.clone();
        controls.mute = true;
        muted.mailbox.publish(&controls);
        let mut a = [0.0; 1026];
        let mut b = [0.0; 1026];
        audible.render_f32(&mut a, 2, MachNanos::new(0), &errors);
        muted.render_f32(&mut b, 2, MachNanos::new(0), &errors);
        assert_eq!(b, [0.0; 1026]);
        assert_eq!(audible.world(), muted.world());
        controls.mute = false;
        muted.mailbox.publish(&controls);
        audible.render_f32(&mut a, 2, MachNanos::new(11_000_000), &errors);
        muted.render_f32(&mut b, 2, MachNanos::new(11_000_000), &errors);
        assert_eq!(a, b);
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn i16_conversion_keeps_channel_semantics_and_world_time() {
        let epoch = StreamEpoch::new(2).unwrap();
        let errors = ErrorCell::new(epoch);
        let mut floating = prepared(71);
        let mut integer = prepared(71);
        let mut a = [0.0; 1026];
        let mut b = [0_i16; 1026];
        floating.render_f32(&mut a, 2, MachNanos::new(0), &errors);
        integer.render_i16(&mut b, 2, MachNanos::new(0), &errors);
        for (float, integer) in a.into_iter().zip(b) {
            assert_eq!(integer, i16::from_sample(float));
        }
        assert_eq!(floating.world(), integer.world());
        assert_eq!(errors.take_bits(), 0);
    }

    fn callback_info(nanos: u64) -> cpal::OutputCallbackInfo {
        let playback = cpal::StreamInstant::from_nanos(nanos);
        cpal::OutputCallbackInfo::new(cpal::OutputStreamTimestamp {
            callback: playback,
            playback,
        })
    }

    #[test]
    fn actual_typed_callbacks_silence_busy_closed_and_worker_owned_epochs() {
        let epoch = StreamEpoch::new(8).unwrap();
        let lease = EngineLease::new(prepared(51), epoch);
        let errors = ErrorCell::new(epoch);
        let info = callback_info(50_000);
        let mut float = [1.0; 1029];
        let mut integer = [123_i16; 1029];
        let mut held = lease.try_borrow().unwrap();
        let before = held.engine().world();
        output_f32(&lease, &mut float, 6, &info, &errors);
        output_i16(&lease, &mut integer, 6, &info, &errors);
        assert_eq!(float, [0.0; 1029]);
        assert_eq!(integer, [0; 1029]);
        assert_eq!(held.engine().world(), before);
        drop(held);
        lease.close();
        float.fill(1.0);
        output_f32(&lease, &mut float, 6, &info, &errors);
        assert_eq!(float, [0.0; 1029]);
        let engine = lease.try_take().unwrap();
        assert_eq!(engine.world(), before);
        integer.fill(123);
        output_i16(&lease, &mut integer, 6, &info, &errors);
        assert_eq!(integer, [0; 1029]);
        assert_eq!(engine.world(), before);
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn callback_timestamp_overflow_is_silent_and_does_not_advance_world() {
        let epoch = StreamEpoch::new(9).unwrap();
        let lease = EngineLease::new(prepared(61), epoch);
        let errors = ErrorCell::new(epoch);
        let playback = cpal::StreamInstant::new(u64::MAX, 0);
        let info = cpal::OutputCallbackInfo::new(cpal::OutputStreamTimestamp {
            callback: playback,
            playback,
        });
        let mut output = [1.0; 254];
        output_f32(&lease, &mut output, 2, &info, &errors);
        assert_eq!(output, [0.0; 254]);
        assert_eq!(lease.try_take().unwrap().world().get(), 0.0);
        assert_eq!(
            errors.take_bits(),
            crate::audio::transport::ErrorCode::TIMING_FAULT
        );
    }
}
