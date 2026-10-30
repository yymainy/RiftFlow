//! Port of `src/main/shards/window-manager/base-riftflow-window.ts` — the 17 IPC
//! methods every riftflow window registers under its own sub-namespace
//! `window-manager-main/<suffix>`, plus the per-window extras.
//!
//! Electron `BrowserWindow` calls map onto Tauri `WebviewWindow`; where the JS
//! original is a macOS-only no-op this stays a no-op rather than failing, so the
//! renderer's behaviour does not change between platforms.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};

use riftflow_ipc::{RiftFlowIpcMain, IpcError};
use riftflow_native::input::KeyboardInjector;
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;
use serde_json::{Value, json};
use tauri::{Size, WebviewWindow};

/// `window-manager-main` base namespace.
pub const WM_NAMESPACE: &str = "window-manager-main";

/// Windows that exist in this port, keyed by the riftflow suffix the renderer uses.
#[derive(Default)]
pub struct WindowManager {
    by_suffix: Mutex<HashMap<String, WebviewWindow>>,
    /// `_saveToStorage('trackedBounds', …, { delay: 1000 })`: the newest bounds
    /// per window, written out by [`WindowManager::flush_pending_bounds`].
    pending_bounds: Mutex<HashMap<String, Value>>,
    /// `BaseRiftFlowWindow._trueClose`, set by the quit paths so `handleClose`
    /// lets the window actually go instead of hiding it.
    true_close: Mutex<Vec<String>>,
    /// `RiftFlowMainWindow._nextCloseAction`: the strategy the renderer asked for
    /// on this one close request, ahead of the `closeAction` setting.
    next_close_action: Mutex<HashMap<String, String>>,
}

impl WindowManager {
    pub fn attach(&self, suffix: &str, window: &WebviewWindow) {
        self.by_suffix
            .lock()
            .unwrap()
            .insert(suffix.to_string(), window.clone());
    }

    pub fn get(&self, suffix: &str) -> Option<WebviewWindow> {
        self.by_suffix.lock().unwrap().get(suffix).cloned()
    }

    pub fn suffixes(&self) -> Vec<String> {
        self.by_suffix.lock().unwrap().keys().cloned().collect()
    }

    pub fn mark_true_close(&self, suffix: &str) {
        let mut list = self.true_close.lock().unwrap();
        if !list.iter().any(|entry| entry == suffix) {
            list.push(suffix.to_string());
        }
    }

    pub fn set_next_close_action(&self, suffix: &str, action: &str) {
        self.next_close_action
            .lock()
            .unwrap()
            .insert(suffix.to_string(), action.to_string());
    }

    fn take_next_close_action(&self, suffix: &str) -> Option<String> {
        self.next_close_action.lock().unwrap().remove(suffix)
    }

    fn take_true_close(&self, suffix: &str) -> bool {
        let mut list = self.true_close.lock().unwrap();
        match list.iter().position(|entry| entry == suffix) {
            Some(index) => {
                list.remove(index);
                true
            }
            None => false,
        }
    }

    pub fn queue_bounds_save(&self, suffix: &str, bounds: &Value) {
        self.pending_bounds
            .lock()
            .unwrap()
            .insert(suffix.to_string(), bounds.clone());
    }

    /// Called on the shell's 1 s tick, which is longer than the original's
    /// 1000 ms debounce and only writes windows that actually moved.
    pub fn flush_pending_bounds(&self, settings: &SettingFactory) {
        let pending: Vec<(String, Value)> = {
            let mut map = self.pending_bounds.lock().unwrap();
            map.drain().collect()
        };
        for (suffix, bounds) in pending {
            let _ = settings.save_raw(&format!("{WM_NAMESPACE}/{suffix}"), "trackedBounds", &bounds);
        }
    }
}

/// `BaseRiftFlowWindow`'s creation config, from each `*-window/window.ts`.
pub struct WindowSpec {
    pub title: &'static str,
    pub base_width: f64,
    pub base_height: f64,
    pub min_width: f64,
    pub min_height: f64,
    pub frameless: bool,
    pub resizable: bool,
    pub maximizable: bool,
    pub minimizable: bool,
    pub focusable: bool,
    pub skip_taskbar: bool,
    pub transparent: bool,
    /// `alwaysOnTop` from `browserWindowOptions`, before the `pinned` setting.
    pub always_on_top: bool,
    pub remember_position: bool,
    pub remember_size: bool,
    /// The main window shows itself on `ready`; the overlays wait for their own
    /// LCU conditions, so they stay hidden until those watchers are ported.
    pub show_on_ready: bool,
}

pub const MAIN_WINDOW: &str = "main-window";

impl Default for WindowSpec {
    fn default() -> Self {
        Self {
            title: "RiftFlow",
            base_width: 1200.0,
            base_height: 760.0,
            min_width: 600.0,
            min_height: 400.0,
            frameless: true,
            resizable: true,
            maximizable: true,
            minimizable: true,
            focusable: true,
            skip_taskbar: false,
            transparent: false,
            always_on_top: false,
            remember_position: true,
            remember_size: true,
            show_on_ready: false,
        }
    }
}

