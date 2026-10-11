use std::sync::Arc;

use ely::{
    buttons::SegmentedControl,
    forms::{Radio, Slider, Switch},
    primitives::FocusScope,
    theme::ActiveTheme,
    typography::{Caption, Paragraph},
};
use gpui::{
    AppContext, Context, DragMoveEvent, ElementId, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Render, Role, ScrollHandle, ScrollWheelEvent, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};

use crate::{
    app::dispatch_control_edit,
    audio::{AudioSource, OutputStatus, transport::ControlReceipt},
    controls::{ControlEdit, ControlState, Normalized, VisualIntensity},
    situations::{KnobIndex, Registry, SituationIndex},
};

struct KnobControl {
    index: KnobIndex,
    owner: SituationIndex,
    id: ElementId,
    label: &'static str,
}

struct SelectionActivation {
    key: &'static str,
    target: SituationIndex,
    focus: FocusHandle,
    _blur: Subscription,
}

#[derive(Clone, Default, PartialEq)]
pub(crate) struct SettingsNotices {
    pub(crate) control: Option<SharedString>,
    pub(crate) persistence: Option<SharedString>,
    /// The system's "reduce transparency" is on and overrides the blur and glass density.
    pub(crate) reduce_transparency: bool,
}

pub(crate) struct SettingsView {
    focus: FocusHandle,
    audio_status: SharedString,
    /// Whether the output is running with nothing to report; anything else is shown as a notice.
    output_normal: bool,
    registry: Option<Arc<Registry>>,
    controls: Option<ControlState>,
    knobs: Vec<KnobControl>,
    receipt: Option<ControlReceipt>,
    notices: SettingsNotices,
    selection_activation: Option<SelectionActivation>,
    scroll: ScrollHandle,
    /// Where on the thumb the pointer took hold, in pixels from its top, while it is dragged.
    scrollbar_grab: f32,
    _display: Subscription,
}

