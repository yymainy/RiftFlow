//! The `app-common-main` state/settings surface, which is the gate every renderer
//! window passes through on boot: its first IPC call is
//! `mobx-utils-main/subscribeAndGetInitialState('app-common-main', …)`, and if that
//! state was never declared the store throws and the whole app stalls.
//!
//! Prop paths and value shapes are taken from
//! `src/main/shards/app-common/{index.ts,state.ts}` and
//! `src/shared/types/common.ts`. Anything the Rust side cannot do yet reports
//! `available: false` rather than pretending — the UI reads these flags to decide
//! what to disable.

use std::sync::Arc;

use riftflow_state::StateSync;
use riftflow_store::settings::{SettingDeclaration, SettingFactory, boolean, string, string_enum};
use serde_json::{Value, json};

use crate::BASE_CONFIG_FILENAME;

pub const APP_COMMON_NAMESPACE: &str = "app-common-main";

const APP_THEME_VALUES: [&str; 9] = [
    "default",
    "light",
    "sakura",
    "butter",
    "mint",
    "dark",
    "graphite",
    "cyber",
    "aurora",
];

/// `NATIVE_SUPPORT`, rebuilt from what this port can actually do today.
pub fn native_support() -> Value {
    let win32 = cfg!(target_os = "windows");
    let elevated = riftflow_native::is_elevated();

    let requirement = |available: bool, requires_elevation: bool| {
        json!({
            "available": available,
            "availableOnCurrentPlatform": win32,
            "requiresElevation": requires_elevation,
        })
    };

    json!({
        // The shipped flag was `isWindows && isElevated && addon loaded && hook
        // installed`. The first two terms were there because one flag covered both
        // *listening* and *injecting into the elevated game client*; this port proves
        // listening works unelevated (verified live: hook → pipeline →
        // `keyboard-shortcuts-main:shortcut` reaches the renderer), so the flag tracks
        // the term that actually decides whether the settings UI's 选择 button has
        // anything behind it. The elevation term stays as `requiresElevation`, which is
        // what picks the hint when the hook is missing.
        "nativeInput": requirement(win32 && crate::hotkeys::input_available(), true),
        // `GetWindowPlacement` on the client's `RCLIENT` frame — no elevation needed,
        // same as the original's `available = isWindows`
        "getLeagueClientWindowPlacement": requirement(win32, false),
        "adjustLeagueClientWindowSize": requirement(win32 && elevated, win32),
        // implemented in riftflow_native, but the flag matches the original's
        // `isWindows && isElevated` condition
        "isProcessForeground": requirement(win32 && elevated, false),
    })
}

/// `<userData>/base-config.json`, the file `bootstrap/base-config.ts` reads.
pub fn read_base_config(user_data_dir: &std::path::Path) -> Option<Value> {
    let text = std::fs::read_to_string(user_data_dir.join(BASE_CONFIG_FILENAME)).ok()?;
    serde_json::from_str(&text).ok()
}

/// `writeBaseConfig(partial)` — merge into the on-disk base config, like
/// `manager.global.setLogLevel` does.
pub fn patch_base_config(user_data_dir: &std::path::Path, patch: &Value) -> anyhow::Result<Value> {
    let path = user_data_dir.join(BASE_CONFIG_FILENAME);
    let mut current = read_base_config(user_data_dir).unwrap_or_else(|| json!({}));

    if let (Some(target), Some(incoming)) = (current.as_object_mut(), patch.as_object()) {
        for (key, value) in incoming {
            target.insert(key.clone(), value.clone());
        }
    }

    std::fs::write(&path, current.to_string())
        .map_err(|error| anyhow::anyhow!("write {}: {error}", path.display()))?;
    Ok(current)
}

/// `baseConfig.disableHardwareAcceleration === true`, the condition bootstrap
/// checks before calling `app.disableHardwareAcceleration()`.
pub fn read_disable_hardware_acceleration(user_data_dir: &std::path::Path) -> bool {
    read_base_config(user_data_dir)
        .and_then(|config| {
            config
                .get("disableHardwareAcceleration")
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)}

/// The WebView2 equivalent of `app.disableHardwareAcceleration()`: the switch has
/// to reach the browser process as a command-line argument, and the only channel
/// a Tauri app has is this environment variable. Editing it in place keeps the
/// relaunched child (which inherits the environment) on the same setting and
/// preserves any other argument already present.
pub fn apply_disable_gpu(disabled: bool) {
    const ARGUMENT: &str = "--disable-gpu";
    const ENVIRONMENT: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";

    let current = std::env::var(ENVIRONMENT).unwrap_or_default();
    let mut arguments: Vec<&str> = current
        .split_whitespace()
        .filter(|argument| *argument != ARGUMENT)
        .collect();

    if disabled {
        arguments.push(ARGUMENT);
    }

    if arguments.is_empty() {
        std::env::remove_var(ENVIRONMENT);
    } else {
        std::env::set_var(ENVIRONMENT, arguments.join(" "));
    }

    // the switch has no visible effect anywhere else, so say what the browser
    // process is actually going to receive
    if disabled {
        eprintln!("[app-common] 已禁用硬件加速，{ENVIRONMENT}={}", arguments.join(" "));
    }
}

/// The seven settings `AppCommonMain` registers.
pub fn declarations(locale_default: &str) -> Vec<SettingDeclaration> {
    let http_proxy_default = json!({ "strategy": "disable", "port": 7890, "host": "127.0.0.1" });

    vec![
        SettingDeclaration {
            key: "showFreeSoftwareDeclaration".to_string(),
            default: json!(true),
            schema: Some(boolean()),
            transform: None,
            side_effect: None,
        },
        SettingDeclaration {
            key: "locale".to_string(),
            default: json!(locale_default),
            schema: Some(string()),
            transform: None,
            side_effect: None,
        },
        SettingDeclaration {
            key: "theme".to_string(),
            default: json!("default"),
            schema: Some(string_enum(&APP_THEME_VALUES)),
            transform: None,
            side_effect: None,
        },
        SettingDeclaration {
            key: "httpProxy".to_string(),
            default: http_proxy_default.clone(),
            // the zod object equivalent, kept narrow on purpose
            schema: Some(Arc::new(move |value: &Value| {
                let Some(map) = value.as_object() else {
                    return Err("Expected an httpProxy object".to_string());
                };
                let strategy = map
                    .get("strategy")
                    .and_then(Value::as_str)
                    .filter(|s| matches!(*s, "force" | "auto" | "disable"))
                    .ok_or_else(|| "Expected strategy of force|auto|disable".to_string())?;
                let port = map
                    .get("port")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| "Expected port number".to_string())?;
                let host = map
                    .get("host")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "Expected host string".to_string())?;
                Ok(json!({ "strategy": strategy, "port": port, "host": host }))
            })),
            transform: None,
            side_effect: None,
        },
        SettingDeclaration {
            key: "streamerMode".to_string(),
            default: json!(false),
            schema: Some(boolean()),
            transform: None,
            side_effect: None,
        },
        SettingDeclaration {
            key: "streamerModeUseRiftFlowStyledName".to_string(),
            default: json!(false),
            schema: Some(boolean()),
            transform: None,
            side_effect: None,
        },
        SettingDeclaration {
            key: "preferredLolSource".to_string(),
            default: json!("sgp"),
            schema: Some(string_enum(&["sgp", "lcu"])),
            transform: None,
            side_effect: None,
        },
    ]
}

