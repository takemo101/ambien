use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use ely::theme::ActiveTheme;
use gpui::{
    BorderStyle, Bounds, BoxShadow, ContentMask, Context, Corners, FocusHandle, InteractiveElement,
    IntoElement, MouseButton, ParentElement, PathBuilder, Pixels, Render, RenderImage, Rgba,
    Styled, Subscription, Task, Window, canvas, div, fill, outline, point, px, size,
};

use crate::{
    audio::{
        AudioShared, AudioSource, OutputStatus,
        timing::MachNanos,
        transport::{VISIBLE_CAPACITY, VisualEvent, WorldVisualHeader},
    },
    background::cover_rect,
    controls::{ControlState, VisualIntensity},
    situations::VisualShape,
    ui::{
        appearance::{AccessibilityState, Motion},
        backdrop,
        look::{Budget, Mark, ParticleInput, look, spikes, vertices},
    },
};

struct VisibleParticles {
    events: Box<[Option<VisualEvent>]>,
    contributions: Box<[f32]>,
    breath: Box<[f32]>,
    scratch_gains: Box<[f32]>,
    scratch_breath: Box<[f32]>,
    scratch_next_gains: Box<[f32]>,
    scratch_next_breath: Box<[f32]>,
    present: MachNanos,
    state: Option<WorldVisualHeader>,
    initialized: bool,
}

impl VisibleParticles {
    fn new(count: usize) -> Self {
        Self {
            events: vec![None; VISIBLE_CAPACITY].into_boxed_slice(),
            contributions: vec![0.0; count].into_boxed_slice(),
            breath: vec![0.0; count].into_boxed_slice(),
            scratch_gains: vec![0.0; count].into_boxed_slice(),
            scratch_breath: vec![0.0; count].into_boxed_slice(),
            scratch_next_gains: vec![0.0; count].into_boxed_slice(),
            scratch_next_breath: vec![0.0; count].into_boxed_slice(),
            present: MachNanos::new(0),
            state: None,
            initialized: false,
        }
    }

