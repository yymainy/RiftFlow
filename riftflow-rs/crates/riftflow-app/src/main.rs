#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri shell. This is the process that replaces Electron's main + preload:
//! it owns the windows, the shard runtime, the IPC router and the renderer shim.
//!
//! Boot order mirrors `src/main/bootstrap/index.ts`: logger → base config →
//! database → shard registrations → `manager.setup()` → windows.

mod riftflow_api_main;
mod app_common;
mod archive;
mod auto_champ_config;
mod auto_gameflow;
mod auto_misc;
mod auto_select;
mod aux_windows;
mod file_io;
mod handlers;
mod hotkeys;
mod in_game_send_main;
mod respawn_timer;
mod launchers;
mod lc_state;
mod logger;
mod ongoing_game_main;
mod protocol;
mod riot_client;
mod rng;
mod saved_player;
mod self_update;
mod settings_registry;
mod sgp_service;
mod shim;
mod state_bootstrap;
mod tray;
mod windows;

/// `<userData>/base-config.json`, read by `bootstrap/base-config.ts` on the JS side.
pub const BASE_CONFIG_FILENAME: &str = "base-config.json";

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, State, WebviewWindow, WebviewWindowBuilder};

use riftflow_ipc::{RiftFlowIpcMain, CallOutcome, RendererId, RendererTransport};
use riftflow_lcu::{
    CredentialStore, LeagueClientMain, LeagueClientUxMain, ProductionDialer, UxCommandLine,
};
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;
use riftflow_store::Database;
use serde_json::{json, Value};

/// Maps the router's numeric renderer ids back to live Tauri windows.
#[derive(Default)]
struct TauriTransport {
    by_id: Mutex<HashMap<RendererId, WebviewWindow>>,
    by_label: Mutex<HashMap<String, RendererId>>,
}

impl TauriTransport {
    fn attach(&self, label: &str, id: RendererId, window: &WebviewWindow) {
        self.by_label.lock().unwrap().insert(label.to_string(), id);
        self.by_id.lock().unwrap().insert(id, window.clone());
    }

    fn id_of(&self, label: &str) -> Option<RendererId> {
        self.by_label.lock().unwrap().get(label).copied()
    }

    fn detach(&self, label: &str) {
        if let Some(id) = self.by_label.lock().unwrap().remove(label) {
            self.by_id.lock().unwrap().remove(&id);
        }
    }
}

impl RendererTransport for TauriTransport {
    /// Electron's `webContents.send('riftflow-event', ...args)` equivalent. Delivered
    /// through `eval` so the shim does not depend on Tauri's private event
    /// protocol; the payload shape is unchanged.
    fn send_to(&self, renderer: RendererId, event: &str, args: Vec<Value>) -> anyhow::Result<()> {
        let Some(window) = self.by_id.lock().unwrap().get(&renderer).cloned() else {
            return Ok(());
        };

        let payload = serde_json::to_string(&args)?;
        let script = format!(
            "window.__riftflowDispatch && window.__riftflowDispatch({:?}, {payload})",
            event
        );
        if std::env::var("RIFTFLOW_IPC_TRACE").is_ok() {
            eprintln!(
                "[ipc-push] renderer={renderer} script={}",
                &script[..script.len().min(120)]
            );
        }
        window.eval(&script).map_err(|e| anyhow::anyhow!(e.to_string()))
    }
}

struct AppState {
    ipc: Arc<RiftFlowIpcMain>,
    transport: Arc<TauriTransport>,
    next_renderer_id: AtomicU64,
    league_client: Arc<LeagueClientMain>,
    ux: Arc<LeagueClientUxMain>,
    window_manager: Arc<windows::WindowManager>,
    state_sync: Arc<StateSync>,
    settings: Arc<SettingFactory>,
    sgp_gateway: Arc<riftflow_sgp::SgpGateway>,
    riot_client: Arc<riot_client::RiotClientService>,
    riftflow_api: Arc<riftflow_api::state::RiftFlowApiRuntime>,
    version: String,
}

