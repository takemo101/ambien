use std::{error::Error, path::PathBuf, sync::Arc, time::Duration};

use ely::theme::{Mode, Theme};
use gpui::{
    App, AppContext, Context, DisplayId, Entity, Global, KeyBinding, Menu, MenuItem, Render,
    SharedString, Subscription, Task, TitlebarOptions, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowId, WindowOptions, actions,
};

use crate::{
    audio::{AudioController, OutputStatus, RecoverySignals},
    controls::{ControlEdit, ControlState, Revision},
    persistence::{LoadedState, SaveError, SaveSnapshot, SaveStage, SaveStartup, SaveWorker},
    platform::macos::{
        PowerMonitor, WindowGeometryError, WindowPlacement, WindowRole, capture_placement,
        constrain_window, prepare_window,
    },
    situations::{Registry, SituationIndex},
    ui::{
        scene::SceneView,
        settings::{SettingsNotices, SettingsView},
    },
};

actions!(
    ambien,
    [ShowScene, ShowSettings, ToggleWander, ToggleMute, Quit]
);

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
    power_monitor: Option<PowerMonitor>,
    controls: ControlState,
    menu_selection: Option<SituationIndex>,
    audio_updates: Option<Task<()>>,
    control_error: Option<SharedString>,
    _quit: Option<Subscription>,
    save_worker: Option<SaveWorker>,
    save_updates: Option<Task<()>>,
    autosave: Option<Task<()>>,
    save_revision: Revision,
    save_notice: Option<SharedString>,
    save_error: Option<SharedString>,
    last_resume: SituationIndex,
    placement: Option<WindowPlacement>,
    placement_display: Option<DisplayId>,
    // The global root keeps subscriptions alive; callbacks do not own the root.
    _window_closed: Option<Subscription>,
    settings_appearance: Option<Subscription>,
}

