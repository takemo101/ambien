use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use gpui::{
    Bounds, BoxShadow, ContentMask, Context, FocusHandle, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, Render, Rgba, Styled, Subscription, Task, Window, canvas,
    div, fill, point, px, size,
};

use crate::{
    audio::{
        AudioShared, AudioSource, OutputStatus,
        timing::MachNanos,
        transport::{VISIBLE_CAPACITY, VisualEvent, WorldVisualHeader},
    },
    situations::VisualShape,
};

struct VisibleParticles {
    events: Box<[Option<VisualEvent>]>,
    gains: Box<[f32]>,
    breath: Box<[f32]>,
    scratch_gains: Box<[f32]>,
    scratch_breath: Box<[f32]>,
    present: MachNanos,
    state: Option<WorldVisualHeader>,
    initialized: bool,
}

impl VisibleParticles {
    fn new(count: usize) -> Self {
        Self {
            events: vec![None; VISIBLE_CAPACITY].into_boxed_slice(),
            gains: vec![0.0; count].into_boxed_slice(),
            breath: vec![0.0; count].into_boxed_slice(),
            scratch_gains: vec![0.0; count].into_boxed_slice(),
            scratch_breath: vec![0.0; count].into_boxed_slice(),
            present: MachNanos::new(0),
            state: None,
            initialized: false,
        }
    }

    fn refresh_at(&mut self, source: &AudioSource, now: MachNanos) {
        source
            .visuals
            .read_live_into(source.epoch, now, &mut self.events);
        if let Some(header) = source.visuals.read_state_into(
            source.epoch,
            now,
            &mut self.scratch_gains,
            &mut self.scratch_breath,
        ) {
            std::mem::swap(&mut self.gains, &mut self.scratch_gains);
            std::mem::swap(&mut self.breath, &mut self.scratch_breath);
            self.state = Some(header);
        }
        self.present = now;
        self.initialized = true;
    }

    fn freeze_at(&mut self, source: &AudioSource, now: MachNanos) {
        // Failure holds the last accepted frame, including its fade time.
        // A scene created while frozen has no such frame and initializes once.
        if !self.initialized {
            self.refresh_at(source, now);
        }
    }

    fn reset_registry(&mut self, count: usize) {
        if self.initialized {
            self.events.fill(None);
        }
        if self.gains.len() != count {
            self.gains = vec![0.0; count].into_boxed_slice();
            self.breath = vec![0.0; count].into_boxed_slice();
            self.scratch_gains = vec![0.0; count].into_boxed_slice();
            self.scratch_breath = vec![0.0; count].into_boxed_slice();
        } else if self.initialized {
            self.gains.fill(0.0);
            self.breath.fill(0.0);
            self.scratch_gains.fill(0.0);
            self.scratch_breath.fill(0.0);
        }
        self.present = MachNanos::new(0);
        self.state = None;
        self.initialized = false;
    }
}

pub(crate) struct SceneView {
    focus: FocusHandle,
    source: Option<Arc<AudioSource>>,
    particles: VisibleParticles,
    running: bool,
    animation: Option<Task<()>>,
    _visibility: Subscription,
}

