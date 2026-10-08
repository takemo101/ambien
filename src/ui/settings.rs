use std::sync::Arc;

use ely::{
    buttons::SegmentedControl,
    forms::{FormField, Slider, Switch},
    primitives::FocusScope,
    theme::ActiveTheme,
    typography::{Heading, Paragraph},
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
    focus: FocusHandle,
    _blur: Subscription,
}

pub(crate) struct SettingsView {
    focus: FocusHandle,
    audio_status: SharedString,
    registry: Option<Arc<Registry>>,
    controls: Option<ControlState>,
    knobs: Vec<KnobControl>,
    receipt: Option<ControlReceipt>,
    control_error: Option<SharedString>,
    selection_activation: Option<SelectionActivation>,
}

impl SettingsView {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        audio_status: String,
        source: Option<Arc<AudioSource>>,
        controls: Option<&ControlState>,
        receipt: Option<ControlReceipt>,
        control_error: Option<SharedString>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut view = Self {
            focus,
            audio_status: audio_status.into(),
            registry: None,
            controls: None,
            knobs: Vec::new(),
            receipt,
            control_error,
            selection_activation: None,
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
        control_error: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let mut changed =
            self.audio_status.as_ref() != status.as_str() || self.control_error != control_error;
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
            self.control_error = control_error;
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

        if let (Some(registry), Some(controls)) = (&self.registry, &self.controls) {
            let selected = &registry.definitions()[controls.manual_target.get()];
            let mut selector = SegmentedControl::new("situation", selected.id);
            for definition in registry.definitions() {
                selector = selector.segment(definition.id, definition.label, None);
            }
            content = content.child(
                FormField::new("situation-field", "シチュエーション").child(
                    div()
                        .id("situation-selector")
                        .role(Role::Group)
                        .aria_label("シチュエーション")
                        .on_click(cx.listener(|view, _, _, cx| {
                            // Ely suppresses changes on the selected segment. Its
                            // bubbled click still represents a new manual intent.
                            if let Some(controls) = &view.controls {
                                dispatch_control_edit(
                                    ControlEdit::Select(controls.manual_target),
                                    cx,
                                );
                            }
                        }))
                        .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
                            view.selection_activation = None;
                            let key = match event.keystroke.key.as_str() {
                                "enter" => "enter",
                                "space" => "space",
                                _ => return,
                            };
                            if event.keystroke.modifiers.modified() || window.default_prevented() {
                                return;
                            }
                            if let Some(focus) = window.focused(cx) {
                                let blur = cx.on_blur(&focus, window, |view, _, _| {
                                    view.selection_activation = None;
                                });
                                view.selection_activation = Some(SelectionActivation {
                                    key,
                                    focus,
                                    _blur: blur,
                                });
                            }
                        }))
                        .on_key_up(cx.listener(|view, event: &gpui::KeyUpEvent, window, cx| {
                            let Some(activation) = view.selection_activation.take() else {
                                return;
                            };
                            if activation.key == event.keystroke.key.as_str()
                                && !event.keystroke.modifiers.modified()
                                && !window.default_prevented()
                                && activation.focus.is_focused(window)
                                && let Some(controls) = &view.controls
                            {
                                dispatch_control_edit(
                                    ControlEdit::Select(controls.manual_target),
                                    cx,
                                );
                            }
                        }))
                        .child(selector.on_change(cx.listener(
                            |view, value: &SharedString, _, cx| {
                                // A changed segment must not also reselect the old one.
                                cx.stop_propagation();
                                view.selection_activation = None;
                                if let Some(index) = view
                                    .registry
                                    .as_ref()
                                    .and_then(|registry| registry.resolve(value.as_ref()))
                                {
                                    dispatch_control_edit(ControlEdit::Select(index), cx);
                                }
                            },
                        ))),
                ),
            );
            for knob in self
                .knobs
                .iter()
                .filter(|knob| knob.owner == controls.manual_target)
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
                "現在の設定: {} / 音量 {:.0}% / 消音 {} / 設定世代 {}・手動世代 {}",
                selected.label,
                controls.volume.get() * 100.0,
                if controls.mute { "オン" } else { "オフ" },
                controls.settings_revision.get(),
                controls.manual_revision.get(),
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
                        "音声側の受理: 設定世代 {}・手動世代 {} / 主に聞こえる環境 {} / 受理通知時点の世界時刻 {:.3} 秒{}",
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
        if let Some(error) = &self.control_error {
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