    fn refresh_at(&mut self, source: &AudioSource, now: MachNanos) {
        let Some(bracket) = source.visuals.read_state_into(
            source.epoch,
            now,
            &mut self.scratch_gains,
            &mut self.scratch_breath,
            &mut self.scratch_next_gains,
            &mut self.scratch_next_breath,
        ) else {
            // A lost atomic snapshot must not partially advance the accepted frame.
            return;
        };
        if let Some(next) = bracket.next {
            let elapsed = now.get() - bracket.current.present.get();
            let interval = next.present.get() - bracket.current.present.get();
            let blend = (elapsed as f64 / interval as f64) as f32;
            for index in 0..self.scratch_gains.len() {
                let current_power = self.scratch_gains[index].powi(2);
                let next_power = self.scratch_next_gains[index].powi(2);
                self.scratch_gains[index] = current_power + (next_power - current_power) * blend;
                self.scratch_breath[index] +=
                    (self.scratch_next_breath[index] - self.scratch_breath[index]) * blend;
            }
        } else {
            for gain in self.scratch_gains.iter_mut() {
                *gain *= *gain;
            }
        }
        std::mem::swap(&mut self.contributions, &mut self.scratch_gains);
        std::mem::swap(&mut self.breath, &mut self.scratch_breath);
        self.state = Some(bracket.current);
        source
            .visuals
            .read_live_into(source.epoch, now, &mut self.events);
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

    fn particle_frame(&self, event: &VisualEvent, motion: Motion) -> Option<(f32, f32)> {
        let contribution = self.contributions[event.situation.get()];
        if contribution == 0.0 {
            return None;
        }
        let elapsed = self.present.get().saturating_sub(event.present.get());
        let age = (elapsed as f64 / event.lifetime.as_nanos() as f64) as f32;
        let fade = motion.particle_fade(age);
        let opacity = event.strength.get() * fade * contribution;
        (opacity > 0.0).then_some((age, opacity))
    }

    fn reset_registry(&mut self, count: usize) {
        self.events.fill(None);
        if self.contributions.len() != count {
            self.contributions = vec![0.0; count].into_boxed_slice();
            self.breath = vec![0.0; count].into_boxed_slice();
            self.scratch_gains = vec![0.0; count].into_boxed_slice();
            self.scratch_breath = vec![0.0; count].into_boxed_slice();
            self.scratch_next_gains = vec![0.0; count].into_boxed_slice();
            self.scratch_next_breath = vec![0.0; count].into_boxed_slice();
        } else {
            self.contributions.fill(0.0);
            self.breath.fill(0.0);
            self.scratch_gains.fill(0.0);
            self.scratch_breath.fill(0.0);
            self.scratch_next_gains.fill(0.0);
            self.scratch_next_breath.fill(0.0);
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
    motion: Motion,
    opaque: bool,
    veil: f32,
    intensity: VisualIntensity,
    /// The 8-bit look is wanted, and the backing scale it was last applied at (`None` = not applied).
    eightbit: bool,
    /// The ready-to-paint background image, with its opacity already in the pixels.
    background: Option<Arc<RenderImage>>,
    dots_applied: Option<f64>,
    _visibility: Subscription,
    _placement: Subscription,
}

impl SceneView {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        audio: Arc<AudioShared>,
        accessibility: AccessibilityState,
        controls: &ControlState,
        background: Option<Arc<RenderImage>>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let visibility = cx.observe_window_visibility(window, |view, _, window, cx| {
            view.update_animation(window, cx);
        });
        let placement = cx.observe_window_bounds(window, |view, window, cx| {
            // A move to a display with another backing scale needs the dots re-sized.
            view.apply_dots(window);
            // Full screen and zoom are the screen's frame, not a place the user chose.
            if let Some(placement) = crate::platform::macos::capture_windowed_placement(window) {
                crate::app::dispatch_scene_placement_changed(
                    window.window_handle().window_id(),
                    placement,
                    cx,
                );
            }
        });
        let (status, source) = audio.snapshot();
        let mut view = Self {
            focus,
            source: None,
            particles: VisibleParticles::new(0),
            running: false,
            animation: None,
            motion: accessibility.motion(),
            opaque: accessibility.reduce_transparency,
            veil: controls.veil.get(),
            intensity: controls.visuals,
            eightbit: controls.eightbit,
            background,
            dots_applied: None,
            _visibility: visibility,
            _placement: placement,
        };
        view.update_audio(status, source, window, cx);
        view.apply_dots(window);
        view
    }

    /// Applies the surface choices only to presentation: the window surface (blur choice
    /// and reduce transparency), the colour film, how particles are drawn, and the 8-bit look. Audio state,
    /// controls and the cached particles are untouched.
    pub(crate) fn set_surface(
        &mut self,
        accessibility: AccessibilityState,
        controls: &ControlState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.set_background_appearance(accessibility.scene_background(controls.blur));
        crate::platform::macos::apply_blur_material(window);
        self.motion = accessibility.motion();
        self.opaque = accessibility.reduce_transparency;
        self.veil = controls.veil.get();
        self.intensity = controls.visuals;
        self.eightbit = controls.eightbit;
        self.apply_dots(window);
        cx.notify();
    }

    /// Shows (or removes) the background image; the pixels are prepared elsewhere.
    pub(crate) fn set_background(
        &mut self,
        background: Option<Arc<RenderImage>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // GPUI keeps a replaced picture's texture in the window's atlas until told to drop it.
        if let Some(old) = std::mem::replace(&mut self.background, background) {
            let _ = window.drop_image(old);
        }
        cx.notify();
    }

    /// Puts the 8-bit look on or off the drawing layer, and keeps its dot size right when the
    /// window moves to a display with another backing scale. Presentation only.
    fn apply_dots(&mut self, window: &Window) {
        let wanted = self.eightbit.then(|| f64::from(window.scale_factor()));
        if wanted == self.dots_applied {
            return;
        }
        // Record the attempt first, so a failure is reported once and not on every bounds change.
        self.dots_applied = wanted;
        let block = wanted.and_then(crate::ui::dots::block_pixels);
        if let Err(error) = crate::platform::macos::set_dot_filter(window, block) {
            eprintln!("Ambien: 8bitモードを反映できません: {error}");
        }
    }

    pub(crate) fn update_audio(
        &mut self,
        status: OutputStatus,
        source: Option<Arc<AudioSource>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut source_changed = false;
        if let Some(source) = source
            && self
                .source
                .as_ref()
                .is_none_or(|old| old.epoch != source.epoch)
        {
            source_changed = true;
            self.particles
                .reset_registry(source.registry.definitions().len());
            self.source = Some(source);
        }
        let running_changed = self.running != status.is_running();
        self.running = status.is_running();
        let was_initialized = self.particles.initialized;
        if let Some(source) = &self.source {
            let freeze = source.freeze_time.load(Ordering::SeqCst);
            if !self.running && freeze != 0 {
                self.particles.freeze_at(source, MachNanos::new(freeze));
            }
        }
        let had_animation = self.animation.is_some();
        self.update_animation(window, cx);
        if window.is_visible()
            && (source_changed || running_changed || was_initialized != self.particles.initialized)
            && (had_animation || self.animation.is_none())
        {
            cx.notify();
        }
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

    /// The world time at the latest accepted frame, advanced by the wall time since that
    /// frame's header so the drift is smooth between audio callbacks.
    fn world_seconds(&self) -> f64 {
        let Some(header) = self.particles.state else {
            return 0.0;
        };
        let elapsed = self
            .particles
            .present
            .get()
            .saturating_sub(header.present.get());
        header.world.get() + elapsed as f64 / 1e9
    }

    fn paint(&self, bounds: Bounds<Pixels>, dark: bool, window: &mut Window) {
        if !is_drawable_viewport(bounds) {
            return;
        }
        // The bottom layer: under the lights and particles, over the glass.
        if let Some(image) = &self.background {
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                paint_background(window, bounds, image);
            });
        }
        let Some(source) = &self.source else {
            return;
        };
        let scale = layout_scale(bounds);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let mut colors = [[0.0_f32; 3]; backdrop::MAX_SITUATIONS];
            for (slot, definition) in colors.iter_mut().zip(source.registry.definitions()) {
                *slot = definition.visual.color;
            }
            // A rock's lights are angular; every other situation's are blurred and round.
            let mut angular = [false; backdrop::MAX_SITUATIONS];
            for (slot, definition) in angular.iter_mut().zip(source.registry.definitions()) {
                *slot = definition.visual.shape == VisualShape::Impact;
            }
            let field = backdrop::backdrop(
                &backdrop::Input {
                    world_seconds: self.world_seconds(),
                    contributions: &self.particles.contributions,
                    breath: &self.particles.breath,
                    colors: &colors,
                    size: [bounds.size.width.into(), bounds.size.height.into()],
                    dark,
                    angular: &angular,
                },
                self.intensity,
                self.motion,
            );
            for item in field.items() {
                paint_soft(window, bounds, item);
            }
            for (index, definition) in source.registry.definitions().iter().enumerate() {
                let intensity = self.particles.contributions[index] * self.particles.breath[index];
                if intensity > 0.0 {
                    let radius = scale
                        * self.motion.glow_radius(
                            self.particles.contributions[index],
                            self.particles.breath[index],
                        );
                    if definition.visual.shape == VisualShape::Impact {
                        // A rock's ambient light has no round edge either.
                        let centre = disc_bounds(bounds, [0.5, 0.58], [0.0, 0.0], 0.0).origin;
                        paint_angular_light(
                            window,
                            centre,
                            radius * 0.8,
                            index as u32 + 1,
                            definition.visual.color,
                            intensity * 0.025,
                        );
                    } else {
                        paint_glow(
                            window,
                            bounds,
                            [0.5, 0.58],
                            radius,
                            definition.visual.color,
                            intensity * 0.025,
                        );
                    }
                }
            }
            // When many particles are alive each gets fewer decoration marks, and the frame has a
            // fixed number of blurred shadows; the core light is always the first mark, so it is
            // never the one given up.
            let live = self
                .particles
                .events
                .iter()
                .flatten()
                .filter(|event| self.particles.particle_frame(event, self.motion).is_some())
                .count();
            let mut budget = Budget::new(live);
            for event in self.particles.events.iter().flatten() {
                let Some((age, opacity)) = self.particles.particle_frame(event, self.motion) else {
                    continue;
                };
                let position = [event.x.get(), event.y.get()];
                let color = source.registry.definitions()[event.situation.get()]
                    .visual
                    .color;
                let look = look(
                    &ParticleInput {
                        shape: event.shape,
                        age,
                        opacity,
                        radius: event.radius.get(),
                        strength: event.strength.get(),
                        serial: event.serial,
                        rgb: color,
                        large: event.large,
                        intensity: self.intensity,
                        dark,
                    },
                    self.motion,
                );
                for (index, mark) in look.marks().enumerate() {
                    if budget.allows(index, mark) {
                        paint_mark(window, bounds, position, &mark.scaled(scale));
                    }
                }
            }
        });
    }
}