impl SettingsView {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        status: &OutputStatus,
        source: Option<Arc<AudioSource>>,
        controls: Option<&ControlState>,
        receipt: Option<ControlReceipt>,
        notices: SettingsNotices,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        // Only the display is reported; the app refits when it differs from the last one.
        let display = cx.observe_window_bounds(window, |_, window, cx| {
            crate::app::dispatch_settings_display(
                window.window_handle().window_id(),
                crate::platform::macos::capture_placement(window).map(|(_, display)| display),
                cx,
            );
        });
        let mut view = Self {
            focus,
            audio_status: status.text().into(),
            output_normal: status.is_normal(),
            registry: None,
            controls: None,
            knobs: Vec::new(),
            receipt,
            notices,
            selection_activation: None,
            scroll: ScrollHandle::new(),
            scrollbar_grab: 0.0,
            _display: display,
        };
        view.initialize_controls(source.as_deref(), controls);
        view
    }

    fn initialize_controls(
        &mut self,
        source: Option<&AudioSource>,
        controls: Option<&ControlState>,
    ) {
        let (Some(source), Some(controls)) = (source, controls) else {
            return;
        };
        if self.controls.is_some() {
            return;
        }
        self.registry = Some(source.registry.clone());
        // Allocate one cache for this window; subsequent edits copy into its storage.
        self.controls = Some(controls.clone());
        for definition in source.registry.definitions() {
            let owner = source
                .registry
                .resolve(definition.id)
                .expect("validated registry resolves its own IDs");
            for (knob, flat) in definition
                .knobs
                .iter()
                .zip(source.registry.knob_range(owner))
            {
                let id: ElementId = (ElementId::from(definition.id), knob.id).into();
                self.knobs.push(KnobControl {
                    index: KnobIndex::from_index(flat, controls.knobs.len())
                        .expect("validated registry range fits controls"),
                    owner,
                    id: id.clone(),
                    label: knob.label,
                });
            }
        }
    }

    pub(crate) fn update_audio(
        &mut self,
        status: &OutputStatus,
        source: Option<Arc<AudioSource>>,
        controls: Option<&ControlState>,
        receipt: Option<ControlReceipt>,
        notices: SettingsNotices,
        cx: &mut Context<Self>,
    ) {
        let mut changed =
            self.audio_status.as_ref() != status.text().as_str() || self.notices != notices;
        if self.controls.is_none() && controls.is_some() && source.is_some() {
            self.initialize_controls(source.as_deref(), controls);
            changed = true;
        } else if let (Some(cache), Some(latest)) = (&mut self.controls, controls)
            && cache.settings_revision != latest.settings_revision
        {
            cache.copy_values_from(latest);
            changed = true;
        }
        // World seconds are a sample at receipt time, never a settings animation clock.
        let receipt_changed = match (self.receipt, receipt) {
            (Some(old), Some(new)) => {
                old.epoch != new.epoch
                    || old.ack_revision != new.ack_revision
                    || old.ack_manual_revision != new.ack_manual_revision
                    || old.display_dominant != new.display_dominant
            }
            (None, None) => false,
            _ => true,
        };
        if receipt_changed {
            self.receipt = receipt;
            changed = true;
        }
        if changed {
            self.audio_status = status.text().into();
            self.output_normal = status.is_normal();
            self.notices = notices;
            cx.notify();
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().colors.bg;
        // The title bar already says "設定"; the body starts with the first section.
        let mut content = div().w_full().flex().flex_col().gap_5().px_6().py_5();

        // Only a problem gets words at the top: a failed save, a rejected edit, or an output that
        // is not running normally.
        let mut notices = div().flex().flex_col().gap_2();
        let mut has_notice = false;
        if let Some(status) = &self.notices.persistence {
            has_notice = true;
            notices = notices.child(
                div()
                    .id("persistence-status")
                    .role(Role::Label)
                    .aria_label(status.clone())
                    .child(Paragraph::new(status.clone())),
            );
        }
        if let Some(error) = &self.notices.control {
            has_notice = true;
            notices = notices.child(Paragraph::new(format!("設定を変更できません: {error}")));
        }
        if !self.output_normal {
            has_notice = true;
            notices = notices.child(Paragraph::new(self.audio_status.clone()));
        }
        if has_notice {
            content = content.child(notices);
        }

        if let (Some(registry), Some(controls)) = (&self.registry, &self.controls) {
            let display_target = self.receipt.map_or(controls.manual_target, |receipt| {
                controls.display_target(receipt.ack_manual_revision, receipt.display_dominant)
            });
            let mut selector = div()
                .id("situation-selector")
                .role(Role::Group)
                .aria_label("シチュエーション")
                .flex()
                .gap_4();
            for (position, definition) in registry.definitions().iter().enumerate() {
                let target = SituationIndex::from_index(position, registry.definitions().len())
                    .expect("validated registry position");
                let radio_id: ElementId = (ElementId::from(definition.id), "selection").into();
                selector = selector.child(
                    div()
                        .id((radio_id.clone(), "activation"))
                        .on_click(cx.listener(move |view, _, _, cx| {
                            cx.stop_propagation();
                            view.selection_activation = None;
                            // Re-selecting the same real choice is a new manual intent.
                            dispatch_control_edit(ControlEdit::Select(target), cx);
                        }))
                        .on_key_down(cx.listener(
                            move |view, event: &gpui::KeyDownEvent, window, cx| {
                                view.selection_activation = None;
                                let key = match event.keystroke.key.as_str() {
                                    "enter" => "enter",
                                    "space" => "space",
                                    _ => return,
                                };
                                if event.keystroke.modifiers.modified()
                                    || window.default_prevented()
                                {
                                    return;
                                }
                                if let Some(focus) = window.focused(cx) {
                                    let blur = cx.on_blur(&focus, window, |view, _, _| {
                                        view.selection_activation = None;
                                    });
                                    view.selection_activation = Some(SelectionActivation {
                                        key,
                                        target,
                                        focus,
                                        _blur: blur,
                                    });
                                }
                            },
                        ))
                        .on_key_up(cx.listener(|view, event: &gpui::KeyUpEvent, window, cx| {
                            let Some(activation) = view.selection_activation.take() else {
                                return;
                            };
                            if activation.key == event.keystroke.key.as_str()
                                && !event.keystroke.modifiers.modified()
                                && !window.default_prevented()
                                && activation.focus.is_focused(window)
                            {
                                dispatch_control_edit(ControlEdit::Select(activation.target), cx);
                            }
                        }))
                        .child(
                            Radio::new(radio_id, target == display_target).label(definition.label),
                        ),
                );
            }
            let mut situation = section("シチュエーション").child(selector);
            for knob in self
                .knobs
                .iter()
                .filter(|knob| knob.owner == display_target)
            {
                let index = knob.index;
                let value = controls.knobs[index.get()].get();
                situation = situation.child(slider_row(
                    knob.label,
                    format!("{:.0}%", value * 100.0),
                    (knob.id.clone(), "a11y").into(),
                    Slider::new(knob.id.clone(), f64::from(value) * 100.0)
                        .range(0.0, 100.0)
                        .step(1.0)
                        .on_change(cx.processor(move |_, value: f64, _, cx| {
                            if let Some(value) = Normalized::new((value / 100.0) as f32) {
                                dispatch_control_edit(ControlEdit::Knob(index, value), cx);
                            }
                        })),
                ));
            }
            content = content.child(situation);

            let sound =
                section("音")
                    .child(slider_row(
                        "アプリの音量",
                        format!("{:.0}%", controls.volume.get() * 100.0),
                        "volume-a11y".into(),
                        Slider::new("volume", f64::from(controls.volume.get()) * 100.0)
                            .range(0.0, 100.0)
                            .step(1.0)
                            .on_change(cx.processor(|_, value: f64, _, cx| {
                                if let Some(value) = Normalized::new((value / 100.0) as f32) {
                                    dispatch_control_edit(ControlEdit::Volume(value), cx);
                                }
                            })),
                    ))
                    .child(Switch::new("mute", controls.mute).label("消音").on_change(
                        cx.processor(|_, mute, _, cx| {
                            dispatch_control_edit(ControlEdit::Mute(mute), cx);
                        }),
                    ));
            content = content.child(sound);

            let mut veil = slider_row(
                "ガラスの濃さ",
                format!("{:.0}%", controls.veil.get() * 100.0),
                "veil-a11y".into(),
                Slider::new("veil", f64::from(controls.veil.get()) * 100.0)
                    .range(0.0, 100.0)
                    .step(1.0)
                    .on_change(cx.processor(|_, value: f64, _, cx| {
                        if let Some(value) = Normalized::new((value / 100.0) as f32) {
                            dispatch_control_edit(ControlEdit::Veil(value), cx);
                        }
                    })),
            );
            if self.notices.reduce_transparency {
                veil = veil.child(Caption::new(
                    "システムの「透明度を下げる」が優先され、背景は不透明です",
                ));
            }
            let appearance = section("見た目")
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(Paragraph::new("演出の強さ"))
                        .child(
                            div()
                                .id("visuals-a11y")
                                .role(Role::Group)
                                .w_full()
                                .aria_label("演出の強さ")
                                .child(
                                    SegmentedControl::new("visuals", controls.visuals.name())
                                        .segment("calm", "控えめ", None)
                                        .segment("standard", "標準", None)
                                        .segment("vivid", "派手", None)
                                        .on_change(
                                            |value: &SharedString, _, cx: &mut gpui::App| {
                                                if let Some(visuals) =
                                                    VisualIntensity::from_name(value.as_ref())
                                                {
                                                    dispatch_control_edit(
                                                        ControlEdit::Visuals(visuals),
                                                        cx,
                                                    );
                                                }
                                            },
                                        ),
                                ),
                        ),
                )
                .child(
                    Switch::new("blur", controls.blur)
                        .label("ぼかし")
                        .on_change(cx.processor(|_, blur, _, cx| {
                            dispatch_control_edit(ControlEdit::Blur(blur), cx);
                        })),
                )
                .child(veil);
            content = content.child(appearance);

            let wander = section("自動で移る")
                .child(
                    Switch::new("wander", controls.wander)
                        .label("ひとりで移る")
                        .on_change(cx.processor(|_, wander, _, cx| {
                            dispatch_control_edit(ControlEdit::Wander(wander), cx);
                        })),
                )
                .child(
                    slider_row(
                        "最小滞在時間",
                        format!("{}分", controls.dwell_min_minutes),
                        "dwell-min-a11y".into(),
                        Slider::new("dwell-min", f64::from(controls.dwell_min_minutes))
                            .range(1.0, 180.0)
                            .step(1.0)
                            .on_change(cx.processor(|_, value: f64, _, cx| {
                                if let Some(value) = time_slider_value(value, 180) {
                                    dispatch_control_edit(ControlEdit::DwellMinMinutes(value), cx);
                                }
                            })),
                    )
                    .child(applies_from("dwell-min-description", "次の滞在から適用")),
                )
                .child(
                    slider_row(
                        "最大滞在時間",
                        format!("{}分", controls.dwell_max_minutes),
                        "dwell-max-a11y".into(),
                        Slider::new("dwell-max", f64::from(controls.dwell_max_minutes))
                            .range(1.0, 180.0)
                            .step(1.0)
                            .on_change(cx.processor(|_, value: f64, _, cx| {
                                if let Some(value) = time_slider_value(value, 180) {
                                    dispatch_control_edit(ControlEdit::DwellMaxMinutes(value), cx);
                                }
                            })),
                    )
                    .child(applies_from("dwell-max-description", "次の滞在から適用")),
                )
                .child(
                    slider_row(
                        "自動切り替え時間",
                        format!("{}秒", controls.transition_seconds),
                        "transition-a11y".into(),
                        Slider::new("transition", f64::from(controls.transition_seconds))
                            .range(1.0, 120.0)
                            .step(1.0)
                            .on_change(cx.processor(|_, value: f64, _, cx| {
                                if let Some(value) = time_slider_value(value, 120) {
                                    dispatch_control_edit(
                                        ControlEdit::TransitionSeconds(value),
                                        cx,
                                    );
                                }
                            })),
                    )
                    .child(applies_from(
                        "transition-description",
                        "次の自動切り替えから適用",
                    )),
                );
            content = content.child(wander);

            // The verification harness and assistive technology read these; the body does not
            // show them, they are not for a person to read.
            let summary: SharedString = format!(
                "現在の設定: {} / 音量 {:.0}% / 消音 {} / 設定世代 {}・手動世代 {} / ひとりで移る {} / 最小滞在時間 {}分 / 最大滞在時間 {}分 / 自動切り替え時間 {}秒",
                registry.definitions()[display_target.get()].label,
                controls.volume.get() * 100.0,
                if controls.mute { "オン" } else { "オフ" },
                controls.settings_revision.get(),
                controls.manual_revision.get(),
                if controls.wander { "オン" } else { "オフ" },
                controls.dwell_min_minutes,
                controls.dwell_max_minutes,
                controls.transition_seconds,
            )
            .into();
            let receipt_text: SharedString = match self.receipt {
                Some(receipt) => {
                    let dominant = &registry.definitions()[receipt.display_dominant.get()];
                    format!(
                        "音声側の受理: 設定世代 {}・手動世代 {} / 表示対象 {} / 受理通知時点の世界時刻 {:.3} 秒{}",
                        receipt.ack_revision.get(), receipt.ack_manual_revision.get(),
                        dominant.label, receipt.world.get(),
                        if receipt.ack_revision == controls.settings_revision { "" } else { "（最新設定の受理待ち）" },
                    )
                }
                None => "音声側の設定受理はまだ確認できていません。".into(),
            }
            .into();
            content = content
                .child(unseen_text("control-summary", summary))
                .child(unseen_text("control-receipt", receipt_text));
        } else {
            content = content.child(Paragraph::new(
                "標準出力の準備が成功した後に、登録されたコントロールを表示します。",
            ));
        }
        content = content.child(unseen_text("output-status", self.audio_status.clone()));

        // The scrollbar: a thin, always-visible thumb when the content is taller than the window.
        // The scroll handle is laid out during this frame's draw, so this render sees the previous
        // layout; a one-shot check on the next frame re-renders if the layout has since changed.
        let viewport = f32::from(self.scroll.bounds().size.height);
        let max_offset = f32::from(self.scroll.max_offset().y);
        let this = cx.entity();
        window.on_next_frame(move |_, cx| {
            this.update(cx, |view, cx| {
                let now_viewport = f32::from(view.scroll.bounds().size.height);
                let now_max = f32::from(view.scroll.max_offset().y);
                if (now_viewport - viewport).abs() > 0.5 || (now_max - max_offset).abs() > 0.5 {
                    cx.notify();
                }
            });
        });
        let track = (viewport - 2.0 * SCROLLBAR_MARGIN).max(0.0);
        let found = thumb(
            track,
            viewport,
            max_offset,
            f32::from(self.scroll.offset().y),
        );
        let colors = &cx.theme().colors;
        let thumb_color = colors.fg_muted.opacity(0.45);
        let thumb_hover = colors.fg_muted.opacity(0.75);
        let view = cx.entity();
        let scrollbar = found.map(|found| {
            div()
                .id("settings-scrollbar")
                .absolute()
                .top(px(SCROLLBAR_MARGIN))
                .right(px(3.0))
                .w(px(SCROLLBAR_WIDTH))
                .h(px(track))
                .on_drag_move::<ScrollbarDrag>(cx.listener(
                    move |view, event: &DragMoveEvent<ScrollbarDrag>, _, cx| {
                        // `bounds` is the track's; the thumb's top follows the pointer.
                        let top = f32::from(event.event.position.y - event.bounds.origin.y)
                            - view.scrollbar_grab;
                        let max = f32::from(view.scroll.max_offset().y);
                        let offset = offset_for_thumb_top(track, found.height, max, top);
                        let current = view.scroll.offset();
                        view.scroll.set_offset(gpui::point(current.x, px(offset)));
                        cx.notify();
                    },
                ))
                .child(
                    div()
                        .id("settings-scrollbar-thumb")
                        .absolute()
                        .top(px(found.top))
                        .w_full()
                        .h(px(found.height))
                        .rounded_full()
                        .bg(thumb_color)
                        .hover(move |style| style.bg(thumb_hover))
                        .cursor_pointer()
                        .on_drag(ScrollbarDrag, move |_, offset, _, cx| {
                            view.update(cx, |view, _| view.scrollbar_grab = f32::from(offset.y));
                            cx.new(|_| NoDragGhost)
                        }),
                )
        });

        FocusScope::new(&self.focus)
            .root()
            .size_full()
            .bg(background)
            .child(
                div()
                    .id("settings-frame")
                    .relative()
                    .size_full()
                    .child(
                        div()
                            .id("settings-content")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .on_scroll_wheel(cx.listener(|_, _: &ScrollWheelEvent, _, cx| {
                                cx.notify();
                            }))
                            .child(content),
                    )
                    .children(scrollbar),
            )
    }
}

