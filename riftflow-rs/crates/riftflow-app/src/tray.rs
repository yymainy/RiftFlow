//! `tray-main` — the notification-area icon and its context menu.
//!
//! Port of `src/main/shards/tray/{index.ts,tray-menu-controller.ts,tray-state-watcher.ts}`.
//! The macOS application-menu branch of the original is not reproduced: this app
//! only ships on Windows, and the items it built there (`about` / `settings`) are
//! `Cmd+,`-style accelerators with no tray equivalent.
//!
//! Without this the `minimize-to-tray` close action hides the main window and
//! nothing in the shell can bring it back.

use std::sync::{Arc, Mutex, OnceLock};

use tauri::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri::image::Image;

use crate::windows::{self, WM_NAMESPACE};

pub const TRAY_ID: &str = "riftflow-tray";

const ICON: &[u8] = include_bytes!("../assets/RIFTFLOW_ICON.ico");

const SUFFIXES: [&str; 5] = [
    "main-window",
    "aux-window",
    "opgg-window",
    "ongoing-game-window",
    "cd-timer-window",
];

/// `tray.*` in `shared/i18n/{zh-CN,en}/main.yaml`, in menu order.
fn labels(locale: &str) -> Labels {
    if locale.starts_with("en") {
        Labels {
            aux_window: "Aux Window",
            opgg_window: "OP.GG Window",
            quit: "Quit",
            adjust_positions: "Adjust All Window Positions",
            dev_main: "Toggle DevTools - Main Window",
            dev_aux: "Toggle DevTools - Aux Window",
            dev_opgg: "Toggle DevTools - OP.GG Window",
            dev_ongoing: "Toggle DevTools - Ongoing Game Window",
            dev_cd_timer: "Toggle DevTools - Cooldown Timer Window",
        }
    } else {
        Labels {
            aux_window: "小窗口",
            opgg_window: "OP.GG 窗口",
            quit: "退出",
            adjust_positions: "调整所有窗口的位置",
            dev_main: "Toggle DevTools - 主窗口",
            dev_aux: "Toggle DevTools - 小窗口",
            dev_opgg: "Toggle DevTools - OP.GG 窗口",
            dev_ongoing: "Toggle DevTools - 对局窗口",
            dev_cd_timer: "Toggle DevTools - 计时器窗口",
        }
    }
}

struct Labels {
    aux_window: &'static str,
    opgg_window: &'static str,
    quit: &'static str,
    adjust_positions: &'static str,
    dev_main: &'static str,
    dev_aux: &'static str,
    dev_opgg: &'static str,
    dev_ongoing: &'static str,
    dev_cd_timer: &'static str,
}

/// The five flags the original's reactions watch: `settings.enabled` and
/// `state.ready` per window, plus the locale.
type Snapshot = (String, [(bool, bool); 4]);

static TRAY: OnceLock<Mutex<Option<Arc<tauri::tray::TrayIcon<Wry>>>>> = OnceLock::new();
static LAST: OnceLock<Mutex<Option<Snapshot>>> = OnceLock::new();

fn tray_slot() -> &'static Mutex<Option<Arc<tauri::tray::TrayIcon<Wry>>>> {
    TRAY.get_or_init(|| Mutex::new(None))
}

fn last_slot() -> &'static Mutex<Option<Snapshot>> {
    LAST.get_or_init(|| Mutex::new(None))
}

/// `window-manager-main/<suffix>` props, exactly what `TrayStateWatcher` reads.
fn window_flags(app: &AppHandle, suffix: &str) -> (bool, bool) {
    let state = app.state::<crate::AppState>();
    let namespace = format!("{WM_NAMESPACE}/{suffix}");
    let flag = |id: &str, key: &str| {
        state
            .state_sync
            .get(&namespace, id, key)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    };
    (flag("settings", "enabled"), flag("state", "ready"))
}