/// Credentials are persisted exactly where RiftFlow puts them, so an existing
/// install's `lastConnectedClient` keeps working across the port.
struct SqliteCredentialStore {
    factory: Arc<SettingFactory>,
}

impl CredentialStore for SqliteCredentialStore {
    fn save_last_connected_client(&self, value: &Value) {
        let _ = self.factory.save_raw("league-client-main", "lastConnectedClient", value);
    }

    fn get_last_connected_client(&self) -> Option<Value> {
        self.factory
            .get_raw("league-client-main", "lastConnectedClient")
            .ok()
            .flatten()
    }

    fn remove_last_connected_client(&self) {
        let _ = self
            .factory
            .remove_raw("league-client-main", "lastConnectedClient");
    }
}

#[tauri::command]
async fn riftflow_call(
    state: State<'_, AppState>,
    window: WebviewWindow,
    namespace: String,
    fn_name: String,
    args: Vec<Value>,
) -> Result<Value, String> {
    if std::env::var("RIFTFLOW_IPC_TRACE").is_ok() {
        eprintln!("[ipc] riftflow_call {namespace}:{fn_name} args={}", args.len());
    }
    // the Electron event carries the sender; the router's handlers key off it
    let renderer = renderer_id_for(&state, &window);

    match state
        .ipc
        .handle_call(renderer, &namespace, &fn_name, args)
        .await
    {
        CallOutcome::Reply(envelope) => {
            // the envelope carries business failures *inside* a successful reply, so
            // without this line a screen that quietly errors looks identical to one
            // that works
            if !envelope.success && std::env::var("RIFTFLOW_IPC_TRACE").is_ok() {
                let detail = envelope
                    .error
                    .as_ref()
                    .map(Value::to_string)
                    .unwrap_or_default();
                eprintln!(
                    "[ipc:err] {namespace}:{fn_name} {}",
                    detail.chars().take(220).collect::<String>()
                );
            }
            Ok(envelope.to_json())
        }
        // unknown methods reject, like Electron rethrowing outside the envelope
        CallOutcome::Rejected(message) => {
            if std::env::var("RIFTFLOW_IPC_TRACE").is_ok() {
                eprintln!("[ipc:reject] {namespace}:{fn_name} {message}");
            }
            Err(message)
        }
    }
}

#[tauri::command]
fn riftflow_renderer_register(
    state: State<'_, AppState>,
    window: WebviewWindow,
    action: Option<String>,
) -> Value {
    let renderer = match state.transport.id_of(window.label()) {
        Some(existing) => existing,
        None => {
            let fresh = state.next_renderer_id.fetch_add(1, Ordering::SeqCst) + 1;
            state.transport.attach(window.label(), fresh, &window);
            fresh
        }
    };

    let envelope = state
        .ipc
        .renderer_register(renderer, action.as_deref());

    match action.as_deref() {
        Some("unregister") => state.transport.detach(window.label()),
        _ => state.transport.attach(window.label(), renderer, &window),
    }

    envelope.to_json()
}

fn renderer_id_for(state: &AppState, window: &WebviewWindow) -> RendererId {
    if let Some(existing) = state.transport.id_of(window.label()) {
        return existing;
    }
    let fresh = state.next_renderer_id.fetch_add(1, Ordering::SeqCst) + 1;
    state.transport.attach(window.label(), fresh, window);
    fresh
}

#[tauri::command]
fn shell_smoke(state: State<'_, AppState>) -> Value {
    // used by the shell's own test page to confirm the round trip both ways
    json!({
        "version": state.version,
        "registeredRenderers": state.ipc.registered_renderer_ids(),
        "connectionState": format!("{:?}", state.league_client.state.connection_state()),
    })
}

#[tauri::command]
fn shell_report(note: String) {
    eprintln!("[renderer] {note}");
}