pub fn window_spec(suffix: &str) -> WindowSpec {
    match suffix {
        MAIN_WINDOW => WindowSpec {
            title: "RiftFlow",
            base_width: 1500.0,
            base_height: 860.0,
            min_width: 840.0,
            min_height: 600.0,
            show_on_ready: true,
            ..WindowSpec::default()
        },
        "aux-window" => WindowSpec {
            title: "RiftFlow Mini",
            base_width: 340.0,
            base_height: 420.0,
            min_width: 340.0,
            min_height: 420.0,
            maximizable: false,
            ..WindowSpec::default()
        },
        "opgg-window" => WindowSpec {
            title: "RiftFlow · OP.GG",
            base_width: 480.0,
            base_height: 720.0,
            // the original's min is larger than its base size, verbatim
            min_width: 530.0,
            min_height: 530.0,
            maximizable: false,
            ..WindowSpec::default()
        },
        "ongoing-game-window" => WindowSpec {
            title: "RiftFlow · Game Inspector",
            base_width: 1300.0,
            base_height: 840.0,
            min_width: 1300.0,
            min_height: 840.0,
            resizable: false,
            maximizable: false,
            minimizable: false,
            focusable: false,
            skip_taskbar: true,
            transparent: true,
            always_on_top: true,
            remember_position: false,
            remember_size: false,
            ..WindowSpec::default()
        },
        "cd-timer-window" => WindowSpec {
            title: "Timer",
            base_width: 100.0,
            base_height: 220.0,
            min_width: 100.0,
            min_height: 100.0,
            resizable: false,
            maximizable: false,
            minimizable: false,
            skip_taskbar: true,
            transparent: true,
            always_on_top: true,
            remember_position: true,
            remember_size: false,
            ..WindowSpec::default()
        },
        // `RIFTFLOW_LAB=1`: the rewrite baseline page, loaded from the same
        // `renderer-dist` as the shipped bundle so both can be compared side by side
        "lab-window" => WindowSpec {
            title: "RiftFlow Lab",
            base_width: 1280.0,
            base_height: 900.0,
            min_width: 760.0,
            min_height: 480.0,
            show_on_ready: true,
            ..WindowSpec::default()
        },
        _ => WindowSpec::default(),
    }
}

fn push_state(state_sync: &Arc<StateSync>, suffix: &str, path: &str, value: Value) {
    let _ = state_sync.set(&format!("{WM_NAMESPACE}/{suffix}"), "state", path, value);
}

/// The `HWND` behind a Tauri window, as the raw pointer [`riftflow_native::placement`]
/// takes.
#[cfg(windows)]
fn native_hwnd(window: &WebviewWindow) -> Option<*mut std::ffi::c_void> {
    window.hwnd().ok().map(|hwnd| hwnd.0 as *mut std::ffi::c_void)
}

#[cfg(not(windows))]
fn native_hwnd(_window: &WebviewWindow) -> Option<*mut std::ffi::c_void> {
    None
}

/// `setAlwaysOnTop(pinned, 'screen-saver', 1)` for the windows that also carry
/// `skipTaskbar`: on Windows that pair is `WS_EX_TOPMOST | WS_EX_TOOLWINDOW` re-asserted
/// in a fresh top-most slot, which is what keeps an overlay above a borderless game
/// client. Every other window keeps plain `set_always_on_top`, because the tool-window
/// style would take it out of Alt+Tab.
pub fn set_topmost(window: &WebviewWindow, suffix: &str, pinned: bool) {
    if pinned && window_spec(suffix).skip_taskbar {
        if let Some(hwnd) = native_hwnd(window) {
            if riftflow_native::placement::raise_overlay(hwnd) {
                return;
            }
        }
    }

    let _ = window.set_always_on_top(pinned);
}