pub(crate) fn start(cx: &mut App) {
    ely::init(cx);
    Theme::set_mode_now(Mode::from(cx.window_appearance()), cx);

    let registry = match Registry::new() {
        Ok(registry) => Arc::new(registry),
        Err(error) => {
            eprintln!("Ambien: 環境の登録を初期化できません: {error}");
            cx.quit();
            return;
        }
    };
    let mut save_worker = None;
    let mut save_updates = None;
    // The worker owns all filesystem work. Wait before any stream/window exists,
    // so a restored mute and endpoint are effective from the first audio sample.
    let startup = std::env::var_os("HOME")
        .ok_or_else(|| "HOME がなく、状態保存先を決められません".to_owned())
        .and_then(|home| {
            let path = PathBuf::from(home).join("Library/Application Support/Ambien/state.json");
            let (worker, startup, updates) =
                SaveWorker::start(path, registry.clone()).map_err(|error| error.to_string())?;
            save_worker = Some(worker);
            save_updates = Some(updates);
            startup
                .recv_blocking()
                .map_err(|error| format!("保存 worker から初期状態を受け取れません: {error}"))
        });
    let SaveStartup {
        settings: LoadedState {
            controls,
            window: placement,
        },
        notice,
    } = startup.unwrap_or_else(|error| SaveStartup {
        settings: LoadedState {
            controls: ControlState::new(&registry),
            window: None,
        },
        notice: Some(format!("状態を読み込めず保存できません: {error}")),
    });
    let last_resume = controls.manual_target;
    let signals = Arc::new(RecoverySignals::new());
    let (power_monitor, power_ready) = match PowerMonitor::new(signals.clone()) {
        Ok(monitor) => (Some(monitor), Ok(())),
        Err(error) => (
            None,
            Err(format!("スリープ復旧通知を登録できません: {error}")),
        ),
    };
    let (audio, updates) = AudioController::start(registry, controls.clone(), signals, power_ready);
    let owner = cx.new(move |_| AppModel {
        scene: None,
        settings: None,
        _window_closed: None,
        settings_appearance: None,
        audio,
        power_monitor,
        controls,
        menu_selection: None,
        audio_updates: None,
        control_error: None,
        _quit: None,
        save_worker,
        save_updates: None,
        autosave: None,
        save_revision: Revision::INITIAL,
        save_notice: notice.map(|notice| format!("保存状態: {notice}").into()),
        save_error: None,
        last_resume,
        placement,
        placement_display: None,
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
        model._quit = Some(cx.on_app_quit(|model, cx| {
            if let Err(error) = model.flush_latest(cx) {
                model.record_save_error(error, cx);
            }
            if let Some(mut monitor) = model.power_monitor.take()
                && let Err(error) = monitor.shutdown()
            {
                eprintln!("Ambien: スリープ復旧通知の解除に失敗しました: {error}");
            }
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
    if let Some(updates) = save_updates {
        let task = owner.update(cx, |_, cx| {
            cx.spawn(async move |owner, cx| {
                while updates.recv().await.is_ok() {
                    if owner
                        .update(cx, |model, cx| model.save_updated(cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
        owner.update(cx, |model, _| model.save_updates = Some(task));
    }

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
    cx.on_action(|_: &ToggleWander, cx| {
        cx.defer(|cx| {
            let owner = cx.global::<AppRoot>().0.clone();
            owner.update(cx, |model, cx| {
                model.apply_control_edit(ControlEdit::Wander(!model.controls.wander), cx);
            });
        });
    });
    cx.on_action(|_: &ToggleMute, cx| {
        cx.defer(|cx| {
            let owner = cx.global::<AppRoot>().0.clone();
            owner.update(cx, |model, cx| {
                model.apply_control_edit(ControlEdit::Mute(!model.controls.mute), cx);
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

pub(crate) fn dispatch_scene_placement_changed(
    window_id: WindowId,
    placement: Result<(WindowPlacement, DisplayId), WindowGeometryError>,
    cx: &mut App,
) {
    cx.defer(move |cx| {
        let Some(owner) = cx.try_global::<AppRoot>().map(|root| root.0.clone()) else {
            return;
        };
        owner.update(cx, |model, cx| {
            model.scene_placement_changed(window_id, placement, cx);
        });
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
            self.placement.as_ref(),
            move |window, cx| SceneView::new(window, cx, audio),
            cx,
        )?);
        let handle = self.scene.expect("the scene was just opened");
        let placement = handle.update(cx, |_, window, _| capture_placement(window))?;
        self.scene_placement_changed(handle.window_id(), placement, cx);
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
        let controls = Some(&self.controls);
        let notices = self.settings_notices();
        let handle = open_window(
            WindowRole::Settings,
            None,
            move |window, cx| {
                SettingsView::new(
                    window,
                    cx,
                    status.text(),
                    source,
                    controls,
                    receipt,
                    notices,
                )
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

    fn update_menu(&mut self, cx: &mut Context<Self>) {
        let mut items = vec![
            MenuItem::action("シーンを表示", ShowScene),
            MenuItem::separator(),
        ];
        let shared = self.audio.shared();
        let (_, source) = shared.snapshot();
        self.menu_selection = Some(
            shared
                .control_receipt()
                .filter(|receipt| {
                    source
                        .as_ref()
                        .is_some_and(|source| source.epoch == receipt.epoch)
                })
                .map_or(self.controls.manual_target, |receipt| {
                    self.controls
                        .display_target(receipt.ack_manual_revision, receipt.display_dominant)
                }),
        );
        let controls_ready = source.is_some();
        if let Some(source) = source {
            let definitions = source.registry.definitions();
            for (position, definition) in definitions.iter().enumerate() {
                let target = SituationIndex::from_index(position, definitions.len())
                    .expect("registered menu position is valid");
                items.push(
                    MenuItem::action(definition.label, SelectSituation { target })
                        .checked(self.menu_selection == Some(target)),
                );
            }
            items.push(MenuItem::separator());
        }
        items.extend([
            MenuItem::action("ひとりで移る", ToggleWander)
                .checked(self.controls.wander)
                .disabled(!controls_ready),
            MenuItem::action("消音", ToggleMute)
                .checked(self.controls.mute)
                .disabled(!controls_ready),
            MenuItem::separator(),
            MenuItem::action("設定…", ShowSettings),
            MenuItem::action("終了", Quit),
        ]);
        if let Some(status) = self.save_error.as_ref().or(self.save_notice.as_ref()) {
            items.insert(
                items.len() - 1,
                MenuItem::action(status.clone(), ShowSettings),
            );
        }
        cx.set_menus([Menu::new("Ambien").items(items)]);
    }

    fn apply_control_edit(&mut self, edit: ControlEdit, cx: &mut Context<Self>) {
        let (_, source) = self.audio.shared().snapshot();
        let Some(source) = source else {
            return; // Preserve the existing preparing/failed-output control gate.
        };
        let state = &mut self.controls;
        let old_mute = state.mute;
        let old_wander = state.wander;
        let old_manual_revision = state.manual_revision;
        let changed = match state.apply(edit) {
            Ok(changed) => {
                self.control_error = None;
                if changed {
                    source.controls.publish(state);
                }
                changed
            }
            Err(error) => {
                self.control_error = Some(error.to_string().into());
                false
            }
        };
        if old_mute != state.mute
            || old_wander != state.wander
            || old_manual_revision != state.manual_revision
        {
            self.update_menu(cx);
        }
        if old_manual_revision != self.controls.manual_revision {
            self.last_resume = self.controls.manual_target;
        }
        if changed {
            self.schedule_save(cx);
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
                Some(&self.controls),
                receipt,
                self.settings_notices(),
                cx,
            );
        }) {
            eprintln!("Ambien: 設定に音声状態を表示できません: {error}");
        }
    }

    fn audio_updated(&mut self, cx: &mut Context<Self>) {
        let (status, source) = self.audio.shared().snapshot();
        let shared = self.audio.shared();
        let receipt = shared.control_receipt().filter(|receipt| {
            source
                .as_ref()
                .is_some_and(|source| source.epoch == receipt.epoch)
        });
        let selection = Some(receipt.map_or(self.controls.manual_target, |receipt| {
            self.controls
                .display_target(receipt.ack_manual_revision, receipt.display_dominant)
        }));
        let resume = receipt.map_or(self.controls.manual_target, |receipt| {
            self.controls
                .display_target(receipt.ack_manual_revision, receipt.resume_target)
        });
        if self.last_resume != resume {
            self.last_resume = resume;
            self.schedule_save(cx);
        }
        if self.menu_selection != selection {
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

    fn settings_notices(&self) -> SettingsNotices {
        SettingsNotices {
            control: self.control_error.clone(),
            persistence: self
                .save_error
                .as_ref()
                .or(self.save_notice.as_ref())
                .cloned(),
        }
    }

    fn remember_placement(
        &mut self,
        mut placement: WindowPlacement,
        display_id: DisplayId,
        cx: &App,
    ) -> bool {
        let changed = self.placement_display != Some(display_id)
            || self.placement.as_ref().is_none_or(|old| {
                old.x != placement.x
                    || old.y != placement.y
                    || old.width != placement.width
                    || old.height != placement.height
            });
        if !changed {
            return false;
        }
        placement.display_uuid = if self.placement_display == Some(display_id) {
            self.placement
                .as_mut()
                .and_then(|old| old.display_uuid.take())
        } else {
            cx.displays()
                .into_iter()
                .find(|display| display.id() == display_id)
                .and_then(|display| display.uuid().ok())
                .map(|uuid| uuid.to_string())
        };
        self.placement_display = Some(display_id);
        self.placement = Some(placement);
        true
    }

    fn scene_placement_changed(
        &mut self,
        window_id: WindowId,
        placement: Result<(WindowPlacement, DisplayId), WindowGeometryError>,
        cx: &mut Context<Self>,
    ) {
        if self
            .scene
            .is_none_or(|window| window.window_id() != window_id)
        {
            return; // A retired scene cannot overwrite a reopened scene's position.
        }
        match placement {
            Ok((placement, display)) => {
                if self.remember_placement(placement, display, cx) {
                    self.schedule_save(cx);
                }
            }
            Err(error) => self.record_save_error(
                SaveError {
                    stage: SaveStage::Serialize,
                    message: format!("窓位置を確認できません: {error}"),
                },
                cx,
            ),
        }
    }

    fn next_save_revision(&mut self) -> Result<Revision, SaveError> {
        let revision = self
            .save_revision
            .get()
            .checked_add(1)
            .ok_or_else(|| SaveError {
                stage: SaveStage::InvalidRevision,
                message: "状態保存の世代が上限に達しました".to_owned(),
            })?;
        self.save_revision = Revision::from_raw(revision);
        Ok(self.save_revision)
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        let revision = match self.next_save_revision() {
            Ok(revision) => revision,
            Err(error) => {
                self.record_save_error(error, cx);
                return;
            }
        };
        self.autosave = Some(cx.spawn(async move |owner, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            let _ = owner.update(cx, |model, cx| {
                model.autosave = None;
                if model.save_revision == revision
                    && let Err(error) = model.submit_save(revision, cx)
                {
                    model.record_save_error(error, cx);
                }
            });
        }));
    }

    fn submit_save(&mut self, revision: Revision, cx: &mut Context<Self>) -> Result<(), SaveError> {
        if let Some(window) = self.scene {
            let capture = window
                .update(cx, |_, window, _| capture_placement(window))
                .map_err(|error| SaveError {
                    stage: SaveStage::Serialize,
                    message: format!("保存時のシーンを確認できません: {error}"),
                })?
                .map_err(|error| SaveError {
                    stage: SaveStage::Serialize,
                    message: format!("保存時の窓位置を確認できません: {error}"),
                })?;
            self.remember_placement(capture.0, capture.1, cx);
        }
        let shared = self.audio.shared();
        let (_, source) = shared.snapshot();
        let resume_target = shared
            .control_receipt()
            .filter(|receipt| {
                source
                    .as_ref()
                    .is_some_and(|source| source.epoch == receipt.epoch)
            })
            .map_or(self.controls.manual_target, |receipt| {
                self.controls
                    .display_target(receipt.ack_manual_revision, receipt.resume_target)
            });
        let worker = self.save_worker.as_ref().ok_or_else(|| SaveError {
            stage: SaveStage::WorkerStopped,
            message: "状態保存 worker が利用できません".to_owned(),
        })?;
        worker.submit(
            revision,
            SaveSnapshot {
                controls: self.controls.clone(),
                resume_target,
                window: self.placement.clone(),
            },
        )
    }

    fn flush_latest(&mut self, cx: &mut Context<Self>) -> Result<(), SaveError> {
        self.autosave = None;
        let revision = self.next_save_revision()?;
        self.submit_save(revision, cx)?;
        self.save_worker
            .as_ref()
            .expect("submit requires a worker")
            .flush(revision)
    }

    fn record_save_error(&mut self, error: SaveError, cx: &mut Context<Self>) {
        self.save_error = Some(format!("状態を保存できません: {error}").into());
        self.update_menu(cx);
        self.update_settings(cx);
    }

    fn save_updated(&mut self, cx: &mut Context<Self>) {
        let Some(receipt) = self.save_worker.as_ref().and_then(SaveWorker::take_receipt) else {
            return;
        };
        if receipt.revision != self.save_revision {
            return; // Completion of an older snapshot cannot acknowledge the latest edit.
        }
        match receipt.result {
            Ok(()) => {
                if self.save_error.take().is_some() {
                    self.update_menu(cx);
                    self.update_settings(cx);
                }
            }
            Err(error) => self.record_save_error(error, cx),
        }
    }
}

fn open_window<V: Render + 'static>(
    role: WindowRole,
    saved: Option<&WindowPlacement>,
    constructor: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    cx: &mut Context<AppModel>,
) -> WindowResult<WindowHandle<V>> {
    let display_id = saved
        .and_then(|saved| saved.display_uuid.as_deref())
        .and_then(|wanted| {
            cx.displays()
                .into_iter()
                .find(|display| {
                    display.uuid().ok().is_some_and(|uuid| {
                        let mut buffer = [0; 36];
                        uuid.hyphenated()
                            .encode_lower(&mut buffer)
                            .eq_ignore_ascii_case(wanted)
                    })
                })
                .map(|display| display.id())
        })
        .or_else(|| cx.primary_display().map(|display| display.id()))
        .ok_or(WindowGeometryError::DisplayUnavailable)?;
    let prepared = prepare_window(role, display_id, saved)?;
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