/// The shortest side of the viewport in logical pixels over the default scene's 600: how much
/// bigger or smaller than the default window every light, glow and ripple is drawn. The backdrop
/// already sizes its lights from the same short side. Positions are normalized and stretch to the
/// window's aspect ratio, so only the sizes follow this, and a circle is never drawn as an ellipse.
fn layout_scale(viewport: Bounds<Pixels>) -> f32 {
    let short = f32::from(viewport.size.width.min(viewport.size.height));
    short / DEFAULT_SHORT_SIDE
}

const DEFAULT_SHORT_SIDE: f32 = 600.0;

/// Draws the background image over the whole viewport, aspect kept and the middle shown.
fn paint_background(window: &mut Window, bounds: Bounds<Pixels>, image: &Arc<RenderImage>) {
    let pixels = image.size(0);
    let Some((x, y, width, height)) = cover_rect(
        pixels.width.0 as f32,
        pixels.height.0 as f32,
        bounds.size.width.into(),
        bounds.size.height.into(),
    ) else {
        return;
    };
    let image_bounds = Bounds::new(
        point(bounds.origin.x + px(x), bounds.origin.y + px(y)),
        size(px(width), px(height)),
    );
    if let Err(error) = window.paint_image(
        bounds,
        image_bounds,
        Corners::default(),
        image.clone(),
        0,
        false,
    ) {
        eprintln!("Ambien: 背景画像を描けません: {error}");
    }
}

fn is_drawable_viewport(bounds: Bounds<Pixels>) -> bool {
    bounds.size.width > px(0.0)
        && bounds.size.height > px(0.0)
        && [
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
            bounds.origin.x + bounds.size.width,
            bounds.origin.y + bounds.size.height,
        ]
        .into_iter()
        .all(|value| f64::from(value).is_finite())
}

