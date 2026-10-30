//! Shell-level IPC handlers that every renderer window needs before any feature
//! shard can boot: the state-sync subscription, the renderer's log mirror, the
//! settings facade and the `app-common-main` basics.
//!
//! These handlers own real side effects (log file, clipboard, relaunch), so they
//! answer with an explicit error when a side effect fails rather than a silent
//! success — a button in the UI can never appear to work when it does not.

use std::sync::Arc;

use riftflow_ipc::{RiftFlowIpcMain, IpcError};
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;
use serde_json::{Value, json};

pub const MOBX_UTILS_NAMESPACE: &str = "mobx-utils-main";
pub const LOGGER_FACTORY_NAMESPACE: &str = "logger-factory-main";
pub const SETTING_FACTORY_NAMESPACE: &str = "setting-factory-main";
pub const APP_COMMON_NAMESPACE: &str = "app-common-main";
pub const RENDERER_DEBUG_NAMESPACE: &str = "renderer-debug-main";

pub fn register(
    ipc: &Arc<RiftFlowIpcMain>,
    state_sync: &Arc<StateSync>,
    settings: &Arc<SettingFactory>,
    user_data_dir: &std::path::Path,
    version: &str,
) -> anyhow::Result<()> {
    // `MobxUtilsIpcHandlers` — every renderer shard store calls this on mount and
    // then receives `update-state-prop/<key>` pushes for its lifetime
    let sync = state_sync.clone();
    ipc.on_call(
        MOBX_UTILS_NAMESPACE,
        "subscribeAndGetInitialState",
        riftflow_ipc::handler(move |ctx, args| {
            let sync = sync.clone();
            async move {
                let namespace = arg_str(&args, 0)?;
                let state_id = arg_str(&args, 1)?;
                let initial = sync
                    .subscribe_and_get_initial_state(ctx.renderer, namespace, state_id)
                    .map_err(|error| {
                        // the JS side throws a plain Error here, so keep the message
                        IpcError::plain(error)
                    })?;
                Ok(initial)
            }
        }),
    )?;

    // `LoggerFactoryIpcHandlers` — the renderer mirrors its logs into main, and
    // main writes them to the same file the Electron build did
    ipc.on_call(
        LOGGER_FACTORY_NAMESPACE,
        "log",
        riftflow_ipc::handler(|_ctx, args| async move {
            let namespace = args.first().and_then(Value::as_str).unwrap_or("?");
            let level = crate::logger::parse_level(args.get(1).and_then(Value::as_str).unwrap_or("info"));
            let message = crate::logger::format_values(args.get(2..).unwrap_or(&[]));

            crate::logger::log(namespace, level, &message);

            Ok(Value::Null)
        }),
    )?;

    let user_data_dir = user_data_dir.to_path_buf();

    // mirrors `global.setLogLevel`: persist, then push the synced prop
    let sync_for_level = state_sync.clone();
    let user_data = user_data_dir.to_path_buf();
    ipc.on_call(
        LOGGER_FACTORY_NAMESPACE,
        "setLogLevel",
        riftflow_ipc::handler(move |_ctx, args| {
            let sync = sync_for_level.clone();
            let user_data = user_data.clone();
            async move {
                let level = arg_str(&args, 0)?;
                let persisted = crate::app_common::patch_base_config(
                    &user_data,
                    &json!({ "logLevel": level }),
                )
                .map_err(IpcError::plain)?;

                let resolved = persisted
                    .get("logLevel")
                    .and_then(Value::as_str)
                    .unwrap_or("info")
                    .to_string();

                // `LoggerFactoryIpcHandlers.setLogLevel` logs the change first, so
                // this line is still written at the old level
                crate::logger::log(
                    LOGGER_FACTORY_NAMESPACE,
                    crate::logger::Level::Info,
                    &format!("Setting log level to {resolved}"),
                );
                crate::logger::set_level(crate::logger::parse_level(&resolved));

                // `LoggerFactoryMain` reacts on the app-level event and sets state
                sync.set(
                    LOGGER_FACTORY_NAMESPACE,
                    "state",
                    "logLevel",
                    Value::String(resolved),
                )
                .map_err(IpcError::plain)?;

                Ok(Value::Null)
            }
        }),
    )?;

    // `loggerFactory.openLogsDir()`: `shell.showItemInFolder(logsDir/LA_*.log)`
    ipc.on_call(
        LOGGER_FACTORY_NAMESPACE,
        "openLogsDir",
        riftflow_ipc::handler(|_ctx, _args| async move {
            let path = crate::logger::log_file_path()
                .ok_or_else(|| IpcError::plain("日志文件尚未创建"))?;

            if crate::file_io::reveal_in_folder(&path.to_string_lossy()) {
                Ok(Value::Null)
            } else {
                Err(IpcError::plain("无法打开日志目录"))
            }
        }),
    )?;

    // `SettingFactoryIpcHandlers`
    let factory = settings.clone();
    ipc.on_call(
        SETTING_FACTORY_NAMESPACE,
        "get",
        riftflow_ipc::handler(move |_ctx, args| {
            let factory = factory.clone();
            async move {
                let namespace = arg_str(&args, 0)?;
                let key = arg_str(&args, 1)?;
                factory
                    .get_setting(namespace, key)
                    .map_err(IpcError::plain)
            }
        }),
    )?;

    let factory = settings.clone();
    ipc.on_call(
        SETTING_FACTORY_NAMESPACE,
        "set",
        riftflow_ipc::handler(move |_ctx, args| {
            let factory = factory.clone();
            async move {
                let namespace = arg_str(&args, 0)?;
                let key = arg_str(&args, 1)?;
                let value = args.get(2).cloned().unwrap_or(Value::Null);
                factory
                    .set_setting(namespace, key, value)
                    .await
                    .map_err(IpcError::plain)?;
                Ok(Value::Null)
            }
        }),
    )?;

    let factory = settings.clone();
    ipc.on_call(
        SETTING_FACTORY_NAMESPACE,
        "getByPrefix",
        riftflow_ipc::handler(move |_ctx, args| {
            let factory = factory.clone();
            async move {
                let namespace = arg_str(&args, 0)?;
                let prefix = args
                    .get(1)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let rows = factory
                    .get_by_prefix(namespace, &prefix)
                    .map_err(IpcError::plain)?;
                Ok(Value::Object(
                    rows.into_iter()
                        .map(|(key, value)| {
                            let short = key
                                .strip_prefix(&format!("{namespace}/"))
                                .unwrap_or(&key)
                                .to_string();
                            (short, value)
                        })
                        .collect(),
                ))
            }
        }),
    )?;

    let factory = settings.clone();
    ipc.on_call(
        SETTING_FACTORY_NAMESPACE,
        "removeByPrefix",
        riftflow_ipc::handler(move |_ctx, args| {
            let factory = factory.clone();
            async move {
                let namespace = arg_str(&args, 0)?;
                let prefix = args
                    .get(1)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let removed = factory
                    .remove_by_prefix(namespace, &prefix)
                    .map_err(IpcError::plain)?;
                Ok(Value::from(removed as i64))
            }
        }),
    )?;

    let factory = settings.clone();
    ipc.on_call(
        SETTING_FACTORY_NAMESPACE,
        "exportSettingsToJsonFile",
        riftflow_ipc::handler(move |_ctx, _args| {
            let factory = factory.clone();
            async move {
                let Some(path) = crate::file_io::pick_path(true, "riftflow-settings.json").await
                else {
                    return Ok(Value::Null);
                };

                let db = factory.db().clone();
                crate::saved_player::export_settings(&db, path).await
            }
        }),
    )?;

    let factory = settings.clone();
    ipc.on_call(
        SETTING_FACTORY_NAMESPACE,
        "importSettingsFromJsonFile",
        riftflow_ipc::handler(move |_ctx, _args| {
            let factory = factory.clone();
            async move {
                let Some(path) = crate::file_io::pick_path(false, "riftflow-settings.json").await
                else {
                    return Ok(Value::Null);
                };

                let db = factory.db().clone();
                let written = crate::saved_player::import_settings(&db, path).await?;
                // `shared.global.restart()` — the settings only take effect on a
                // fresh boot, and every shard has already read them
                crate::file_io::restart(&factory);
                Ok(written)
            }
        }),
    )?;

    // `AppCommonIpcHandlers`
    let version_owned = version.to_string();
    ipc.on_call(
        APP_COMMON_NAMESPACE,
        "getVersion",
        riftflow_ipc::handler(move |_ctx, _args| {
            let version = version_owned.clone();
            async move { Ok(Value::String(version)) }
        }),
    )?;

    ipc.on_call(
        APP_COMMON_NAMESPACE,
        "getRuntimeInfo",
        riftflow_ipc::handler(|_ctx, _args| async move {
            Ok(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "platform": if cfg!(target_os = "windows") { "win32" } else { "unknown" },
                "arch": std::env::consts::ARCH,
                "pid": std::process::id(),
                "isElevated": riftflow_native::is_elevated(),
                "appIsQuitting": false,
            }))
        }),
    )?;

    let user_data_for_open = user_data_dir.clone();
    ipc.on_call(
        APP_COMMON_NAMESPACE,
        "openUserDataDir",
        riftflow_ipc::handler(move |_ctx, _args| {
            let dir = user_data_for_open.clone();
            async move {
                if crate::file_io::reveal(&dir.to_string_lossy()) {
                    Ok(Value::Null)
                } else {
                    Err(IpcError::plain("无法打开数据目录"))
                }
            }
        }),
    )?;

    // `setDisableHardwareAccelerationAndRelaunch`: the same two early returns,
    // then baseConfig + relaunch. Electron had `app.disableHardwareAcceleration()`;
    // WebView2 takes it as a browser argument, which is what `apply_disable_gpu`
    // writes into the inherited environment so the relaunched process picks it up.
    let sync_for_gpu = state_sync.clone();
    let user_data_for_gpu = user_data_dir.clone();
    let settings_for_restart = settings.clone();
    ipc.on_call(
        APP_COMMON_NAMESPACE,
        "setDisableHardwareAcceleration",
        riftflow_ipc::handler(move |_ctx, args| {
            let sync = sync_for_gpu.clone();
            let user_data = user_data_for_gpu.clone();
            let settings = settings_for_restart.clone();
            async move {
                let disabled = args
                    .first()
                    .and_then(Value::as_bool)
                    .ok_or_else(|| IpcError::plain("disabled should be a boolean"))?;

                let current = sync
                    .get(APP_COMMON_NAMESPACE, "state", "disableHardwareAcceleration")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);

                if disabled == current {
                    return Ok(Value::Null);
                }

                crate::app_common::patch_base_config(
                    &user_data,
                    &json!({ "disableHardwareAcceleration": disabled }),
                )
                .map_err(IpcError::plain)?;

                crate::app_common::apply_disable_gpu(disabled);
                crate::file_io::restart(&settings);

                Ok(Value::Null)
            }
        }),
    )?;

    // `relaunchAsAdministrator`: elevate a new instance, then `app.exit()`
    let settings_for_exit = settings.clone();
    ipc.on_call(
        APP_COMMON_NAMESPACE,
        "relaunchAsAdministrator",
        riftflow_ipc::handler(move |_ctx, _args| {
            let settings = settings_for_exit.clone();
            async move {
                crate::file_io::relaunch_elevated().map_err(IpcError::plain)?;
                crate::file_io::quit(&settings);

                Ok(Value::Null)
            }
        }),
    )?;
    // `clipboard.readText()`: an empty clipboard is an empty string, not an error
    ipc.on_call(
        APP_COMMON_NAMESPACE,
        "readClipboardText",
        riftflow_ipc::handler(|_ctx, _args| async move {
            Ok(Value::String(
                riftflow_native::clipboard::read_text().unwrap_or_default(),
            ))
        }),
    )?;

    let settings_for_quit = settings.clone();
    ipc.on_call(
        APP_COMMON_NAMESPACE,
        "exit",
        riftflow_ipc::handler(move |_ctx, _args| {
            let settings = settings_for_quit.clone();
            async move {
                crate::file_io::quit(&settings);

                Ok(Value::Null)
            }
        }),
    )?;

    // `RendererDebugIpcHandlers` — both flags are real writes against the synced
    // state; only the LCU-event side effects wait for the debugger shard.
    for (method, prop) in [
        ("setSendAllNativeLcuEvents", "sendAllNativeLcuEvents"),
        ("setLogAllLcuEvents", "logAllLcuEvents"),
    ] {
        let sync = state_sync.clone();
        let prop = prop.to_string();
        ipc.on_call(
            RENDERER_DEBUG_NAMESPACE,
            method,
            riftflow_ipc::handler(move |_ctx, args| {
                let sync = sync.clone();
                let prop = prop.clone();
                async move {
                    let enabled = args
                        .first()
                        .and_then(Value::as_bool)
                        .ok_or_else(|| IpcError::plain("enabled should be a boolean"))?;
                    sync.set(RENDERER_DEBUG_NAMESPACE, "state", &prop, json!(enabled))
                        .map(|_| Value::Null)
                        .map_err(IpcError::plain)
                }
            }),
        )?;
    }

    Ok(())
}

fn arg_str(args: &[Value], index: usize) -> Result<&str, IpcError> {
    args.get(index)
        .and_then(Value::as_str)
        .ok_or_else(|| IpcError::plain(format!("argument {index} should be a string")))
}
