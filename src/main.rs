mod app;
mod audio;
mod controls;
mod platform;
#[cfg(test)]
mod rt_alloc;
mod situations;
mod ui;

fn main() {
    gpui_platform::application()
        .with_assets(ely::Assets)
        .with_quit_mode(gpui::QuitMode::Explicit)
        .run(app::start);
}