fn glow_bounds(viewport: Bounds<Pixels>, position: [f32; 2], radius: f32) -> Bounds<Pixels> {
    let center = point(
        viewport.origin.x + viewport.size.width * position[0],
        viewport.origin.y + viewport.size.height * position[1],
    );
    Bounds::new(
        center - point(px(radius), px(radius)),
        size(px(radius * 2.0), px(radius * 2.0)),
    )
}

fn paint_glow(
    window: &mut Window,
    viewport: Bounds<Pixels>,
    position: [f32; 2],
    radius: f32,
    rgb: [f32; 3],
    opacity: f32,
) {
    paint_glow_at(window, viewport, position, [0.0, 0.0], radius, rgb, opacity);
}

fn rgba(rgb: [f32; 3], opacity: f32) -> gpui::Hsla {
    Rgba {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
        a: opacity,
    }
    .into()
}

/// Bounds of a disc centred on a normalized position plus a logical-pixel offset.
fn disc_bounds(
    viewport: Bounds<Pixels>,
    position: [f32; 2],
    offset: [f32; 2],
    radius: f32,
) -> Bounds<Pixels> {
    let bounds = glow_bounds(viewport, position, radius);
    Bounds::new(
        bounds.origin + point(px(offset[0]), px(offset[1])),
        bounds.size,
    )
}

/// One soft backdrop light. GPUI's shadow is a Gaussian whose sigma is `blur_radius`, so the
/// shadow alone is the whole light: a hard-edged disc on top would show as a crisp circle. Plain
/// alpha, never additive, so it reads the same on a light or a dark window.
fn paint_soft(window: &mut Window, viewport: Bounds<Pixels>, item: &backdrop::Item) {
    if item.angular {
        let centre = disc_bounds(viewport, item.center, [0.0, 0.0], 0.0).origin;
        paint_angular_light(
            window,
            centre,
            item.radius + item.blur * 0.5,
            item.seed,
            item.rgb,
            item.opacity,
        );
        return;
    }
    let bounds = disc_bounds(viewport, item.center, [0.0, 0.0], item.radius);
    window.paint_drop_shadows(
        bounds,
        px(item.radius).into(),
        &[BoxShadow {
            color: rgba(item.rgb, item.opacity),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(item.blur),
            spread_radius: px(0.0),
            inset: false,
        }],
    );
}

fn paint_glow_at(
    window: &mut Window,
    viewport: Bounds<Pixels>,
    position: [f32; 2],
    offset: [f32; 2],
    radius: f32,
    rgb: [f32; 3],
    opacity: f32,
) {
    let color = rgba(rgb, opacity);
    let bounds = disc_bounds(viewport, position, offset, radius);
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

fn paint_mark(window: &mut Window, viewport: Bounds<Pixels>, position: [f32; 2], mark: &Mark) {
    match *mark {
        Mark::Glow {
            offset,
            radius,
            rgb,
            opacity,
        } => paint_glow_at(window, viewport, position, offset, radius, rgb, opacity),
        Mark::Haze {
            offset,
            radius,
            rgb,
            opacity,
        } => {
            let bounds = disc_bounds(viewport, position, offset, radius);
            window.paint_drop_shadows(
                bounds,
                px(radius).into(),
                &[BoxShadow {
                    color: rgba(rgb, opacity),
                    offset: point(px(0.0), px(0.0)),
                    blur_radius: px(radius * 0.6),
                    spread_radius: px(0.0),
                    inset: false,
                }],
            );
        }
        Mark::Ring {
            offset,
            radius,
            width,
            rgb,
            opacity,
        } => {
            let bounds = disc_bounds(viewport, position, offset, radius);
            window.paint_quad(
                outline(bounds, rgba(rgb, opacity), BorderStyle::default())
                    .border_widths(px(width))
                    .corner_radii(px(radius)),
            );
        }
        Mark::Dot {
            offset,
            radius,
            rgb,
            opacity,
        } => {
            let bounds = disc_bounds(viewport, position, offset, radius);
            window.paint_quad(fill(bounds, rgba(rgb, opacity)).corner_radii(px(radius)));
        }
        Mark::Cross {
            arm,
            width,
            rgb,
            opacity,
        } => {
            let color = rgba(rgb, opacity);
            let centre = disc_bounds(viewport, position, [0.0, 0.0], 0.0).origin;
            window.paint_quad(fill(
                Bounds::new(
                    centre - point(px(arm), px(width / 2.0)),
                    size(px(arm * 2.0), px(width)),
                ),
                color,
            ));
            window.paint_quad(fill(
                Bounds::new(
                    centre - point(px(width / 2.0), px(arm)),
                    size(px(width), px(arm * 2.0)),
                ),
                color,
            ));
        }
        Mark::Shards {
            arm,
            width,
            seed,
            rgb,
            opacity,
        } => {
            let centre = disc_bounds(viewport, position, [0.0, 0.0], 0.0).origin;
            for (angle, share) in spikes(seed) {
                paint_spike(window, centre, angle, arm * share, width, rgb, opacity);
            }
        }
        Mark::Poly { .. } => paint_poly(window, viewport, position, mark),
    }
}

/// One spike of a rock's glint: a thin diamond from just behind `centre` out to `length` along
/// `angle`, `width` across at its widest. A path that cannot be built is skipped.
fn paint_spike(
    window: &mut Window,
    centre: gpui::Point<Pixels>,
    angle: f32,
    length: f32,
    width: f32,
    rgb: [f32; 3],
    opacity: f32,
) {
    let (sin, cos) = angle.sin_cos();
    let along = |distance: f32, across: f32| {
        centre
            + point(
                px(cos * distance - sin * across),
                px(sin * distance + cos * across),
            )
    };
    let mut builder = PathBuilder::fill();
    builder.move_to(along(length, 0.0));
    builder.line_to(along(length * 0.18, width * 0.5));
    builder.line_to(along(-length * 0.14, 0.0));
    builder.line_to(along(length * 0.18, -width * 0.5));
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, rgba(rgb, opacity));
    }
}

