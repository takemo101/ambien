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
}

impl SettingsView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self { focus }
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
                                "この実装では、シーンと設定の固定サイズの窓を操作できます。",
                            ))
                            .child(Paragraph::new(
                                "音の再生と設定操作はまだ接続されていません。音量や環境を変更するコントロールは、この実装にはありません。",
                            ))
                            .child(Paragraph::new(
                                "シーンの空白部分をドラッグすると窓を移動できます。閉じた窓は、アプリのメニューから再び表示できます。",
                            )),
                    ),
            )
    }
}