impl SceneView {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        audio: Arc<AudioShared>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let visibility = cx.observe_window_visibility(window, |view, _, window, cx| {
            view.update_animation(window, cx);
        });
        let (status, source) = audio.snapshot();
        let mut view = Self {
            focus,
            source: None,
            particles: VisibleParticles::new(0),
            running: false,
            animation: None,
            _visibility: visibility,
        };
        view.update_audio(status, source, window, cx);
        view
    }

    pub(crate) fn update_audio(
        &mut self,
        status: OutputStatus,
        source: Option<Arc<AudioSource>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(source) = source
            && self
                .source
                .as_ref()
                .is_none_or(|old| old.epoch != source.epoch)
        {
            self.particles
                .reset_registry(source.registry.definitions().len());
            self.source = Some(source);
        }
        self.running = status.is_running();
        if let Some(source) = &self.source {
            let freeze = source.freeze_time.load(Ordering::SeqCst);
            if !self.running && freeze != 0 {
                self.particles.freeze_at(source, MachNanos::new(freeze));
            }
        }
        self.update_animation(window, cx);
        cx.notify();
    }

    fn update_animation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.is_visible() || !self.running || self.source.is_none() {
            self.animation = None;
            return;
        }
        if self.animation.is_some() {
            return;
        }
        // Restore cannot paint pre-hide particles while waiting for the first timer.
        self.refresh();
        cx.notify();
        if !self.running {
            return;
        }
        self.animation = Some(cx.spawn_in(window, async move |view, cx| {
            loop {
                // Never exceed 30 Hz. Closing/hiding the scene drops this task, not the engine.
                cx.background_executor()
                    .timer(Duration::from_nanos(33_333_334))
                    .await;
                let keep_running = cx.update(|window, cx| {
                    view.update(cx, |view, cx| {
                        if !window.is_visible() || !view.running {
                            return false;
                        }
                        view.refresh();
                        cx.notify();
                        view.running
                    })
                });
                if !matches!(keep_running, Ok(Ok(true))) {
                    break;
                }
            }
        }));
    }

    fn refresh(&mut self) {
        let Some(source) = &self.source else {
            return;
        };
        match source.clock.now() {
            Ok(now) => self.refresh_at(now),
            Err(error) => {
                source.errors.report_timing_fault();
                self.running = false;
                eprintln!("Ambien: 視覚の予定時刻を取得できません: {error}");
            }
        }
    }

    fn refresh_at(&mut self, now: MachNanos) {
        let Some(source) = &self.source else {
            return;
        };
        self.particles.refresh_at(source, now);
    }

    fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let Some(source) = &self.source else {
            return;
        };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (index, definition) in source.registry.definitions().iter().enumerate() {
                let intensity = self.particles.gains[index] * self.particles.breath[index];
                if intensity > 0.0 {
                    let radius = 24.0 + intensity * 70.0;
                    let center = point(
                        bounds.origin.x + bounds.size.width * 0.5,
                        bounds.origin.y + bounds.size.height * 0.58,
                    );
                    paint_glow(
                        window,
                        center,
                        radius,
                        definition.visual.color,
                        intensity * 0.025,
                    );
                }
            }
            for event in self.particles.events.iter().flatten() {
                let elapsed = self
                    .particles
                    .present
                    .get()
                    .saturating_sub(event.present.get());
                let age = (elapsed as f64 / event.lifetime.as_nanos() as f64) as f32;
                let fade = (1.0 - age).clamp(0.0, 1.0).powi(2);
                let radius = event.radius.get()
                    * match event.shape {
                        VisualShape::Point => 1.0,
                        VisualShape::Impact => 1.0 + age * 1.6,
                        VisualShape::SlowGlow => 1.0 + age * 0.4,
                    };
                let center = point(
                    bounds.origin.x + bounds.size.width * event.x.get(),
                    bounds.origin.y + bounds.size.height * event.y.get(),
                );
                let color = source.registry.definitions()[event.situation.get()]
                    .visual
                    .color;
                paint_glow(window, center, radius, color, event.strength.get() * fade);
            }
        });
    }
}

fn paint_glow(
    window: &mut Window,
    center: gpui::Point<Pixels>,
    radius: f32,
    rgb: [f32; 3],
    opacity: f32,
) {
    let color: gpui::Hsla = Rgba {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
        a: opacity,
    }
    .into();
    let bounds = Bounds::new(
        center - point(px(radius), px(radius)),
        size(px(radius * 2.0), px(radius * 2.0)),
    );
    window.paint_drop_shadows(
        bounds,
        px(radius).into(),
        &[BoxShadow {
            color,
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(radius * 2.4),
            spread_radius: px(0.0),
            inset: false,
        }],
    );
    window.paint_quad(fill(bounds, color).corner_radii(px(radius)));
}

