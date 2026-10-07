use gpui::{
    Context, FocusHandle, InteractiveElement, IntoElement, MouseButton, Render, Styled, Window, div,
};

pub(crate) struct SceneView {
    focus: FocusHandle,
}

impl SceneView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self { focus }
    }
}

impl Render for SceneView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // No background paint: the native blurred window remains visible through the surface.
        div().size_full().track_focus(&self.focus).on_mouse_down(
            MouseButton::Left,
            |_, window, _| {
                window.start_window_move();
            },
        )
    }
}