/// The renderer's own key stream, handed to the same shortcut pipeline as the hook's.
/// `hotkeys::process_external_key_event` explains why the hook cannot see keystrokes
/// aimed at this process's own windows; the shim forwards them with the side-specific
/// modifier code, and the hook stays authoritative where it does report.
#[tauri::command]
fn riftflow_key_event(key_code: u16, is_down: bool) {
    if let Some(shard) = crate::hotkeys::instance() {
        shard.process_external_key_event(key_code, is_down);
    }
}

/// The `riftflow://` proxy as a command, which is how the renderer's `fetch` traffic
/// reaches the LCU (see `protocol.rs`).
#[tauri::command]
async fn riftflow_protocol_fetch(
    app: AppHandle,
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
) -> Result<Value, String> {
    protocol::fetch(&app, url, method, headers, body).await
}

fn user_data_dir() -> PathBuf {
    // Electron's `app.getPath('userData')` for this app id
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("riftflow")
}

/// Writes the router's real `(namespace, method)` registry as JSON, for
/// `tmp/contract_namespaces.py`. A grep cannot see the per-window namespaces or the
/// loop registrations, and the renderer rejects a call whose namespace is wrong even
/// when the method name exists.
fn dump_ipc_registry(handle: &tauri::AppHandle, path: &std::ffi::OsStr) -> bool {
    let state = handle.state::<AppState>();
    let mut grouped: std::collections::BTreeMap<String, Vec<String>> = Default::default();

    for (namespace, method) in state.ipc.registered_calls() {
        grouped.entry(namespace).or_default().push(method);
    }

    let total: usize = grouped.values().map(|methods| methods.len()).sum();
    let document = json!({ "total": total, "namespaces": grouped });
    let path = std::path::PathBuf::from(path);

    match std::fs::write(&path, serde_json::to_string_pretty(&document).unwrap_or_default()) {
        Ok(()) => eprintln!("[shell] IPC 注册表 ({total} 个方法) 已写到 {}", path.display()),
        Err(error) => eprintln!(
            "[shell] 无法写 IPC 注册表 {}: {error}",
            path.display()
        ),
    }

    // one attempt per run; retrying every poll would only rewrite the same file
    true
}

