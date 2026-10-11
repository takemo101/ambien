use std::sync::Arc;

use ely::{
    forms::{FormField, Radio, Slider, Switch},
    primitives::FocusScope,
    theme::ActiveTheme,
    typography::{Caption, Heading, Paragraph},
};
use gpui::{
    Context, ElementId, FocusHandle, InteractiveElement, IntoElement, ParentElement, Render, Role,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
};

use crate::{
    app::dispatch_control_edit,
    audio::{AudioSource, transport::ControlReceipt},
    controls::{ControlEdit, ControlState, Normalized},
    situations::{KnobIndex, Registry, SituationIndex},
};

struct KnobControl {
    index: KnobIndex,
    owner: SituationIndex,
    id: ElementId,
    field_id: ElementId,
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
    registry: Option<Arc<Registry>>,
    controls: Option<ControlState>,
    knobs: Vec<KnobControl>,
    receipt: Option<ControlReceipt>,
    notices: SettingsNotices,
    selection_activation: Option<SelectionActivation>,
    _display: Subscription,
}

impl SettingsView {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        audio_status: String,
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
            audio_status: audio_status.into(),
            registry: None,
            controls: None,
            knobs: Vec::new(),
            receipt,
            notices,
            selection_activation: None,
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
                    field_id: (id, "field").into(),
                    label: knob.label,
                });
            }
        }
    }

    pub(crate) fn update_audio(
        &mut self,
        status: String,
        source: Option<Arc<AudioSource>>,
        controls: Option<&ControlState>,
        receipt: Option<ControlReceipt>,
        notices: SettingsNotices,
        cx: &mut Context<Self>,
    ) {
        let mut changed = self.audio_status.as_ref() != status.as_str() || self.notices != notices;
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
            self.audio_status = status.into();
            self.notices = notices;
            cx.notify();
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().colors.bg;
        let mut content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .child(Heading::h2("設定"));

        if let Some(status) = &self.notices.persistence {
            content = content.child(
                div()
                    .id("persistence-status")
                    .role(Role::Label)
                    .aria_label(status.clone())
                    .child(Paragraph::new(status.clone())),
            );
        }
        if let (Some(registry), Some(controls)) = (&self.registry, &self.controls) {
            let display_target = self.receipt.map_or(controls.manual_target, |receipt| {
                controls.display_target(receipt.ack_manual_revision, receipt.display_dominant)
            });
            let selected = &registry.definitions()[display_target.get()];
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
            content = content
                .child(FormField::new("situation-field", "シチュエーション").child(selector))
                .child(Heading::h3(format!("選択中: {}", selected.label)));
            for knob in self
                .knobs
                .iter()
                .filter(|knob| knob.owner == display_target)
            {
                let index = knob.index;
                let value = controls.knobs[index.get()].get();
                content = content.child(
                    FormField::new(knob.field_id.clone(), knob.label)
                        .description(format!("{:.0}%", value * 100.0))
                        .child(
                            div()
                                .id((knob.id.clone(), "a11y"))
                                .role(Role::Group)
                                .w_full()
                                .aria_label(knob.label)
                                .child(
                                    Slider::new(knob.id.clone(), f64::from(value) * 100.0)
                                        .range(0.0, 100.0)
                                        .step(1.0)
                                        .on_change(cx.processor(move |_, value: f64, _, cx| {
                                            if let Some(value) =
                                                Normalized::new((value / 100.0) as f32)
                                            {
                                                dispatch_control_edit(
                                                    ControlEdit::Knob(index, value),
                                                    cx,
                                                );
                                            }
                                        })),
                                ),
                        ),
                );
            }
            let summary: SharedString = format!(
                "現在の設定: {} / 音量 {:.0}% / 消音 {} / 設定世代 {}・手動世代 {} / ひとりで移る {} / 最小滞在時間 {}分 / 最大滞在時間 {}分 / 自動切り替え時間 {}秒",
                selected.label,
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
            content = content
                .child(Heading::h3("共通設定"))
                .child(
                    FormField::new("volume-field", "アプリの音量")
                        .description(format!(
                            "{:.0}%（OS 音量は変更しません）",
                            controls.volume.get() * 100.0
                        ))
                        .child(
                            div()
                                .id("volume-a11y")
                                .role(Role::Group)
                                .w_full()
                                .aria_label("アプリの音量")
                                .child(
                                    Slider::new("volume", f64::from(controls.volume.get()) * 100.0)
                                        .range(0.0, 100.0)
                                        .step(1.0)
                                        .on_change(cx.processor(|_, value: f64, _, cx| {
                                            if let Some(value) =
                                                Normalized::new((value / 100.0) as f32)
                                            {
                                                dispatch_control_edit(
                                                    ControlEdit::Volume(value),
                                                    cx,
                                                );
                                            }
                                        })),
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
                .child(
                    FormField::new("veil-field", "ガラスの濃さ")
                        .description(format!(
                            "{:.0}%{}",
                            controls.veil.get() * 100.0,
                            if self.notices.reduce_transparency {
                                "（システムの「透明度を下げる」が優先され、背景は不透明です）"
                            } else {
                                ""
                            }
                        ))
                        .child(
                            div()
                                .id("veil-a11y")
                                .role(Role::Group)
                                .w_full()
                                .aria_label("ガラスの濃さ")
                                .child(
                                    Slider::new("veil", f64::from(controls.veil.get()) * 100.0)
                                        .range(0.0, 100.0)
                                        .step(1.0)
                                        .on_change(cx.processor(|_, value: f64, _, cx| {
                                            if let Some(value) =
                                                Normalized::new((value / 100.0) as f32)
                                            {
                                                dispatch_control_edit(ControlEdit::Veil(value), cx);
                                            }
                                        })),
                                ),
                        ),
                )
                .child(
                    Switch::new("wander", controls.wander)
                        .label("ひとりで移る")
                        .on_change(cx.processor(|_, wander, _, cx| {
                            dispatch_control_edit(ControlEdit::Wander(wander), cx);
                        })),
                )
                .child(
                    FormField::new("dwell-min-field", "最小滞在時間")
                        .child(
                            div()
                                .id("dwell-min-description")
                                .role(Role::Label)
                                .aria_label("次の滞在から適用")
                                .child(Caption::new("次の滞在から適用")),
                        )
                        .child(
                            div()
                                .id("dwell-min-a11y")
                                .role(Role::Group)
                                .w_full()
                                .aria_label("最小滞在時間")
                                .child(
                                    Slider::new("dwell-min", f64::from(controls.dwell_min_minutes))
                                        .range(1.0, 180.0)
                                        .step(1.0)
                                        .on_change(cx.processor(|_, value: f64, _, cx| {
                                            if let Some(value) = time_slider_value(value, 180) {
                                                dispatch_control_edit(
                                                    ControlEdit::DwellMinMinutes(value),
                                                    cx,
                                                );
                                            }
                                        })),
                                )
                                .child(Paragraph::new(format!("{}分", controls.dwell_min_minutes))),
                        ),
                )
                .child(
                    FormField::new("dwell-max-field", "最大滞在時間")
                        .child(
                            div()
                                .id("dwell-max-description")
                                .role(Role::Label)
                                .aria_label("次の滞在から適用")
                                .child(Caption::new("次の滞在から適用")),
                        )
                        .child(
                            div()
                                .id("dwell-max-a11y")
                                .role(Role::Group)
                                .w_full()
                                .aria_label("最大滞在時間")
                                .child(
                                    Slider::new("dwell-max", f64::from(controls.dwell_max_minutes))
                                        .range(1.0, 180.0)
                                        .step(1.0)
                                        .on_change(cx.processor(|_, value: f64, _, cx| {
                                            if let Some(value) = time_slider_value(value, 180) {
                                                dispatch_control_edit(
                                                    ControlEdit::DwellMaxMinutes(value),
                                                    cx,
                                                );
                                            }
                                        })),
                                )
                                .child(Paragraph::new(format!("{}分", controls.dwell_max_minutes))),
                        ),
                )
                .child(
                    FormField::new("transition-field", "自動切り替え時間")
                        .child(
                            div()
                                .id("transition-description")
                                .role(Role::Label)
                                .aria_label("次の自動切り替えから適用")
                                .child(Caption::new("次の自動切り替えから適用")),
                        )
                        .child(
                            div()
                                .id("transition-a11y")
                                .role(Role::Group)
                                .w_full()
                                .aria_label("自動切り替え時間")
                                .child(
                                    Slider::new(
                                        "transition",
                                        f64::from(controls.transition_seconds),
                                    )
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
                                .child(Paragraph::new(format!(
                                    "{}秒",
                                    controls.transition_seconds
                                ))),
                        ),
                )
                .child(
                    Switch::new("mute", controls.mute)
                        .label("消音")
                        .on_change(cx.processor(|_, mute, _, cx| {
                            dispatch_control_edit(ControlEdit::Mute(mute), cx);
                        })),
                )
                .child(
                    div()
                        .id("control-summary")
                        .role(Role::Label)
                        .aria_label(summary.clone())
                        .child(Paragraph::new(summary)),
                );
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
            content = content.child(
                div()
                    .id("control-receipt")
                    .role(Role::Label)
                    .aria_label(receipt_text.clone())
                    .child(Paragraph::new(receipt_text)),
            );
        } else {
            content = content.child(Paragraph::new(
                "標準出力の準備が成功した後に、登録されたコントロールを表示します。",
            ));
        }
        content = content.child(Paragraph::new(self.audio_status.clone()));
        if let Some(error) = &self.notices.control {
            content = content.child(Paragraph::new(format!("設定を変更できません: {error}")));
        }
        content = content
            .child(Paragraph::new("消音や音量0でも、世界とシーンの光は進みます。試聴候補は音質を確認済みの既定値ではありません。"))
            .child(Paragraph::new("シーンの空白部分をドラッグすると窓を移動できます。閉じた窓は、アプリのメニューから再び表示できます。"));

        FocusScope::new(&self.focus)
            .root()
            .size_full()
            .bg(background)
            .child(
                div()
                    .id("settings-content")
                    .size_full()
                    .overflow_y_scroll()
                    .child(content),
            )
    }
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
    use super::time_slider_value;

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
}