/// Registers the base 17 handlers for one window, mirroring
/// `BaseRiftFlowWindow._baseWindowIpcCall()`.
///
/// Known gaps against Electron, both because Tauri core has no equivalent call:
/// `setOpacity` (a layered-window / DWM path) and `setContentProtection`.
/// `downloadUrl` is registered but reports `NotImplemented` rather than lying.
pub fn register_window(
    ipc: &Arc<RiftFlowIpcMain>,
    state_sync: &Arc<StateSync>,
    manager: &Arc<WindowManager>,
    settings: &Arc<SettingFactory>,
    suffix: &str,
) -> anyhow::Result<()> {
    let namespace = format!("{WM_NAMESPACE}/{suffix}");

    // `_getFromStorage('trackedBounds')` — the saved position is applied by the
    // renderer's own `applyBounds`, so it only has to be in the synced state
    if let Ok(Some(bounds)) = settings.get_raw(&namespace, "trackedBounds") {
        state_sync.set(&namespace, "state", "trackedBounds", bounds)?;
    }

    ipc.on_call(&namespace, "setSize", {
        let manager = manager.clone();
        let suffix_key = suffix.to_string();
        riftflow_ipc::handler(move |_ctx, args| {
            let manager = manager.clone();
            let suffix_key = suffix_key.clone();
            async move {
                let window = window_of(&manager, &suffix_key)?;
                let width = arg_f64(&args, 0)?;
                let height = arg_f64(&args, 1)?;
                let _animate = args.get(2).and_then(Value::as_bool).unwrap_or(false);
                window
                    .set_size(Size::Logical((width, height).into()))
                    .map_err(IpcError::plain)?;
                    Ok(Value::Null)
            }
        })
    })?;

    ipc.on_call(&namespace, "getSize", {
        let manager = manager.clone();
        let suffix_key = suffix.to_string();
        riftflow_ipc::handler(move |_ctx, _args| {
            let manager = manager.clone();
            let suffix_key = suffix_key.clone();
            async move {
                let window = window_of(&manager, &suffix_key)?;
                let (width, height) = logical_size(&window)?;
                Ok(json!([width, height]))
            }
        })
    })?;

    ipc.on_call(&namespace, "setPosition", {
        let manager = manager.clone();
        let suffix_key = suffix.to_string();
        riftflow_ipc::handler(move |_ctx, args| {
            let manager = manager.clone();
            let suffix_key = suffix_key.clone();
            async move {
                let window = window_of(&manager, &suffix_key)?;
                let x = arg_f64(&args, 0)?;
                let y = arg_f64(&args, 1)?;
                let _animate = args.get(2).and_then(Value::as_bool).unwrap_or(false);
                window
                    .set_position(tauri::Position::Logical((x, y).into()))
                    .map_err(IpcError::plain)?;
                    Ok(Value::Null)
            }
        })
    })?;

    ipc.on_call(&namespace, "getPosition", {
        let manager = manager.clone();
        let suffix_key = suffix.to_string();
        riftflow_ipc::handler(move |_ctx, _args| {
            let manager = manager.clone();
            let suffix_key = suffix_key.clone();
            async move {
                let window = window_of(&manager, &suffix_key)?;
                let (x, y) = logical_position(&window)?;
                Ok(json!([x, y]))
            }
        })
    })?;

    for (method, action) in [
        ("minimize", WindowAction::Minimize),
        ("maximize", WindowAction::Maximize),
        ("unmaximize", WindowAction::Unmaximize),
        ("restore", WindowAction::Restore),
        ("close", WindowAction::Close),
        ("hide", WindowAction::Hide),
        ("show", WindowAction::Show),
    ] {
        let manager = manager.clone();
        let suffix_key = suffix.to_string();
        let sync = state_sync.clone();
        ipc.on_call(
            &namespace,
            method,
            riftflow_ipc::handler(move |_ctx, args| {
                let manager = manager.clone();
                let action = action;
                let suffix_key = suffix_key.clone();
                let sync = sync.clone();
                async move {
                    let window = window_of(&manager, &suffix_key)?;
                    match action {
                        WindowAction::Minimize => {
                            window.minimize().map_err(IpcError::plain)?;
                            // Tauri has no minimize event; Electron's does the same push
                            push_state(&sync, &suffix_key, "status", json!("minimized"));
                        }
                        WindowAction::Maximize => {
                            window.maximize().map_err(IpcError::plain)?;
                            push_state(&sync, &suffix_key, "status", json!("maximized"));
                        }
                        WindowAction::Unmaximize => {
                            window.unmaximize().map_err(IpcError::plain)?;
                            push_state(&sync, &suffix_key, "status", json!("normal"));
                        }
                        // Electron's restore() is unminimize + show
                        WindowAction::Restore => {
                            let _ = window.unminimize();
                            window.show().map_err(IpcError::plain)?;
                            push_state(&sync, &suffix_key, "status", json!("normal"));
                            push_state(&sync, &suffix_key, "show", json!(true));
                        }
                        WindowAction::Close => window.close().map_err(IpcError::plain)?,
                        WindowAction::Hide => {
                            if window.is_visible().unwrap_or(true) {
                                window.hide().map_err(IpcError::plain)?;
                                push_state(&sync, &suffix_key, "show", json!(false));
                            }
                        }
                        WindowAction::Show => {
                            let inactive = args.first().and_then(Value::as_bool).unwrap_or(false);
                            show_or_restore(&window, inactive, &sync, &suffix_key);
                        }
                    }
                    Ok(Value::Null)
                }
            }),
        )?;
    }

    ipc.on_call(&namespace, "toggleDevtools", {
        let manager = manager.clone();
        let suffix_key = suffix.to_string();
        riftflow_ipc::handler(move |_ctx, _args| {
            let manager = manager.clone();
            let suffix_key = suffix_key.clone();
            async move {
                let window = window_of(&manager, &suffix_key)?;
                if window.is_devtools_open() {
                    window.close_devtools();
                } else {
                    window.open_devtools();
                }
                Ok(Value::Null)
            }
        })
    })?;

    ipc.on_call(&namespace, "setTitle", {
        let manager = manager.clone();
        let suffix_key = suffix.to_string();
        riftflow_ipc::handler(move |_ctx, args| {
            let manager = manager.clone();
            let suffix_key = suffix_key.clone();
            async move {
                let window = window_of(&manager, &suffix_key)?;
                let title = args
                    .first()
                    .and_then(Value::as_str)
                    .ok_or_else(|| IpcError::plain("title should be a string"))?;
                window.set_title(title).map_err(IpcError::plain)?;
                Ok(Value::Null)
            }
        })
    })?;

    ipc.on_call(&namespace, "resetPosition", {
        let manager = manager.clone();
        let suffix_key = suffix.to_string();
        riftflow_ipc::handler(move |_ctx, _args| {
            let manager = manager.clone();
            let suffix_key = suffix_key.clone();
            async move {
                let window = window_of(&manager, &suffix_key)?;
                window.center().map_err(IpcError::plain)?;
                Ok(Value::Null)
            }
        })
    })?;

    // `repositionWindowIfInvisible` puts an off-screen window back on a display;
    // the equivalent check is "is the saved position inside a monitor work area"
    ipc.on_call(&namespace, "repositionWindowIfInvisible", {
        let manager = manager.clone();
        let suffix_key = suffix.to_string();
        riftflow_ipc::handler(move |_ctx, _args| {
            let manager = manager.clone();
            let suffix_key = suffix_key.clone();
            async move {
                let window = window_of(&manager, &suffix_key)?;
                let position = window.outer_position().map_err(IpcError::plain)?;
                let on_screen = window
                    .current_monitor()
                    .ok()
                    .flatten()
                    .map(|monitor| {
                        let work = monitor.position();
                        let size = monitor.size();
                        position.x >= work.x
                            && position.y >= work.y
                            && position.x < work.x + size.width as i32
                            && position.y < work.y + size.height as i32
                    })
                    .unwrap_or(true);

                if !on_screen {
                    window.center().map_err(IpcError::plain)?;
                }
                Ok(Value::Null)
            }
        })
    })?;

    ipc.on_call(&namespace, "setIgnoreMouseEvents", {
        let manager = manager.clone();
        let suffix_key = suffix.to_string();
        riftflow_ipc::handler(move |_ctx, args| {
            let manager = manager.clone();
            let suffix_key = suffix_key.clone();
            async move {
                let window = window_of(&manager, &suffix_key)?;
                let ignore = args.first().and_then(Value::as_bool).unwrap_or(false);
                let forward = args
                    .get(1)
                    .and_then(|options| options.get("forward"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                // Tauri core only has the plain "ignore" switch; Electron's
                // `forward` mode (click-through but still hover-aware) needs the
                // native subclass hook, so it is reported rather than faked.
                if ignore && forward {
                    eprintln!("[window] {suffix_key}: mouse-event forwarding is not supported yet");
                }
                window
                    .set_ignore_cursor_events(ignore)
                    .map_err(IpcError::plain)?;
                Ok(Value::Null)
            }
        })
    })?;

    ipc.on_call(&namespace, "downloadUrl", riftflow_ipc::handler(|_ctx, _args| async move {
        Err(IpcError::business(
            "downloadUrl needs the shell plugin",
            "NotImplemented",
        ))
    }))?;

    // `state` props are declared once by `state_bootstrap`; here we only push
    // updates, so there is no second declaration to collide with.
    register_window_extras(ipc, manager, suffix, &namespace)?;

    Ok(())
}

/// The per-window subclasses add their own handlers on top of the base 17.
fn register_window_extras(
    ipc: &Arc<RiftFlowIpcMain>,
    manager: &Arc<WindowManager>,
    suffix: &str,
    namespace: &str,
) -> anyhow::Result<()> {
    match suffix {
        MAIN_WINDOW => {
            // `closeMainWindow(strategy)` only records the strategy and asks the
            // window to close; the decision is `handleClose`'s, i.e. `handle_close`
            let manager_for_close = manager.clone();
            ipc.on_call(namespace, "closeMainWindow", riftflow_ipc::handler(
                move |_ctx, args| {
                    let manager = manager_for_close.clone();
                    async move {
                        let window = window_of(&manager, MAIN_WINDOW)?;
                        if let Some(strategy) = args.first().and_then(Value::as_str) {
                            manager.set_next_close_action(MAIN_WINDOW, strategy);
                        }
                        window.close().map_err(IpcError::plain)?;
                        Ok(Value::Null)
                    }
                },
            ))?;

            // `closeMainWindowForce()` → `close(true)`
            let manager_for_force = manager.clone();
            ipc.on_call(namespace, "closeMainWindowForce", riftflow_ipc::handler(
                move |_ctx, _args| {
                    let manager = manager_for_force.clone();
                    async move {
                        let window = window_of(&manager, MAIN_WINDOW)?;
                        manager.mark_true_close(MAIN_WINDOW);
                        window.close().map_err(IpcError::plain)?;
                        Ok(Value::Null)
                    }
                },
            ))?;

            // macOS traffic lights: a no-op elsewhere, exactly like the original
            ipc.on_call(namespace, "setTrafficLightPosition", riftflow_ipc::handler(
                |_ctx, _args| async move { Ok(Value::Null) },
            ))?;
        }
        "aux-window" | "opgg-window" => {
            let which = suffix.to_string();
            let align_manager = manager.clone();
            ipc.on_call(namespace, "repositionToAlignLeagueClientUx", riftflow_ipc::handler(
                move |_ctx, args| {
                    let which = which.clone();
                    let align_manager = align_manager.clone();
                    async move {
                        let placement = align_placement(args.first().and_then(Value::as_str));
                        let window = window_of(&align_manager, &which)?;
                        align_to_league_client_ux(&window, placement);
                        Ok(Value::Null)
                    }
                },
            ))?;
        }
        "cd-timer-window" => {
            ipc.on_call(namespace, "sendInGame", riftflow_ipc::handler(|_ctx, args| async move {
                let text = args.first().and_then(Value::as_str).unwrap_or_default().to_string();
                send_in_game(text).await
            }))?;
        }
        _ => {}
    }

    Ok(())
}

/// Pushes the window props that a Tauri event can answer truthfully.
pub fn publish_window_event(
    state_sync: &Arc<StateSync>,
    manager: &Arc<WindowManager>,
    label: &str,
    event: &str,
) {
    if !manager.suffixes().iter().any(|suffix| suffix == label) {
        return;
    }
    let suffix = label;

    // `BrowserWindow.on('closed')` → `ready = false`; no live window to query
    if event == "closed" {
        push_state(state_sync, suffix, "ready", json!(false));
        return;
    }

    let Some(window) = manager.get(suffix) else {
        return;
    };

    match event {
        "focus" => push_state(state_sync, suffix, "focus", json!("focused")),
        "blur" => push_state(state_sync, suffix, "focus", json!("blurred")),
        "resized" | "moved" => {
            // Electron's saveBounds() skips a minimized or maximized window, so a
            // restore does not overwrite the remembered normal bounds
            if window.is_minimized().unwrap_or(false) || window.is_maximized().unwrap_or(false) {
                return;
            }
            let bounds = window_bounds(&window);
            if bounds.is_null() {
                return;
            }
            manager.queue_bounds_save(suffix, &bounds);
            push_state(state_sync, suffix, "trackedBounds", bounds);
        }
        _ => {}
    }
}

/// `getContentBounds()` maps to Tauri's inner (client-area) rect, in DIP.
fn window_bounds(window: &WebviewWindow) -> Value {
    match (logical_position(window), logical_size(window)) {
        (Ok((x, y)), Ok((width, height))) => json!({ "x": x, "y": y, "width": width, "height": height }),
        _ => Value::Null,
    }
}

#[derive(Clone, Copy)]
enum WindowAction {
    Minimize,
    Maximize,
    Unmaximize,
    Restore,
    Close,
    Hide,
    Show,
}

fn window_of(manager: &Arc<WindowManager>, suffix: &str) -> Result<WebviewWindow, IpcError> {
    manager
        .get(suffix)
        .ok_or_else(|| IpcError::business(format!("window {suffix} is not open"), "NoWindow"))
}

/// `BaseRiftFlowWindow.showOrRestore()`.
pub fn show_or_restore(window: &WebviewWindow, inactive: bool, sync: &Arc<StateSync>, suffix: &str) {
    if !window.is_visible().unwrap_or(false) {
        let _ = window.show();
        push_state(sync, suffix, "show", json!(true));
        // the original returns here: a freshly shown window is not restored/focused
        return;
    }

    if window.is_minimized().unwrap_or(false) {
        let _ = window.unminimize();
        push_state(sync, suffix, "status", json!("normal"));
    }

    if !inactive {
        let _ = window.set_focus();
    }
}

/// The block right after `new BrowserWindow(...)`: `pinned` → always-on-top, the
/// real focus state, and the remembered bounds. `opacity` and
/// `contentProtection` have no Tauri equivalent, so they are not pretended about.
pub fn apply_window_settings(
    state_sync: &Arc<StateSync>,
    settings: &Arc<SettingFactory>,
    window: &WebviewWindow,
    suffix: &str,
) {
    let namespace = format!("{WM_NAMESPACE}/{suffix}");
    let spec = window_spec(suffix);

    let stored_pinned = settings
        .get_setting(&namespace, "pinned")
        .ok()
        .and_then(|value| value.as_bool());
    let pinned = stored_pinned.unwrap_or(spec.always_on_top);
    set_topmost(window, suffix, pinned);

    let focused = window.is_focused().unwrap_or(false);
    push_state(
        state_sync,
        suffix,
        "focus",
        json!(if focused { "focused" } else { "blurred" }),
    );
    push_state(state_sync, suffix, "show", json!(window.is_visible().unwrap_or(false)));

    let tracked = state_sync.get(&namespace, "state", "trackedBounds");

    if let Some(bounds) = tracked.filter(|value| !value.is_null()) {
        let read = |key: &str| bounds.get(key).and_then(Value::as_f64);
        let (Some(x), Some(y), Some(width), Some(height)) =
            (read("x"), read("y"), read("width"), read("height"))
        else {
            return;
        };

        match (spec.remember_position, spec.remember_size) {
            (true, true) => {
                let _ = window.set_position(tauri::Position::Logical((x, y).into()));
                let _ = window.set_size(Size::Logical((width, height).into()));
            }
            (true, false) => {
                let _ = window.set_position(tauri::Position::Logical((x, y).into()));
            }
            (false, true) => {
                let _ = window.set_size(Size::Logical((width, height).into()));
                let _ = window.center();
            }
            (false, false) => {}
        }
    }
}

/// Electron's `ready-to-show`: the window becomes usable, gets put back on a
/// display if it was saved off-screen, and the main window reveals itself.
pub fn on_window_ready(
    state_sync: &Arc<StateSync>,
    settings: &Arc<SettingFactory>,
    window: &WebviewWindow,
    suffix: &str,
) {
    let spec = window_spec(suffix);

    // Tauri fires `page-load` on every navigation; Electron's `ready-to-show`
    // is once, so a reload must not re-apply the saved bounds
    if state_sync
        .get(&format!("{WM_NAMESPACE}/{suffix}"), "state", "ready")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return;
    }

    push_state(state_sync, suffix, "ready", json!(true));

    if window
        .outer_position()
        .ok()
        .map(|position| {
            let on_screen = window
                .current_monitor()
                .ok()
                .flatten()
                .map(|monitor| {
                    let work = monitor.position();
                    let size = monitor.size();
                    position.x >= work.x
                        && position.y >= work.y
                        && position.x < work.x + size.width as i32
                        && position.y < work.y + size.height as i32
                })
                .unwrap_or(true);
            !on_screen
        })
        .unwrap_or(false)
    {
        let _ = window.center();
    }

    apply_window_settings(state_sync, settings, window, suffix);

    if spec.show_on_ready {
        let _ = window.show();
        push_state(state_sync, suffix, "show", json!(true));
    }
}

/// `BaseRiftFlowWindow.handleClose()` / `RiftFlowMainWindow.handleClose()`. Returns
/// `true` when the window may actually close.
pub fn handle_close(
    ipc: &Arc<RiftFlowIpcMain>,
    state_sync: &Arc<StateSync>,
    settings: &Arc<SettingFactory>,
    manager: &Arc<WindowManager>,
    label: &str,
) -> bool {
    if manager.take_true_close(label) {
        return true;
    }

    if label != MAIN_WINDOW {
        // the base class hides on close, whatever the user picked elsewhere
        if let Some(window) = manager.get(label) {
            if window.is_visible().unwrap_or(false) {
                let _ = window.hide();
                push_state(state_sync, label, "show", json!(false));
            }
        }
        return false;
    }

    // `_nextCloseAction || this.settings.closeAction`
    let action = manager
        .take_next_close_action(label)
        .unwrap_or_else(|| close_action(settings, label));

    match action.as_str() {
        "quit" => true,
        "minimize-to-tray" => {
            if let Some(window) = manager.get(label) {
                if window.is_visible().unwrap_or(false) {
                    let _ = window.hide();
                    push_state(state_sync, label, "show", json!(false));
                }
            }
            false
        }
        // 'ask': reveal the window and let the renderer show its close dialog
        _ => {
            if let Some(window) = manager.get(label) {
                show_or_restore(&window, false, state_sync, label);
            }
            let _ = ipc.send_event(&format!("{WM_NAMESPACE}/{MAIN_WINDOW}"), "close-asking", &[]);
            false
        }
    }
}

fn close_action(settings: &Arc<SettingFactory>, suffix: &str) -> String {
    settings
        .get_setting(&format!("{WM_NAMESPACE}/{suffix}"), "closeAction")
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "ask".to_string())
}