/// A jagged polygon mark, filled or as an outline, drawn as a GPUI path.
fn paint_poly(window: &mut Window, viewport: Bounds<Pixels>, position: [f32; 2], mark: &Mark) {
    let Mark::Poly {
        offset,
        radius,
        sides,
        rotation,
        seed,
        stroke,
        rgb,
        opacity,
    } = *mark
    else {
        return;
    };
    let centre = disc_bounds(viewport, position, offset, 0.0).origin;
    paint_polygon(
        window,
        centre,
        sides,
        [radius, rotation],
        seed,
        stroke,
        rgb,
        opacity,
    );
}

/// One closed polygon path at `centre`. `sides` is the corner count, `size` the radius and
/// rotation, `seed` shapes the corners. A path that cannot be built (the tessellator
/// refused it) is skipped; it is only decoration.
#[allow(clippy::too_many_arguments)]
fn paint_polygon(
    window: &mut Window,
    centre: gpui::Point<Pixels>,
    sides: u8,
    size: [f32; 2],
    seed: u32,
    stroke: f32,
    rgb: [f32; 3],
    opacity: f32,
) {
    let (corners, count) = vertices(sides, size[0], size[1], seed);
    let mut builder = if stroke > 0.0 {
        PathBuilder::stroke(px(stroke))
    } else {
        PathBuilder::fill()
    };
    for (index, corner) in corners[..count].iter().enumerate() {
        let at = centre + point(px(corner[0]), px(corner[1]));
        if index == 0 {
            builder.move_to(at);
        } else {
            builder.line_to(at);
        }
    }
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, rgba(rgb, opacity));
    }
}

/// A light with no round edge: nested filled polygons of one shape, each a little fainter in
/// proportion to its size, so the light is brightest at the centre and steps out in hard corners.
/// It stands in for the blurred shadow for a rock's lights.
fn paint_angular_light(
    window: &mut Window,
    centre: gpui::Point<Pixels>,
    radius: f32,
    seed: u32,
    rgb: [f32; 3],
    opacity: f32,
) {
    let turn = (seed as f32 / u32::MAX as f32) * std::f32::consts::TAU;
    for (scale, share) in [(2.0, 0.4), (1.4, 0.4), (0.85, 0.4)] {
        paint_polygon(
            window,
            centre,
            6,
            [radius * scale, turn],
            seed,
            0.0,
            rgb,
            opacity * share,
        );
    }
}

/// Opacity of the theme-coloured film laid over the scene. Reduce transparency lays it fully
/// opaque; otherwise it is the user's glass density, 0 meaning no film at all.
fn film_alpha(opaque: bool, veil: f32) -> f32 {
    if opaque { 1.0 } else { veil.clamp(0.0, 1.0) }
}