impl Render for SceneView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.weak_entity();
        // No background paint: the native blurred surface stays visible beneath the lights.
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| {
                        // Borrow the fixed cache during paint; don't clone a particle vector each frame.
                        let _ = view.update(cx, |view, _| view.paint(bounds, window));
                    },
                )
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audio::{
            timing::{MachClock, StreamEpoch, WorldSeconds},
            transport::{ControlMailbox, ErrorCell, LogicalPixels, VisualRing, WorldVisualState},
        },
        controls::{ControlState, Normalized},
        situations::Registry,
    };
    use std::sync::atomic::AtomicU64;

    fn source() -> AudioSource {
        let registry = Arc::new(Registry::new().unwrap());
        let epoch = StreamEpoch::new(1).unwrap();
        AudioSource {
            controls: Arc::new(ControlMailbox::new(&ControlState::new(&registry))),
            visuals: Arc::new(VisualRing::new(1, registry.definitions().len())),
            registry,
            epoch,
            clock: MachClock::new().unwrap(),
            errors: Arc::new(ErrorCell::new(epoch)),
            freeze_time: AtomicU64::new(0),
        }
    }

    fn publish(source: &AudioSource, world: f64, present: u64, gain: f32, breath: f32) {
        assert!(source.visuals.push_state(WorldVisualState {
            epoch: source.epoch,
            world: WorldSeconds::new(world).unwrap(),
            present: MachNanos::new(present),
            phase: 0,
            gains: &[gain],
            breath_intensity: &[breath],
        }));
        assert!(source.visuals.push_event(VisualEvent {
            epoch: source.epoch,
            world: WorldSeconds::new(world).unwrap(),
            present: MachNanos::new(present),
            situation: source.registry.first_index(),
            x: Normalized::new(0.25).unwrap(),
            y: Normalized::new(0.5).unwrap(),
            strength: Normalized::ONE,
            radius: LogicalPixels::new(2.0).unwrap(),
            shape: VisualShape::Point,
            lifetime: Duration::from_nanos(50),
            serial: 0,
        }));
    }

    #[test]
    fn output_failure_preserves_the_already_accepted_frame_not_a_newer_ring_snapshot() {
        let source = source();
        publish(&source, 1.0, 10, 1.0, 0.25);
        let mut cache = VisibleParticles::new(1);
        cache.refresh_at(&source, MachNanos::new(40));
        let header = cache.state;
        let particle = cache.events[0].unwrap().serial;
        publish(&source, 2.0, 41, 0.5, 0.75);
        cache.freeze_at(&source, MachNanos::new(99));
        assert_eq!(cache.present, MachNanos::new(40));
        assert_eq!(cache.state, header);
        assert_eq!(cache.events[0].map(|event| event.serial), Some(particle));
        assert_eq!(cache.gains.as_ref(), &[1.0]);
        assert_eq!(cache.breath.as_ref(), &[0.25]);
    }

    #[test]
    fn a_new_frozen_scene_initializes_once_at_the_worker_checkpoint() {
        let source = source();
        publish(&source, 2.0, 41, 0.5, 0.75);
        let mut cache = VisibleParticles::new(1);
        cache.freeze_at(&source, MachNanos::new(60));
        assert_eq!(cache.present, MachNanos::new(60));
        assert_eq!(
            cache.state,
            Some(WorldVisualHeader {
                epoch: source.epoch,
                world: WorldSeconds::new(2.0).unwrap(),
                present: MachNanos::new(41),
                phase: 0,
            })
        );
        assert_eq!(cache.events[0].unwrap().serial, 1);
        assert_eq!(cache.gains.as_ref(), &[0.5]);
        assert_eq!(cache.breath.as_ref(), &[0.75]);
    }
}