fn arg_f64(args: &[Value], index: usize) -> Result<f64, IpcError> {
    args.get(index)
        .and_then(Value::as_f64)
        .ok_or_else(|| IpcError::plain(format!("argument {index} should be a number")))
}

/// Electron hands DIP to and from its size/position getters; Tauri reports
/// physical pixels, so the scale factor has to come out.
fn logical_size(window: &WebviewWindow) -> Result<(f64, f64), IpcError> {
    let scale = window.scale_factor().map_err(IpcError::plain)?;
    let size = window.inner_size().map_err(IpcError::plain)?;
    Ok((size.width as f64 / scale, size.height as f64 / scale))
}

fn logical_position(window: &WebviewWindow) -> Result<(f64, f64), IpcError> {
    let scale = window.scale_factor().map_err(IpcError::plain)?;
    let position = window.inner_position().map_err(IpcError::plain)?;
    Ok((position.x as f64 / scale, position.y as f64 / scale))
}

// ------------------------------------------------------------ align to the LCU

/// `repositionToAlignLeagueClientUx`'s `placement` argument. An unrecognised value
/// falls back to the original's default rather than doing nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignPlacement {
    TopLeft,
    BottomLeft,
    TopRight,
    BottomRight,
}

pub fn align_placement(value: Option<&str>) -> AlignPlacement {
    match value {
        Some("bottom-left") => AlignPlacement::BottomLeft,
        Some("top-right") => AlignPlacement::TopRight,
        Some("bottom-right") => AlignPlacement::BottomRight,
        _ => AlignPlacement::TopLeft,
    }
}

