use std::error::Error;

use ely::theme::{Mode, Theme};
use gpui::{
    App, AppContext, Context, Entity, Global, KeyBinding, Menu, MenuItem, Render, Subscription,
    TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowId,
    WindowOptions, actions,
};

use crate::{
    platform::macos::{WindowGeometryError, WindowRole, constrain_window, prepare_window},
    ui::{scene::SceneView, settings::SettingsView},
};

actions!(ambien, [ShowScene, ShowSettings, Quit]);

type WindowResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

struct AppRoot(Entity<AppModel>);

impl Global for AppRoot {}

pub(crate) struct AppModel {
    scene: Option<WindowHandle<SceneView>>,
    settings: Option<WindowHandle<SettingsView>>,
    // The global root keeps subscriptions alive; callbacks do not own the root.
    _window_closed: Option<Subscription>,
    settings_appearance: Option<Subscription>,
}

pub(crate) fn start(cx: &mut App) {
    ely::init(cx);
    Theme::set_mode_now(Mode::from(cx.window_appearance()), cx);

    let owner = cx.new(|_| AppModel {
        scene: None,
        settings: None,
        _window_closed: None,
        settings_appearance: None,
    });
    cx.set_global(AppRoot(owner.clone()));

    let closed = cx.on_window_closed(|cx, window_id| {
        // Closing a failed, still-hidden window can occur during an entity update.
        cx.defer(move |cx| {
            let Some(owner) = cx.try_global::<AppRoot>().map(|root| root.0.clone()) else {
                return; // The application owner has already been released.
            };
            owner.update(cx, |model, _| model.window_closed(window_id));
        });
    });
    owner.update(cx, |model, _| model._window_closed = Some(closed));

    cx.bind_keys([
        KeyBinding::new("cmd-,", ShowSettings, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    // Action routing temporarily borrows the active window; update it only after release.
    cx.on_action(|_: &ShowScene, cx| {
        cx.defer(|cx| {
            let owner = cx.global::<AppRoot>().0.clone();
            owner.update(cx, |model, cx| {
                if let Err(error) = model.show_scene(cx) {
                    eprintln!("Ambien: シーンを表示できません: {error}");
                }
            });
        });
    });
    cx.on_action(|_: &ShowSettings, cx| {
        cx.defer(|cx| {
            let owner = cx.global::<AppRoot>().0.clone();
            owner.update(cx, |model, cx| {
                if let Err(error) = model.show_settings(cx) {
                    eprintln!("Ambien: 設定を表示できません: {error}");
                }
            });
        });
    });
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus([Menu::new("Ambien").items([
        MenuItem::action("シーンを表示", ShowScene),
        MenuItem::action("設定…", ShowSettings),
        MenuItem::separator(),
        MenuItem::action("終了", Quit),
    ])]);

    owner.update(cx, |model, cx| {
        if let Err(error) = model.show_scene(cx) {
            // Keep the explicit-quit host and menus available for a later display retry.
            eprintln!("Ambien: シーンの起動に失敗しました: {error}");
        }
    });
}

impl AppModel {
    fn show_scene(&mut self, cx: &mut Context<Self>) -> WindowResult<()> {
        if let Some(handle) = self.scene {
            handle.update(cx, |_, window, cx| {
                window.activate_window();
                cx.activate(true);
            })?;
            return Ok(());
        }
        self.scene = Some(open_window(WindowRole::Scene, SceneView::new, cx)?);
        Ok(())
    }

    fn show_settings(&mut self, cx: &mut Context<Self>) -> WindowResult<()> {
        if let Some(handle) = self.settings {
            handle.update(cx, |_, window, cx| {
                window.activate_window();
                cx.activate(true);
            })?;
            return Ok(());
        }
        Theme::set_mode_now(Mode::from(cx.window_appearance()), cx);
        let handle = open_window(WindowRole::Settings, SettingsView::new, cx)?;
        self.settings_appearance = Some(handle.update(cx, |_, window, _| {
            window.observe_window_appearance(|_, cx| {
                cx.defer(|cx| {
                    Theme::set_mode_now(Mode::from(cx.window_appearance()), cx);
                });
            })
        })?);
        self.settings = Some(handle);
        Ok(())
    }

    fn window_closed(&mut self, window_id: WindowId) {
        if self
            .scene
            .is_some_and(|window| window.window_id() == window_id)
        {
            self.scene = None;
        }
        if self
            .settings
            .is_some_and(|window| window.window_id() == window_id)
        {
            self.settings = None;
            self.settings_appearance = None;
        }
    }
}

fn open_window<V: Render + 'static>(
    role: WindowRole,
    constructor: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    cx: &mut Context<AppModel>,
) -> WindowResult<WindowHandle<V>> {
    let display_id = cx
        .primary_display()
        .ok_or(WindowGeometryError::DisplayUnavailable)?
        .id();
    let prepared = prepare_window(role, display_id)?;
    let (titlebar, background) = match role {
        WindowRole::Scene => (
            TitlebarOptions {
                title: None,
                appears_transparent: true,
                ..Default::default()
            },
            WindowBackgroundAppearance::Blurred,
        ),
        WindowRole::Settings => (
            TitlebarOptions {
                title: Some("設定".into()),
                appears_transparent: false,
                ..Default::default()
            },
            WindowBackgroundAppearance::Opaque,
        ),
    };
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(prepared.bounds)),
            display_id: Some(prepared.display_id),
            titlebar: Some(titlebar),
            window_background: background,
            is_resizable: false,
            is_minimizable: true,
            is_movable: true,
            app_owns_titlebar_drag: matches!(role, WindowRole::Scene),
            show: false,
            focus: false,
            tabbing_identifier: None,
            ..Default::default()
        },
        |window, cx| cx.new(|cx| constructor(window, cx)),
    )?;
    handle.update(cx, |_, window, cx| {
        if let Err(error) = constrain_window(window, &prepared) {
            window.remove_window();
            return Err(error);
        }
        window.activate_window();
        cx.activate(true);
        Ok(())
    })??;
    Ok(handle)
}