impl Render for SceneView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.weak_entity();
        let dark = cx.theme().is_dark();
        let root = div()
            .size_full()
            .track_focus(&self.focus)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move());
        // Normally no background is painted and the native blurred surface shows through.
        // The colour film (the user's glass density) and reduce transparency's opaque palette
        // are both the theme background, differing only in how much of it is laid over.
        let alpha = film_alpha(self.opaque, self.veil);
        let root = if alpha > 0.0 {
            root.bg(cx.theme().colors.bg.opacity(alpha))
        } else {
            root
        };
        root.child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, cx| {
                    // Borrow the fixed cache during paint; don't clone a particle vector each frame.
                    let _ = view.update(cx, |view, _| view.paint(bounds, dark, window));
                },
            )
            .size_full(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::situations::VisualShape;
    use crate::{
        audio::{
            timing::{MachClock, StreamEpoch, WorldSeconds},
            transport::{
                ControlMailbox, ErrorCell, LogicalPixels, StatusMailbox, VisualRing,
                WorldVisualState,
            },
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
            receipts: Arc::new(StatusMailbox::new(registry.definitions().len())),
            registry,
            epoch,
            clock: MachClock::new().unwrap(),
            errors: Arc::new(ErrorCell::new(epoch)),
            freeze_time: AtomicU64::new(0),
        }
    }

    fn publish_state(
        source: &AudioSource,
        world: f64,
        present: u64,
        phase: u32,
        gain: f32,
        breath: f32,
    ) {
        let count = source.registry.definitions().len();
        let gains = vec![gain; count];
        let breaths = vec![breath; count];
        assert!(source.visuals.push_state(WorldVisualState {
            epoch: source.epoch,
            world: WorldSeconds::new(world).unwrap(),
            present: MachNanos::new(present),
            phase,
            gains: &gains,
            breath_intensity: &breaths,
        }));
    }

    fn publish(source: &AudioSource, world: f64, present: u64, gain: f32, breath: f32) {
        publish_state(source, world, present, 0, gain, breath);
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
            large: true,
            lifetime: Duration::from_nanos(50),
            serial: 0,
        }));
    }

    #[test]
    fn the_colour_film_is_absent_at_zero_follows_the_slider_and_is_opaque_under_reduce_transparency()
     {
        // Zero density lays nothing over the scene: the glass looks as it always did.
        assert_eq!(film_alpha(false, 0.0), 0.0);
        assert_eq!(film_alpha(false, 0.35), 0.35);
        assert_eq!(film_alpha(false, 1.0), 1.0);
        // The system setting wins whatever the slider says.
        assert_eq!(film_alpha(true, 0.0), 1.0);
        assert_eq!(film_alpha(true, 0.35), 1.0);
        // A stray out-of-range value cannot lay a negative or over-full film.
        assert_eq!(film_alpha(false, -2.0), 0.0);
        assert_eq!(film_alpha(false, 9.0), 1.0);
    }

    #[test]
    fn p08_presented_power_interpolates_without_advancing_future_particle_births() {
        let mut source = source();
        let count = source.registry.definitions().len();
        source.visuals = Arc::new(VisualRing::new(8, count));
        let rain = source.registry.first_index();
        let rock = source.registry.resolve("rock").unwrap();
        let mut gains = vec![0.0; count];
        let breaths = vec![0.25; count];
        for (world, present, target) in [(10.0, 100, rain), (20.0, 200, rock)] {
            gains.fill(0.0);
            gains[target.get()] = 1.0;
            assert!(source.visuals.push_state(WorldVisualState {
                epoch: source.epoch,
                world: WorldSeconds::new(world).unwrap(),
                present: MachNanos::new(present),
                phase: 1,
                gains: &gains,
                breath_intensity: &breaths,
            }));
        }
        let event = |situation, present| VisualEvent {
            epoch: source.epoch,
            world: WorldSeconds::new(10.0).unwrap(),
            present: MachNanos::new(present),
            situation,
            x: Normalized::new(0.25).unwrap(),
            y: Normalized::new(0.75).unwrap(),
            strength: Normalized::ONE,
            radius: LogicalPixels::new(5.0).unwrap(),
            shape: VisualShape::Point,
            large: true,
            lifetime: Duration::from_nanos(200),
            serial: 0,
        };
        let departed = event(rain, 100);
        let arriving = event(rock, 100);
        let future = event(rock, 175);
        for particle in [departed, arriving, future] {
            assert!(source.visuals.push_event(particle));
        }
        let mut cache = VisibleParticles::new(count);
        cache.refresh_at(&source, MachNanos::new(150));
        // Halfway power is .5/.5, not the square of linearly mixed amplitudes.
        for particle in [departed, arriving] {
            assert!(
                (cache.particle_frame(&particle, Motion::Full).unwrap().1 - 0.28125).abs() < 1e-7
            );
        }
        assert!(
            !cache
                .events
                .iter()
                .flatten()
                .any(|event| event.present == future.present)
        );
        cache.refresh_at(&source, MachNanos::new(175));
        assert!(
            cache
                .events
                .iter()
                .flatten()
                .any(|event| event.present == future.present)
        );
        assert!((cache.particle_frame(&future, Motion::Full).unwrap().1 - 0.75).abs() < 1e-7);
    }

    #[test]
    fn p08_breath_and_power_interpolate_without_adopting_a_future_header() {
        let source = source();
        publish_state(&source, 1.0, 100, 2, 0.2, 0.1);
        publish_state(&source, 2.0, 200, 2, 0.8, 0.7);
        let mut cache = VisibleParticles::new(source.registry.definitions().len());
        cache.refresh_at(&source, MachNanos::new(125));
        assert!((cache.contributions[0] - 0.19).abs() < 1e-6);
        assert!((cache.breath[0] - 0.25).abs() < 1e-6);
        assert!((cache.contributions[0] * cache.breath[0] - 0.0475).abs() < 1e-6);
        let header = cache.state.unwrap();
        assert_eq!(header.present, MachNanos::new(100));
        assert_eq!(header.world, WorldSeconds::new(1.0).unwrap());
        assert_eq!(header.phase, 2);
        cache.refresh_at(&source, MachNanos::new(250));
        assert!((cache.contributions[0] - 0.64).abs() < 1e-6);
        assert!((cache.breath[0] - 0.7).abs() < 1e-6);
    }

    #[test]
    fn p08_phase_boundary_holds_until_its_presentation_time() {
        let source = source();
        publish_state(&source, 1.0, 100, 0, 0.4, 0.3);
        publish_state(&source, 2.0, 200, 1, 1.0, 0.9);
        publish_state(&source, 3.0, 300, 0, 0.8, 0.6);
        let mut cache = VisibleParticles::new(source.registry.definitions().len());
        cache.refresh_at(&source, MachNanos::new(150));
        assert!((cache.contributions[0] - 0.16).abs() < 1e-6);
        assert_eq!(cache.breath[0], 0.3);
        assert_eq!(cache.state.unwrap().phase, 0);
        cache.refresh_at(&source, MachNanos::new(200));
        assert_eq!(cache.contributions[0], 1.0);
        assert_eq!(cache.breath[0], 0.9);
        assert_eq!(cache.state.unwrap().phase, 1);
    }

    #[test]
    fn p08_future_only_state_does_not_light_a_new_scene() {
        let source = source();
        publish(&source, 1.0, 100, 1.0, 0.8);
        let mut cache = VisibleParticles::new(source.registry.definitions().len());
        cache.refresh_at(&source, MachNanos::new(99));
        assert!(cache.state.is_none());
        assert!(cache.contributions.iter().all(|power| *power == 0.0));
        assert!(cache.breath.iter().all(|breath| *breath == 0.0));
        assert!(cache.events.iter().all(Option::is_none));
        cache.refresh_at(&source, MachNanos::new(100));
        assert_eq!(cache.contributions[0], 1.0);
        assert!(cache.events.iter().any(Option::is_some));
    }

    #[test]
    fn p08_epoch_reset_clears_old_colors_and_live_particles_before_new_state() {
        let source = source();
        let count = source.registry.definitions().len();
        publish(&source, 1.0, 100, 1.0, 0.8);
        let mut cache = VisibleParticles::new(count);
        cache.refresh_at(&source, MachNanos::new(110));
        assert!(cache.events.iter().any(Option::is_some));
        let mut replacement = self::source();
        replacement.epoch = StreamEpoch::new(2).unwrap();
        replacement.visuals = Arc::clone(&source.visuals);
        publish_state(&replacement, 2.0, 120, 0, 0.5, 0.4);
        cache.reset_registry(count);
        cache.refresh_at(&replacement, MachNanos::new(115));
        assert!(cache.state.is_none());
        assert!(cache.contributions.iter().all(|power| *power == 0.0));
        assert!(cache.breath.iter().all(|breath| *breath == 0.0));
        assert!(cache.events.iter().all(Option::is_none));
        cache.refresh_at(&replacement, MachNanos::new(120));
        assert_eq!(cache.state.unwrap().epoch, replacement.epoch);
        assert_eq!(cache.contributions[0], 0.25);
        assert!(cache.events.iter().all(Option::is_none));
    }

    #[test]
    fn p08_restore_reads_only_currently_live_particles_and_holds_future_births() {
        let mut source = source();
        let count = source.registry.definitions().len();
        source.visuals = Arc::new(VisualRing::new(8, count));
        publish(&source, 1.0, 10, 1.0, 0.2);
        let mut cache = VisibleParticles::new(count);
        cache.refresh_at(&source, MachNanos::new(40));
        assert_eq!(cache.events.iter().flatten().count(), 1);
        // No scene refresh occurs while hidden; the audio producer keeps publishing.
        publish(&source, 2.0, 95, 0.5, 0.4);
        publish(&source, 3.0, 140, 0.5, 0.4);
        cache.refresh_at(&source, MachNanos::new(100));
        let live: Vec<_> = cache.events.iter().flatten().collect();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].present, MachNanos::new(95));
        assert_eq!(cache.contributions[0], 0.25);
    }

    #[test]
    fn p08_empty_and_nonfinite_viewports_suspend_painting() {
        for (x, y, width, height) in [
            (0.0, 0.0, 0.0, 600.0),
            (0.0, 0.0, 960.0, 0.0),
            (0.0, 0.0, -1.0, 600.0),
            (0.0, 0.0, f32::NAN, 600.0),
            (0.0, 0.0, 960.0, f32::INFINITY),
            (f32::INFINITY, 0.0, 960.0, 600.0),
            (0.0, f32::NAN, 960.0, 600.0),
        ] {
            assert!(!is_drawable_viewport(Bounds::new(
                point(px(x), px(y)),
                size(px(width), px(height)),
            )));
        }
    }

    #[test]
    fn p08_latest_viewport_places_normalized_particles_with_a_logical_circle() {
        let original = Bounds::new(point(px(0.0), px(0.0)), size(px(960.0), px(600.0)));
        let current = Bounds::new(point(px(20.0), px(30.0)), size(px(480.0), px(240.0)));
        assert!(is_drawable_viewport(current));
        let first = glow_bounds(original, [0.25, 0.75], 5.0);
        let latest = glow_bounds(current, [0.25, 0.75], 5.0);
        assert_eq!(first.origin, point(px(235.0), px(445.0)));
        assert_eq!(latest.origin, point(px(135.0), px(205.0)));
        assert_eq!(latest.size, size(px(10.0), px(10.0)));
        assert_eq!(first.size, latest.size);
        // Edge glows remain circular; the painting viewport clips their overflow.
        let edge = glow_bounds(current, [0.0, 1.0], 5.0);
        assert_eq!(edge.origin, point(px(15.0), px(265.0)));
        assert_eq!(edge.size, size(px(10.0), px(10.0)));
    }

    #[test]
    fn reduced_motion_never_starts_a_particle_at_full_brightness() {
        let source = source();
        publish(&source, 1.0, 100, 1.0, 0.8);
        let mut cache = VisibleParticles::new(source.registry.definitions().len());
        cache.refresh_at(&source, MachNanos::new(100));
        let particle = cache.events[0].unwrap();
        assert_eq!(
            cache.particle_frame(&particle, Motion::Full).unwrap().1,
            1.0
        );
        assert_eq!(cache.particle_frame(&particle, Motion::Reduced), None);
        // Twelve of its fifty nanoseconds later it has softly faded in, never past full.
        cache.refresh_at(&source, MachNanos::new(112));
        let (_, soft) = cache.particle_frame(&particle, Motion::Reduced).unwrap();
        assert!(soft > 0.0 && soft <= 1.0);
    }

    #[test]
    fn a_new_viewport_keeps_every_particle_at_the_same_normalized_position_and_size() {
        let narrow = Bounds::new(point(px(10.0), px(20.0)), size(px(300.0), px(200.0)));
        let wide = Bounds::new(point(px(-50.0), px(5.0)), size(px(1200.0), px(800.0)));
        for position in [[0.0, 0.0], [0.25, 0.5], [1.0, 1.0]] {
            let first = glow_bounds(narrow, position, 3.5);
            let second = glow_bounds(wide, position, 3.5);
            assert_eq!(first.size, second.size, "logical radius is not scaled");
            for (viewport, glow) in [(narrow, first), (wide, second)] {
                let center = glow.center();
                let normalized_x = (center.x - viewport.origin.x) / viewport.size.width;
                let normalized_y = (center.y - viewport.origin.y) / viewport.size.height;
                assert!((normalized_x - position[0]).abs() < 1e-5);
                assert!((normalized_y - position[1]).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn sizes_follow_the_short_side_so_the_default_window_is_unchanged_and_circles_stay_round() {
        let of =
            |w: f32, h: f32| layout_scale(Bounds::new(point(px(0.0), px(0.0)), size(px(w), px(h))));
        // The default window draws exactly what it always drew.
        assert_eq!(of(960.0, 600.0), 1.0);
        // Half the default, and a window narrower than it is tall: the short side decides.
        assert_eq!(of(480.0, 300.0), 0.5);
        assert_eq!(of(300.0, 900.0), 0.5);
        // A wide window only spreads the world; it does not make the lights bigger.
        assert_eq!(of(2400.0, 600.0), 1.0);
        // A full-screen 16:9 window.
        assert_eq!(of(1920.0, 1080.0), 1.8);
    }

    #[test]
    fn output_failure_preserves_the_already_accepted_frame_not_a_newer_ring_snapshot() {
        let source = source();
        publish(&source, 1.0, 10, 0.5, 0.25);
        let mut cache = VisibleParticles::new(source.registry.definitions().len());
        cache.refresh_at(&source, MachNanos::new(40));
        let header = cache.state;
        let particle = cache.events[0].unwrap();
        let accepted_particle_frame = cache.particle_frame(&particle, Motion::Full).unwrap();
        assert!((accepted_particle_frame.1 - 0.04).abs() < 1e-7);
        let accepted_contributions = cache.contributions.clone();
        let accepted_breath = cache.breath.clone();
        publish(&source, 2.0, 41, 0.5, 0.75);
        cache.freeze_at(&source, MachNanos::new(99));
        assert_eq!(cache.present, MachNanos::new(40));
        assert_eq!(cache.state, header);
        assert_eq!(
            cache.events[0].map(|event| event.serial),
            Some(particle.serial)
        );
        assert_eq!(
            cache.particle_frame(&particle, Motion::Full),
            Some(accepted_particle_frame)
        );
        assert_eq!(cache.contributions, accepted_contributions);
        assert_eq!(cache.breath, accepted_breath);
    }

    #[test]
    fn a_new_frozen_scene_initializes_once_at_the_worker_checkpoint() {
        let source = source();
        publish(&source, 2.0, 41, 0.5, 0.75);
        let mut cache = VisibleParticles::new(source.registry.definitions().len());
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
        assert!(cache.contributions.iter().all(|power| *power == 0.25));
        assert!(cache.breath.iter().all(|breath| *breath == 0.75));
    }
}