/// `getPositionWithSnap`, in DIPs. `work` clamps the result so the overlay can never
/// end up outside the display's work area.
pub fn position_with_snap(
    win: (i32, i32),
    target: (i32, i32, i32, i32),
    placement: AlignPlacement,
    work: (i32, i32, i32, i32),
) -> (i32, i32) {
    let (win_w, win_h) = win;
    let (tx, ty, tw, th) = target;
    let (wx, wy, ww, wh) = work;

    let (mut new_x, mut new_y) = match placement {
        AlignPlacement::TopLeft => (tx - win_w, ty),
        AlignPlacement::BottomLeft => (tx - win_w, ty + th - win_h),
        AlignPlacement::TopRight => (tx + tw, ty),
        AlignPlacement::BottomRight => (tx + tw, ty + th - win_h),
    };

    if new_x < wx {
        new_x = wx;
    } else if new_x + win_w > wx + ww {
        new_x = wx + ww - win_w;
    }

    if new_y < wy {
        new_y = wy;
    } else if new_y + win_h > wy + wh {
        new_y = wy + wh - win_h;
    }

    (new_x, new_y)
}

/// `screen.getDisplayMatching(targetBounds).workArea`, in DIPs: the monitor the
/// client frame actually sits on.
fn monitor_work_area_dips(
    window: &WebviewWindow,
    scale: f64,
    target: (i32, i32, i32, i32),
) -> Option<(i32, i32, i32, i32)> {
    let to_dips = |area: &tauri::PhysicalRect<i32, u32>| {
        (
            area.position.x as f64 / scale,
            area.position.y as f64 / scale,
            area.size.width as f64 / scale,
            area.size.height as f64 / scale,
        )
            .round_to_ints()
    };

    let monitors = window.available_monitors().ok()?;
    let overlapping = monitors.into_iter().find(|monitor| {
        let (x, y, w, h) = to_dips(&monitor.work_area());
        target.0 < x + w && target.0 + target.2 > x && target.1 < y + h && target.1 + target.3 > y
    });
    let chosen = match overlapping {
        Some(monitor) => Some(monitor),
        None => window.current_monitor().ok().flatten(),
    };
    chosen.map(|monitor| to_dips(&monitor.work_area()))
}

