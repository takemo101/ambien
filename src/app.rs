use std::{error::Error, path::PathBuf, sync::Arc, time::Duration};

use ely::theme::{Mode, Theme};
use gpui::{
    AnyWindowHandle, App, AppContext, Context, DisplayId, Entity, Global, KeyBinding, Menu,
    MenuItem, Render, SharedString, Subscription, Task, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowId, WindowOptions, actions,
};

use crate::{
    audio::{AudioController, OutputStatus, RecoverySignals},
    controls::{ControlEdit, ControlState, Revision},
    persistence::{LoadedState, SaveError, SaveSnapshot, SaveStage, SaveStartup, SaveWorker},
    platform::macos::{
        EnvironmentMonitor, PowerMonitor, ScreenLayout, WindowGeometryError, WindowPlacement,
        WindowRole, capture_placement, constrain_window, prepare_window, read_display_options,
        read_screen_layout, refit_window,
    },
    situations::{Registry, SituationIndex},
    ui::{
        appearance::{AccessibilityState, ChangeNotifier, change_channel},
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
    scene_appearance: Option<Subscription>,
    settings_appearance: Option<Subscription>,
    settings_display: Option<DisplayId>,
    screen_layout: Option<ScreenLayout>,
    accessibility: AccessibilityState,
    environment: Option<EnvironmentMonitor>,
    environment_changes: Option<ChangeNotifier>,
    environment_task: Option<Task<()>>,
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
    let accessibility = read_display_options()
        .map(AccessibilityState::from)
        .unwrap_or_else(|error| {
            eprintln!("Ambien: 表示のアクセシビリティを読めません: {error}");
            AccessibilityState::default()
        });
    accessibility.apply_to_app(cx);
    let (change_notifier, change_receiver) = change_channel();
    let environment = {
        let notifier = change_notifier.clone();
        // The native block only signals; the newest state is read on the main thread.
        match EnvironmentMonitor::new(move || {
            notifier.notify();
        }) {
            Ok(monitor) => Some(monitor),
            Err(error) => {
                eprintln!("Ambien: 環境変更通知を登録できません: {error}");
                None
            }
        }
    };
    let owner_notifier = change_notifier.clone();
    let owner = cx.new(move |_| AppModel {
        scene: None,
        settings: None,
        _window_closed: None,
        scene_appearance: None,
        settings_appearance: None,
        settings_display: None,
        screen_layout: read_screen_layout().ok(),
        accessibility,
        environment,
        environment_changes: Some(owner_notifier),
        environment_task: None,
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
    // Re-read once after registration so a change made before it was observed is not missed.
    change_notifier.notify();
    let environment_task = owner.update(cx, |_, cx| {
        cx.spawn(async move |owner, cx| {
            while change_receiver.recv().await.is_ok() {
                if owner
                    .update(cx, |model, cx| model.environment_changed(cx))
                    .is_err()
                {
                    break; // The application owner has been released.
                }
            }
        })
    });
    owner.update(cx, |model, _| {
        model.environment_task = Some(environment_task)
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
            // Observer removal, then channel close, then the owner Task; never detach it.
            if let Some(mut monitor) = model.environment.take()
                && let Err(error) = monitor.shutdown()
            {
                eprintln!("Ambien: 環境通知の解除に失敗しました: {error}");
            }
            if let Some(changes) = model.environment_changes.take() {
                changes.close();
            }
            model.environment_task = None;
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

pub(crate) fn dispatch_settings_display(
    window_id: WindowId,
    display: Result<DisplayId, WindowGeometryError>,
    cx: &mut App,
) {
    cx.defer(move |cx| {
        let Some(owner) = cx.try_global::<AppRoot>().map(|root| root.0.clone()) else {
            return;
        };
        owner.update(cx, |model, cx| {
            model.settings_display_changed(window_id, display, cx);
        });
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
        // A scene reopened after both windows closed has no appearance observer to lean on.
        Theme::set_mode_now(Mode::from(cx.window_appearance()), cx);
        let audio = self.audio.shared();
        let accessibility = self.accessibility;
        self.scene = Some(open_window(
            WindowRole::Scene,
            accessibility,
            self.placement.as_ref(),
            move |window, cx| SceneView::new(window, cx, audio, accessibility),
            cx,
        )?);
        let handle = self.scene.expect("the scene was just opened");
        self.scene_appearance =
            Some(handle.update(cx, |_, window, _| follow_system_appearance(window))?);
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
            self.accessibility,
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
        self.settings_appearance =
            Some(handle.update(cx, |_, window, _| follow_system_appearance(window))?);
        // The first bounds callback only records the display this window was fitted to.
        self.settings_display = handle
            .update(cx, |_, window, _| capture_placement(window))
            .ok()
            .and_then(Result::ok)
            .map(|(_, display)| display);
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
            self.scene_appearance = None;
        }
        if self
            .settings
            .is_some_and(|window| window.window_id() == window_id)
        {
            self.settings = None;
            self.settings_appearance = None;
            self.settings_display = None;
        }
    }

    /// Re-reads the newest native state after any environment notification. A burst of
    /// notifications leaves one pending signal, so stale values are never queued.
    fn environment_changed(&mut self, cx: &mut Context<Self>) {
        match read_display_options() {
            Ok(options) => self.apply_accessibility(options.into(), cx),
            Err(error) => eprintln!("Ambien: 表示のアクセシビリティを読めません: {error}"),
        }
        // An accessibility change must not move a window the user placed. Re-fit only when
        // the screens or workareas themselves changed.
        match read_screen_layout() {
            Ok(layout) if self.screen_layout.as_ref() != Some(&layout) => {
                self.screen_layout = Some(layout);
                self.refit(WindowRole::Scene, cx);
                self.refit(WindowRole::Settings, cx);
            }
            Ok(_) => {}
            Err(error) => eprintln!("Ambien: 画面構成を読めません: {error}"),
        }
    }

    /// Re-fits one live fixed window from its baseline for its current screen. Failure
    /// leaves the last finite size and constraints in place. The untyped handle does not
    /// lease the window's root view, which the bounds observers triggered by
    /// `bounds_changed` update themselves.
    fn refit(&mut self, role: WindowRole, cx: &mut Context<Self>) {
        let (name, handle) = match role {
            WindowRole::Scene => ("シーン", self.scene.map(AnyWindowHandle::from)),
            WindowRole::Settings => ("設定", self.settings.map(AnyWindowHandle::from)),
        };
        let result = handle
            .map(|handle| handle.update(cx, |_, window, cx| refit_and_sync(window, role, cx)));
        match result {
            None | Some(Ok(Ok(()))) => {}
            Some(Ok(Err(error))) => {
                eprintln!("Ambien: {name}窓を現在の画面へ収め直せません: {error}")
            }
            Some(Err(error)) => eprintln!("Ambien: {name}窓を現在の画面へ収め直せません: {error}"),
        }
    }

    fn settings_display_changed(
        &mut self,
        window_id: WindowId,
        display: Result<DisplayId, WindowGeometryError>,
        cx: &mut Context<Self>,
    ) {
        if self
            .settings
            .is_none_or(|window| window.window_id() != window_id)
        {
            return; // A retired settings window cannot refit its replacement.
        }
        // An unreadable display keeps the last known fit; the first capture is only recorded.
        let Ok(display) = display else {
            return;
        };
        if self
            .settings_display
            .replace(display)
            .is_some_and(|old| old != display)
        {
            self.refit(WindowRole::Settings, cx);
        }
    }

    fn apply_accessibility(&mut self, accessibility: AccessibilityState, cx: &mut Context<Self>) {
        if accessibility == self.accessibility {
            return;
        }
        self.accessibility = accessibility;
        accessibility.apply_to_app(cx);
        if let Some(handle) = self.scene
            && let Err(error) = handle.update(cx, |view, window, cx| {
                view.set_accessibility(accessibility, window, cx);
            })
        {
            eprintln!("Ambien: シーンへアクセシビリティを反映できません: {error}");
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
                // A move onto another display re-fits from the baseline. The first capture
                // only records the display the window was just fitted to.
                let moved = self.placement_display.is_some_and(|old| old != display);
                if self.remember_placement(placement, display, cx) {
                    self.schedule_save(cx);
                }
                if moved {
                    self.refit(WindowRole::Scene, cx);
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

/// Re-fits a window and, when AppKit changed its frame, tells GPUI. AppKit's resize/move
/// callbacks run synchronously inside `refit_window`, but GPUI's handlers need the App
/// borrow that the caller holds and silently drop the update.
fn refit_and_sync(
    window: &mut Window,
    role: WindowRole,
    cx: &mut App,
) -> Result<(), WindowGeometryError> {
    if refit_window(window, role)? {
        window.bounds_changed(cx);
    }
    Ok(())
}

/// Follows the system light/dark appearance for this window's lifetime. The Theme is
/// global, so every observer re-reads the current appearance rather than a stale one.
fn follow_system_appearance(window: &mut Window) -> Subscription {
    window.observe_window_appearance(|_, cx| {
        cx.defer(|cx| {
            Theme::set_mode_now(Mode::from(cx.window_appearance()), cx);
        });
    })
}

fn open_window<V: Render + 'static>(
    role: WindowRole,
    accessibility: AccessibilityState,
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
            accessibility.scene_background(),
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
