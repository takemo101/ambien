use std::error::Error;

use ely::theme::{Mode, Theme};
use gpui::{
    App, AppContext, Context, Entity, Global, KeyBinding, Menu, MenuItem, Render, SharedString,
    Subscription, Task, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowId, WindowOptions, actions,
};

use crate::{
    audio::{AudioController, OutputStatus},
    controls::{ControlEdit, ControlState},
    platform::macos::{WindowGeometryError, WindowRole, constrain_window, prepare_window},
    situations::SituationIndex,
    ui::{scene::SceneView, settings::SettingsView},
};

actions!(ambien, [ShowScene, ShowSettings, ToggleMute, Quit]);

#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = ambien, no_json)]
struct SelectSituation {
    target: SituationIndex,
}

type WindowResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

struct AppRoot(Entity<AppModel>);

impl Global for AppRoot {}

pub(crate) struct AppModel {
    scene: Option<WindowHandle<SceneView>>,
    settings: Option<WindowHandle<SettingsView>>,
    audio: AudioController,
    controls: Option<ControlState>,
    audio_updates: Option<Task<()>>,
    control_error: Option<SharedString>,
    audio_quit: Option<Subscription>,
    // The global root keeps subscriptions alive; callbacks do not own the root.
    _window_closed: Option<Subscription>,
    settings_appearance: Option<Subscription>,
}