const SCROLLBAR_WIDTH: f32 = 6.0;
const SCROLLBAR_MARGIN: f32 = 8.0;
/// The smallest the thumb gets, so a very long page still leaves something to grab.
const MIN_THUMB: f32 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Thumb {
    top: f32,
    height: f32,
}

/// The payload of a thumb drag. GPUI keeps delivering the drag's moves, and ends it on release,
/// even when the pointer is outside the window.
struct ScrollbarDrag;

/// The view GPUI draws while dragging; the thumb itself is the feedback, so it draws nothing.
struct NoDragGhost;

impl Render for NoDragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The scrollbar's thumb inside a track of `track` pixels: sized by the share of the content that
/// is visible and placed by how far it is scrolled. `offset` is the scroll offset (0 at the top,
/// negative as it scrolls down) and `max_offset` how far the content can scroll. `None` when the
/// content fits (or the numbers are unusable): then there is no scrollbar.
fn thumb(track: f32, viewport: f32, max_offset: f32, offset: f32) -> Option<Thumb> {
    if ![track, viewport, max_offset, offset]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    if track <= 0.0 || viewport <= 0.0 || max_offset <= 0.5 {
        return None;
    }
    let content = viewport + max_offset;
    let height = (track * viewport / content)
        .max(MIN_THUMB.min(track))
        .min(track);
    let progress = (-offset / max_offset).clamp(0.0, 1.0);
    Some(Thumb {
        top: progress * (track - height),
        height,
    })
}