trait RoundToInts {
    fn round_to_ints(self) -> (i32, i32, i32, i32);
}

impl RoundToInts for (f64, f64, f64, f64) {
    fn round_to_ints(self) -> (i32, i32, i32, i32) {
        (
            self.0.round() as i32,
            self.1.round() as i32,
            self.2.round() as i32,
            self.3.round() as i32,
        )
    }
}

/// `repositionToAlignLeagueClientUx`: read the client frame through the same Win32
/// call the addon used, skip the minimized stub, then snap this window to it.
pub fn align_to_league_client_ux(window: &WebviewWindow, placement: AlignPlacement) {
    let Some(ux) = riftflow_native::placement::league_client_ux() else {
        eprintln!("[windows] 找不到 LeagueClientUx 窗口（RCLIENT），跳过对齐");
        return;
    };
    if !riftflow_native::placement::is_usable_as_align_target(&ux) {
        // minimized: the frame collapses to about 134x37 and snapping to it would
        // push the overlay out of view
        eprintln!(
            "[windows] LeagueClientUx 看起来被最小化 ({}x{})，跳过对齐",
            ux.width, ux.height
        );
        return;
    }

    let scale = match window.scale_factor() {
        Ok(scale) if scale > 0.0 => scale,
        Ok(_) => {
            eprintln!("[windows] 窗口缩放因子为 0，跳过对齐");
            return;
        }
        Err(error) => {
            eprintln!("[windows] 读取缩放因子失败 ({error})，跳过对齐");
            return;
        }
    };
    let Ok(size) = window.outer_size() else {
        eprintln!("[windows] 读不到窗口尺寸，跳过对齐");
        return;
    };
    let win = (
        (size.width as f64 / scale).round() as i32,
        (size.height as f64 / scale).round() as i32,
    );
    let target = (ux.left, ux.top, ux.width, ux.height);

    let work = match monitor_work_area_dips(window, scale, target) {
        Some(area) => area,
        None => {
            eprintln!("[windows] 无法确定客户端所在的显示器，跳过对齐");
            return;
        }
    };
    eprintln!(
        "[windows] 对齐 {target:?} 到 work={work:?} win={}x{} scale={scale}",
        win.0, win.1
    );

    let (x, y) = position_with_snap(win, target, placement, work);
    let _ = window.set_position(tauri::Position::Logical((x as f64, y as f64).into()))
        .map_err(|error| eprintln!("[windows] 对齐失败: {error}"));
}