fn current_locale(app: &AppHandle) -> String {
    let state = app.state::<crate::AppState>();
    state
        .settings
        .get_setting("app-common-main", "locale")
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "zh-CN".to_string())
}

fn snapshot(app: &AppHandle) -> Snapshot {
    (
        current_locale(app),
        [
            window_flags(app, "aux-window"),
            window_flags(app, "opgg-window"),
            window_flags(app, "ongoing-game-window"),
            window_flags(app, "cd-timer-window"),
        ],
    )
}

fn item(
    app: &AppHandle,
    id: &str,
    text: &str,
    enabled: bool,
) -> anyhow::Result<MenuItem<Wry>> {
    Ok(MenuItem::with_id(app, id, text, enabled, None::<&str>)?)
}

/// `TrayMenuController.build()`'s template. The menu is rebuilt (not patched) on
/// change, which is also how the original handled a locale switch.
fn build_menu(app: &AppHandle) -> anyhow::Result<Menu<Wry>> {
    let snapshot = snapshot(app);
    let labels = labels(&snapshot.0);
    let [(aux_enabled, aux_ready), (opgg_enabled, opgg_ready), (ongoing_enabled, ongoing_ready), (cd_enabled, cd_ready)] =
        snapshot.1;

    let show_main = item(app, "show-main-window", "RiftFlow", true)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let dev_main = item(app, "dev-main-window", labels.dev_main, true)?;
    let dev_aux = item(
        app,
        "dev-aux-window",
        labels.dev_aux,
        aux_enabled && aux_ready,
    )?;
    let dev_opgg = item(
        app,
        "dev-opgg-window",
        labels.dev_opgg,
        opgg_enabled && opgg_ready,
    )?;
    let dev_ongoing = item(
        app,
        "dev-ongoing-game-window",
        labels.dev_ongoing,
        ongoing_enabled && ongoing_ready,
    )?;
    let dev_cd_timer = item(
        app,
        "dev-cd-timer-window",
        labels.dev_cd_timer,
        cd_enabled && cd_ready,
    )?;
    let adjust = item(app, "adjust-positions", labels.adjust_positions, true)?;

    let dev_items: Vec<&dyn IsMenuItem<Wry>> = vec![
        &dev_main,
        &dev_aux,
        &dev_opgg,
        &dev_ongoing,
        &dev_cd_timer,
        &separator,
        &adjust,
    ];
    let dev_submenu = Submenu::with_items(app, "Dev", true, &dev_items)?;

    let show_aux = item(
        app,
        "show-aux-window",
        labels.aux_window,
        aux_enabled && aux_ready,
    )?;
    let show_opgg = item(
        app,
        "show-opgg-window",
        labels.opgg_window,
        opgg_enabled && opgg_ready,
    )?;
    let quit = item(app, "quit", labels.quit, true)?;

    let items: Vec<&dyn IsMenuItem<Wry>> = vec![
        &show_main,
        &separator,
        &dev_submenu,
        &separator,
        &show_aux,
        &show_opgg,
        &quit,
    ];
    Ok(Menu::with_items(app, &items)?)
}

/// `onInit`'s `build()`. Call after the windows are created so the first snapshot
/// already sees the real `ready` flags.
pub fn start(app: &AppHandle) -> anyhow::Result<()> {
    let menu = build_menu(app)?;
    let icon = Image::from_bytes(ICON)?;

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("峡序 · RiftFlow")
        // the original pops the menu on right-click and restores the main window
        // on left-click
        .show_menu_on_left_click(false)
        .menu(&menu)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_or_restore(tray.app_handle(), "main-window");
            }
        })
        .on_menu_event(|app, event: MenuEvent| {
            handle_menu_event(app, event.id().as_ref());
        })
        .build(app)?;

    *tray_slot().lock().unwrap() = Some(Arc::new(tray));
    *last_slot().lock().unwrap() = Some(snapshot(app));
    eprintln!(
        "[tray] icon registered with {} window flags",
        SUFFIXES.len() - 1
    );
    Ok(())
}