/// The scroll offset for a thumb whose top edge is `top` pixels down its track (the inverse of
/// [`thumb`]); a drag past either end clamps. An unusable track gives 0.
fn offset_for_thumb_top(track: f32, thumb_height: f32, max_offset: f32, top: f32) -> f32 {
    let travel = track - thumb_height;
    if ![track, thumb_height, max_offset, top]
        .iter()
        .all(|v| v.is_finite())
        || travel <= 0.0
        || max_offset <= 0.0
    {
        return 0.0;
    }
    -(top / travel).clamp(0.0, 1.0) * max_offset
}

/// A titled group of settings: a small muted heading and its rows.
fn section(title: &'static str) -> gpui::Div {
    div().flex().flex_col().gap_2().child(Caption::new(title))
}

/// One setting with a slider: its name and current value on one line, the slider below. The slider
/// sits in a labelled group so assistive technology and the harness find it by name.
fn slider_row(
    label: &'static str,
    value: impl Into<SharedString>,
    group: ElementId,
    slider: Slider,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .child(Paragraph::new(label))
                .child(Caption::new(value.into())),
        )
        .child(
            div()
                .id(group)
                .role(Role::Group)
                .w_full()
                .aria_label(label)
                .child(slider),
        )
}

/// The short note that says when a time setting takes effect.
fn applies_from(id: &'static str, text: &'static str) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(text)
        .child(Caption::new(text))
}