#[cfg(test)]
mod align_tests {
    use super::*;

    #[test]
    fn an_unknown_placement_falls_back_to_the_default() {
        assert_eq!(align_placement(None), AlignPlacement::TopLeft);
        assert_eq!(align_placement(Some("sideways")), AlignPlacement::TopLeft);
        assert_eq!(align_placement(Some("bottom-right")), AlignPlacement::BottomRight);
    }

    #[test]
    fn each_corner_snaps_the_way_the_original_does() {
        // 400x300 overlay against a client frame at (1000,200) 1280x720, on a
        // 1920x1080 work area
        let win = (400, 300);
        let target = (1000, 200, 1280, 720);
        let work = (0, 0, 1920, 1080);

        assert_eq!(position_with_snap(win, target, AlignPlacement::TopLeft, work), (600, 200));
        assert_eq!(position_with_snap(win, target, AlignPlacement::BottomLeft, work), (600, 620));
        // 2280 would be off the display, so the clamp hugs the right edge
        assert_eq!(position_with_snap(win, target, AlignPlacement::TopRight, work), (1520, 200));
        assert_eq!(position_with_snap(win, target, AlignPlacement::BottomRight, work), (1520, 620));
    }

    #[test]
    fn the_result_is_clamped_into_the_work_area() {
        // top-right would sit at 2280, past a 1920-wide display, so it hugs the edge
        let win = (400, 300);
        let target = (1000, 200, 1280, 720);
        let work = (0, 0, 1920, 1080);
        let (x, y) = position_with_snap(win, target, AlignPlacement::TopRight, work);
        assert_eq!((x, y), (1920 - 400, 200));

        // and a client frame taller than the display keeps the overlay inside it
        let (x, y) = position_with_snap(win, (0, 0, 1280, 2000), AlignPlacement::BottomLeft, work);
        // -400 clamps to the left edge, and 1700 is pushed up so 300 fits in 1080
        assert_eq!((x, y), (0, 780));
    }
}

