//! The four non-main riftflow windows: `aux-window`, `opgg-window`,
//! `ongoing-game-window`, `cd-timer-window`.
//!
//! `windows.rs` already owns everything shared — the `WindowSpec` table, the base
//! 17 handlers per `window-manager-main/<suffix>` sub-namespace, `ready-to-show`
//! (`on_window_ready`), the close semantics (`handle_close`) and the bounds
//! memory. This module adds only what the per-window subclasses in
//! `src/main/shards/window-manager/*/{window,windows}.ts` add on top:
//!
//! * the entry-file mapping (verified against `renderer-dist/`, not guessed)
//! * the `enabled` / `autoShow` / `pinned` reactions that create, reveal, hide
//!   and truly close each window, i.e. the thing that makes them appear without
//!   anyone pressing a button
//! * a settings schema per window, which the original registers in
//!   `SettingFactoryMain` from `BaseRiftFlowWindow`'s constructor. Without it a
//!   renderer `setting.set(...)` writes the row but never pushes the `settings`
//!   prop the store is bound to, so the toggle snaps back.
//!
//! Known gaps, each named rather than faked:
//! * `repositionToAlignLeagueClientUx` (aux/opgg) and `sendInGame` (cd-timer) are
//!   registered by `windows::register_window` as explicit business errors:
//!   `riftflow-native` exposes process/foreground probes only, no window placement
//!   and no input injection, so there is nothing to align to and nothing to type
//!   with. This module must not re-register those names — `on_call` treats a
//!   duplicate as a hard error.
//! * `opacity` has no Tauri equivalent, same as `windows.rs`.
//! * the cd-timer's `gameTime` comes from `GET https://127.0.0.1:2999/liveclientdata/gamestats`,
//!   polled every 4 s while the overlay is in use — the same live-client data
//!   server `respawn_timer.rs` reads, reached the same way.
//!
//! `showShortcut` is bound for real, through `crate::hotkeys` (the
//! `keyboard-shortcuts-main` shard), including the ongoing-game window's
//! press/release `fakeShow` and the cd-timer's `show(true)`; when that shard
//! reports no native input the binding is dropped and the setting is cleared,
//! which is what the original's `catch` does.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tauri::{AppHandle, Manager, WebviewWindow};

use riftflow_ipc::{RiftFlowIpcMain, IpcError};
use riftflow_lcu::http::LcuHttp;
use riftflow_lcu::{ConnectionState, LeagueClientMain};
use riftflow_state::StateSync;
use riftflow_store::settings::{
    SettingDeclaration, SettingFactory, TransformContext, Validator, boolean, nullable_string,
    number, string_enum,
};

use crate::hotkeys::{RegistrationType, ShortcutDetails};
use crate::windows::{self, WM_NAMESPACE};
use crate::AppState;

pub const AUX_WINDOW: &str = "aux-window";
pub const OPGG_WINDOW: &str = "opgg-window";
pub const ONGOING_GAME_WINDOW: &str = "ongoing-game-window";
pub const CD_TIMER_WINDOW: &str = "cd-timer-window";

/// Every window this module owns, in the order the original's lifecycle
/// controller initializes them. `main-window` deliberately absent: `main.rs`
/// creates and registers it.
pub const AUX_SUFFIXES: [&str; 4] =
    [AUX_WINDOW, OPGG_WINDOW, ONGOING_GAME_WINDOW, CD_TIMER_WINDOW];

/// The LCU sub-states the show/hide decisions read. Both are hydrated by
/// `lc_state.rs`, so this never adds a request.
const LEAGUE: &str = "league-client-main";

/// `RiftFlowCdTimerWindow.GAME_STATS_POLL_INTERVAL` / the original's reaction
/// `delay: 500`. The tick is only a backstop: LCU events and setting writes both
/// nudge the loop directly, so a phase change still acts immediately.
const TICK: Duration = Duration::from_millis(500);

/// `RiftFlowCdTimerWindow.GAME_STATS_POLL_INTERVAL` (`cd-timer-window/windows.ts:21`).
const GAME_STATS_POLL_INTERVAL: Duration = Duration::from_millis(4000);

/// `GameClientHttpApiAxiosHelper.getGameStats` (`game-client/index.ts:42`), i.e.
/// the live-client data server's game clock.
const GAME_STATS_PATH: &str = "/liveclientdata/gamestats";

/// `NATIVE_SUPPORT.nativeInput.available`, i.e. what the original's ongoing-game
/// and cd-timer presence checks AND with `enabled`. It is `false` for a normal
/// (non-elevated) process, and there the faithful behaviour is to not build the
/// overlay at all — the shortcut that reveals it cannot fire either, so the
/// window would only ever be an invisible click-through band.
pub fn native_input_available() -> bool {
    crate::hotkeys::input_available()
}

