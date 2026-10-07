use ely::{
    primitives::FocusScope,
    theme::ActiveTheme,
    typography::{Heading, Paragraph},
};
use gpui::{
    Context, FocusHandle, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div,
};

pub(crate) struct SettingsView {
    focus: FocusHandle,
    audio_status: String,
    control_summary: Option<String>,
}

impl SettingsView {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        audio_status: String,
        control_summary: Option<String>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            focus,
            audio_status,
            control_summary,
        }
    }

    pub(crate) fn update_audio(
        &mut self,
        status: String,
        control_summary: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.audio_status = status;
        self.control_summary = control_summary;
        cx.notify();
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().colors.bg;

        FocusScope::new(&self.focus)
            .root()
            .size_full()
            .bg(background)
            .child(
                div()
                    .id("settings-content")
                    .size_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .p_6()
                            .child(Heading::h2("設定"))
                            .child(Paragraph::new(
                                self.audio_status.clone(),
                            ))
                            .child(Paragraph::new(
                                self.control_summary.clone().unwrap_or_else(|| "試聴候補は、標準出力の準備が成功した後に表示します。".into()),
                            ))
                            .child(Paragraph::new(
                                "雨の音と同じ粒を、シーンの光として表示します。音量・雨量・風のコントロールは、この段階ではまだありません。試聴候補は音質を確認済みの既定値ではありません。",
                            ))
                            .child(Paragraph::new(
                                "シーンの空白部分をドラッグすると窓を移動できます。閉じた窓は、アプリのメニューから再び表示できます。",
                            )),
                    ),
            )
    }
}