/// `AppCommonMain._setupState` — declare, load from storage, then publish.
pub fn publish(
    state_sync: &Arc<StateSync>,
    settings: &Arc<SettingFactory>,
    user_data_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let locale_default = if locale_is_chinese() { "zh-CN" } else { "en" };

    let service = settings.register(APP_COMMON_NAMESPACE, declarations(locale_default))?;
    service.apply_to_state()?;

    let sync = state_sync.clone();
    service.set_change_sink(Arc::new(move |namespace, key, value| {
        let _ = sync.set(namespace, "settings", key, value.clone());
    }));

    let elevated = riftflow_native::is_elevated();
    let base_config = read_base_config(user_data_dir);
    let disable_hardware_acceleration = base_config
        .as_ref()
        .and_then(|config| config.get("disableHardwareAcceleration"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let settings_props: Vec<(&str, Value)> = service
        .declaration("showFreeSoftwareDeclaration")
        .map(|_| {
            vec![
                (
                    "showFreeSoftwareDeclaration",
                    service.get("showFreeSoftwareDeclaration"),
                ),
                ("locale", service.get("locale")),
                ("theme", service.get("theme")),
                ("httpProxy", service.get("httpProxy")),
                ("streamerMode", service.get("streamerMode")),
                (
                    "streamerModeUseRiftFlowStyledName",
                    service.get("streamerModeUseRiftFlowStyledName"),
                ),
                ("preferredLolSource", service.get("preferredLolSource")),
            ]
        })
        .unwrap_or_default();

    state_sync.prop_sync(APP_COMMON_NAMESPACE, "settings", &settings_props, true)?;

    state_sync.prop_sync(
        APP_COMMON_NAMESPACE,
        "state",
        &[
            ("isElevated", json!(elevated)),
            (
                "platform",
                json!(if cfg!(target_os = "windows") {
                    "win32"
                } else {
                    "unknown"
                }),
            ),
            ("disableHardwareAcceleration", json!(disable_hardware_acceleration)),
            ("baseConfig", base_config.unwrap_or(Value::Null)),
            ("startupDeepLink", Value::Null),
            ("isRunInTempDir", json!(runs_from_temp_dir())),
            ("nativeSupport", native_support()),
        ],
        true,
    )?;

    Ok(())
}

/// `_getSystemLocale`: Chinese when the OS locale starts with `zh`.
fn locale_is_chinese() -> bool {
    std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .map(|value| value.to_lowercase().starts_with("zh"))
        // Windows does not export LANG; read the user locale name instead.
        // Note `map` keeps this a `Result`, so the fallback takes the error.
        .unwrap_or_else(|_| win_locale_is_chinese())
}

#[cfg(windows)]
fn win_locale_is_chinese() -> bool {
    // GetUserDefaultLocaleName returns e.g. "zh-CN"
    use std::mem::MaybeUninit;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, length: i32) -> i32;
    }
    let mut buffer = [MaybeUninit::<u16>::new(0); 86];
    let written = unsafe {
        GetUserDefaultLocaleName(
            buffer.as_mut_ptr().cast(),
            buffer.len() as i32,
        )
    };
    if written <= 0 {
        return false;
    }
    let units: Vec<u16> = buffer
        .iter()
        .map(|unit| unsafe { unit.assume_init() })
        .collect();
    String::from_utf16_lossy(&units).to_lowercase().starts_with("zh")
}

#[cfg(not(windows))]
fn win_locale_is_chinese() -> bool {
    false
}

/// `isRunInTempDir` — the packaged app warns when run out of a temp directory.
fn runs_from_temp_dir() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let path = exe.to_string_lossy().to_lowercase();
    path.contains("\\temp\\") || path.contains("\\appdata\\local\\temp") || path.contains("/tmp/")
}