/// `RiftFlow*Window.HTML_ENTRY`, matched against the real files in
/// `riftflow-rs/renderer-dist/`.
pub fn html_entry(suffix: &str) -> Option<&'static str> {
    match suffix {
        AUX_WINDOW => Some("aux-window.html"),
        OPGG_WINDOW => Some("opgg-window.html"),
        ONGOING_GAME_WINDOW => Some("ongoing-game-window.html"),
        CD_TIMER_WINDOW => Some("cd-timer-window.html"),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn is_aux_suffix(suffix: &str) -> bool {
    AUX_SUFFIXES.contains(&suffix)
}

pub fn namespace_of(suffix: &str) -> String {
    format!("{WM_NAMESPACE}/{suffix}")
}

/// `RiftFlowOpggWindow.shortcutTargetId` and friends: `${namespace}/show`. Kept for
/// the day `keyboard-shortcuts-main` lands; the target ids are part of the
/// persisted shortcut registry.
pub fn shortcut_target_id(suffix: &str) -> String {
    format!("{}/show", namespace_of(suffix))
}

// ---------------------------------------------------------------- IPC surface

/// Registers every non-main window's `window-manager-main/<suffix>` surface: the
/// base 17 methods plus the per-window extras, through the same
/// [`windows::register_window`] the main window uses, and the settings schema
/// each subclass hands to `settingFactory.register`.
///
/// Call this before any window loads — the renderer's first IPC call subscribes to
/// these surfaces, and an unknown namespace rejects rather than returning empty.
pub fn register(
    ipc: &Arc<RiftFlowIpcMain>,
    state_sync: &Arc<StateSync>,
    manager: &Arc<windows::WindowManager>,
    settings: &Arc<SettingFactory>,
) -> anyhow::Result<()> {
    for suffix in AUX_SUFFIXES {
        // `show`/`hide`/`close`/`setSize`/`setPosition`/`resetPosition`/
        // `repositionWindowIfInvisible`/`setIgnoreMouseEvents`, plus the extras
        // (`repositionToAlignLeagueClientUx`, `sendInGame`) it already answers —
        // as honest errors — for these four suffixes.
        windows::register_window(ipc, state_sync, manager, settings, suffix)?;
        register_settings(state_sync, settings, suffix);
    }

    Ok(())
}

/// The settings surface for one window. A duplicate registration means another
/// module owns that namespace already, which is fine: the watcher falls back to
/// the store row whenever no service answers for it.
fn register_settings(state_sync: &Arc<StateSync>, settings: &Arc<SettingFactory>, suffix: &str) {
    let namespace = namespace_of(suffix);

    let service = match settings.register(&namespace, declarations(suffix)) {
        Ok(service) => service,
        Err(error) => {
            eprintln!("[windows] {namespace} settings schema not owned here: {error}");
            return;
        }
    };

    // `applyToState()`, then the push half of what a MobX mutation did implicitly:
    // `setting-factory-main/set` has to land in the synced prop, or the store that
    // issued the write never sees its own value.
    if let Err(error) = service.apply_to_state() {
        eprintln!("[windows] {namespace} settings load failed: {error}");
    }

    let sync = state_sync.clone();
    service.set_change_sink(Arc::new(move |ns, key, value| {
        let _ = sync.set(ns, "settings", key, value.clone());
    }));
}

/// `BaseRiftFlowWindow`'s base schema (`pinned`, `opacity`) plus that subclass's
/// `settingSchema`, defaults straight from each `*/state.ts`.
fn declarations(suffix: &str) -> Vec<SettingDeclaration> {
    let mut out = vec![
        // the two overlays force `pinned` true through a zod `transform`
        if matches!(suffix, ONGOING_GAME_WINDOW | CD_TIMER_WINDOW) {
            pinned_forced_true()
        } else {
            flag_setting("pinned", true)
        },
        amount_setting("opacity", json!(1)),
    ];

    match suffix {
        AUX_WINDOW => out.extend([
            flag_setting("enabled", true),
            flag_setting("autoShow", true),
            flag_setting("showSkinSelector", false),
        ]),
        OPGG_WINDOW => out.extend([
            flag_setting("enabled", true),
            flag_setting("autoShow", true),
            setting("showShortcut", Value::Null, Some(nullable_string())),
        ]),
        ONGOING_GAME_WINDOW => out.extend([
            flag_setting("enabled", false),
            setting("showShortcut", Value::Null, Some(nullable_string())),
        ]),
        CD_TIMER_WINDOW => out.extend([
            flag_setting("enabled", false),
            setting("showShortcut", Value::Null, Some(nullable_string())),
            choice("timerType", "countdown", &["countdown", "countup"]),
            flag_setting("reverseAdjustmentDirection", false),
        ]),
        _ => {}
    }

    out
}

fn setting(key: &str, default: Value, schema: Option<Validator>) -> SettingDeclaration {
    SettingDeclaration {
        key: key.to_string(),
        default,
        schema,
        transform: None,
        side_effect: None,
    }
}

fn flag_setting(key: &str, default: bool) -> SettingDeclaration {
    setting(key, json!(default), Some(boolean()))
}

fn amount_setting(key: &str, default: Value) -> SettingDeclaration {
    setting(key, default, Some(number()))
}

fn choice(key: &str, default: &str, allowed: &[&str]) -> SettingDeclaration {
    setting(key, json!(default), Some(string_enum(allowed)))
}

fn pinned_forced_true() -> SettingDeclaration {
    SettingDeclaration {
        key: "pinned".to_string(),
        default: json!(true),
        schema: Some(boolean()),
        transform: Some(Arc::new(|_: &TransformContext| json!(true))),
        side_effect: None,
    }
}

// ------------------------------------------------------------------- building

/// Builds one non-main window with its `WindowSpec` and entry file.
///
/// Deliberately ungated: it ignores `enabled` and the native-input hook, so an
/// integrator can open any of the four on demand. The boot path the original has
/// is [`create_enabled`], and the watcher enforces the same condition afterwards
/// (it closes a window whose `enabled` goes false).
///
/// Delegates to `main.rs`'s `create_window`, which is the single place the shim's
/// `riftflowWindowType` (the suffix, i.e. the original's
/// `--riftflow-window-type=<suffix>`), the Electron-equivalent creation options, the
/// `ready-to-show` hook and the `WindowManager` attach already live. Windows are
/// created hidden; the watchers below decide when each one reveals itself.
pub fn create(app: &AppHandle, suffix: &str) -> anyhow::Result<WebviewWindow> {
    if suffix == windows::MAIN_WINDOW {
        anyhow::bail!("main-window is created by the shell, not by the aux-window layer");
    }

    let entry = html_entry(suffix).ok_or_else(|| {
        anyhow::anyhow!(
            "no renderer entry html is known for {suffix}; expected one of {}",
            AUX_SUFFIXES.join(", ")
        )
    })?;

    let window = crate::create_window(app, suffix, entry)?;
    eprintln!("[windows] created {suffix} ({entry})");
    Ok(window)
}

#[allow(dead_code)]
pub fn create_aux(app: &AppHandle) -> anyhow::Result<WebviewWindow> {
    create(app, AUX_WINDOW)
}

#[allow(dead_code)]
pub fn create_opgg(app: &AppHandle) -> anyhow::Result<WebviewWindow> {
    create(app, OPGG_WINDOW)
}

#[allow(dead_code)]
pub fn create_ongoing_game(app: &AppHandle) -> anyhow::Result<WebviewWindow> {
    create(app, ONGOING_GAME_WINDOW)
}

#[allow(dead_code)]
pub fn create_cd_timer(app: &AppHandle) -> anyhow::Result<WebviewWindow> {
    create(app, CD_TIMER_WINDOW)
}

/// The boot-time call: create exactly the windows the user has enabled, which is
/// what the original's `[enabled, isManagerFinishedInit]` reaction does on its
/// first (delayed 500 ms) pass. Disabled windows are not built at all and stay
/// unknown to `WindowManager` until `enabled` flips.
pub fn create_enabled(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();

    for suffix in AUX_SUFFIXES {
        let enabled = flag(&state.settings, &state.state_sync, suffix, "enabled");
        if presence_allowed(suffix, enabled) {
            create(app, suffix)?;
        }
    }

    Ok(())
}

/// Only the two overlays are gated on the native input hook, as in the original.
fn overlay_needs_hook(suffix: &str) -> bool {
    matches!(suffix, ONGOING_GAME_WINDOW | CD_TIMER_WINDOW)
}

/// `enabled && NATIVE_SUPPORT.nativeInput.available`, with
/// `RIFTFLOW_OPEN_OVERLAYS=1` as the developer's opt-out: the hook needs an elevated
/// process, and without one these two windows would never appear in a dev run.
fn presence_allowed(suffix: &str, enabled: bool) -> bool {
    enabled && (!overlay_needs_hook(suffix) || native_input_available()
        || std::env::var("RIFTFLOW_OPEN_OVERLAYS").is_ok())
}

// ------------------------------------------------------------------- watching

/// `WindowManagerMain.onFinish()`'s side of the watchers: from here on
/// `isManagerFinishedInit` is true, so the `enabled` reactions are live.
pub fn start(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();

    let controls = WindowControls {
        app: app.clone(),
        state_sync: state.state_sync.clone(),
        settings: state.settings.clone(),
    };

    let mut watcher = Watcher {
        controls,
        league_client: state.league_client.clone(),
        applied: HashMap::new(),
        game_time_stop: None,
    };

    // the events the decisions depend on. `RadixEventEmitter::on` is synchronous
    // and the subscription is never removed, which is the point: the watchers live
    // as long as the shell.
    let (sender, nudges) = mpsc::unbounded_channel::<()>();
    let emitter = state.league_client.emitter();
    for uri in [
        "/lol-gameflow/v1/gameflow-phase",
        "/lol-gameflow/v1/session",
        "/lol-champ-select/v1/session",
    ] {
        let sender = sender.clone();
        emitter.on(uri, move |_payload, _params| {
            let _ = sender.send(());
        })?;
    }

    tauri::async_runtime::spawn(async move { watcher.run(nudges).await });
    Ok(())
}

#[derive(Default)]
struct Applied {
    /// the per-window work that must happen exactly once after `ready`
    ready_done: bool,
    /// Auto-show OP.GG only once per champion-select phase. A manual close must stick.
    opgg_shown_in_champ_select: bool,
    pinned: Option<bool>,
    fake_show: Option<bool>,
    overlay: Option<bool>,
    /// last seen `isGameClientForeground()`, so the overlay is re-raised on the
    /// transition instead of on every poll
    game_foreground: Option<bool>,
    /// the shortcut id currently bound for this window, i.e. what `showShortcut`
    /// was the last time the binding was touched
    shortcut: Option<String>,
    gated_warned: bool,
}

impl Applied {
    fn take_opgg_auto_show(&mut self, phase: Option<&str>, eligible: bool) -> bool {
        if phase != Some("ChampSelect") {
            self.opgg_shown_in_champ_select = false;
            return false;
        }
        if !eligible || self.opgg_shown_in_champ_select {
            return false;
        }
        self.opgg_shown_in_champ_select = true;
        true
    }
}

/// The window controls the shortcuts call, which have to outlive the watcher's
/// borrow of itself: `KeyboardShortcutsMain` keeps the callback for the process
/// lifetime, so it can only own cheap clones.
#[derive(Clone)]
struct WindowControls {
    app: AppHandle,
    state_sync: Arc<StateSync>,
    settings: Arc<SettingFactory>,
}

impl WindowControls {
    fn window(&self, suffix: &str) -> Option<WebviewWindow> {
        self.app.get_webview_window(suffix)
    }

    /// `state.show`, the prop the original's `show()` / `hide()` guard on.
    fn shown(&self, suffix: &str) -> bool {
        self.state_sync
            .get(&namespace_of(suffix), "state", "show")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    fn ready(&self, suffix: &str) -> bool {
        self.state_sync
            .get(&namespace_of(suffix), "state", "ready")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    /// `BaseRiftFlowWindow.show(inactive)`: reveals a hidden window and nothing else —
    /// no restore, no focus, unlike `showOrRestore`.
    fn reveal_if_hidden(&self, suffix: &str, inactive: bool) {
        if self.shown(suffix) {
            return;
        }
        let Some(window) = self.window(suffix) else {
            return;
        };

        let _ = window.show();
        if !inactive {
            let _ = window.set_focus();
        }
        let _ = self
            .state_sync
            .set(&namespace_of(suffix), "state", "show", json!(true));
    }

    /// The ongoing-game window's resting state: the whole overlay is transparent
    /// except while `fakeShow` holds it open.
    fn apply_click_through(&self, suffix: &str) {
        if let Some(window) = self.window(suffix) {
            let _ = window.set_ignore_cursor_events(true);
        }
    }

    /// `setIgnoreMouseEvents(false)`: the panel takes the mouse for as long as the
    /// reveal key is held down.
    fn accept_mouse(&self, suffix: &str) {
        if let Some(window) = self.window(suffix) {
            let _ = window.set_ignore_cursor_events(false);
        }
    }

    /// `BaseRiftFlowWindow.hide()`.
    fn conceal(&self, suffix: &str) {
        if !self.shown(suffix) {
            return;
        }
        if let Some(window) = self.window(suffix) {
            let _ = window.hide();
            let _ = self
                .state_sync
                .set(&namespace_of(suffix), "state", "show", json!(false));
        }
    }

    /// `RiftFlowCdTimerWindow._applyOverlayWindowBehavior()`, called after every
    /// programmatic show because Windows can drop the top-most band when the game
    /// takes focus.
    fn apply_overlay(&self, suffix: &str) {
        if let Some(window) = self.window(suffix) {
            windows::set_topmost(&window, suffix, true);
            let _ = window.set_ignore_cursor_events(false);
        }
    }

    /// `this.settings.pinned = value` on the main side: the propSync push is what
    /// the renderer sees, the row is what survives a restart.
    fn write_setting(&self, suffix: &str, key: &str, value: Value) {
        let namespace = namespace_of(suffix);

        if let Some(service) = self.settings.get_service(&namespace) {
            let service = service.clone();
            let key = key.to_string();
            tauri::async_runtime::spawn(async move {
                let _ = service.set(&key, value).await;
            });
            return;
        }

        // degraded path (no service for this namespace): commit the same two ways
        if value.is_null() {
            let _ = self.settings.remove_raw(&namespace, key);
        } else {
            let _ = self.settings.save_raw(&namespace, key, &value);
        }
        let _ = self.state_sync.set(&namespace, "settings", key, value);
    }

    /// `GameClientMain.isGameClientForeground()`: the foreground window belongs to
    /// League of Legends.exe.
    fn game_client_foreground(&self) -> bool {
        riftflow_native::pids_by_name(riftflow_native::GAME_CLIENT_PROCESS_NAME)
            .into_iter()
            .any(|pid| riftflow_native::is_process_foreground(pid))
    }

    fn set_fake_show(&self, value: bool) {
        set_fake_show(&self.app, value);
    }
}

struct Watcher {
    controls: WindowControls,
    league_client: Arc<LeagueClientMain>,
    applied: HashMap<String, Applied>,
    /// `_gameStatsPollTimer`: one poll while `shouldUseCdTimer` is on
    game_time_stop: Option<Arc<AtomicBool>>,
}

impl Watcher {
    fn state_prop(&self, suffix: &str, id: &str, path: &str) -> Option<Value> {
        self.controls.state_sync.get(&namespace_of(suffix), id, path)
    }

    fn flag(&self, suffix: &str, key: &str) -> bool {
        flag(
            &self.controls.settings,
            &self.controls.state_sync,
            suffix,
            key,
        )
    }

    fn enabled(&self, suffix: &str) -> bool {
        self.flag(suffix, "enabled")
    }

    fn ready(&self, suffix: &str) -> bool {
        self.controls.ready(suffix)
    }

    fn window(&self, suffix: &str) -> Option<WebviewWindow> {
        self.controls.window(suffix)
    }

    /// The two overlays also want the native input hook, exactly as the original's
    /// `enabled && NATIVE_SUPPORT.nativeInput.available` reaction says. Without it
    /// the reveal shortcut can never fire, so an enabled-but-hookless overlay would
    /// be an invisible click-through band — reported, not silently shown.
    /// `RIFTFLOW_OPEN_OVERLAYS=1` opts out for a dev run (the hook needs elevation).
    fn overlay_should_exist(&mut self, suffix: &str, enabled: bool) -> bool {
        let should = presence_allowed(suffix, enabled);

        if enabled && !should {
            let applied = self.applied.entry(suffix.to_string()).or_default();
            if !applied.gated_warned {
                applied.gated_warned = true;
                eprintln!(
                    "[windows] {suffix} stays closed: the original only builds it once the native \
                     input hook is up (set RIFTFLOW_OPEN_OVERLAYS=1 to open it anyway)"
                );
            }
        }

        should
    }
}

impl Watcher {
    async fn run(&mut self, mut nudges: mpsc::UnboundedReceiver<()>) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            // `Some(_)` keeps this branch out of the way once the senders are gone,
            // instead of spinning on an endless stream of `None`
            tokio::select! {
                _ = tick.tick() => {}
                Some(()) = nudges.recv() => {}
            }
            self.evaluate();
        }
    }

    fn evaluate(&mut self) {
        let connected = self.league_client.state.connection_state() == ConnectionState::Connected;
        let phase = self
            .league("gameflow", "phase")
            .and_then(|value| value.as_str().map(str::to_string));
        let session = self.league("gameflow", "session");
        let spectating = self
            .league("champSelect", "session")
            .and_then(|value| value.get("isSpectating").and_then(Value::as_bool))
            .unwrap_or(false);

        self.aux(connected, phase.as_deref(), spectating);
        self.opgg(phase.as_deref());
        self.ongoing_game();
        self.cd_timer(&session);
    }

    // --- per window

    /// `RiftFlowAuxWindow._watchAuxWindow`.
    fn aux(&mut self, connected: bool, phase: Option<&str>, spectating: bool) {
        let suffix = AUX_WINDOW;
        let enabled = self.enabled(suffix);
        self.set_presence(suffix, enabled);
        self.apply_pinned(suffix);

        if !enabled {
            return;
        }

        // the connectionState reaction is not part of `showTiming`: it hides the
        // window even when `autoShow` is off
        if !connected {
            self.conceal(suffix);
            return;
        }

        let auto_show = self.flag(suffix, "autoShow");
        let ready = self.ready(suffix);

        match aux_show_timing(auto_show, ready, phase, spectating) {
            ShowTiming::Show => self.reveal(suffix, false),
            ShowTiming::Hide => self.conceal(suffix),
            ShowTiming::Ignore => {}
        }
    }

    /// `RiftFlowOpggWindow._watchOpggWindow`. Drafts reveal it; nothing about the
    /// phase ever hides it — leaving a lobby keeps the panel up until the player
    /// closes it, which is the original's `return 'normal'` no-op. The
    /// hovered-player follow-up is renderer-driven: the main window calls
    /// `repositionToAlignLeagueClientUx` + `show` per hover.
    fn opgg(&mut self, phase: Option<&str>) {
        let suffix = OPGG_WINDOW;
        let enabled = self.enabled(suffix);
        self.set_presence(suffix, enabled);
        self.apply_pinned(suffix);
        self.sync_shortcut(suffix, RegistrationType::Normal);

        let eligible = enabled && self.flag(suffix, "autoShow") && self.ready(suffix);
        if self.applied.entry(suffix.to_string()).or_default().take_opgg_auto_show(phase, eligible) {
            self.reveal(suffix, false);
        }
    }

    /// `RiftFlowOngoingGameWindow._watchOngoingGameWindow`.
    fn ongoing_game(&mut self) {
        let suffix = ONGOING_GAME_WINDOW;
        let enabled = self.enabled(suffix);
        let should_exist = self.overlay_should_exist(suffix, enabled);
        self.set_presence(suffix, should_exist);
        self.apply_pinned(suffix);
        self.sync_shortcut(suffix, RegistrationType::Stateful);

        if !should_exist {
            return;
        }

        let ready = self.ready(suffix);

        // `reaction(() => this.state.fakeShow)`, which the stateful shortcut drives
        // through [`set_fake_show`]; the memo makes this pass do the original's
        // `fireImmediately` call, i.e. start out click-through
        let fake_show = self
            .state_prop(suffix, "state", "fakeShow")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let (reveal_now, mouse_changed) = {
            let applied = self.applied.entry(suffix.to_string()).or_default();

            // `reaction(() => this.state.ready)`: the overlay is click-through from
            // the moment it exists and it shows itself — the content stays hidden
            // until `fakeShow` says otherwise
            let reveal = ready && !applied.ready_done;
            if reveal {
                applied.ready_done = true;
            }

            let mouse = applied.fake_show != Some(fake_show);
            applied.fake_show = Some(fake_show);
            (reveal, mouse)
        };

        if mouse_changed {
            if fake_show {
                self.controls.apply_overlay(suffix);
            } else {
                self.controls.apply_click_through(suffix);
            }
        }
        if reveal_now {
            self.reveal(suffix, false);
        }
    }

    /// `RiftFlowCdTimerWindow._watchCdTimerWindow`.
    fn cd_timer(&mut self, session: &Option<Value>) {
        let suffix = CD_TIMER_WINDOW;
        let enabled = self.enabled(suffix);
        let should_exist = self.overlay_should_exist(suffix, enabled);
        self.set_presence(suffix, should_exist);
        self.apply_pinned(suffix);
        self.sync_shortcut(suffix, RegistrationType::Normal);

        if !should_exist {
            return;
        }

        let ready = self.ready(suffix);

        let first_ready = {
            let applied = self.applied.entry(suffix.to_string()).or_default();
            let first = ready && !applied.ready_done;
            if first {
                applied.ready_done = true;
            }
            first
        };

        if first_ready {
            // `_applyOverlayWindowBehavior` on `state.ready`, minus `setSkipTaskbar`
            // which the builder has to set and the spec already carries. The rest of
            // the re-applings happen on the show transitions below, as in the original
            self.controls.apply_overlay(suffix);
        }

        // `shouldUseCdTimer`: an InProgress game in a queue the timer knows about
        let game_mode = session
            .as_ref()
            .and_then(|value| {
                value
                    .get("gameData")?
                    .get("queue")?
                    .get("gameMode")?
                    .as_str()
                    .map(str::to_string)
            });
        let session_phase = session
            .as_ref()
            .and_then(|value| value.get("phase"))
            .and_then(|value| value.as_str())
            .map(str::to_string);

        let should_show = cd_timer_should_show(
            ready,
            session_phase.as_deref(),
            game_mode.as_deref(),
            &self.supported_game_modes(),
        );

        let (reveal_now, conceal_now) = {
            let applied = self.applied.entry(suffix.to_string()).or_default();
            if applied.overlay == Some(should_show) {
                (false, false)
            } else {
                applied.overlay = Some(should_show);
                (should_show, !should_show)
            }
        };

        if reveal_now {
            // `show(true)` + `_applyOverlayWindowBehavior()`: revealed without
            // stealing focus from the game
            self.reveal(suffix, true);
            self.controls.apply_overlay(suffix);
            // the second `shouldUseCdTimer` reaction: start polling, immediately
            // and then every four seconds
            self.start_game_time_poll();
        } else if conceal_now {
            self.conceal(suffix);
            // `clearInterval` + `setGameTime(null)`
            self.stop_game_time_poll();
        }

        self.reassert_on_game_foreground(suffix);
    }

    /// Re-take the top-most band at the moment `League of Legends.exe` becomes the
    /// foreground window, which is when Windows sinks the overlay below the borderless
    /// game client. The original gets this from Electron re-applying
    /// `setAlwaysOnTop(true, 'screen-saver', 1)` on the window's own `show`/`focus`
    /// events; a poller has to act on the transition instead, because re-asserting on
    /// every tick is the same focus-jumping bug the reveal path already had.
    fn reassert_on_game_foreground(&mut self, suffix: &str) {
        let shown = self.controls.shown(suffix);
        let foreground = shown && self.controls.game_client_foreground();

        let rising = {
            let applied = self.applied.entry(suffix.to_string()).or_default();
            let rising = foreground && applied.game_foreground != Some(true);
            applied.game_foreground = if shown { Some(foreground) } else { None };
            rising
        };

        if rising {
            self.controls.apply_overlay(suffix);
            eprintln!("[cd-timer-window] 游戏抢到前台，重新置顶");
        }
    }

    /// `_updateGameStats` on a timer. `getGameStats` answers from the game process,
    /// so this only has a clock to read while a match is actually running; every
    /// failure path stores `null` exactly like the original's `catch`, which is what
    /// makes the window fall back to the non-countdown layout instead of freezing a
    /// stale number.
    fn start_game_time_poll(&mut self) {
        if let Some(previous) = self.game_time_stop.replace(Arc::new(AtomicBool::new(false))) {
            previous.store(true, Ordering::Relaxed);
        }

        let stop = self.game_time_stop.clone().unwrap_or_default();
        let sync = self.controls.state_sync.clone();
        let game_client = LcuHttp::new(crate::respawn_timer::LIVE_CLIENT_DATA_PORT, String::new());

        tauri::async_runtime::spawn(async move {
            loop {
                let game_time = match game_client.get(GAME_STATS_PATH).await {
                    Ok(stats) => game_time_of(&stats),
                    Err(error) => {
                        eprintln!("[cd-timer-window] {GAME_STATS_PATH} 不可达: {error}");
                        None
                    }
                };

                set_game_time(&sync, game_time);

                // the flag is checked around the wait rather than via `abort()`, so a
                // stopped poll finishes its current request instead of being cancelled
                // mid-flight
                for _ in 0..GAME_STATS_POLL_INTERVAL.as_millis() / 200 {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        });
    }

    fn stop_game_time_poll(&mut self) {
        if let Some(stop) = self.game_time_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }

        set_game_time(&self.controls.state_sync, None);
    }

    // --- shared primitives

    fn league(&self, state_id: &str, path: &str) -> Option<Value> {
        self.controls.state_sync.get(LEAGUE, state_id, path)
    }

    fn supported_game_modes(&self) -> Vec<String> {
        self.state_prop(CD_TIMER_WINDOW, "state", "supportedGameModes")
            .and_then(|value| {
                let list = value.as_array()?;
                Some(
                    list.iter()
                        .filter_map(|entry| entry.get("gameMode")?.as_str().map(str::to_string))
                        .collect(),
                )
            })
            .unwrap_or_default()
    }

    /// `createWindow()` / `close(true)` from the `enabled` reaction. Creation hops to
    /// the main thread, which is what wry/WebView2 wants for window construction.
    fn set_presence(&self, suffix: &str, should_exist: bool) {
        let exists = self.window(suffix).is_some();

        if should_exist && !exists {
            let Some(entry) = html_entry(suffix) else {
                return;
            };
            let queue = self.controls.app.clone();
            let handle = self.controls.app.clone();
            let label = suffix.to_string();

            // best effort: the next tick sees the window and takes over
            if let Err(error) = queue.run_on_main_thread(move || {
                if handle.get_webview_window(&label).is_some() {
                    return;
                }
                if let Err(error) = crate::create_window(&handle, &label, entry) {
                    eprintln!("[windows] {label} could not be created: {error}");
                }
            }) {
                eprintln!("[windows] {suffix} creation not queued: {error}");
            }
        } else if !should_exist && exists {
            self.close_forced(suffix);
        }
    }

    /// `BaseRiftFlowWindow.close(trueClose = true)`: `_trueClose` first, so
    /// `handle_close` lets the window actually go instead of hiding it.
    fn close_forced(&self, suffix: &str) {
        let state = self.controls.app.state::<AppState>();
        if let Some(window) = self.window(suffix) {
            state.window_manager.mark_true_close(suffix);
            let _ = window.close();
            eprintln!("[windows] {suffix} closed");
        }
    }

    /// `showOrRestore(inactive)` — the same helper `windows.rs` uses for the
    /// renderer's `show` call, so the pushed props are identical either way.
    fn reveal(&self, suffix: &str, inactive: bool) {
        // The watcher polls every 500 ms, so a "this window should be visible"
        // decision is re-delivered on every tick. Re-asserting `show()` +
        // `set_focus()` each time made the champ-select window jump back over
        // whatever the user clicked and made closing it look impossible; the
        // original acted on the transition, not on the poll (`BaseRiftFlowWindow
        // .showOrRestore` starts with `if (!this.state.show)`).
        if self.controls.shown(suffix) {
            return;
        }

        if let Some(window) = self.window(suffix) {
            windows::show_or_restore(
                &window,
                inactive,
                &self.controls.state_sync,
                suffix,
            );
        }
    }

    /// `BaseRiftFlowWindow.hide()`, which is a no-op for an already hidden window.
    fn conceal(&self, suffix: &str) {
        self.controls.conceal(suffix);
    }

    /// `_baseWindowObservations`' `pinned` reaction. The two overlays re-assert
    /// always-on-top through `_applyOverlayWindowBehavior` as well, which their own
    /// branches above do.
    fn apply_pinned(&mut self, suffix: &str) {
        let Some(window) = self.window(suffix) else {
            return;
        };
        let pinned = self.flag(suffix, "pinned");

        let applied = self.applied.entry(suffix.to_string()).or_default();
        if applied.pinned != Some(pinned) {
            applied.pinned = Some(pinned);
            windows::set_topmost(&window, suffix, pinned);
        }
    }

    /// The `reaction(() => this.settings.showShortcut)` of each subclass: unbind the
    /// old target, bind the new one, and on a failure clear the setting like the
    /// original's `catch` does. Registration is a no-op with no native input hook,
    /// which `hotkeys` itself reports.
    fn sync_shortcut(&mut self, suffix: &str, registration: RegistrationType) {
        let current = self
            .state_prop(suffix, "settings", "showShortcut")
            .and_then(|value| value.as_str().map(str::to_string));

        {
            let applied = self.applied.entry(suffix.to_string()).or_default();
            if applied.shortcut == current {
                return;
            }
            applied.shortcut = current.clone();
        }

        let target_id = shortcut_target_id(suffix);
        let Some(shard) = crate::hotkeys::instance() else {
            eprintln!("[windows] {suffix}: no keyboard-shortcuts-main shard, {target_id} unbound");
            return;
        };
        shard.unregister_by_target_id(&target_id);

        let Some(shortcut_id) = current else {
            return;
        };

        let controls = self.controls.clone();
        let label = suffix.to_string();
        let is_opgg = suffix == OPGG_WINDOW;

        // one closure type for `register`'s `impl Fn`, whatever the window's logic is
        type ShortcutFn = dyn Fn(&ShortcutDetails) + Send + Sync + 'static;
        let action: Arc<ShortcutFn> = match registration {
            // `RiftFlowOpggWindow`: pin-on-demand while the game is up, then toggle.
            // The order is the original's — the pin happens before the toggle, so
            // the first reveal from in-game already lands on top.
            RegistrationType::Normal if is_opgg => Arc::new(move |_event: &ShortcutDetails| {
                if !controls.shown(&label) && controls.game_client_foreground() {
                    controls.write_setting(&label, "pinned", json!(true));
                }
                if controls.shown(&label) {
                    controls.conceal(&label);
                } else {
                    controls.reveal_if_hidden(&label, false);
                }
            }),
            // `RiftFlowCdTimerWindow`'s overrides → always inactive, always with the
            // overlay band re-applied
            RegistrationType::Normal => Arc::new(move |_event: &ShortcutDetails| {
                if controls.shown(&label) {
                    controls.conceal(&label);
                } else {
                    controls.reveal_if_hidden(&label, true);
                    controls.apply_overlay(&label);
                }
            }),
            // `RiftFlowOngoingGameWindow`, `stateful`: held down shows, released hides
            _ => Arc::new(move |event: &ShortcutDetails| {
                if event.pressed {
                    // `is.dev ||` is the original's dev-mode escape; here the overlay
                    // only ever appears with the game in front, which is the case the
                    // released app actually reaches
                    if controls.game_client_foreground() {
                        controls.reveal_if_hidden(&label, false);
                        controls.accept_mouse(&label);
                        controls.set_fake_show(true);
                    }
                } else {
                    controls.apply_click_through(&label);
                    controls.set_fake_show(false);
                }
            }),
        };

        let forward = move |event: ShortcutDetails| action(&event);

        if let Err(message) = shard.register(&target_id, &shortcut_id, registration, forward) {
            eprintln!("[windows] {suffix}: failed to bind {shortcut_id} ({message})");
            self.controls.write_setting(suffix, "showShortcut", Value::Null);
        }
    }
}

/// `*WindowSettings.enabled`, and the two always-on-top defaults, verbatim from
/// each `state.ts`.
fn default_flag(suffix: &str, key: &str) -> bool {
    match key {
        "enabled" => matches!(suffix, AUX_WINDOW | OPGG_WINDOW),
        "autoShow" => matches!(suffix, AUX_WINDOW | OPGG_WINDOW),
        // aux/opgg default to pinned true as well; `opacity` is not a flag
        "pinned" => true,
        _ => false,
    }
}

/// The flag as the renderer sees it: the synced `settings` prop first, then the
/// store row for a namespace no service owns, then the original's default.
fn flag(settings: &SettingFactory, state_sync: &StateSync, suffix: &str, key: &str) -> bool {
    let namespace = namespace_of(suffix);

    if let Some(value) = state_sync
        .get(&namespace, "settings", key)
        .and_then(|value| value.as_bool())
    {
        return value;
    }

    settings
        .get_raw(&namespace, key)
        .ok()
        .flatten()
        .and_then(|value| value.as_bool())
        .unwrap_or_else(|| default_flag(suffix, key))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShowTiming {
    Show,
    Hide,
    Ignore,
}

/// `RiftFlowAuxWindow`'s `showTiming` computed. `ChampSelect` has no `break` in the
/// original, so a non-spectating draft falls through into the show branch — that
/// is not a typo to "fix", it is why the window appears in champ select at all.
fn aux_show_timing(auto_show: bool, ready: bool, phase: Option<&str>, spectating: bool) -> ShowTiming {
    if !auto_show || !ready {
        return ShowTiming::Ignore;
    }

    // the fall-through, spelled out: a draft that is not being spectated is a Lobby
    let shows = matches!(phase, Some("Lobby") | Some("Matchmaking") | Some("ReadyCheck"))
        || (phase == Some("ChampSelect") && !spectating);

    if shows {
        ShowTiming::Show
    } else if phase == Some("ChampSelect") {
        // `isSpectating` returns 'ignore' from inside the switch case
        ShowTiming::Ignore
    } else {
        ShowTiming::Hide
    }
}

/// `data.gameTime` off the gamestats response. The game reports elapsed seconds as
/// a number; a missing or non-numeric field is the same `null` the original's
/// `catch` produced, so the window drops back to its no-clock layout.
fn game_time_of(stats: &Value) -> Option<f64> {
    stats.get("gameTime").and_then(Value::as_f64)
}

fn set_game_time(sync: &Arc<StateSync>, game_time: Option<f64>) {
    let namespace = namespace_of(CD_TIMER_WINDOW);
    let value = game_time.map(Value::from).unwrap_or(Value::Null);

    if let Err(error) = sync.set(&namespace, "state", "gameTime", value) {
        eprintln!("[cd-timer-window] gameTime 推送失败: {error}");
    }
}

/// `shouldUseCdTimer` without the MobX wrapper: the session must be `InProgress`
/// and its queue's `gameMode` must be one the timer has a cooldown baseline for.
fn cd_timer_should_show(
    ready: bool,
    session_phase: Option<&str>,
    game_mode: Option<&str>,
    supported: &[String],
) -> bool {
    ready
        && session_phase == Some("InProgress")
        && game_mode.is_some_and(|mode| supported.iter().any(|known| known == mode))
}

// ------------------------------------------------------------------ entrypoints

/// `RiftFlowOngoingGameWindow`'s `fakeShow`, which the original only ever sets from
/// the global-shortcut handler: `true` makes the overlay accept input and paints
/// the panel, `false` puts it back to click-through. The stateful binding in
/// [`Watcher::sync_shortcut`] is its only caller, so with no native input hook
/// nothing flips it and the overlay stays invisible — which is the truth, not a
/// half-shown window.
pub fn set_fake_show(app: &AppHandle, value: bool) {
    let state = app.state::<AppState>();

    let _ = state
        .state_sync
        .set(&namespace_of(ONGOING_GAME_WINDOW), "state", "fakeShow", json!(value));

    // the same two halves the original's `fakeShow` reaction performs, in one call
    if let Some(window) = state.window_manager.get(ONGOING_GAME_WINDOW) {
        let _ = window.set_ignore_cursor_events(!value);
    }
}

/// What still cannot be honoured for one window, in envelope form, so a caller
/// that wants to surface it in the UI has a real answer to show. `windows.rs`
/// already registers `repositionToAlignLeagueClientUx` and `sendInGame` this way.
#[allow(dead_code)]
pub fn unsupported_reason(suffix: &str) -> IpcError {
    match suffix {
        // the two windows that snap to the LCU frame
        AUX_WINDOW | OPGG_WINDOW => IpcError::business(
            format!(
                "aligning {suffix} to the League Client window needs a Win32 window-placement \
                 probe, which riftflow-native does not expose"
            ),
            "NativeNotSupported",
        ),
        // the overlay that reveals itself on a held key: without the hook the
        // shortcut can never fire, so showing it would only add a click-through band
        _ => IpcError::business(
            format!(
                "{suffix} needs the native input hook for its shortcut (available: {})",
                native_input_available()
            ),
            "NativeInputNotSupported",
        ),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn opgg_close_stays_hidden_until_next_champion_select() {
        let mut applied = super::Applied::default();
        assert!(!applied.take_opgg_auto_show(Some("ChampSelect"), false));
        assert!(applied.take_opgg_auto_show(Some("ChampSelect"), true));
        // The user hid the window; the next poll must not reveal it again.
        assert!(!applied.take_opgg_auto_show(Some("ChampSelect"), true));
        assert!(!applied.take_opgg_auto_show(Some("InProgress"), true));
        assert!(applied.take_opgg_auto_show(Some("ChampSelect"), true));
    }
    use super::*;

    #[test]
    fn the_game_clock_accepts_only_a_real_number() {
        assert_eq!(game_time_of(&json!({ "gameTime": 123.5 })), Some(123.5));
        assert_eq!(game_time_of(&json!({ "gameTime": 0 })), Some(0.0));
        // a string clock, a missing field and a non-object are all the `null` the
        // original's catch branch produced
        assert_eq!(game_time_of(&json!({ "gameTime": "123" })), None);
        assert_eq!(game_time_of(&json!({})), None);
        assert_eq!(game_time_of(&Value::Null), None);
    }

    #[test]
    fn the_draft_shows_but_a_spectated_one_does_not() {
        assert_eq!(
            aux_show_timing(true, true, Some("ChampSelect"), false),
            ShowTiming::Show
        );
        assert_eq!(
            aux_show_timing(true, true, Some("ChampSelect"), true),
            ShowTiming::Ignore
        );
        for phase in ["Lobby", "Matchmaking", "ReadyCheck"] {
            assert_eq!(
                aux_show_timing(true, true, Some(phase), false),
                ShowTiming::Show,
                "{phase}"
            );
        }
        assert_eq!(
            aux_show_timing(true, true, Some("GameStart"), false),
            ShowTiming::Hide
        );
        // disconnected: `lc_state::clear` leaves the phase as "None", which hides
        assert_eq!(
            aux_show_timing(true, true, Some("None"), false),
            ShowTiming::Hide
        );
    }

    #[test]
    fn nothing_moves_before_auto_show_arms_or_the_page_is_ready() {
        assert_eq!(
            aux_show_timing(false, true, Some("Lobby"), false),
            ShowTiming::Ignore
        );
        assert_eq!(
            aux_show_timing(true, false, Some("Lobby"), false),
            ShowTiming::Ignore
        );
    }

    #[test]
    fn the_timer_only_asks_for_a_game_it_knows() {
        let supported: Vec<String> = ["CLASSIC", "ARAM"].iter().map(|m| m.to_string()).collect();

        assert!(cd_timer_should_show(
            true,
            Some("InProgress"),
            Some("ARAM"),
            &supported
        ));
        // not InProgress yet, an unknown queue, or not ready
        assert!(!cd_timer_should_show(
            true,
            Some("ChampSelect"),
            Some("ARAM"),
            &supported
        ));
        assert!(!cd_timer_should_show(
            true,
            Some("InProgress"),
            Some("NEXUSBLITZ"),
            &supported
        ));
        assert!(!cd_timer_should_show(
            false,
            Some("InProgress"),
            Some("ARAM"),
            &supported
        ));
        assert!(!cd_timer_should_show(true, Some("InProgress"), None, &supported));
    }

    #[test]
    fn entries_match_the_built_renderer_dist() {
        let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../renderer-dist");

        for suffix in AUX_SUFFIXES {
            let entry = html_entry(suffix).unwrap_or_else(|| panic!("{suffix} has no entry"));
            assert!(
                dist.join(entry).is_file(),
                "{} is not in {}",
                entry,
                dist.display()
            );
        }
        // the main window is the shell's, and must not be reachable from here
        assert!(html_entry(windows::MAIN_WINDOW).is_none());
    }

    #[test]
    fn the_overlays_cannot_be_unpinned() {
        for suffix in [ONGOING_GAME_WINDOW, CD_TIMER_WINDOW] {
            let pinned = declarations(suffix)
                .into_iter()
                .find(|declaration| declaration.key == "pinned")
                .expect("pinned is declared");
            let forced = (pinned
                .transform
                .as_ref()
                .expect("the transform is the original's zod transform"))(
                &TransformContext {
                    value: json!(false),
                    old_value: Some(json!(true)),
                    default_value: json!(true),
                },
            );
            assert_eq!(forced, json!(true), "{suffix}");
        }

        // aux/opgg keep a real pin toggle
        for suffix in [AUX_WINDOW, OPGG_WINDOW] {
            let pinned = declarations(suffix)
                .into_iter()
                .find(|declaration| declaration.key == "pinned")
                .expect("pinned is declared");
            assert!(pinned.transform.is_none(), "{suffix}");
        }
    }

    #[test]
    fn declared_defaults_match_the_original_state_files() {
        let enabled_keys = |suffix: &str| {
            declarations(suffix)
                .into_iter()
                .map(|declaration| (declaration.key, declaration.default))
                .collect::<HashMap<String, Value>>()
        };

        let aux = enabled_keys(AUX_WINDOW);
        assert_eq!(aux["enabled"], json!(true));
        assert_eq!(aux["autoShow"], json!(true));
        assert_eq!(aux["showSkinSelector"], json!(false));
        assert_eq!(aux["pinned"], json!(true));
        assert_eq!(aux["opacity"], json!(1));

        let ongoing = enabled_keys(ONGOING_GAME_WINDOW);
        assert_eq!(ongoing["enabled"], json!(false));
        assert_eq!(ongoing["showShortcut"], Value::Null);

        let timer = enabled_keys(CD_TIMER_WINDOW);
        assert_eq!(timer["enabled"], json!(false));
        assert_eq!(timer["timerType"], json!("countdown"));
        assert_eq!(timer["reverseAdjustmentDirection"], json!(false));

        assert_eq!(default_flag(CD_TIMER_WINDOW, "enabled"), false);
        assert_eq!(default_flag(AUX_WINDOW, "enabled"), true);
    }
}