pub(crate) fn start(cx: &mut App) {
    ely::init(cx);
    Theme::set_mode_now(Mode::from(cx.window_appearance()), cx);

    let (audio, updates) = AudioController::start();
    let owner = cx.new(move |_| AppModel {
        scene: None,
        settings: None,
        _window_closed: None,
        settings_appearance: None,
        audio,
        controls: None,
        audio_updates: None,
        control_error: None,
        audio_quit: None,
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
    owner.update(cx, |model, cx| {
        // GPUI retains globals through native shutdown; root Drop alone is not a quit hook.
        model.audio_quit = Some(cx.on_app_quit(|model, _| {
            model.audio.shutdown();
            std::future::ready(())
        }));
    });
    let update_task = owner.update(cx, |_, cx| {
        cx.spawn(async move |owner, cx| {
            while updates.recv().await.is_ok() {
                if owner
                    .update(cx, |model, cx| model.audio_updated(cx))
                    .is_err()
                {
                    break; // The application owner has been released.
                }
            }
        })
    });
    owner.update(cx, |model, _| model.audio_updates = Some(update_task));

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
    cx.on_action(|_: &ToggleMute, cx| {
        cx.defer(|cx| {
            let owner = cx.global::<AppRoot>().0.clone();
            owner.update(cx, |model, cx| {
                if let Some(state) = &model.controls {
                    model.apply_control_edit(ControlEdit::Mute(!state.mute), cx);
                }
            });
        });
    });
    cx.on_action(|action: &SelectSituation, cx| {
        let target = action.target;
        cx.defer(move |cx| {
            let owner = cx.global::<AppRoot>().0.clone();
            owner.update(cx, |model, cx| {
                model.apply_control_edit(ControlEdit::Select(target), cx);
            });
        });
    });
    owner.update(cx, |model, cx| model.update_menu(cx));

    owner.update(cx, |model, cx| {
        if let Err(error) = model.show_scene(cx) {
            // Keep the explicit-quit host and menus available for a later display retry.
            eprintln!("Ambien: シーンの起動に失敗しました: {error}");
        }
    });
}

// Component processors borrow SettingsView. Release that borrow before the reducer
// publishes and copies the latest endpoints back into the same settings entity.
pub(crate) fn dispatch_control_edit(edit: ControlEdit, cx: &mut App) {
    cx.defer(move |cx| {
        let owner = cx.global::<AppRoot>().0.clone();
        owner.update(cx, |model, cx| model.apply_control_edit(edit, cx));
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
        let audio = self.audio.shared();
        self.scene = Some(open_window(
            WindowRole::Scene,
            move |window, cx| SceneView::new(window, cx, audio),
            cx,
        )?);
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
        let shared = self.audio.shared();
        let (status, source) = shared.snapshot();
        let receipt = shared.control_receipt().filter(|receipt| {
            source
                .as_ref()
                .is_some_and(|source| source.epoch == receipt.epoch)
        });
        let controls = self.controls.as_ref();
        let error = self.control_error.clone();
        let handle = open_window(
            WindowRole::Settings,
            move |window, cx| {
                SettingsView::new(window, cx, status.text(), source, controls, receipt, error)
            },
            cx,
        )?;
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

    fn update_menu(&self, cx: &mut Context<Self>) {
        let mut items = vec![
            MenuItem::action("シーンを表示", ShowScene),
            MenuItem::action("設定…", ShowSettings),
            MenuItem::separator(),
        ];
        if let (Some(controls), (_, Some(source))) =
            (&self.controls, self.audio.shared().snapshot())
        {
            let definitions = source.registry.definitions();
            for (position, definition) in definitions.iter().enumerate() {
                let target = SituationIndex::from_index(position, definitions.len())
                    .expect("registered menu position is valid");
                items.push(
                    MenuItem::action(definition.label, SelectSituation { target })
                        .checked(controls.manual_target == target),
                );
            }
            items.push(MenuItem::separator());
        }
        items.extend([
            MenuItem::action("消音", ToggleMute)
                .checked(self.controls.as_ref().is_some_and(|state| state.mute))
                .disabled(self.controls.is_none()),
            MenuItem::separator(),
            MenuItem::action("終了", Quit),
        ]);
        cx.set_menus([Menu::new("Ambien").items(items)]);
    }

    fn apply_control_edit(&mut self, edit: ControlEdit, cx: &mut Context<Self>) {
        let Some(state) = &mut self.controls else {
            return;
        };
        let old_mute = state.mute;
        let old_target = state.manual_target;
        match state.apply(edit) {
            Ok(changed) => {
                self.control_error = None;
                if changed {
                    let (_, source) = self.audio.shared().snapshot();
                    if let Some(source) = source {
                        source.controls.publish(state);
                    }
                }
            }
            Err(error) => {
                self.control_error = Some(error.to_string().into());
            }
        }
        if old_mute != state.mute || old_target != state.manual_target {
            self.update_menu(cx);
        }
        self.update_settings(cx);
    }

    fn update_settings(&self, cx: &mut Context<Self>) {
        let Some(handle) = self.settings else {
            return;
        };
        let shared = self.audio.shared();
        let (status, source) = shared.snapshot();
        let receipt = shared.control_receipt().filter(|receipt| {
            source
                .as_ref()
                .is_some_and(|source| source.epoch == receipt.epoch)
        });
        if let Err(error) = handle.update(cx, |view, _, cx| {
            view.update_audio(
                status.text(),
                source,
                self.controls.as_ref(),
                receipt,
                self.control_error.clone(),
                cx,
            );
        }) {
            eprintln!("Ambien: 設定に音声状態を表示できません: {error}");
        }
    }

    fn audio_updated(&mut self, cx: &mut Context<Self>) {
        let (status, source) = self.audio.shared().snapshot();
        if self.controls.is_none()
            && let Some(source) = &source
        {
            let state = ControlState::new(&source.registry);
            // Main now owns the full control state and its sole publication authority.
            source.controls.publish(&state);
            self.controls = Some(state);
            self.update_menu(cx);
        }
        if let Some(handle) = self.scene {
            let status = status.clone();
            let source = source.clone();
            if let Err(error) = handle.update(cx, |view, window, cx| {
                view.update_audio(status, source, window, cx);
            }) {
                eprintln!("Ambien: シーンに音声状態を表示できません: {error}");
            }
        }
        if matches!(status, OutputStatus::Failed { .. })
            && self.settings.is_none()
            && let Err(error) = self.show_settings(cx)
        {
            eprintln!("Ambien: 音声出力の失敗を表示できません: {error}");
        }
        self.update_settings(cx);
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