fn main() -> anyhow::Result<()> {
    // WebView2 defaults its user-data folder next to the executable, which fails
    // for a dev build sitting under target/debug (0x800700AA). Point it at the
    // app's own data directory instead, like the Electron build does.
    let webview_data = user_data_dir().join("EBWebView");
    std::fs::create_dir_all(&webview_data).ok();
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview_data);

    let transport = Arc::new(TauriTransport::default());
    let ipc = Arc::new(RiftFlowIpcMain::new(transport.clone()));
    let state_sync = Arc::new(StateSync::new(ipc.clone()));

    let db_path = {
        let dir = user_data_dir();
        std::fs::create_dir_all(&dir).ok();
        Database::default_path(&dir)
    };

    // bootstrap's own order: read base config, `initAppLogger(logLevel)`, then the
    // hardware-acceleration switch — all before a shard can emit a log line
    let base_config_dir = db_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    let log_level = app_common::read_base_config(&base_config_dir)
        .and_then(|config| {
            config
                .get("logLevel")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "info".to_string());
    let log_file = logger::init(&logger::default_logs_dir(), logger::parse_level(&log_level));
    app_common::apply_disable_gpu(app_common::read_disable_hardware_acceleration(&base_config_dir));

    let (db, storage_state) = Database::initialize(&db_path)?;
    let db = Arc::new(Mutex::new(db));
    let settings = Arc::new(SettingFactory::new(db.clone()));

    let credential_store = Arc::new(SqliteCredentialStore {
        factory: settings.clone(),
    });

    let league_client = Arc::new(LeagueClientMain::new(
        ipc.clone(),
        state_sync.clone(),
        credential_store,
        Arc::new(ProductionDialer),
    ));

    // the native Toolhelp + NtQueryInformationProcess path, i.e. the same source
    // the `riftflow-tools` addon used; PowerShell remains available as the fallback
    let ux = Arc::new(LeagueClientUxMain::new(
        ipc.clone(),
        state_sync.clone(),
        Arc::new(riftflow_native::Win32Probe),
        riftflow_native::is_elevated(),
    ));

    // `NATIVE_SUPPORT.adjustLeagueClientWindowSize.available` is `isWindows &&
    // isElevated`; the shard reads an injected adjuster as exactly that flag, so an
    // un-elevated run keeps `fixWindowMethodA` the silent no-op the original was
    if cfg!(windows) && riftflow_native::is_elevated() {
        riftflow_lcu::shard::set_window_adjuster(riftflow_native::placement::adjust_league_client_window_size);
    }

    league_client.publish_initial_state()?;
    league_client.register_ipc()?;
    ux.publish_initial_state()?;
    ux.register_ipc(league_client.clone())?;

    // Started here, not further down: `app_common::publish` snapshots the hook's
    // liveness into `nativeSupport.nativeInput.available`, which is what the settings
    // UI uses to enable the shortcut-binding button.
    hotkeys::start(&ipc)?;

    // the renderer's first IPC call subscribes to these props, so they have to
    // exist before any window loads
    app_common::publish(&state_sync, &settings, &base_config_dir)?;

    let declared =
        state_bootstrap::declare_all(&state_sync, &settings, storage_state.using_higher_version_db, &log_level)?;
    eprintln!("[shell] 日志文件 {}", log_file.display());
    eprintln!("[shell] declared {} synced state surfaces", declared.len());
    if !state_bootstrap::PLACEHOLDER_NAMESPACES.is_empty() {
        eprintln!(
            "[shell] placeholder (shard not ported yet): {}",
            state_bootstrap::PLACEHOLDER_NAMESPACES.join(", ")
        );
    }

    // the LCU sub-states and the SGP availability the renderer needs to draw
    // anything at all (`lc-state/index.ts`, `sgp/state.ts`)
    lc_state::publish_builtin(&state_sync)?;
    lc_state::mark_set_props(&state_sync);
    lc_state::watch(&state_sync, &league_client)?;
    lc_state::sync_availability(&state_sync, &league_client);

    let settings_hub = settings_registry::register_all(&state_sync, &settings)?;

    auto_gameflow::start(
        league_client.clone(),
        state_sync.clone(),
        ipc.clone(),
        settings.clone(),
        &settings_hub,
    )?;

    auto_misc::start(
        league_client.clone(),
        state_sync.clone(),
        ipc.clone(),
        settings.clone(),
        &settings_hub,
    )?;

    auto_champ_config::start(
        league_client.clone(),
        state_sync.clone(),
        ipc.clone(),
        settings.clone(),
        &settings_hub,
    )?;

    auto_select::start(
        league_client.clone(),
        state_sync.clone(),
        ipc.clone(),
        settings.clone(),
        &settings_hub,
    )?;

    respawn_timer::start(
        league_client.clone(),
        state_sync.clone(),
        ipc.clone(),
        settings.clone(),
        &settings_hub,
    )?;

    handlers::register(
        &ipc,
        &state_sync,
        &settings,
        db_path.parent().unwrap_or(std::path::Path::new(".")),
        env!("CARGO_PKG_VERSION"),
    )?;

    let sgp_gateway = Arc::new(riftflow_sgp::SgpGateway::new());

    // The real shard: the stub that used to answer `getAll` with empty objects is
    // gone, because an all-empty ongoing game looked like a working feature
    let ongoing_game = ongoing_game_main::register(
        &ipc,
        &state_sync,
        &league_client,
        &settings,
        &sgp_gateway,
        env!("CARGO_PKG_VERSION"),
    )?;
    ongoing_game_main::start(&ongoing_game, &league_client, &state_sync, &settings)?;

    saved_player::register(&ipc, &settings)?;
    archive::register(&ipc, &league_client, &state_sync, &base_config_dir, &settings)?;

    // The preset send surface: 22 methods, the three selection props, and the two
    // reactions that keep them in step with the game. Its keyboard route is still
    // refused — see the module docs for the `riftflow-native` seam.
    let in_game_send = in_game_send_main::register(
        &ipc,
        &state_sync,
        &settings,
        &league_client,
        &in_game_send_main::ongoing_summoners(&ongoing_game),
    )?;
    in_game_send_main::start(&in_game_send, &settings_hub)?;

    protocol::register_ipc(&ipc)?;

    let riot_client_service = Arc::new(riot_client::RiotClientService::new());
    riot_client::register(&ipc, &league_client, &riot_client_service)?;

    // The remote-config client: builtin seeds are published immediately, then the
    // bootstrap document and the cached/newer documents replace them. Built before
    // `launchers` because `self-update-main` answers `checkUpdates` from it
    let riftflow_api = riftflow_api_main::build(
        db_path.parent().unwrap_or(std::path::Path::new(".")),
        env!("CARGO_PKG_VERSION"),
        &state_sync,
    );

    launchers::register(
        &ipc,
        &state_sync,
        &league_client,
        &riftflow_api,
        env!("CARGO_PKG_VERSION"),
        db_path.parent().unwrap_or(std::path::Path::new(".")),
    )?;

    // `startUpdate` / `cancelUpdate` / `uninstallApp` and the staged updater run
    self_update::register(
        &ipc,
        &state_sync,
        &settings,
        db_path.parent().unwrap_or(std::path::Path::new(".")),
        &std::env::temp_dir(),
    )?;
    self_update::start(
        &state_sync,
        &settings,
        &riftflow_api,
        db_path.parent().unwrap_or(std::path::Path::new(".")),
        env!("CARGO_PKG_VERSION"),
    );

    riftflow_api_main::start(
        riftflow_api.clone(),
        settings.clone(),
        settings_hub.clone(),
        state_sync.clone(),
    );

    // One `window-manager-main/<suffix>` sub-namespace per riftflow window; the base
    // handlers exist before the window does, and answer "window is not open"
    // until `attach()` runs.
    let window_manager = Arc::new(windows::WindowManager::default());
    windows::register_window(&ipc, &state_sync, &window_manager, &settings, windows::MAIN_WINDOW)?;
    aux_windows::register(&ipc, &state_sync, &window_manager, &settings)?;

    let state = AppState {
        ipc: ipc.clone(),
        transport: transport.clone(),
        next_renderer_id: AtomicU64::new(0),
        league_client: league_client.clone(),
        ux: ux.clone(),
        window_manager: window_manager.clone(),
        state_sync: state_sync.clone(),
        settings: settings.clone(),
        sgp_gateway: sgp_gateway.clone(),
        riot_client: riot_client_service.clone(),
        riftflow_api: riftflow_api.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let app = tauri::Builder::default()
        // the renderer's `riftflow://` proxy: LCU HTTP, local files, remote APIs
        .register_asynchronous_uri_scheme_protocol("riftflow", protocol::handle)
        .invoke_handler(tauri::generate_handler![
            riftflow_call,
            riftflow_renderer_register,
            riftflow_protocol_fetch,
            shell_smoke,
            shell_report,
            riftflow_key_event
        ])
        .manage(state)
        .setup(move |app| {
            let handle = app.handle().clone();
            create_window(&handle, windows::MAIN_WINDOW, "main-window.html")?;

            // `RIFTFLOW_LAB=1` opens the rewrite baseline page next to the shipped UI, so
            // the two can be compared in one session and any regression is one env var
            // away from the reference.
            if std::env::var("RIFTFLOW_LAB").is_ok() {
                let lab = create_window(&handle, "lab-window", "lab.html")?;
                let _ = lab.show();
                let _ = lab.set_focus();
            }

            aux_windows::create_enabled(&handle)?;
            aux_windows::start(&handle)?;
            tray::start(&handle)?;

            // mirror LeagueClientUxMain._watchExistingUx(): poll for credentials,
            // and connect automatically once a client shows up
            let app_state = handle.state::<AppState>();
            let ux_for_loop = app_state.ux.clone();
            let client_for_loop = app_state.league_client.clone();
            let manager_for_loop = app_state.window_manager.clone();
            let settings_for_loop = app_state.settings.clone();
            let sync_for_loop = app_state.state_sync.clone();
            let tray_for_loop = handle.clone();
            let handle_for_loop = handle.clone();

            tauri::async_runtime::spawn(async move {
                let mut hydrated = false;
                let mut registry_dumped = false;

                loop {
                    // the settings debounce only fires when someone drains it, so
                    // a write would otherwise never reach SQLite
                    let _ = settings_for_loop.flush_all_due(std::time::Instant::now());
                    manager_for_loop.flush_pending_bounds(&settings_for_loop);

                    if !registry_dumped {
                        if let Some(path) = std::env::var_os("RIFTFLOW_DUMP_IPC_REGISTRY") {
                            registry_dumped = dump_ipc_registry(&handle_for_loop, &path);
                        }
                    }
                    // the tray's enabled flags are `[settings.enabled, state.ready]`;
                    // this poll is the only place both are guaranteed to have settled
                    tray::refresh_if_changed(&tray_for_loop);

                    let clients = ux_for_loop.update();
                    client_for_loop.set_launched_clients(clients.clone());

                    let auto_connect = std::env::var("RIFTFLOW_AUTO_CONNECT").as_deref() != Ok("0")
                        && settings_for_loop.get_setting("league-client-main", "autoConnect")
                            .ok().and_then(|v| v.as_bool()).unwrap_or(true);
                    if auto_connect && !clients.is_empty() {
                        // one client → connect, like the autoConnect reaction
                        if let Some(first) = clients.first() {
                            if client_for_loop.state.connection_state()
                                == riftflow_lcu::ConnectionState::Disconnected
                            {
                                let auth = UxCommandLine {
                                    port: first.port,
                                    pid: first.pid,
                                    auth_token: first.auth_token.clone(),
                                    rso_platform_id: first.rso_platform_id.clone(),
                                    region: first.region.clone(),
                                    certificate: String::new(),
                                    riot_client_port: first.riot_client_port,
                                    riot_client_auth_token: first.riot_client_auth_token.clone(),
                                };
                                eprintln!(
                                    "[shell] connecting to pid={} port={}",
                                    auth.pid, auth.port
                                );
                                client_for_loop.connect(auth, false);
                            }
                        }
                    }

                    // `_stateInitializer` runs once per connection; a disconnect
                    // clears the sub-states like `_onLcuNotConnected` does
                    let connected = client_for_loop.state.connection_state()
                        == riftflow_lcu::ConnectionState::Connected;
                    if connected && !hydrated {
                        hydrated = true;
                        let sync = sync_for_loop.clone();
                        let client = client_for_loop.clone();
                        tauri::async_runtime::spawn(async move {
                            lc_state::hydrate(&sync, &client).await;
                        });
                    } else if !connected && hydrated {
                        hydrated = false;
                        lc_state::clear(&sync_for_loop);
                    }

                    // CLIENT_CMD_DEFAULT_POLL_INTERVAL
                    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())?;

    app.run(|handle, event| {
        // the original flushes every debounced settings task before the DataSource
        // is destroyed; quitting must not drop the last second of writes
        if matches!(event, tauri::RunEvent::Exit) {
            let _ = handle.state::<AppState>().settings.flush_all();
            // `before-quit`'s job in the original: run the staged updater, which
            // replaces this exe while it is gone from disk
            self_update::spawn_pending_on_quit(handle);
        }

        if let tauri::RunEvent::WindowEvent { label, event, .. } = event {
            let state = handle.state::<AppState>();
            match event {
                tauri::WindowEvent::Focused(true) => {
                    windows::publish_window_event(&state.state_sync, &state.window_manager, &label, "focus");
                }
                tauri::WindowEvent::Focused(false) => {
                    windows::publish_window_event(&state.state_sync, &state.window_manager, &label, "blur");
                }
                tauri::WindowEvent::Resized(_) => {
                    windows::publish_window_event(&state.state_sync, &state.window_manager, &label, "resized");
                }
                tauri::WindowEvent::Moved(_) => {
                    windows::publish_window_event(&state.state_sync, &state.window_manager, &label, "moved");
                }
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    // `BaseRiftFlowWindow.handleClose`: the close button hides the
                    // window unless the user chose 'quit' or the app is quitting
                    let may_close = windows::handle_close(
                        &state.ipc,
                        &state.state_sync,
                        &state.settings,
                        &state.window_manager,
                        &label,
                    );
                    if !may_close {
                        api.prevent_close();
                    } else if label == windows::MAIN_WINDOW {
                        // Hidden auxiliary webviews keep Tauri alive after the main
                        // window closes. Quit the entire app on the explicit quit path.
                        api.prevent_close();
                        handle.exit(0);
                    }
                }
                tauri::WindowEvent::Destroyed => {
                    state.transport.detach(&label);
                    windows::publish_window_event(
                        &state.state_sync,
                        &state.window_manager,
                        &label,
                        "closed",
                    );
                }
                _ => {}
            }
        }
    });

    Ok(())
}

/// Every renderer window gets the shim, with its own `riftflowWindowType`.
///
/// Windows are created hidden and shown on `ready`, matching Electron's
/// `show: false` + `ready-to-show` → `showOrRestore()` sequence, so no white
/// frame flashes before the renderer paints.
fn create_window(app: &AppHandle, suffix: &str, entry: &str) -> tauri::Result<WebviewWindow> {
    let script = shim::initialization_script(suffix);
    let spec = windows::window_spec(suffix);

    let state = app.state::<AppState>();
    let sync_for_load = state.state_sync.clone();
    let settings_for_load = state.settings.clone();
    let suffix_for_load = suffix.to_string();

    let builder = WebviewWindowBuilder::new(app, suffix, tauri::WebviewUrl::App(entry.into()))
        .title(spec.title)
        .initialization_script(&script)
        // Electron creates every riftflow window with `show: false`
        .visible(false)
        .decorations(!spec.frameless)
        .resizable(spec.resizable)
        .maximizable(spec.maximizable)
        .minimizable(spec.minimizable)
        .focusable(spec.focusable)
        .skip_taskbar(spec.skip_taskbar)
        .always_on_top(spec.always_on_top)
        .transparent(spec.transparent)
        .shadow(!spec.transparent)
        .inner_size(spec.base_width, spec.base_height)
        .min_inner_size(spec.min_width, spec.min_height)
        // `toggleDevtools` is a real IPC method the settings page calls, and in
        // Electron it works in packaged builds too. WebView2 gates it on
        // `AreDevToolsEnabled`, which wry takes from this attribute — and the default
        // is `false` in release, which would leave the button calling
        // `OpenDevToolsWindow` on a disabled webview. The same switch also turns on
        // F12 / Ctrl+Shift+J; the original blocks the harmful half of those in
        // `ShortcutSelector.preventFn`, so the accelerators are the cheaper trade.
        .devtools(true)
        .on_page_load(move |window, payload| {
            if !matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                return;
            }
            windows::on_window_ready(&sync_for_load, &settings_for_load, &window, &suffix_for_load);
        });

    let window = builder.build()?;
    let label = window.label().to_string();

    let id = state.next_renderer_id.fetch_add(1, Ordering::SeqCst) + 1;
    state.transport.attach(&label, id, &window);
    state.window_manager.attach(&label, &window);

    if spec.transparent {
        // `backgroundColor: '#00000000'`
        let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    }

    Ok(window)
}