/// Text for the harness and assistive technology that takes no room on screen.
fn unseen_text(id: &'static str, text: SharedString) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(text)
        .w(px(0.0))
        .h(px(0.0))
        .overflow_hidden()
}

fn time_slider_value(value: f64, maximum: u32) -> Option<u32> {
    if value.is_finite() && value.fract() == 0.0 && value >= 1.0 && value <= f64::from(maximum) {
        Some(value as u32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{MIN_THUMB, offset_for_thumb_top, thumb, time_slider_value};

    #[test]
    fn time_slider_boundary_requires_finite_integer_in_spec_range() {
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1.0,
            0.0,
            1.5,
            181.0,
        ] {
            assert_eq!(time_slider_value(value, 180), None);
        }
        assert_eq!(time_slider_value(1.0, 180), Some(1));
        assert_eq!(time_slider_value(180.0, 180), Some(180));
        assert_eq!(time_slider_value(120.0, 120), Some(120));
        assert_eq!(time_slider_value(121.0, 120), None);
    }

    #[test]
    fn the_thumb_appears_only_when_the_content_overflows_and_is_sized_by_the_visible_share() {
        // Content that fits: no scrollbar at all.
        assert_eq!(thumb(500.0, 600.0, 0.0, 0.0), None);
        assert_eq!(thumb(500.0, 600.0, 0.4, 0.0), None);
        // Half of the content visible: the thumb is half of the track.
        let half = thumb(400.0, 500.0, 500.0, 0.0).unwrap();
        assert!((half.height - 200.0).abs() < 1e-3 && half.top == 0.0);
        // A very long content keeps a thumb that can still be grabbed.
        let long = thumb(400.0, 500.0, 500_000.0, 0.0).unwrap();
        assert_eq!(long.height, MIN_THUMB);
    }

    #[test]
    fn the_thumb_follows_the_scroll_offset_and_stays_inside_the_track() {
        let (track, viewport, max) = (400.0, 500.0, 500.0);
        let top = |offset: f32| thumb(track, viewport, max, offset).unwrap();
        assert_eq!(top(0.0).top, 0.0);
        let bottom = top(-max);
        assert!(
            (bottom.top + bottom.height - track).abs() < 1e-3,
            "rests on the track's end"
        );
        let middle = top(-max / 2.0);
        assert!((middle.top - (track - middle.height) / 2.0).abs() < 1e-3);
        // Offsets outside the real range (an overscroll) never leave the track.
        assert_eq!(top(50.0).top, 0.0);
        assert_eq!(top(-max * 3.0).top, bottom.top);
    }

    #[test]
    fn dragging_the_thumb_maps_back_to_the_scroll_offset_and_round_trips() {
        let (track, viewport, max) = (400.0, 500.0, 500.0);
        let height = thumb(track, viewport, max, 0.0).unwrap().height;
        assert_eq!(offset_for_thumb_top(track, height, max, 0.0), 0.0);
        let end = offset_for_thumb_top(track, height, max, track - height);
        assert!(
            (end + max).abs() < 1e-3,
            "the track's end is the content's end"
        );
        // Dragging past either end clamps.
        assert_eq!(offset_for_thumb_top(track, height, max, -90.0), 0.0);
        assert!((offset_for_thumb_top(track, height, max, track * 4.0) + max).abs() < 1e-3);
        for offset in [-1.0, -123.0, -250.0, -499.0] {
            let placed = thumb(track, viewport, max, offset).unwrap();
            let back = offset_for_thumb_top(track, placed.height, max, placed.top);
            assert!((back - offset).abs() < 1e-2, "{offset} -> {back}");
        }
    }

    #[test]
    fn extreme_scroll_numbers_give_no_thumb_or_a_finite_one() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0] {
            for case in 0..4 {
                let args = [400.0_f32, 500.0, 500.0, -100.0];
                let mut args = args;
                args[case] = value;
                if let Some(found) = thumb(args[0], args[1], args[2], args[3]) {
                    assert!(found.top.is_finite() && found.height.is_finite());
                    assert!(found.top >= 0.0 && found.height > 0.0);
                }
            }
        }
        assert_eq!(offset_for_thumb_top(0.0, 30.0, 500.0, 10.0), 0.0);
        assert_eq!(offset_for_thumb_top(f32::NAN, 30.0, 500.0, 10.0), 0.0);
    }
}