/// `RiftFlowCdTimerWindow`'s `isSending` latch: a second `sendInGame` while a run is in
/// flight answers `AlreadySending` instead of interleaving keystrokes.
static CD_TIMER_SENDING: AtomicBool = AtomicBool::new(false);

/// Clears the latch on the way out, including on an early return — the original did
/// the same in `finally`.
struct SendingGuard;

impl Drop for SendingGuard {
    fn drop(&mut self) {
        CD_TIMER_SENDING.store(false, Ordering::SeqCst);
    }
}

/// `RiftFlowCdTimerWindow.sendInGame`: the same three gates in the same order, then
/// Enter · text · Enter with the addon's own 20 ms / 65 ms cadence.
async fn send_in_game(text: String) -> Result<Value, IpcError> {
    if !riftflow_native::input::probe().0 {
        return Err(IpcError::business(
            "native input not supported",
            "NativeInputNotSupported",
        ));
    }

    if CD_TIMER_SENDING.swap(true, Ordering::SeqCst) {
        return Err(IpcError::business("cd-timer is sending", "AlreadySending"));
    }
    let _guard = SendingGuard;

    if !game_client_foreground() {
        return Err(IpcError::business(
            "game client is not foreground",
            "GameClientNotForeground",
        ));
    }

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }

    let failed = tokio::task::spawn_blocking(move || {
        let mut keyboard = KeyboardInjector::new();
        keyboard.send_key(CD_TIMER_ENTER_KEY_CODE, true);
        keyboard.wait(CD_TIMER_ENTER_KEY_INTERNAL_DELAY);
        keyboard.send_key(CD_TIMER_ENTER_KEY_CODE, false);
        keyboard.wait(CD_TIMER_INPUT_DELAY);
        keyboard.send_text(&trimmed);
        keyboard.wait(CD_TIMER_INPUT_DELAY);
        keyboard.send_key(CD_TIMER_ENTER_KEY_CODE, true);
        keyboard.wait(CD_TIMER_ENTER_KEY_INTERNAL_DELAY);
        keyboard.send_key(CD_TIMER_ENTER_KEY_CODE, false);
    })
    .await
    .is_err();

    if failed {
        return Err(IpcError::business(
            "native input not supported",
            "NativeInputNotSupported",
        ));
    }
    Ok(Value::Null)
}

const CD_TIMER_ENTER_KEY_CODE: u32 = 13;
const CD_TIMER_ENTER_KEY_INTERNAL_DELAY: u64 = 20;
const CD_TIMER_INPUT_DELAY: u64 = 65;

/// `GameClientMain.isGameClientForeground()`.
fn game_client_foreground() -> bool {
    riftflow_native::pids_by_name(riftflow_native::GAME_CLIENT_PROCESS_NAME)
        .into_iter()
        .any(|pid| riftflow_native::is_process_foreground(pid))
}