/// The watcher half: the shell's existing poll calls this, and the menu is only
/// rebuilt when one of the six watched flags actually moved.
pub fn refresh_if_changed(app: &AppHandle) {
    let next = snapshot(app);
    let changed = {
        let mut last = last_slot().lock().unwrap();
        let changed = last.as_ref() != Some(&next);
        *last = Some(next);
        changed
    };
    if !changed {
        return;
    }

    let tray = tray_slot().lock().unwrap().clone();
    let Some(tray) = tray else { return };
    match build_menu(app) {
        Ok(menu) => {
            if let Err(error) = tray.set_menu(Some(menu)) {
                eprintln!("[tray] menu rebuild failed: {error}");
            }
        }
        Err(error) => eprintln!("[tray] menu rebuild failed: {error}"),
    }
}

fn show_or_restore(app: &AppHandle, suffix: &str) {
    let Some(window) = app.get_webview_window(suffix) else {
        return;
    };
    let state = app.state::<crate::AppState>();
    windows::show_or_restore(&window, false, &state.state_sync, suffix);
}

fn toggle_devtools(app: &AppHandle, suffix: &str) {
    let Some(window) = app.get_webview_window(suffix) else {
        return;
    };
    if window.is_devtools_open() {
        window.close_devtools();
    } else {
        window.open_devtools();
    }
}

/// `repositionWindowIfInvisible()` for every window: a window whose position is
/// outside every monitor gets pulled back on screen and shown.
fn adjust_positions(app: &AppHandle) {
    for suffix in SUFFIXES {
        let Some(window) = app.get_webview_window(suffix) else {
            continue;
        };
        let position = match window.outer_position() {
            Ok(position) => position,
            Err(_) => continue,
        };
        let visible = window
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .any(|monitor| {
                let work = monitor.position();
                let size = monitor.size();
                position.x >= work.x
                    && position.y >= work.y
                    && position.x < work.x + size.width as i32
                    && position.y < work.y + size.height as i32
            });
        if !visible {
            let _ = window.set_position(tauri::PhysicalPosition::new(80, 80));
            let _ = window.show();
        }
    }
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "show-main-window" => show_or_restore(app, "main-window"),
        "show-aux-window" => show_or_restore(app, "aux-window"),
        "show-opgg-window" => show_or_restore(app, "opgg-window"),
        "dev-main-window" => toggle_devtools(app, "main-window"),
        "dev-aux-window" => toggle_devtools(app, "aux-window"),
        "dev-opgg-window" => toggle_devtools(app, "opgg-window"),
        "dev-ongoing-game-window" => toggle_devtools(app, "ongoing-game-window"),
        "dev-cd-timer-window" => toggle_devtools(app, "cd-timer-window"),
        "adjust-positions" => adjust_positions(app),
        // `tray.quit` is `mainWindow.close(true)` — the forced path, so settings
        // have to reach SQLite before the process ends
        "quit" => {
            let state = app.state::<crate::AppState>();
            let flushed = state.settings.flush_all().unwrap_or_default();
            if flushed > 0 {
                eprintln!("[tray] flushed {flushed} pending setting writes");
            }
            app.exit(0);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_locale_picks_the_matching_label_table() {
        assert_eq!(labels("zh-CN").quit, "退出");
        assert_eq!(labels("en").quit, "Quit");
        assert_eq!(labels("en-US").aux_window, "Aux Window");
        assert_eq!(labels("").aux_window, "小窗口");
    }

    #[test]
    fn every_window_has_a_tray_action() {
        // the menu can only reach these five, and `SUFFIXES` is what the original
        // repositioned
        assert_eq!(SUFFIXES.len(), 5);
        for suffix in SUFFIXES {
            assert!(suffix.ends_with("-window"));
        }
    }
}
