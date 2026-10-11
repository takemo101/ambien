use std::{ops::Range, sync::Arc, time::Duration};

use cpal::{FromSample, SizedSample};

use crate::{
    controls::ControlState,
    lifecycle::AttemptId,
    situations::{
        Registry, SituationIndex, Voice, VoiceFrame, VoiceLatencyFrames, VoicePreparation,
        VoicePreparationError, synthesis::WorldRandom,
    },
};

#[cfg(test)]
use crate::controls::Normalized;

use super::{
    lease::EngineLease,
    mix::GainRamp,
    timing::{MachNanos, StreamEpoch, WorldSeconds, presentation_time},
    transport::{
        ControlMailbox, ControlReceipt, ErrorCell, StatusMailbox, VisualRing, VisualWriter,
        WorldVisualState,
    },
    world::{DrawBudget, Movement, WanderTimer, WorldClock},
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

// Implementation constants, not product timing controls. Volume, mute and
// recovery use independent finite smoothstep envelopes.
const OUTPUT_RAMP_MILLISECONDS: u32 = 20;
// The shutdown fade after a successful final save (detailed design: first implementation).
const SHUTDOWN_RAMP_MILLISECONDS: u32 = 100;

const MANUAL_CROSSFADE: Duration = Duration::from_secs(2);

struct OutputRamp {
    current: f32,
    start: f32,
    target: f32,
    elapsed_seconds: f64,
    duration_seconds: f64,
    step_seconds: f64,
    inverse_duration: f64,
    endpoint_seconds: f64,
}

impl OutputRamp {
    fn new(value: f32, rate: u32) -> Self {
        Self::with_milliseconds(value, rate, OUTPUT_RAMP_MILLISECONDS)
    }

    fn with_milliseconds(value: f32, rate: u32, milliseconds: u32) -> Self {
        debug_assert!(rate > 0);
        let frames = (u64::from(rate) * u64::from(milliseconds)).div_ceil(1_000) as u32;
        let step_seconds = 1.0 / f64::from(rate);
        let duration_seconds = f64::from(frames) * step_seconds;
        Self {
            current: value,
            start: value,
            target: value,
            elapsed_seconds: duration_seconds,
            duration_seconds,
            step_seconds,
            inverse_duration: 1.0 / duration_seconds,
            endpoint_seconds: duration_seconds * (1.0 - 1e-10),
        }
    }

    fn retarget(&mut self, target: f32) {
        if self.target == target {
            return;
        }
        self.start = self.current;
        self.target = target;
        self.elapsed_seconds = if self.current == target {
            self.duration_seconds
        } else {
            0.0
        };
    }

    fn prepare_rate(&mut self, rate: u32) {
        debug_assert!(rate > 0);
        self.step_seconds = 1.0 / f64::from(rate);
    }

    fn advance(&mut self) -> f32 {
        if self.elapsed_seconds < self.duration_seconds {
            self.elapsed_seconds += self.step_seconds;
            // Bound summation roundoff, not a fraction of a sample. The exact
            // endpoint stays reachable at integer-rate envelope boundaries.
            if self.elapsed_seconds >= self.endpoint_seconds {
                self.elapsed_seconds = self.duration_seconds;
                self.current = self.target;
            } else {
                let fraction = (self.elapsed_seconds * self.inverse_duration) as f32;
                let eased = fraction * fraction * (3.0 - 2.0 * fraction);
                self.current = self.start + (self.target - self.start) * eased;
            }
        }
        self.current
    }

    /// Reached `value` for good: no more movement and exactly at the endpoint.
    fn settled_at(&self, value: f32) -> bool {
        self.elapsed_seconds >= self.duration_seconds && self.current == value
    }
}

struct PreparedVoice {
    index: SituationIndex,
    knobs: Range<usize>,
    delay: VoiceLatencyFrames,
    contributing: bool,
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
    receipts: Arc<StatusMailbox>,
    receipt_epoch: Option<StreamEpoch>,
    volume_gain: OutputRamp,
    mute_gain: OutputRamp,
    recovery_gain: OutputRamp,
    shutdown_gain: OutputRamp,
    shutdown_attempt: Option<AttemptId>,
    shutdown_published: bool,
    rate_prepared: bool,
    world: WorldClock,
    movement: Movement,
    wander: WanderTimer,
    wander_random: WorldRandom,
    mix: GainRamp,
    visual_gains: Box<[f32]>,
    breath: Box<[f32]>,
    state_interval: u32,
    state_delay: VoiceLatencyFrames,
    until_state: u32,
    published_phase: Option<u32>,
    #[cfg(test)]
    drop_observer: Option<(std::thread::ThreadId, Arc<std::sync::atomic::AtomicU8>)>,
}

impl PreparedEngine {
    pub(crate) fn prepare(
        registry: Arc<Registry>,
        controls: ControlState,
        visuals: Arc<VisualRing>,
        mailbox: Arc<ControlMailbox>,
        receipts: Arc<StatusMailbox>,
        sample_rate: u32,
        seed: u64,
    ) -> Result<Self, VoicePreparationError> {
        let count = registry.definitions().len();
        if controls.registry_count() != count || controls.knobs.len() != registry.knob_count() {
            return Err(VoicePreparationError::InvalidParameters);
        }
        let mut voices = Vec::with_capacity(count);
        let mix = GainRamp::new(count, controls.manual_target);
        let movement = Movement::Staying {
            current: controls.manual_target,
        };
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
            voices.push(PreparedVoice {
                index,
                knobs: registry.knob_range(index),
                delay,
                contributing,
                voice,
            });
        }
        let scratch = controls.clone();
        let volume_gain = OutputRamp::new(controls.volume.get(), sample_rate);
        let mute_gain = OutputRamp::new(if controls.mute { 0.0 } else { 1.0 }, sample_rate);
        let wander = WanderTimer::new(controls.wander, WorldSeconds::new(0.0).unwrap());
        Ok(Self {
            voices: voices.into_boxed_slice(),
            controls,
            scratch,
            mailbox,
            visuals,
            receipts,
            receipt_epoch: None,
            volume_gain,
            mute_gain,
            world: WorldClock::new(sample_rate)?,
            recovery_gain: OutputRamp::new(1.0, sample_rate),
            shutdown_gain: OutputRamp::with_milliseconds(
                1.0,
                sample_rate,
                SHUTDOWN_RAMP_MILLISECONDS,
            ),
            shutdown_attempt: None,
            shutdown_published: false,
            rate_prepared: true,
            movement,
            wander,
            wander_random: WorldRandom::new(seed ^ 0x5741_4e44_4552),
            mix,
            visual_gains: vec![0.0; count].into_boxed_slice(),
            breath: vec![0.0; count].into_boxed_slice(),
            state_delay: VoiceLatencyFrames::new(0.0)
                .expect("aggregate state tracks generated samples without a voice dry path"),
            state_interval: sample_rate.div_ceil(30),
            until_state: 0,
            published_phase: None,
            #[cfg(test)]
            drop_observer: None,
        })
    }

    fn accept_controls(&mut self) -> bool {
        // Failed reads touch only the prepared scratch, never committed values.
        if !self.mailbox.try_read_into(&mut self.scratch) {
            return false;
        }
        let manual_changed = self.controls.manual_revision != self.scratch.manual_revision;
        let wander_changed = self.controls.wander_revision != self.scratch.wander_revision;
        let changed =
            self.controls.settings_revision != self.scratch.settings_revision || manual_changed;
        // A fault after acceptance must not leave the previous revision acknowledged.
        if changed {
            self.receipt_epoch = None;
        }
        std::mem::swap(&mut self.controls, &mut self.scratch);
        self.volume_gain.retarget(self.controls.volume.get());
        self.mute_gain
            .retarget(if self.controls.mute { 0.0 } else { 1.0 });
        if manual_changed {
            self.mix
                .retarget(self.controls.manual_target, MANUAL_CROSSFADE);
            self.movement
                .select_manual(self.controls.manual_target, self.mix.is_crossing());
        }
        self.wander.update(
            self.controls.wander,
            wander_changed,
            manual_changed,
            self.movement,
            self.world.now(),
        );
        changed
    }

    /// A new attempt starts one fade to silence. Once begun it is never undone: the
    /// coordinator only requests it after the final save succeeded.
    fn accept_shutdown(&mut self) {
        if let Some(attempt) = self.mailbox.shutdown_request()
            && self.shutdown_attempt != Some(attempt)
        {
            self.shutdown_attempt = Some(attempt);
            self.shutdown_published = false;
            self.shutdown_gain.retarget(0.0);
        }
    }

    fn publish_receipt(&mut self, epoch: StreamEpoch) -> bool {
        // Manual target is immediate; automatic target follows mixer power.
        // Resume uses the automatic departure on ties, not the display history.
        let published = self.receipts.publish(ControlReceipt {
            epoch,
            ack_revision: self.controls.settings_revision,
            ack_manual_revision: self.controls.manual_revision,
            display_dominant: self.movement.display_target(),
            resume_target: self.movement.resume_target(self.mix.gains()),
            world: self.world.now(),
        });
        if published {
            self.receipt_epoch = Some(epoch);
        }
        published
    }

    /// Called only by the worker after taking exclusive ownership of the lease.
    /// Control acknowledgment remains live while generated world time is frozen.
    pub(crate) fn sync_frozen_controls(&mut self, epoch: StreamEpoch) -> bool {
        if self.accept_controls() || self.receipt_epoch != Some(epoch) {
            self.publish_receipt(epoch)
        } else {
            true
        }
    }

    pub(crate) fn world(&self) -> WorldSeconds {
        self.world.now()
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.world.sample_rate()
    }

    /// Worker-exclusive: move this engine between epoch cells, never recreate
    /// its voices, logical clocks, random streams, control or visual handles.
    pub(crate) fn prepare_output_epoch(&mut self, rate: u32) -> Result<(), VoicePreparationError> {
        if rate == 0 {
            return Err(VoicePreparationError::InvalidRate);
        }
        if rate != self.sample_rate() || !self.rate_prepared {
            // Any error can leave a prefix of voices at the attempted rate.
            // Keep the world at its committed rate, and revalidate EVERY voice
            // next time, even when retrying that previous committed rate.
            self.rate_prepared = false;
            for prepared in &mut self.voices {
                let delay = prepared.voice.prepare_rate(rate)?;
                prepared.delay = VoiceLatencyFrames::new(delay.get())
                    .ok_or(VoicePreparationError::UnresolvedCausalDelay)?;
            }
            self.world.prepare_rate(rate)?;
            self.volume_gain.prepare_rate(rate);
            self.mute_gain.prepare_rate(rate);
            self.shutdown_gain.prepare_rate(rate);
            self.rate_prepared = true;
        }
        self.accept_controls();
        // A zero endpoint edited while output was stopped is already silent:
        // do not resurrect the pre-freeze user gain during the recovery fade.
        if self.controls.mute {
            self.mute_gain = OutputRamp::new(0.0, rate);
        }
        if self.controls.volume.get() == 0.0 {
            self.volume_gain = OutputRamp::new(0.0, rate);
        }
        self.receipt_epoch = None;
        self.state_interval = rate.div_ceil(30);
        self.until_state = 0;
        self.published_phase = None;
        self.recovery_gain = OutputRamp::new(0.0, rate);
        self.recovery_gain.retarget(1.0);
        Ok(())
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
        self.accept_controls();
        self.accept_shutdown();
        let rate = self.world.sample_rate();
        let delta_seconds = 1.0 / f64::from(rate);
        self.mix.prepare_interval(rate);
        let mut draw_budget = DrawBudget::new();
        for (frame_offset, output) in data.chunks_exact_mut(channels).enumerate() {
            if let Some(target) = self.wander.poll(
                self.world.now(),
                self.movement,
                self.voices.len(),
                (
                    self.controls.dwell_min_minutes,
                    self.controls.dwell_max_minutes,
                ),
                &mut draw_budget,
                &mut || self.wander_random.next_u64(),
            ) {
                self.mix.retarget(
                    target,
                    Duration::from_secs(u64::from(self.controls.transition_seconds)),
                );
                self.movement.select_automatic(target);
            }
            let was_crossing = self.mix.is_crossing();
            self.mix.advance_frames(1, rate);
            let arrived = was_crossing && !self.mix.is_crossing();
            if arrived {
                self.movement.arrive();
            }
            let gains = self.mix.gains();
            self.movement.observe_gains(gains);
            let frame = VoiceFrame {
                world: self.world.now(),
                delta_seconds,
                sample_rate: rate,
            };
            let mut stereo = [0.0_f64; 2];
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
                let contributing = gains[position] > 0.0;
                if prepared.contributing != contributing {
                    prepared.voice.set_contributing(contributing);
                    prepared.contributing = contributing;
                }
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
                    stereo[0] += f64::from(sample[0]) * gains[position];
                    stereo[1] += f64::from(sample[1]) * gains[position];
                }
            }
            // The first newly generated sample is exactly silent. Recovery is
            // independent of both user envelopes, including frozen edits. The
            // shutdown fade is a fourth, independent envelope.
            let recovery = self.recovery_gain.current;
            self.recovery_gain.advance();
            let gain = f64::from(
                self.volume_gain.advance()
                    * self.mute_gain.advance()
                    * recovery
                    * self.shutdown_gain.advance(),
            );
            stereo[0] *= gain;
            stereo[1] *= gain;
            if stereo
                .iter()
                .any(|sample| !sample.is_finite() || sample.abs() > 1.0)
            {
                errors.report_invalid_sample();
                return;
            }
            let stereo = [stereo[0] as f32, stereo[1] as f32];
            if channels == 1 {
                output[0] = T::from_sample((stereo[0] + stereo[1]) * 0.5);
            } else {
                output[0] = T::from_sample(stereo[0]);
                output[1] = T::from_sample(stereo[1]);
            }
            if let Some(attempt) = self.shutdown_attempt
                && !self.shutdown_published
                && self.shutdown_gain.settled_at(0.0)
            {
                // The final (silent) sample ends one frame after its start; keep the
                // stream until then so no part of the fade is cut off.
                let Ok(deadline) =
                    presentation_time(playback, frame_offset + 1, self.state_delay, rate)
                else {
                    errors.report_timing_fault();
                    return;
                };
                self.mailbox.publish_shutdown_deadline(attempt, deadline);
                self.shutdown_published = true;
            }
            let phase = self.movement.phase();
            if self.until_state == 0 || self.published_phase != Some(phase) {
                for (position, prepared) in self.voices.iter().enumerate() {
                    self.visual_gains[position] = gains[position] as f32;
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
                    phase,
                    gains: &self.visual_gains,
                    breath_intensity: &self.breath,
                }) {
                    errors.report_timing_fault();
                    return;
                }
                self.until_state = self.state_interval;
                self.published_phase = Some(phase);
            }
            self.until_state -= 1;
            self.world.advance_sample();
            if arrived {
                self.wander.arrived(self.world.now());
            }
        }
        if !self.publish_receipt(errors.epoch()) {
            errors.report_timing_fault();
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

    fn prepare_engine(
        registry: Arc<Registry>,
        controls: ControlState,
        visuals: Arc<VisualRing>,
        rate: u32,
        seed: u64,
    ) -> Result<PreparedEngine, VoicePreparationError> {
        let mailbox = Arc::new(ControlMailbox::new(&controls));
        let receipts = Arc::new(StatusMailbox::new(controls.registry_count()));
        PreparedEngine::prepare(registry, controls, visuals, mailbox, receipts, rate, seed)
    }

    fn consume_engine(
        engine: &mut PreparedEngine,
        frames: usize,
        playback: u64,
        errors: &ErrorCell,
    ) {
        let mut buffer = [0.0; 1_024];
        let mut remaining = frames;
        let mut offset = 0;
        while remaining > 0 {
            let count = remaining.min(buffer.len() / 2);
            engine.render_f32(
                &mut buffer[..count * 2],
                2,
                MachNanos::new(playback + offset * 1_000_000_000 / u64::from(engine.sample_rate())),
                errors,
            );
            remaining -= count;
            offset += count as u64;
        }
    }

    #[test]
    fn recovery_rate_change_keeps_remaining_cross_seconds_and_muted_frozen_edits() {
        use crate::controls::ControlEdit;

        let registry = Registry::new().unwrap();
        let rock = registry.resolve("rock").unwrap();
        let old_epoch = StreamEpoch::new(101).unwrap();
        let new_epoch = StreamEpoch::new(102).unwrap();
        let old_errors = ErrorCell::new(old_epoch);
        let new_errors = ErrorCell::new(new_epoch);
        let mut engine = prepared(1_011);
        let mailbox = engine.mailbox.clone();
        let receipts = engine.receipts.clone();
        let mut controls = engine.controls.clone();
        controls.apply(ControlEdit::Mute(true)).unwrap();
        mailbox.publish(&controls);
        consume_engine(&mut engine, 960, 0, &old_errors);
        controls.apply(ControlEdit::Select(rock)).unwrap();
        mailbox.publish(&controls);
        consume_engine(&mut engine, 36_000, 20_000_000, &old_errors);
        let frozen = engine.world();
        let gains = engine.mix.gains().to_vec();
        assert_eq!(engine.movement, Movement::ManualCrossing { target: rock });
        controls
            .apply(ControlEdit::Volume(Normalized::new(0.8).unwrap()))
            .unwrap();
        mailbox.publish(&controls);
        assert!(engine.sync_frozen_controls(old_epoch));
        assert_eq!(receipts.try_read().unwrap().world, frozen);
        engine.prepare_output_epoch(44_100).unwrap();
        assert_eq!(engine.world(), frozen);
        assert_eq!(engine.sample_rate(), 44_100);
        assert_eq!(engine.mix.gains(), gains);
        assert!(engine.sync_frozen_controls(new_epoch));
        assert_eq!(receipts.try_read().unwrap().epoch, new_epoch);
        assert_eq!(
            receipts.try_read().unwrap().ack_revision,
            controls.settings_revision
        );
        let lease = EngineLease::new(engine, new_epoch);
        let wall_hour_later = 3_600_000_000_000;
        let mut silence = vec![1.0; (55_125 - 1) * 2];
        output_f32(
            &lease,
            &mut silence,
            2,
            &callback_info(wall_hour_later),
            &new_errors,
        );
        assert!(silence.iter().all(|sample| *sample == 0.0));
        assert!(lease.try_borrow().unwrap().engine().mix.is_crossing());
        let mut last = [1.0; 2];
        output_f32(
            &lease,
            &mut last,
            2,
            &callback_info(wall_hour_later + 1_250_000_000),
            &new_errors,
        );
        assert_eq!(last, [0.0; 2]);
        let mut recovered = lease.try_take().unwrap();
        assert!(recovered.controls.mute);
        assert_eq!(recovered.volume_gain.current, 0.8);
        assert_eq!(recovered.movement, Movement::Staying { current: rock });
        assert_eq!(recovered.mix.gains()[rock.get()], 1.0);
        assert!((recovered.world().get() - frozen.get() - 1.25).abs() < 1e-12);
        let count = recovered.voices.len();
        let mut gains = vec![0.0; count];
        let mut breath = vec![0.0; count];
        let mut next_gains = vec![0.0; count];
        let mut next_breath = vec![0.0; count];
        let first_state = recovered
            .visuals
            .read_state_into(
                new_epoch,
                MachNanos::new(wall_hour_later),
                &mut gains,
                &mut breath,
                &mut next_gains,
                &mut next_breath,
            )
            .unwrap()
            .current;
        assert_eq!(first_state.world, frozen);
        assert_eq!(first_state.present, MachNanos::new(wall_hour_later));
        recovered.prepare_output_epoch(48_000).unwrap();
        assert_eq!(old_errors.take_bits(), 0);
        assert_eq!(new_errors.take_bits(), 0);
    }

    #[test]
    fn recovery_preserves_dwell_deadline_and_uses_no_wall_clock_catchup() {
        let mut engine = prepared(1_013);
        let mut controls = engine.controls.clone();
        controls.dwell_min_minutes = 1;
        controls.dwell_max_minutes = 1;
        controls.mute = true;
        controls.knobs.fill(Normalized::ZERO);
        engine.mailbox.publish(&controls);
        let errors = ErrorCell::new(StreamEpoch::new(103).unwrap());
        consume_engine(&mut engine, 47 * 48_000, 0, &errors);
        let departure = engine.movement.display_target();
        let frozen = engine.world();
        engine.prepare_output_epoch(44_100).unwrap();
        assert_eq!(engine.world(), frozen);
        consume_engine(&mut engine, 13 * 44_100, 3_600_000_000_000, &errors);
        assert_eq!(engine.world().get(), 60.0);
        assert_eq!(engine.movement, Movement::Staying { current: departure });
        engine.render_f32(&mut [1.0; 2], 2, MachNanos::new(3_613_000_000_000), &errors);
        assert!(
            matches!(engine.movement, Movement::AutomaticCrossing { departure: from, target, .. } if from == departure && target != departure)
        );
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn same_rate_recovery_ramps_output_without_resetting_real_dsp_or_world_events() {
        for muted in [false, true] {
            let mut recovered = prepared(1_017);
            let mut reference = prepared(1_017);
            let old_epoch = StreamEpoch::new(104).unwrap();
            let epoch = StreamEpoch::new(105).unwrap();
            let old_errors = ErrorCell::new(old_epoch);
            let errors = ErrorCell::new(epoch);
            let mut controls = recovered.controls.clone();
            controls.mute = muted;
            controls.knobs.fill(Normalized::ONE);
            recovered.mailbox.publish(&controls);
            reference.mailbox.publish(&controls);
            consume_engine(&mut recovered, 12_000, 0, &old_errors);
            consume_engine(&mut reference, 12_000, 0, &old_errors);
            let frozen = recovered.world();
            recovered.prepare_output_epoch(48_000).unwrap();
            assert_eq!(recovered.world(), frozen);
            let mut actual = vec![0.0; 96_000];
            let mut unchanged = vec![0.0; 96_000];
            let playback = MachNanos::new(3_600_000_000_000);
            recovered.render_f32(&mut actual, 2, playback, &errors);
            reference.render_f32(&mut unchanged, 2, playback, &errors);
            assert_eq!(&actual[..2], &[0.0; 2]);
            let mut audible = false;
            for (frame, (a, b)) in actual
                .chunks_exact(2)
                .zip(unchanged.chunks_exact(2))
                .enumerate()
            {
                let fraction = (frame as f32 / 960.0).min(1.0);
                let recovery = fraction * fraction * (3.0 - 2.0 * fraction);
                for channel in 0..2 {
                    assert!((a[channel] - b[channel] * recovery).abs() < 1e-7);
                    audible |= a[channel] != 0.0;
                }
            }
            assert_eq!(&actual[1_920..], &unchanged[1_920..]);
            assert_eq!(audible, !muted);
            assert_eq!(recovered.world(), reference.world());
            let mut a_events = [None; crate::audio::transport::VISIBLE_CAPACITY];
            let mut b_events = [None; crate::audio::transport::VISIBLE_CAPACITY];
            let now = MachNanos::new(playback.get() + 900_000_000);
            let a_count = recovered.visuals.read_live_into(epoch, now, &mut a_events);
            let b_count = reference.visuals.read_live_into(epoch, now, &mut b_events);
            assert!(
                a_count > 0,
                "registered rain must publish live world particles"
            );
            assert_eq!(a_count, b_count);
            for (a, b) in a_events.iter().flatten().zip(b_events.iter().flatten()) {
                assert_eq!(a.world, b.world);
                assert_eq!(a.present, b.present);
                assert_eq!(a.x, b.x);
                assert_eq!(a.strength, b.strength);
            }
            assert_eq!(old_errors.take_bits(), 0);
            assert_eq!(errors.take_bits(), 0);
        }
    }

    // Wrap a registered real voice only at its worker preparation boundary.
    // The second voice fails after the first has already changed rate; this
    // catches an engine incorrectly treating its old rate as fully prepared.
    struct PreparationFailureVoice {
        inner: Box<dyn Voice>,
        fail_once: bool,
        rates: Arc<parking_lot::Mutex<Vec<u32>>>,
    }

    impl Voice for PreparationFailureVoice {
        fn advance_world(
            &mut self,
            frame: VoiceFrame,
            knobs: &[Normalized],
            events: &mut VisualWriter<'_>,
        ) {
            self.inner.advance_world(frame, knobs, events);
        }
        fn render_sample(&mut self) -> [f32; 2] {
            self.inner.render_sample()
        }
        fn set_contributing(&mut self, active: bool) {
            self.inner.set_contributing(active);
        }
        fn breath_intensity(&self) -> Normalized {
            self.inner.breath_intensity()
        }
        fn prepared_delay(&self) -> Result<VoiceLatencyFrames, VoicePreparationError> {
            self.inner.prepared_delay()
        }
        fn prepare_rate(&mut self, rate: u32) -> Result<VoiceLatencyFrames, VoicePreparationError> {
            self.rates.lock().push(rate);
            if std::mem::take(&mut self.fail_once) {
                return Err(VoicePreparationError::UnresolvedCausalDelay);
            }
            self.inner.prepare_rate(rate)
        }
    }

    // A real registered voice with one additional sample of causal delay.
    // The marker is deterministic so the consumer's use of RETURNED metadata
    // can be observed independently of the inner voice's Poisson arrivals.
    struct OneFrameDelayedVoice {
        inner: Box<dyn Voice>,
        pending: [f32; 2],
        delay: Option<VoiceLatencyFrames>,
    }

    impl Voice for OneFrameDelayedVoice {
        fn advance_world(
            &mut self,
            frame: VoiceFrame,
            knobs: &[Normalized],
            events: &mut VisualWriter<'_>,
        ) {
            self.inner.advance_world(frame, knobs, events);
            events.emit(crate::audio::transport::ParticleVisual {
                x: Normalized::ONE,
                y: Normalized::ZERO,
                strength: Normalized::ONE,
                radius: crate::audio::transport::LogicalPixels::new(1.0).unwrap(),
                shape: crate::situations::VisualShape::Point,
                large: true,
                lifetime: Duration::from_secs(1),
            });
        }
        fn render_sample(&mut self) -> [f32; 2] {
            let next = self.inner.render_sample();
            std::mem::replace(&mut self.pending, next)
        }
        fn set_contributing(&mut self, active: bool) {
            self.inner.set_contributing(active);
        }
        fn breath_intensity(&self) -> Normalized {
            self.inner.breath_intensity()
        }
        fn prepared_delay(&self) -> Result<VoiceLatencyFrames, VoicePreparationError> {
            self.delay
                .ok_or(VoicePreparationError::UnresolvedCausalDelay)
        }
        fn prepare_rate(&mut self, rate: u32) -> Result<VoiceLatencyFrames, VoicePreparationError> {
            self.delay = None;
            let inner = self.inner.prepare_rate(rate)?;
            let delay = VoiceLatencyFrames::new(inner.get() + 1.0)
                .ok_or(VoicePreparationError::UnresolvedCausalDelay)?;
            self.pending = [0.0; 2];
            self.delay = Some(delay);
            Ok(delay)
        }
    }

    #[test]
    fn new_epoch_uses_returned_dry_delay_at_its_actual_rate_without_zero_fallback() {
        let mut engine = prepared(1_021);
        let voices = std::mem::replace(&mut engine.voices, Box::new([]));
        engine.voices = voices
            .into_vec()
            .into_iter()
            .enumerate()
            .map(|(position, mut prepared)| {
                if position == 0 {
                    prepared.voice = Box::new(OneFrameDelayedVoice {
                        inner: prepared.voice,
                        pending: [0.0; 2],
                        delay: None,
                    });
                }
                prepared
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let mut live = [None; 8];
        for (rate, epoch, playback, expected_ns) in [
            (44_100, 107, 3_600_000_000_000, 22_675),
            (48_000, 108, 1_000, 20_833),
        ] {
            let epoch = StreamEpoch::new(epoch).unwrap();
            let errors = ErrorCell::new(epoch);
            let frozen = engine.world();
            engine.prepare_output_epoch(rate).unwrap();
            let mut sample = [1.0; 2];
            engine.render_f32(&mut sample, 2, MachNanos::new(playback), &errors);
            assert_eq!(sample, [0.0; 2]);
            assert_eq!(
                engine.visuals.read_live_into(
                    epoch,
                    MachNanos::new(playback + expected_ns - 1),
                    &mut live,
                ),
                0
            );
            assert!(
                engine.visuals.read_live_into(
                    epoch,
                    MachNanos::new(playback + expected_ns),
                    &mut live,
                ) > 0
            );
            let marker = live
                .iter()
                .flatten()
                .find(|event| event.x == Normalized::ONE)
                .unwrap();
            assert_eq!(marker.world, frozen);
            assert_eq!(marker.present.get(), playback + expected_ns);
            assert_eq!(marker.x, Normalized::ONE);
            assert_eq!(errors.take_bits(), 0);
        }
    }

    #[test]
    fn failed_partial_rate_preparation_requires_all_voices_to_revalidate_the_old_rate() {
        let mut engine = prepared(1_019);
        let voices = std::mem::replace(&mut engine.voices, Box::new([]));
        let mut rates = Vec::new();
        engine.voices = voices
            .into_vec()
            .into_iter()
            .enumerate()
            .map(|(position, mut prepared)| {
                let calls = Arc::new(parking_lot::Mutex::new(Vec::new()));
                prepared.voice = Box::new(PreparationFailureVoice {
                    inner: prepared.voice,
                    fail_once: position == 1,
                    rates: calls.clone(),
                });
                rates.push(calls);
                prepared
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let frozen = engine.world();
        assert_eq!(
            engine.prepare_output_epoch(44_100),
            Err(VoicePreparationError::UnresolvedCausalDelay)
        );
        assert_eq!(engine.world(), frozen);
        assert_eq!(engine.sample_rate(), 48_000);
        assert_eq!(*rates[0].lock(), [44_100]);
        assert_eq!(*rates[1].lock(), [44_100]);
        assert!(rates[2].lock().is_empty());
        engine.prepare_output_epoch(48_000).unwrap();
        assert_eq!(*rates[0].lock(), [44_100, 48_000]);
        assert_eq!(*rates[1].lock(), [44_100, 48_000]);
        assert_eq!(*rates[2].lock(), [48_000]);
        engine.prepare_output_epoch(48_000).unwrap();
        assert_eq!(*rates[0].lock(), [44_100, 48_000]);
        let errors = ErrorCell::new(StreamEpoch::new(106).unwrap());
        consume_engine(&mut engine, 2_000, 3_600_000_000_000, &errors);
        assert!(engine.world().get() > frozen.get());
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn output_envelope_rate_change_keeps_elapsed_seconds_and_exact_endpoint() {
        let mut ramp = OutputRamp::new(0.3, 48_000);
        ramp.retarget(0.8);
        for _ in 0..480 {
            ramp.advance();
        }
        let frozen = ramp.current;
        ramp.prepare_rate(44_100);
        assert_eq!(ramp.current, frozen);
        for _ in 0..440 {
            assert!(ramp.advance() < 0.8);
        }
        assert_eq!(ramp.advance(), 0.8);
    }

    #[test]
    fn mute_or_zero_volume_edited_while_frozen_cannot_leak_audio_during_recovery() {
        use crate::controls::ControlEdit;

        for muted in [false, true] {
            let mut engine = prepared(1_023);
            let old_epoch = StreamEpoch::new(109).unwrap();
            let new_epoch = StreamEpoch::new(110).unwrap();
            consume_engine(&mut engine, 4_800, 0, &ErrorCell::new(old_epoch));
            let frozen = engine.world();
            let mut controls = engine.controls.clone();
            if muted {
                controls.apply(ControlEdit::Mute(true)).unwrap();
                controls
                    .apply(ControlEdit::Volume(Normalized::new(0.8).unwrap()))
                    .unwrap();
            } else {
                controls
                    .apply(ControlEdit::Volume(Normalized::ZERO))
                    .unwrap();
            }
            engine.mailbox.publish(&controls);
            assert!(engine.sync_frozen_controls(old_epoch));
            engine.prepare_output_epoch(44_100).unwrap();
            assert_eq!(engine.world(), frozen);
            assert!(engine.sync_frozen_controls(new_epoch));
            let receipt = engine.receipts.try_read().unwrap();
            assert_eq!(receipt.epoch, new_epoch);
            assert_eq!(receipt.ack_revision, controls.settings_revision);
            let mut output = [1.0; 4_000];
            let errors = ErrorCell::new(new_epoch);
            engine.render_f32(&mut output, 2, MachNanos::new(3_600_000_000_000), &errors);
            assert!(output.iter().all(|sample| *sample == 0.0));
            assert_eq!(engine.controls.mute, muted);
            assert_eq!(engine.controls.volume, controls.volume);
            assert!(engine.world().get() > frozen.get());
            assert_eq!(errors.take_bits(), 0);
        }
    }

    #[test]
    fn automatic_callback_keeps_old_wait_and_arc_then_uses_new_arrival_settings() {
        use crate::controls::ControlEdit;

        fn consume(lease: &EngineLease, frames: usize, errors: &ErrorCell) {
            let mut buffer = [0.0; 1_024];
            let mut remaining = frames;
            while remaining > 0 {
                let count = remaining.min(buffer.len() / 2);
                let nanos = {
                    let mut guard = lease.try_borrow().unwrap();
                    (guard.engine().world().get() * 1_000_000_000.0) as u64
                };
                output_f32(
                    lease,
                    &mut buffer[..count * 2],
                    2,
                    &callback_info(nanos),
                    errors,
                );
                assert!(buffer[..count * 2].iter().all(|value| *value == 0.0));
                remaining -= count;
            }
        }

        let registry = Arc::new(Registry::new().unwrap());
        let mut controls = ControlState::new(&registry);
        controls.volume = Normalized::ZERO;
        controls.mute = true;
        controls.dwell_min_minutes = 1;
        controls.dwell_max_minutes = 1;
        controls.transition_seconds = 2;
        controls.knobs.fill(Normalized::ZERO);
        let departure = controls.manual_target;
        let visuals = Arc::new(VisualRing::new(EVENT_CAPACITY, controls.registry_count()));
        let engine = prepare_engine(registry, controls.clone(), visuals, 8_000, 911).unwrap();
        let mailbox = engine.mailbox.clone();
        let receipts = engine.receipts.clone();
        let epoch = StreamEpoch::new(91).unwrap();
        let lease = EngineLease::new(engine, epoch);
        let errors = ErrorCell::new(epoch);
        let (_, heap) = crate::rt_alloc::measure(|| {
            consume(&lease, 43 * 8_000, &errors);
            controls.apply(ControlEdit::DwellMinMinutes(2)).unwrap();
            controls.apply(ControlEdit::TransitionSeconds(4)).unwrap();
            mailbox.publish(&controls);
            consume(&lease, 17 * 8_000, &errors);
            assert_eq!(receipts.try_read().unwrap().world.get(), 60.0);
            assert_eq!(
                lease.try_borrow().unwrap().engine().movement,
                Movement::Staying { current: departure }
            );
            consume(&lease, 8_000, &errors);
            let target = {
                let mut guard = lease.try_borrow().unwrap();
                let engine = guard.engine();
                let Movement::AutomaticCrossing { target, .. } = engine.movement else {
                    panic!("the original dwell must finish despite time edits");
                };
                let target_power = engine.mix.gains()[target.get()].powi(2);
                assert!((target_power - (std::f64::consts::PI / 8.0).sin().powi(2)).abs() < 1e-10);
                assert_ne!(target, departure);
                assert_eq!(engine.movement.display_target(), departure);
                target
            };
            controls.apply(ControlEdit::DwellMaxMinutes(1)).unwrap();
            controls.apply(ControlEdit::TransitionSeconds(1)).unwrap();
            controls.apply(ControlEdit::Wander(false)).unwrap();
            mailbox.publish(&controls);
            consume(&lease, 8_000, &errors);
            assert_eq!(lease.try_borrow().unwrap().engine().movement.phase(), 2);
            controls.apply(ControlEdit::Wander(true)).unwrap();
            mailbox.publish(&controls);
            consume(&lease, 8_000, &errors);
            let receipt = receipts.try_read().unwrap();
            assert_eq!(receipt.display_dominant, target);
            assert_eq!(receipt.resume_target, target);
            consume(&lease, 8_000, &errors);
            assert_eq!(
                lease.try_borrow().unwrap().engine().movement,
                Movement::Staying { current: target }
            );
            assert_eq!(receipts.try_read().unwrap().world.get(), 64.0);
            consume(&lease, 59 * 8_000, &errors);
            assert_eq!(
                lease.try_borrow().unwrap().engine().movement,
                Movement::Staying { current: target }
            );
            consume(&lease, 8_000, &errors);
            assert_eq!(
                lease.try_borrow().unwrap().engine().movement,
                Movement::Staying { current: target }
            );
            consume(&lease, 8_000, &errors);
            let mut guard = lease.try_borrow().unwrap();
            let engine = guard.engine();
            let Movement::Staying { current } = engine.movement else {
                panic!("the next automatic arc must use the edited one-second duration");
            };
            assert_ne!(current, target);
            assert_eq!(engine.world().get(), 125.0);
            assert_eq!(engine.mix.gains()[current.get()], 1.0);
            assert_eq!(
                engine
                    .mix
                    .gains()
                    .iter()
                    .filter(|gain| **gain > 0.0)
                    .count(),
                1
            );
            assert!(controls.wander && controls.mute);
            assert_eq!(controls.manual_revision, crate::controls::Revision::INITIAL);
            assert_eq!(errors.take_bits(), 0);
        });
        assert_eq!(heap, crate::rt_alloc::Counts::default());
    }

    fn prepared(seed: u64) -> PreparedEngine {
        let registry = Arc::new(Registry::new().unwrap());
        let controls = ControlState::new(&registry);
        let visuals = Arc::new(VisualRing::new(
            EVENT_CAPACITY,
            registry.definitions().len(),
        ));
        prepare_engine(registry, controls, visuals, 48_000, seed).unwrap()
    }

    #[test]
    fn p08_phase_boundaries_publish_before_the_next_regular_visual_tick() {
        use crate::controls::ControlEdit;

        let mut engine = prepared(181);
        let registry = Registry::new().unwrap();
        let count = registry.definitions().len();
        let target = registry.resolve("rock").unwrap();
        let epoch = StreamEpoch::new(1).unwrap();
        let errors = ErrorCell::new(epoch);
        let mut gains = vec![0.0; count];
        let mut breath = vec![0.0; count];
        let mut next_gains = vec![0.0; count];
        let mut next_breath = vec![0.0; count];
        let mut read = |engine: &PreparedEngine| {
            engine
                .visuals
                .read_state_into(
                    epoch,
                    MachNanos::new(u64::MAX),
                    &mut gains,
                    &mut breath,
                    &mut next_gains,
                    &mut next_breath,
                )
                .unwrap()
                .current
        };
        engine.render_f32(&mut [0.0; 20], 2, MachNanos::new(0), &errors);
        assert_eq!(read(&engine).phase, 0);
        let mut controls = engine.controls.clone();
        controls.apply(ControlEdit::Select(target)).unwrap();
        engine.mailbox.publish(&controls);
        engine.render_f32(&mut [0.0; 2], 2, MachNanos::new(208_334), &errors);
        let crossing = read(&engine);
        assert_eq!(crossing.phase, 1);
        assert_eq!(crossing.present, MachNanos::new(208_334));
        let mut remaining = vec![0.0; 95_998 * 2];
        engine.render_f32(&mut remaining, 2, MachNanos::new(229_168), &errors);
        assert_eq!(read(&engine).phase, 1);
        engine.render_f32(&mut [0.0; 2], 2, MachNanos::new(2_000_187_501), &errors);
        let arrival = read(&engine);
        assert_eq!(arrival.phase, 0);
        assert_eq!(arrival.present, MachNanos::new(2_000_187_501));
        assert_eq!(gains[target.get()], 1.0);
        assert!(
            gains
                .iter()
                .enumerate()
                .all(|(index, gain)| index == target.get() || *gain == 0.0)
        );
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn p04_manual_selection_moves_real_gains_while_muted_without_volume_restarting_it() {
        use crate::controls::ControlEdit;

        let registry = Arc::new(Registry::new().unwrap());
        let target = registry.resolve("rock").expect("rock must be registered");
        let count = registry.definitions().len();
        let initial = registry.first_index();
        let mut controls = ControlState::new(&registry);
        controls.apply(ControlEdit::Mute(true)).unwrap();
        let visuals = Arc::new(VisualRing::new(EVENT_CAPACITY, count));
        let mut engine =
            prepare_engine(registry, controls.clone(), visuals.clone(), 8_000, 137).unwrap();
        controls.apply(ControlEdit::Select(target)).unwrap();
        engine.mailbox.publish(&controls);
        let errors = ErrorCell::new(StreamEpoch::new(1).unwrap());
        let mut buffer = [1.0; 1_024];
        let mut gains = vec![0.0; count];
        let mut breath = vec![0.0; count];
        let mut next_gains = vec![0.0; count];
        let mut next_breath = vec![0.0; count];
        for block in 0..36 {
            engine.render_f32(&mut buffer, 2, MachNanos::new(block * 64_000_000), &errors);
            assert!(buffer.iter().all(|sample| *sample == 0.0));
            if block == 15 {
                visuals
                    .read_state_into(
                        StreamEpoch::new(1).unwrap(),
                        MachNanos::new(u64::MAX),
                        &mut gains,
                        &mut breath,
                        &mut next_gains,
                        &mut next_breath,
                    )
                    .expect("actual callback must publish gains");
                assert!(gains[initial.get()] > 0.0);
                assert!(gains[target.get()] > 0.0);
                let power: f32 = gains.iter().map(|gain| gain * gain).sum();
                assert!((power - 1.0).abs() < 1e-5);
                controls
                    .apply(ControlEdit::Volume(Normalized::new(0.2).unwrap()))
                    .unwrap();
                engine.mailbox.publish(&controls);
            }
        }
        visuals
            .read_state_into(
                StreamEpoch::new(1).unwrap(),
                MachNanos::new(u64::MAX),
                &mut gains,
                &mut breath,
                &mut next_gains,
                &mut next_breath,
            )
            .unwrap();
        for (index, gain) in gains.iter().enumerate() {
            assert_eq!(*gain, if index == target.get() { 1.0 } else { 0.0 });
        }
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn actual_callback_crossfade_retargets_and_contribution_boundaries_do_not_touch_heap() {
        use crate::controls::ControlEdit;

        let registry = Registry::new().unwrap();
        let initial = registry.first_index();
        let target = registry.resolve("rock").expect("rock must be registered");
        let epoch = StreamEpoch::new(18).unwrap();
        let engine = prepared(613);
        let mailbox = engine.mailbox.clone();
        let mut controls = engine.controls.clone();
        let lease = EngineLease::new(engine, epoch);
        let errors = ErrorCell::new(epoch);
        let mut output = [0.0; 1_024];
        let (_, heap) = crate::rt_alloc::measure(|| {
            for turn in 0..12 {
                controls
                    .apply(ControlEdit::Select(if turn % 2 == 0 {
                        target
                    } else {
                        initial
                    }))
                    .unwrap();
                mailbox.publish(&controls);
                output_f32(
                    &lease,
                    &mut output,
                    2,
                    &callback_info(turn * 11_000_000),
                    &errors,
                );
            }
            controls.apply(ControlEdit::Select(target)).unwrap();
            mailbox.publish(&controls);
            for block in 0..188 {
                output_f32(
                    &lease,
                    &mut output,
                    2,
                    &callback_info(200_000_000 + block * 11_000_000),
                    &errors,
                );
            }
            controls.apply(ControlEdit::Select(initial)).unwrap();
            mailbox.publish(&controls);
            output_f32(
                &lease,
                &mut output,
                2,
                &callback_info(2_300_000_000),
                &errors,
            );
        });
        assert_eq!(heap, crate::rt_alloc::Counts::default());
        let engine = lease.try_take().unwrap();
        assert!(engine.mix.gains()[initial.get()] > 0.0);
        assert!(engine.mix.gains()[target.get()] > 0.0);
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn actual_three_voice_callback_keeps_latest_arc_and_dsp_history_through_silent_edits() {
        use crate::controls::ControlEdit;

        for rate in [8_000, 44_100, 48_000, 96_000] {
            let registry = Arc::new(Registry::new().unwrap());
            let rain = registry.resolve("rain").unwrap();
            let rock = registry.resolve("rock").unwrap();
            let deep = registry.resolve("deep").unwrap();
            let count = registry.definitions().len();
            let epoch = StreamEpoch::new(20).unwrap();
            let mut reference_controls = ControlState::new(&registry);
            reference_controls.knobs.fill(Normalized::ONE);
            reference_controls
                .apply(ControlEdit::Volume(Normalized::ONE))
                .unwrap();
            let mut edited_controls = reference_controls.clone();
            let reference_visuals = Arc::new(VisualRing::new(EVENT_CAPACITY, count));
            let edited_visuals = Arc::new(VisualRing::new(EVENT_CAPACITY, count));
            let reference_engine = prepare_engine(
                registry.clone(),
                reference_controls.clone(),
                reference_visuals.clone(),
                rate,
                613,
            )
            .unwrap();
            let edited_engine = prepare_engine(
                registry,
                edited_controls.clone(),
                edited_visuals.clone(),
                rate,
                613,
            )
            .unwrap();
            let reference_mailbox = reference_engine.mailbox.clone();
            let edited_mailbox = edited_engine.mailbox.clone();
            let reference = EngineLease::new(reference_engine, epoch);
            let edited = EngineLease::new(edited_engine, epoch);
            let errors = ErrorCell::new(epoch);
            let frames = rate as usize / 10;
            let mut a = vec![0.0; frames * 2];
            let mut b = vec![0.0; frames * 2];
            let mut a_events = [None; crate::audio::transport::VISIBLE_CAPACITY];
            let mut b_events = [None; crate::audio::transport::VISIBLE_CAPACITY];
            let mut silent_event_observed = false;
            let mut audible_energy = 0.0_f64;
            let (_, heap) = crate::rt_alloc::measure(|| {
                for block in 0..10 {
                    if block == 5 {
                        reference_controls.apply(ControlEdit::Select(rock)).unwrap();
                        edited_controls.apply(ControlEdit::Select(rock)).unwrap();
                        reference_mailbox.publish(&reference_controls);
                        edited_mailbox.publish(&edited_controls);
                    }
                    let info = callback_info(block * 100_000_000);
                    output_f32(&reference, &mut a, 2, &info, &errors);
                    output_f32(&edited, &mut b, 2, &info, &errors);
                    assert_eq!(a, b);
                }
                let mut before = [0.0; 3];
                before.copy_from_slice(edited.try_borrow().unwrap().engine().mix.gains());
                assert!(before[rain.get()] > 0.0 && before[rock.get()] > 0.0);
                assert_eq!(before[deep.get()], 0.0);

                // Several accepted main-thread intents before one callback must
                // behave like receiving only the latest, not a queued itinerary.
                for target in [rain, rock, deep] {
                    edited_controls.apply(ControlEdit::Select(target)).unwrap();
                    edited_mailbox.publish(&edited_controls);
                }
                reference_controls.apply(ControlEdit::Select(deep)).unwrap();
                reference_mailbox.publish(&reference_controls);
                let info = callback_info(1_000_000_000);
                output_f32(&reference, &mut [], 2, &info, &errors);
                output_f32(&edited, &mut [], 2, &info, &errors);
                {
                    let mut guard = edited.try_borrow().unwrap();
                    let engine = guard.engine();
                    assert_eq!(engine.mix.gains(), before);
                    assert_eq!(engine.world().get(), 1.0);
                    assert_eq!(engine.movement, Movement::ManualCrossing { target: deep });
                }
                let mut first = [0.0; 2];
                let mut first_reference = [0.0; 2];
                output_f32(&reference, &mut first_reference, 2, &info, &errors);
                output_f32(&edited, &mut first, 2, &info, &errors);
                assert_eq!(first, first_reference);
                {
                    let mut guard = edited.try_borrow().unwrap();
                    let engine = guard.engine();
                    assert!(engine.voices.iter().all(|voice| voice.contributing));
                    for (gain, old) in engine.mix.gains().iter().zip(before) {
                        assert!(*gain > 0.0);
                        // Numerical one-frame continuity bound, not a listening
                        // or correlated-waveform loudness guarantee.
                        assert!((*gain - old).abs() <= std::f64::consts::PI / f64::from(rate));
                    }
                }
                for block in 0..20 {
                    match block {
                        2 => {
                            edited_controls
                                .apply(ControlEdit::Volume(Normalized::ZERO))
                                .unwrap();
                        }
                        4 => {
                            edited_controls.apply(ControlEdit::Mute(true)).unwrap();
                        }
                        6 => {
                            edited_controls
                                .apply(ControlEdit::Volume(Normalized::ONE))
                                .unwrap();
                        }
                        8 => {
                            edited_controls.apply(ControlEdit::Mute(false)).unwrap();
                        }
                        _ => {}
                    }
                    // Re-publishing an unchanged manual revision every callback
                    // must not restart the two-second movement.
                    edited_mailbox.publish(&edited_controls);
                    reference_mailbox.publish(&reference_controls);
                    let samples = if block == 0 || block == 19 {
                        (frames - 1) * 2
                    } else {
                        frames * 2
                    };
                    let playback = 1_000_000_000
                        + block * 100_000_000
                        + if block == 0 {
                            1_000_000_000 / u64::from(rate)
                        } else {
                            0
                        };
                    let info = callback_info(playback);
                    output_f32(&reference, &mut a[..samples], 2, &info, &errors);
                    output_f32(&edited, &mut b[..samples], 2, &info, &errors);
                    assert!(
                        a[..samples]
                            .iter()
                            .chain(&b[..samples])
                            .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
                    );
                    audible_energy += a[..samples]
                        .iter()
                        .map(|sample| f64::from(*sample).powi(2))
                        .sum::<f64>();
                    if (3..8).contains(&block) {
                        assert!(b[..samples].iter().all(|sample| *sample == 0.0));
                    }
                    if block >= 9 {
                        assert_eq!(a[..samples], b[..samples]);
                    }
                    let now = MachNanos::new(1_000_000_000 + (block + 1) * 100_000_000);
                    let event_count = reference_visuals.read_live_into(epoch, now, &mut a_events);
                    assert_eq!(
                        edited_visuals.read_live_into(epoch, now, &mut b_events),
                        event_count
                    );
                    for (a, b) in a_events.iter().zip(&b_events).take(event_count) {
                        let a = a.unwrap();
                        let b = b.unwrap();
                        assert_eq!(
                            (a.serial, a.situation, a.world, a.present),
                            (b.serial, b.situation, b.world, b.present)
                        );
                        assert_eq!(
                            (a.x, a.y, a.strength, a.radius, a.shape, a.lifetime),
                            (b.x, b.y, b.strength, b.radius, b.shape, b.lifetime)
                        );
                        silent_event_observed |= (1.3..1.8).contains(&b.world.get());
                    }
                    let mut reference_guard = reference.try_borrow().unwrap();
                    let mut edited_guard = edited.try_borrow().unwrap();
                    let reference_engine = reference_guard.engine();
                    let edited_engine = edited_guard.engine();
                    assert_eq!(reference_engine.mix.gains(), edited_engine.mix.gains());
                    assert_eq!(reference_engine.world(), edited_engine.world());
                    assert!(edited_engine.voices.iter().all(|voice| voice.contributing));
                    assert!(edited_engine.mix.is_crossing());
                }
                let final_info = callback_info(3_000_000_000 - 1_000_000_000 / u64::from(rate));
                output_f32(&reference, &mut first_reference, 2, &final_info, &errors);
                output_f32(&edited, &mut first, 2, &final_info, &errors);
                assert_eq!(first, first_reference);
                for lease in [&reference, &edited] {
                    let mut guard = lease.try_borrow().unwrap();
                    let engine = guard.engine();
                    assert_eq!(engine.mix.gains()[deep.get()], 1.0);
                    assert!(!engine.mix.is_crossing());
                    assert_eq!(engine.movement, Movement::Staying { current: deep });
                    for voice in &engine.voices {
                        assert_eq!(voice.contributing, voice.index == deep);
                        assert_eq!(
                            engine.mix.gains()[voice.index.get()],
                            if voice.index == deep { 1.0 } else { 0.0 }
                        );
                    }
                    assert_eq!(engine.world().get(), 3.0);
                }
                output_f32(
                    &reference,
                    &mut a,
                    2,
                    &callback_info(3_000_000_000),
                    &errors,
                );
                output_f32(&edited, &mut b, 2, &callback_info(3_000_000_000), &errors);
                assert_eq!(a, b);
                assert_eq!(
                    edited.try_borrow().unwrap().engine().movement,
                    Movement::Staying { current: deep }
                );
                for turn in 0..1_000 {
                    let targets = [rain, rock, deep];
                    let target = targets[turn as usize % targets.len()];
                    before.copy_from_slice(edited.try_borrow().unwrap().engine().mix.gains());
                    edited_controls
                        .apply(ControlEdit::Select(
                            targets[(turn as usize + 1) % targets.len()],
                        ))
                        .unwrap();
                    edited_mailbox.publish(&edited_controls);
                    edited_controls.apply(ControlEdit::Select(target)).unwrap();
                    edited_mailbox.publish(&edited_controls);
                    reference_controls
                        .apply(ControlEdit::Select(target))
                        .unwrap();
                    reference_mailbox.publish(&reference_controls);
                    let info =
                        callback_info(3_100_000_000 + turn * 127 * 1_000_000_000 / u64::from(rate));
                    output_f32(&reference, &mut [], 2, &info, &errors);
                    output_f32(&edited, &mut [], 2, &info, &errors);
                    assert_eq!(edited.try_borrow().unwrap().engine().mix.gains(), before);
                    output_f32(&reference, &mut a[..254], 2, &info, &errors);
                    output_f32(&edited, &mut b[..254], 2, &info, &errors);
                    assert_eq!(a[..254], b[..254]);
                    {
                        let mut guard = edited.try_borrow().unwrap();
                        let gains = guard.engine().mix.gains();
                        assert!(gains.iter().all(|gain| gain.is_finite() && *gain >= 0.0));
                        // Arithmetic tolerance only, not a loudness guarantee.
                        assert!(
                            (gains.iter().map(|gain| gain * gain).sum::<f64>() - 1.0).abs() < 1e-12
                        );
                    }
                    if turn > 0 {
                        assert!(
                            edited
                                .try_borrow()
                                .unwrap()
                                .engine()
                                .voices
                                .iter()
                                .all(|voice| voice.contributing)
                        );
                    }
                }
                // The thousandth rapid intent is rain, already audible in the
                // mix. Its 127 generated frames count toward the same arc.
                for block in 0..20 {
                    edited_mailbox.publish(&edited_controls);
                    reference_mailbox.publish(&reference_controls);
                    let samples = if block == 19 {
                        (frames - 127) * 2
                    } else {
                        frames * 2
                    };
                    let playback = 3_100_000_000
                        + 1_000 * 127 * 1_000_000_000 / u64::from(rate)
                        + block * 100_000_000;
                    let info = callback_info(playback);
                    output_f32(&reference, &mut a[..samples], 2, &info, &errors);
                    output_f32(&edited, &mut b[..samples], 2, &info, &errors);
                    assert_eq!(a[..samples], b[..samples]);
                }
                let mut guard = edited.try_borrow().unwrap();
                let engine = guard.engine();
                assert_eq!(engine.movement, Movement::Staying { current: rain });
                for voice in &engine.voices {
                    assert_eq!(voice.contributing, voice.index == rain);
                }
            });
            assert_eq!(heap, crate::rt_alloc::Counts::default(), "rate {rate}");
            assert!(
                audible_energy > 0.0,
                "real three-voice DSP must generate sound"
            );
            assert!(
                silent_event_observed,
                "real particles must continue during silent crossing"
            );
            assert_eq!(errors.take_bits(), 0, "rate {rate}");
        }
    }

    #[test]
    fn real_voice_consumer_keeps_a_rounded_one_target_crossing_while_another_gain_is_positive() {
        use crate::controls::ControlEdit;

        let rate = 48_000;
        let registry = Arc::new(Registry::new().unwrap());
        let rain = registry.resolve("rain").unwrap();
        let rock = registry.resolve("rock").unwrap();
        let mut controls = ControlState::new(&registry);
        controls.knobs.fill(Normalized::ONE);
        let visuals = Arc::new(VisualRing::new(
            EVENT_CAPACITY,
            registry.definitions().len(),
        ));
        let mut engine = prepare_engine(registry, controls.clone(), visuals, rate, 617).unwrap();
        // Seed only the numeric boundary via existing ramp operations, not a
        // fake voice or a production control duration.
        engine
            .mix
            .retarget(rock, Duration::from_secs(1_000_000_000_000_000));
        engine.mix.advance_frames(1, 100_000);
        let before = engine.mix.gains().to_vec();
        let tiny = before[rock.get()];
        assert_eq!(before[rain.get()], 1.0);
        assert!(tiny > 1e-20 && tiny < 2e-20);
        let mailbox = engine.mailbox.clone();
        let epoch = StreamEpoch::new(22).unwrap();
        let lease = EngineLease::new(engine, epoch);
        let errors = ErrorCell::new(epoch);
        let mut block = vec![0.0; (rate as usize - 1) * 2];
        let mut frame = [0.0; 2];
        controls.apply(ControlEdit::Select(rain)).unwrap();
        mailbox.publish(&controls);
        let (_, heap) = crate::rt_alloc::measure(|| {
            output_f32(&lease, &mut [], 2, &callback_info(0), &errors);
            {
                let mut guard = lease.try_borrow().unwrap();
                let engine = guard.engine();
                assert_eq!(engine.mix.gains(), before);
                assert_eq!(engine.movement, Movement::ManualCrossing { target: rain });
                assert!(engine.mix.is_crossing());
            }
            output_f32(&lease, &mut frame, 2, &callback_info(0), &errors);
            assert!(lease.try_borrow().unwrap().engine().voices[rock.get()].contributing);
            output_f32(&lease, &mut block, 2, &callback_info(1), &errors);
            {
                let mut guard = lease.try_borrow().unwrap();
                let engine = guard.engine();
                assert!(engine.mix.is_crossing());
                assert!(engine.voices[rock.get()].contributing);
                assert!(engine.mix.gains()[rock.get()] > 0.0);
                // Numerical arc tolerance only, not perceived loudness.
                assert!((engine.mix.gains()[rock.get()] / tiny - 0.5).abs() < 1e-12);
            }
            output_f32(
                &lease,
                &mut block,
                2,
                &callback_info(1_000_000_000),
                &errors,
            );
            assert!(lease.try_borrow().unwrap().engine().mix.is_crossing());
            output_f32(
                &lease,
                &mut frame,
                2,
                &callback_info(2_000_000_000),
                &errors,
            );
            {
                let mut guard = lease.try_borrow().unwrap();
                let engine = guard.engine();
                assert_eq!(engine.movement, Movement::Staying { current: rain });
                for voice in &engine.voices {
                    assert_eq!(
                        engine.mix.gains()[voice.index.get()],
                        if voice.index == rain { 1.0 } else { 0.0 }
                    );
                    assert_eq!(voice.contributing, voice.index == rain);
                }
            }
            // A new manual revision for an exact one-hot target is a real no-op.
            controls.apply(ControlEdit::Select(rain)).unwrap();
            mailbox.publish(&controls);
            output_f32(&lease, &mut [], 2, &callback_info(2_000_000_000), &errors);
            assert!(!lease.try_borrow().unwrap().engine().mix.is_crossing());
        });
        assert_eq!(heap, crate::rt_alloc::Counts::default());
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn recovered_real_rate_consumer_finishes_its_existing_arc_in_remaining_seconds() {
        use crate::controls::ControlEdit;

        for (old_rate, new_rate) in [(48_000, 32_000), (44_100, 96_000)] {
            let registry = Arc::new(Registry::new().unwrap());
            let rock = registry.resolve("rock").unwrap();
            let deep = registry.resolve("deep").unwrap();
            let count = registry.definitions().len();
            let mut controls = ControlState::new(&registry);
            controls.knobs.fill(Normalized::ONE);
            let engine = prepare_engine(
                registry.clone(),
                controls.clone(),
                Arc::new(VisualRing::new(EVENT_CAPACITY, count)),
                old_rate,
                619,
            )
            .unwrap();
            let mailbox = engine.mailbox.clone();
            let epoch = StreamEpoch::new(23).unwrap();
            let old = EngineLease::new(engine, epoch);
            let errors = ErrorCell::new(epoch);
            let mut half_second = vec![0.0; old_rate as usize];
            controls.apply(ControlEdit::Select(rock)).unwrap();
            mailbox.publish(&controls);
            output_f32(&old, &mut half_second, 2, &callback_info(0), &errors);
            controls.apply(ControlEdit::Select(deep)).unwrap();
            mailbox.publish(&controls);
            output_f32(
                &old,
                &mut half_second,
                2,
                &callback_info(500_000_000),
                &errors,
            );
            old.close();
            let mut previous = old.try_take().unwrap();
            let before = previous.mix.gains().to_vec();
            assert!(before.iter().all(|gain| *gain > 0.0));
            assert_eq!(previous.movement, Movement::ManualCrossing { target: deep });
            previous.prepare_output_epoch(new_rate).unwrap();
            let next_epoch = StreamEpoch::new(24).unwrap();
            let errors = ErrorCell::new(next_epoch);
            let next = EngineLease::new(previous, next_epoch);
            let remaining_frames = new_rate as usize * 3 / 2;
            let mut almost_remaining = vec![0.0; (remaining_frames - 1) * 2];
            let mut last = [0.0; 2];
            let (_, heap) = crate::rt_alloc::measure(|| {
                output_f32(&next, &mut [], 2, &callback_info(1_000_000_000), &errors);
                assert_eq!(next.try_borrow().unwrap().engine().mix.gains(), before);
                output_f32(
                    &next,
                    &mut almost_remaining,
                    2,
                    &callback_info(1_000_000_000),
                    &errors,
                );
                {
                    let mut guard = next.try_borrow().unwrap();
                    let engine = guard.engine();
                    assert!(engine.mix.is_crossing());
                    assert_eq!(engine.movement, Movement::ManualCrossing { target: deep });
                    assert!(engine.voices.iter().all(|voice| voice.contributing));
                }
                output_f32(&next, &mut last, 2, &callback_info(2_500_000_000), &errors);
                {
                    let mut guard = next.try_borrow().unwrap();
                    let engine = guard.engine();
                    assert_eq!(engine.movement, Movement::Staying { current: deep });
                    for voice in &engine.voices {
                        assert_eq!(
                            engine.mix.gains()[voice.index.get()],
                            if voice.index == deep { 1.0 } else { 0.0 }
                        );
                        assert_eq!(voice.contributing, voice.index == deep);
                    }
                }
            });
            assert_eq!(heap, crate::rt_alloc::Counts::default());
            assert_eq!(errors.take_bits(), 0, "{old_rate}->{new_rate}");
        }
    }

    #[test]
    fn actual_callback_reentry_with_zero_excitation_cannot_revive_departed_rain_or_rock() {
        use crate::{controls::ControlEdit, situations::KnobIndex};

        for id in ["rain", "rock"] {
            let registry = Arc::new(Registry::new().unwrap());
            let returning = registry.resolve(id).unwrap();
            let deep = registry.resolve("deep").unwrap();
            let knob_range = registry.knob_range(returning);
            let epoch = StreamEpoch::new(21).unwrap();
            let mut excited_controls = ControlState::new(&registry);
            excited_controls
                .apply(ControlEdit::Select(returning))
                .unwrap();
            excited_controls.knobs[knob_range.clone()].fill(Normalized::ONE);
            let mut quiet_controls = excited_controls.clone();
            quiet_controls.knobs[knob_range.clone()].fill(Normalized::ZERO);
            let excited_visuals = Arc::new(VisualRing::new(
                EVENT_CAPACITY,
                registry.definitions().len(),
            ));
            let quiet_visuals = Arc::new(VisualRing::new(
                EVENT_CAPACITY,
                registry.definitions().len(),
            ));
            let excited_engine = prepare_engine(
                registry.clone(),
                excited_controls.clone(),
                excited_visuals.clone(),
                48_000,
                827,
            )
            .unwrap();
            let quiet_engine =
                prepare_engine(registry, quiet_controls.clone(), quiet_visuals, 48_000, 827)
                    .unwrap();
            let excited_mailbox = excited_engine.mailbox.clone();
            let quiet_mailbox = quiet_engine.mailbox.clone();
            let excited = EngineLease::new(excited_engine, epoch);
            let quiet = EngineLease::new(quiet_engine, epoch);
            let errors = ErrorCell::new(epoch);
            let mut a = [0.0; 9_600];
            let mut b = [0.0; 9_600];
            let mut visible = [None; crate::audio::transport::VISIBLE_CAPACITY];
            let mut new_excitation_energy = 0.0_f64;
            let (_, heap) =
                crate::rt_alloc::measure(|| {
                    for block in 0..10 {
                        let info = callback_info(block * 100_000_000);
                        output_f32(&excited, &mut a, 2, &info, &errors);
                        output_f32(&quiet, &mut b, 2, &info, &errors);
                    }
                    assert!(a.iter().any(|sample| sample.abs() > 0.000_001));
                    for controls in [&mut excited_controls, &mut quiet_controls] {
                        controls.apply(ControlEdit::Select(deep)).unwrap();
                    }
                    excited_mailbox.publish(&excited_controls);
                    quiet_mailbox.publish(&quiet_controls);
                    for block in 10..30 {
                        let info = callback_info(block * 100_000_000);
                        output_f32(&excited, &mut a, 2, &info, &errors);
                        output_f32(&quiet, &mut b, 2, &info, &errors);
                    }
                    {
                        let mut guard = excited.try_borrow().unwrap();
                        let engine = guard.engine();
                        assert_eq!(engine.mix.gains()[returning.get()], 0.0);
                        assert!(!engine.voices[returning.get()].contributing);
                    }
                    // Keep the departed voice's birth knobs high for a full second:
                    // its world still advances, but it must admit no fresh particles.
                    for block in 30..43 {
                        if block == 40 {
                            for index in knob_range.clone() {
                                excited_controls
                                    .apply(ControlEdit::Knob(
                                        KnobIndex::from_index(index, excited_controls.knobs.len())
                                            .unwrap(),
                                        Normalized::ZERO,
                                    ))
                                    .unwrap();
                            }
                            // Knob edits issue a manual command for their owner.
                            // Coalesce the zero-excitation edit with deep selection
                            // so this phase really leaves the returning voice inactive.
                            excited_controls.apply(ControlEdit::Select(deep)).unwrap();
                            quiet_controls.apply(ControlEdit::Select(deep)).unwrap();
                            quiet_mailbox.publish(&quiet_controls);
                            excited_mailbox.publish(&excited_controls);
                        }
                        let info = callback_info(block * 100_000_000);
                        output_f32(&excited, &mut a, 2, &info, &errors);
                        output_f32(&quiet, &mut b, 2, &info, &errors);
                        assert_eq!(a, b, "departed {id} must not affect deep");
                        excited_visuals.read_live_into(
                            epoch,
                            MachNanos::new((block + 1) * 100_000_000),
                            &mut visible,
                        );
                        assert!(visible.iter().flatten().all(|event| {
                            event.situation != returning || event.world.get() < 3.0
                        }));
                    }
                    for controls in [&mut excited_controls, &mut quiet_controls] {
                        controls.apply(ControlEdit::Select(returning)).unwrap();
                    }
                    excited_mailbox.publish(&excited_controls);
                    quiet_mailbox.publish(&quiet_controls);
                    // The deep samples are identical in both real engines. During
                    // the whole return arc any difference therefore exposes revived
                    // audio history, not a GainRamp-only metadata discrepancy.
                    for block in 43..64 {
                        let info = callback_info(block * 100_000_000);
                        output_f32(&excited, &mut a, 2, &info, &errors);
                        output_f32(&quiet, &mut b, 2, &info, &errors);
                        assert_eq!(a, b, "zero-input reentry revived an old {id} tail");
                    }
                    assert!(a.iter().all(|sample| *sample == 0.0));
                    {
                        let mut guard = excited.try_borrow().unwrap();
                        let engine = guard.engine();
                        assert_eq!(engine.movement, Movement::Staying { current: returning });
                        // Floating sample time is not exact decimal 6.4; this
                        // arithmetic tolerance is not an audible timing guarantee.
                        assert!((engine.world().get() - 6.4).abs() < 1e-12);
                    }
                    for index in knob_range.clone() {
                        excited_controls
                            .apply(ControlEdit::Knob(
                                KnobIndex::from_index(index, excited_controls.knobs.len()).unwrap(),
                                Normalized::ONE,
                            ))
                            .unwrap();
                    }
                    excited_mailbox.publish(&excited_controls);
                    for block in 64..74 {
                        output_f32(
                            &excited,
                            &mut a,
                            2,
                            &callback_info(block * 100_000_000),
                            &errors,
                        );
                        assert!(
                            a.iter()
                                .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
                        );
                        new_excitation_energy += a
                            .iter()
                            .map(|sample| f64::from(*sample).powi(2))
                            .sum::<f64>();
                    }
                });
            assert_eq!(
                heap,
                crate::rt_alloc::Counts::default(),
                "{id} departure and reentry"
            );
            assert!(
                new_excitation_energy > 0.0,
                "reentered {id} must accept fresh excitation"
            );
            assert_eq!(errors.take_bits(), 0);
        }
    }

    #[test]
    fn frozen_manual_acceptance_updates_display_without_advancing_actual_mix() {
        use crate::controls::ControlEdit;

        let registry = Registry::new().unwrap();
        let initial = registry.first_index();
        let target = registry.resolve("rock").expect("rock must be registered");
        let epoch = StreamEpoch::new(19).unwrap();
        let errors = ErrorCell::new(epoch);
        let mut engine = prepared(719);
        let mut controls = engine.controls.clone();
        controls.apply(ControlEdit::Select(target)).unwrap();
        engine.mailbox.publish(&controls);
        let mut output = vec![0.0; 144_000];
        engine.render_f32(&mut output, 2, MachNanos::new(0), &errors);
        assert!(engine.mix.gains()[target.get()] > engine.mix.gains()[initial.get()]);
        let gains = engine.mix.gains().to_vec();
        let frozen_world = engine.world();
        controls.apply(ControlEdit::Select(initial)).unwrap();
        engine.mailbox.publish(&controls);
        assert!(engine.sync_frozen_controls(epoch));
        assert!(engine.sync_frozen_controls(epoch));
        assert_eq!(engine.mix.gains(), gains);
        assert_eq!(engine.world(), frozen_world);
        let receipt = engine.receipts.try_read().unwrap();
        assert_eq!(receipt.display_dominant, initial);
        assert_eq!(receipt.world, frozen_world);
        assert_eq!(engine.movement.target(), initial);
        engine.render_f32(&mut output, 2, MachNanos::new(1_500_000_000), &errors);
        engine.render_f32(
            &mut output[..48_000],
            2,
            MachNanos::new(3_000_000_000),
            &errors,
        );
        for (position, gain) in engine.mix.gains().iter().enumerate() {
            assert_eq!(*gain, if position == initial.get() { 1.0 } else { 0.0 });
        }
        assert_eq!(errors.take_bits(), 0);
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
        let mut a = [0.0; 2_560];
        let mut b = [0.0; 2_560];
        audible.render_f32(&mut a, 2, MachNanos::new(0), &errors);
        muted.render_f32(&mut b, 2, MachNanos::new(0), &errors);
        assert!(b[1_918..].iter().all(|sample| *sample == 0.0));
        assert_eq!(audible.world(), muted.world());
        controls.mute = false;
        muted.mailbox.publish(&controls);
        audible.render_f32(&mut a, 2, MachNanos::new(30_000_000), &errors);
        muted.render_f32(&mut b, 2, MachNanos::new(30_000_000), &errors);
        assert_eq!(a[1_918..], b[1_918..]);
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

    fn callback_gain_step_is_smoothed(mute: bool) {
        let epoch = StreamEpoch::new(3).unwrap();
        let reference_engine = prepared(103);
        let edited_engine = prepared(103);
        let mailbox = edited_engine.mailbox.clone();
        let mut controls = edited_engine.controls.clone();
        let reference = EngineLease::new(reference_engine, epoch);
        let edited = EngineLease::new(edited_engine, epoch);
        let errors = ErrorCell::new(epoch);
        let mut a = [0.0; 9_600];
        let mut b = [0.0; 9_600];
        let info = callback_info(0);
        output_f32(&reference, &mut a, 2, &info, &errors);
        output_f32(&edited, &mut b, 2, &info, &errors);
        assert_eq!(a, b);

        controls.volume = Normalized::new(0.8).unwrap();
        controls.mute = mute;
        mailbox.publish(&controls);
        let info = callback_info(100_000_000);
        let (_, heap) = crate::rt_alloc::measure(|| {
            output_f32(&reference, &mut a, 2, &info, &errors);
            output_f32(&edited, &mut b, 2, &info, &errors);
        });
        assert_eq!(heap, crate::rt_alloc::Counts::default());
        assert!(a[..1_920].iter().any(|sample| sample.abs() > 0.000_001));
        for (frame, (reference, edited)) in a.chunks_exact(2).zip(b.chunks_exact(2)).enumerate() {
            // A conservative implementation ramp lasts 20 ms (960 frames here).
            // Both independent endpoints move smoothly and reach exact values.
            let fraction = ((frame + 1) as f32 / 960.0).min(1.0);
            let eased = fraction * fraction * (3.0 - 2.0 * fraction);
            let volume = if frame + 1 >= 960 {
                0.8
            } else {
                0.3 + (0.8 - 0.3) * eased
            };
            let mute_gain = if mute { 1.0 - eased } else { 1.0 };
            for channel in 0..2 {
                let expected = reference[channel] * (volume * mute_gain / 0.3);
                assert!(
                    (edited[channel] - expected).abs() < 0.000_001,
                    "abrupt {kind} endpoint at frame {frame}: expected {expected}, got {actual}",
                    kind = if mute { "mute" } else { "volume" },
                    actual = edited[channel],
                );
            }
        }
        if mute {
            assert!(b[1_918..].iter().all(|sample| *sample == 0.0));
        }
        assert_eq!(
            reference.try_take().unwrap().world(),
            edited.try_take().unwrap().world()
        );
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn actual_callback_volume_step_uses_a_finite_post_mix_ramp() {
        callback_gain_step_is_smoothed(false);
    }

    #[test]
    fn actual_callback_mute_step_uses_an_independent_finite_post_mix_ramp() {
        callback_gain_step_is_smoothed(true);
    }

    #[test]
    fn output_ramps_reach_exact_endpoints_and_do_not_restart_unchanged_targets() {
        for rate in [1, 22_050, 44_100, 48_000, 96_000] {
            let frames = (u64::from(rate) * 20).div_ceil(1_000);
            let mut ramp = OutputRamp::new(0.3, rate);
            ramp.retarget(0.0);
            let mut previous = 0.3;
            for frame in 1..=frames {
                ramp.retarget(0.0);
                let value = ramp.advance();
                assert!(value.is_finite());
                assert!((0.0..=previous).contains(&value));
                if frame == frames {
                    assert_eq!(value, 0.0);
                }
                previous = value;
            }
            assert_eq!(ramp.advance(), 0.0);
            ramp.retarget(1.0);
            for _ in 0..frames {
                ramp.advance();
            }
            assert_eq!(ramp.advance(), 1.0);
        }
    }

    #[test]
    fn output_ramp_retargets_from_the_actual_current_value_without_a_step() {
        let mut ramp = OutputRamp::new(0.3, 48_000);
        ramp.retarget(0.8);
        for _ in 0..317 {
            ramp.advance();
        }
        let current = ramp.current;
        assert!((0.3..0.8).contains(&current));
        ramp.retarget(0.0);
        assert_eq!(ramp.current, current);
        assert!((ramp.advance() - current).abs() < 0.000_01);
        for _ in 1..960 {
            ramp.advance();
        }
        assert_eq!(ramp.current, 0.0);
    }

    #[test]
    fn silent_output_keeps_actual_particle_world_and_voice_history_alive() {
        for mute in [false, true] {
            let epoch = StreamEpoch::new(12).unwrap();
            let errors = ErrorCell::new(epoch);
            let mut audible = prepared(211);
            let mut silent = prepared(211);
            let mut controls = silent.controls.clone();
            if mute {
                controls.mute = true;
            } else {
                controls.volume = Normalized::ZERO;
            }
            silent.mailbox.publish(&controls);
            let mut a = [0.0; 9_600];
            let mut b = [0.0; 9_600];
            let mut a_events = [None; crate::audio::transport::VISIBLE_CAPACITY];
            let mut b_events = [None; crate::audio::transport::VISIBLE_CAPACITY];
            let mut first_event = None;
            let mut latest_event = 0.0_f64;
            for block in 0..10 {
                let playback = MachNanos::new(block * 100_000_000);
                audible.render_f32(&mut a, 2, playback, &errors);
                silent.render_f32(&mut b, 2, playback, &errors);
                assert!(b[1_918..].iter().all(|sample| *sample == 0.0));
                let now = MachNanos::new((block + 1) * 100_000_000);
                let count = audible.visuals.read_live_into(epoch, now, &mut a_events);
                assert_eq!(
                    silent.visuals.read_live_into(epoch, now, &mut b_events),
                    count
                );
                for (a, b) in a_events.iter().zip(&b_events).take(count) {
                    let a = a.unwrap();
                    let b = b.unwrap();
                    assert_eq!(a.serial, b.serial);
                    assert_eq!(a.world, b.world);
                    assert_eq!(a.present, b.present);
                    assert_eq!(a.x, b.x);
                    assert_eq!(a.y, b.y);
                    assert_eq!(a.strength, b.strength);
                    first_event.get_or_insert(b.world.get());
                    latest_event = latest_event.max(b.world.get());
                }
            }
            assert_eq!(audible.world(), silent.world());
            assert_eq!(silent.world().get(), 1.0);
            assert!(
                first_event.is_some_and(|first| latest_event > first && latest_event > 0.5),
                "actual seeded particle event times must continue during silent output"
            );
            let now = MachNanos::new(1_000_000_000);
            controls.mute = false;
            controls.volume = Normalized::new(0.3).unwrap();
            silent.mailbox.publish(&controls);
            audible.render_f32(&mut a, 2, now, &errors);
            silent.render_f32(&mut b, 2, now, &errors);
            assert_eq!(a[1_918..], b[1_918..]);
            assert_eq!(errors.take_bits(), 0);
        }
    }

    #[test]
    fn actual_callback_receipts_ack_latest_endpoints_without_rewriting_main_controls() {
        use crate::{controls::ControlEdit, situations::KnobIndex};

        let epoch = StreamEpoch::new(13).unwrap();
        let engine = prepared(307);
        let mut main = engine.controls.clone();
        let mailbox = engine.mailbox.clone();
        let receipts = engine.receipts.clone();
        assert_eq!(receipts.try_read(), None);
        main.edit_knob(
            KnobIndex::from_index(0, main.knobs.len()).unwrap(),
            Normalized::ONE,
        )
        .unwrap();
        mailbox.publish(&main);
        main.apply(ControlEdit::Mute(true)).unwrap();
        main.apply(ControlEdit::Volume(Normalized::new(0.8).unwrap()))
            .unwrap();
        let latest = main.clone();
        mailbox.publish(&main);
        let lease = EngineLease::new(engine, epoch);
        let errors = ErrorCell::new(epoch);
        let mut output = [0.0; 2_048];
        let (_, heap) = crate::rt_alloc::measure(|| {
            output_f32(&lease, &mut output, 2, &callback_info(0), &errors);
        });
        assert_eq!(heap, crate::rt_alloc::Counts::default());
        let receipt = receipts.try_read().unwrap();
        assert_eq!(receipt.epoch, epoch);
        assert_eq!(receipt.ack_revision, latest.settings_revision);
        assert_eq!(receipt.ack_manual_revision, latest.manual_revision);
        assert_eq!(receipt.display_dominant, latest.manual_target);
        assert!(receipt.world.get() > 0.0);
        assert_eq!(main, latest);
        let engine = lease.try_take().unwrap();
        assert_eq!(engine.controls, latest);
        assert_eq!(engine.volume_gain.current, 0.8);
        assert_eq!(engine.mute_gain.current, 0.0);
        assert_eq!(receipt.world, engine.world());
        assert!(output[1_918..].iter().all(|sample| *sample == 0.0));
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn frozen_owner_accepts_controls_without_advancing_the_generated_world() {
        use crate::controls::ControlEdit;

        let epoch = StreamEpoch::new(14).unwrap();
        let mut engine = prepared(409);
        let mut main = engine.controls.clone();
        let before = engine.world();
        main.apply(ControlEdit::Mute(true)).unwrap();
        engine.mailbox.publish(&main);
        assert!(engine.sync_frozen_controls(epoch));
        let receipt = engine.receipts.try_read().unwrap();
        assert_eq!(receipt.ack_revision, main.settings_revision);
        assert_eq!(receipt.world, before);
        assert_eq!(engine.world(), before);
        assert!(engine.controls.mute);
        assert!(engine.sync_frozen_controls(epoch));
        assert_eq!(engine.receipts.try_read(), Some(receipt));
    }

    #[test]
    fn frozen_worker_acknowledges_controls_accepted_before_a_callback_fault() {
        use crate::controls::ControlEdit;

        let epoch = StreamEpoch::new(17).unwrap();
        let engine = prepared(409);
        let mailbox = engine.mailbox.clone();
        let receipts = engine.receipts.clone();
        let mut main = engine.controls.clone();
        let lease = EngineLease::new(engine, epoch);
        let errors = ErrorCell::new(epoch);
        let mut output = [1.0; 4];
        output_f32(&lease, &mut output, 2, &callback_info(0), &errors);
        let previous = receipts.try_read().unwrap();
        main.apply(ControlEdit::Mute(true)).unwrap();
        mailbox.publish(&main);
        output_f32(&lease, &mut output, 2, &callback_info(u64::MAX), &errors);
        assert_eq!(
            errors.take_bits(),
            crate::audio::transport::ErrorCode::TIMING_FAULT
        );
        assert_eq!(receipts.try_read(), Some(previous));
        lease.close();
        let mut frozen = lease.try_take().unwrap();
        let frozen_world = frozen.world();
        assert!(frozen.controls.mute);
        assert!(frozen.sync_frozen_controls(epoch));
        let accepted = receipts.try_read().unwrap();
        assert_eq!(accepted.ack_revision, main.settings_revision);
        assert_eq!(accepted.world, frozen_world);
        assert_eq!(frozen.world(), frozen_world);
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

    #[test]
    fn failed_candidate_epoch_does_not_hide_latest_frozen_control_receipt() {
        use crate::controls::ControlEdit;

        let mut engine = prepared(2_063);
        let current = StreamEpoch::new(151).unwrap();
        let candidate = StreamEpoch::new(152).unwrap();
        assert!(engine.sync_frozen_controls(current));
        let held_world = engine.world();
        let mut controls = engine.controls.clone();
        let target = SituationIndex::from_index(1, engine.voices.len()).unwrap();
        controls.apply(ControlEdit::Select(target)).unwrap();
        controls.apply(ControlEdit::Mute(true)).unwrap();
        engine.mailbox.publish(&controls);
        engine.prepare_output_epoch(48_000).unwrap();
        assert!(engine.sync_frozen_controls(candidate));
        let prepared = engine.receipts.try_read().unwrap();
        assert_eq!(prepared.epoch, candidate);
        assert_eq!(prepared.ack_revision, controls.settings_revision);
        // A rejected build keeps the current UI source and returns this same
        // engine to the frozen worker. No further edit should be needed to
        // publish its already accepted latest intent in the current generation.
        assert!(engine.sync_frozen_controls(current));
        let retained = engine.receipts.try_read().unwrap();
        assert_eq!(retained.epoch, current);
        assert_eq!(retained.ack_revision, controls.settings_revision);
        assert_eq!(retained.ack_manual_revision, controls.manual_revision);
        assert_eq!(retained.display_dominant, target);
        assert_eq!(retained.world, held_world);
    }

    fn render_blocks(
        engine: &mut PreparedEngine,
        errors: &ErrorCell,
        start: u64,
        frames: usize,
    ) -> Vec<f32> {
        // 2 ms blocks at 48 kHz; playback advances exactly with the generated frames.
        let mut out = Vec::with_capacity(frames * 2);
        let mut done = 0_usize;
        while done < frames {
            let count = (frames - done).min(96);
            let mut block = vec![0.0_f32; count * 2];
            let playback = start + (done as u64) * 1_000_000_000 / 48_000;
            engine.render_f32(&mut block, 2, MachNanos::new(playback), errors);
            out.extend_from_slice(&block);
            done += count;
        }
        out
    }

    #[test]
    fn shutdown_fade_reaches_exact_silence_and_reports_the_final_sample_time() {
        let epoch = StreamEpoch::new(1).unwrap();
        let errors = ErrorCell::new(epoch);
        let mut engine = prepared(2_024);
        let mut reference = prepared(2_024);
        let start = 7_000_000_000_u64;
        // Warm up so the rain is actually audible when the fade begins.
        let warm = render_blocks(&mut engine, &errors, start, 48_000);
        render_blocks(&mut reference, &errors, start, 48_000);
        assert!(
            warm.iter().any(|sample| sample.abs() > 0.0),
            "audible before"
        );

        let attempt = AttemptId::from_raw(1).unwrap();
        let begin = start + 1_000_000_000;
        engine.mailbox.request_shutdown(attempt);
        // Half the fade: no deadline yet, the stream must still be held.
        let first = render_blocks(&mut engine, &errors, begin, 2_400);
        assert_eq!(engine.mailbox.shutdown_deadline(attempt), None);
        // The rest of the fade and beyond.
        let rest = render_blocks(&mut engine, &errors, begin + 50_000_000, 4_800);
        let deadline = engine
            .mailbox
            .shutdown_deadline(attempt)
            .expect("a finished fade reports its final sample time");
        // Exactly 100 ms of fade (4800 frames at 48 kHz) plus the final sample's own frame.
        let fade_end = begin + 100_000_000;
        assert!(
            deadline.get() >= fade_end,
            "deadline {deadline:?} before the fade ended"
        );
        assert!(deadline.get() <= fade_end + 2 * 1_000_000_000 / 48_000);
        // The fade is a smooth decrease to exact silence, never an abrupt cut.
        let early_peak = first[..480]
            .iter()
            .fold(0.0_f32, |peak, s| peak.max(s.abs()));
        let late_peak = first[first.len() - 480..]
            .iter()
            .fold(0.0_f32, |peak, s| peak.max(s.abs()));
        assert!(late_peak <= early_peak + 1e-6);
        assert!(
            rest[2 * 2_400..].iter().all(|sample| *sample == 0.0),
            "silent after 100 ms"
        );
        // Another attempt's id never inherits this deadline.
        assert_eq!(
            engine
                .mailbox
                .shutdown_deadline(AttemptId::from_raw(2).unwrap()),
            None
        );
        // The world and voices kept generating through the fade (independent output gain).
        render_blocks(&mut reference, &errors, begin, 7_200);
        assert_eq!(engine.world(), reference.world());
        assert_eq!(errors.take_bits(), 0);
    }

    #[test]
    fn without_a_shutdown_request_output_gain_is_untouched() {
        let epoch = StreamEpoch::new(1).unwrap();
        let errors = ErrorCell::new(epoch);
        let mut engine = prepared(2_025);
        render_blocks(&mut engine, &errors, 0, 24_000);
        assert_eq!(
            engine
                .mailbox
                .shutdown_deadline(AttemptId::from_raw(1).unwrap()),
            None
        );
        assert!(engine.shutdown_gain.settled_at(1.0));
        assert_eq!(errors.take_bits(), 0);
    }
}
