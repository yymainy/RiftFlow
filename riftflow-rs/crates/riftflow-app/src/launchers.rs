//! Shell/launcher shards the renderer still calls, ported as plain IPC
//! handlers:
//!
//! * `client-installation-main` — the four launch methods from
//!   `src/main/shards/client-installation/` (launcher + installation detector).
//!   The original detects paths once at boot and the launchers only read the
//!   cached state; here detection is lazy (first launch call needs it) and its
//!   result is pushed through the `client-installation-main:state` surface that
//!   `state_bootstrap` already declares.
//! * `game-client-main` — terminate + the PersistedSettings.json read-only
//!   toggle from `src/main/shards/game-client/`.
//! * `riot-client-main` — delegated to `crate::riot_client`.
//! * `self-update-main` — the six methods from `src/main/shards/self-update/`.
//!   The download/apply flow depends on the Electron build's bundled
//!   `riftflow-updater.exe` resource and on the RiftFlow API release feed; neither
//!   exists in this port, so those paths answer with explicit failures instead
//!   of pretending (see each handler).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use riftflow_ipc::{handler, RiftFlowIpcMain, IpcError, IpcResult};
use riftflow_lcu::LeagueClientMain;
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;
use serde_json::{Value, json};

pub const CLIENT_INSTALLATION_NAMESPACE: &str = "client-installation-main";
pub const GAME_CLIENT_NAMESPACE: &str = "game-client-main";
pub const SELF_UPDATE_NAMESPACE: &str = "self-update-main";

/// `client-installation/context.ts` constants — the registry layout WeGame and
/// the Tencent client write on install.
const TENCENT_REG_INSTALL_PATH: &str = r"HKCU\Software\Tencent\LOL";
const TENCENT_REG_INSTALL_VALUE: &str = "InstallPath";
const TENCENT_INSTALL_DIRNAME: &str = "WeGameApps";
const TENCENT_LOL_DIRNAME: &str = "英雄联盟";
const WEGAME_DEFAULTICON_PATH: &str = r"HKCU\wegame\DefaultIcon";

/// `self-update/context.ts` — `DOWNLOAD_DIR_NAME`.
const DOWNLOAD_DIR_NAME: &str = "NewUpdates";

/// What the self-update handlers answer when a step cannot exist in this port.
const SELF_UPDATE_MISSING_REASON: &str =
    "self-update needs the RiftFlow API release feed and the bundled riftflow-updater.exe, neither of which exists in the Tauri build";

/// The paths `ClientInstallationDetector` collects; every launcher handler
/// reads exactly the field the JS `state` exposed.
#[derive(Clone, Default)]
pub struct Detection {
    league_client_executable_paths: Vec<String>,
    tencent_installation_path: Option<String>,
    we_game_executable_path: Option<String>,
    official_riot_client_executable_path: Option<String>,
    tcls_executable_path: Option<String>,
    we_game_launcher_executable_path: Option<String>,
}

/// One-shot detection cache; `None` until the first launch call (or the
/// background poll at registration) completes.
type DetectionCache = Arc<Mutex<Option<Arc<Detection>>>>;

pub fn register(
    ipc: &Arc<RiftFlowIpcMain>,
    state_sync: &Arc<StateSync>,
    league_client: &Arc<LeagueClientMain>,
    riftflow_api: &Arc<riftflow_api::state::RiftFlowApiRuntime>,
    version: &str,
    user_data_dir: &Path,
) -> anyhow::Result<()> {
    let cache: DetectionCache = Arc::new(Mutex::new(None));

    // `ClientInstallationMain.onInit` runs detection without awaiting it; the
    // state props appear whenever the scan finishes
    {
        let cache = cache.clone();
        let state_sync = state_sync.clone();
        tauri::async_runtime::spawn(async move {
            let detection = tokio::task::spawn_blocking(detect).await.unwrap_or_default();
            publish_detection(&state_sync, &detection);
            *cache.lock().unwrap() = Some(Arc::new(detection));
        });
    }

    register_client_installation(ipc, cache)?;
    register_game_client(ipc, league_client)?;
    register_self_update(ipc, state_sync, riftflow_api, version, user_data_dir)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// client-installation-main
// ---------------------------------------------------------------------------

fn register_client_installation(ipc: &Arc<RiftFlowIpcMain>, cache: DetectionCache) -> anyhow::Result<()> {
    for (method, field) in [
        ("launchTencentTcls", Field::Tcls),
        ("launchWeGame", Field::WeGame),
        ("launchWeGameLeagueOfLegends", Field::WeGameLauncher),
        ("launchDefaultRiotClient", Field::RiotClient),
    ] {
        let cache = cache.clone();
        ipc.on_call(
            CLIENT_INSTALLATION_NAMESPACE,
            method,
            handler(move |_ctx, _args| {
                let cache = cache.clone();
                async move {
                    let detection = ensure_detection(cache).await;
                    match field.launch(&detection).await {
                        LaunchOutcome::Launched => Ok(Value::Null),
                        // the original resolves with `undefined` when nothing
                        // was detected (or the platform is skipped)
                        LaunchOutcome::Skipped => Ok(Value::Null),
                        LaunchOutcome::Failed(message) => Err(IpcError::plain(message)),
                    }
                }
            }),
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Field {
    Tcls,
    WeGame,
    WeGameLauncher,
    RiotClient,
}

enum LaunchOutcome {
    Launched,
    Skipped,
    Failed(String),
}

impl Field {
    /// `ClientInstallationLauncher` — the three Windows-only launches read the
    /// registry-detected path and spawn it detached; a missing path is a silent
    /// no-op, only a spawn failure rejects.
    async fn launch(self, detection: &Detection) -> LaunchOutcome {
        match self {
            Field::Tcls => windows_only_launch("TCLS client", detection.tcls_executable_path.as_deref()),
            Field::WeGame => windows_only_launch("WeGame client", detection.we_game_executable_path.as_deref()),
            Field::WeGameLauncher => windows_only_launch(
                "WeGame (LoL) client",
                detection.we_game_launcher_executable_path.as_deref(),
            ),
            Field::RiotClient => launch_default_riot_client(detection).await,
        }
    }
}

fn windows_only_launch(label: &str, executable: Option<&str>) -> LaunchOutcome {
    if !cfg!(windows) {
        // `shouldAllowWindowsOnlyLaunch()` → log + resolve undefined
        return LaunchOutcome::Skipped;
    }
    let Some(executable) = executable else {
        return LaunchOutcome::Skipped;
    };
    match spawn_detached(executable, &[]) {
        Ok(()) => LaunchOutcome::Launched,
        Err(error) => LaunchOutcome::Failed(format!("Failed to launch {label}: {error}")),
    }
}

/// `launchDefaultRiotClient` — same product/patchline arguments the Electron
/// build passes on every platform; win32 spawns detached, macOS goes through
/// `open -a`, everything else execs directly (awaiting, like `execFileAsync`).
async fn launch_default_riot_client(detection: &Detection) -> LaunchOutcome {
    let Some(executable) = detection.official_riot_client_executable_path.as_deref() else {
        return LaunchOutcome::Skipped;
    };
    let args = ["--launch-product=league_of_legends", "--launch-patchline=live"];

    if cfg!(windows) {
        return match spawn_detached(executable, &args) {
            Ok(()) => LaunchOutcome::Launched,
            Err(error) => LaunchOutcome::Failed(format!("Failed to launch Riot client: {error}")),
        };
    }

    let result: Result<bool, std::io::Error> =
        if cfg!(target_os = "macos") && executable.ends_with(".app") {
            std::process::Command::new("open")
                .args(["-a", executable, "--args"])
                .args(args)
                .stdin(Stdio::null())
                .output()
                .map(|output| output.status.success())
        } else {
            // the original awaited `execFileAsync`, so this path waits too
            std::process::Command::new(executable)
                .args(args)
                .stdin(Stdio::null())
                .spawn()
                .and_then(|mut child| child.wait().map(|status| status.success()))
        };

    match result {
        Ok(true) => LaunchOutcome::Launched,
        Ok(false) => LaunchOutcome::Failed(format!("Failed to launch Riot client: {executable}")),
        Err(error) => LaunchOutcome::Failed(format!("Failed to launch Riot client: {error}")),
    }
}

/// `cp.spawn(exe, {detached, stdio: 'ignore'})` — the quoted shell invocation
/// the original needed for spaces in paths is handled by `Command` itself;
/// `CREATE_NO_WINDOW` keeps a console from flashing behind the launcher.
fn spawn_detached(executable: &str, args: &[&str]) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        std::process::Command::new(executable)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_child| ()) // dropped without waiting: detached, like unref()
            .map_err(|error| error.to_string())
    }

    #[cfg(not(windows))]
    {
        let _ = (executable, args);
        Err("detached spawning is only implemented on Windows".to_string())
    }
}

/// Waits for the background detection when it has not finished yet; a launch
/// call never races a half-populated state.
async fn ensure_detection(cache: DetectionCache) -> Arc<Detection> {
    if let Some(cached) = cache.lock().unwrap().as_ref() {
        return cached.clone();
    }

    let detection = tokio::task::spawn_blocking(detect)
        .await
        .unwrap_or_default();
    let detection = Arc::new(detection);
    *cache.lock().unwrap() = Some(detection.clone());
    detection
}

/// `ClientInstallationDetector.runPlatformDetection` — registry first, then the
/// drive scan fills whatever the registry did not, then the Riot install json.
fn detect() -> Detection {
    #[cfg(windows)]
    {
        let mut detection = Detection::default();
        detect_tencent_by_registry(&mut detection);
        if detection.tencent_installation_path.is_none() {
            detect_tencent_by_drives(&mut detection);
        }
        detect_riot_installs_json(&mut detection);
        detection
    }

    #[cfg(not(windows))]
    {
        detect_mac_candidates()
    }
}

/// `updateTencentPathsByReg` — `HKCU\Software\Tencent\LOL\InstallPath` plus the
/// executables under it, and `HKCU\wegame\DefaultIcon` for the WeGame client.
#[cfg(windows)]
fn detect_tencent_by_registry(detection: &mut Detection) {
    let tencent_key = reg_export(TENCENT_REG_INSTALL_PATH);

    let mut tencent_usable = false;
    if let Some(content) = tencent_key.as_deref() {
        if let Some(path) = parse_reg_value(content, TENCENT_REG_INSTALL_VALUE) {
            let path = path.trim();
            if Path::new(path).exists() {
                detection.tencent_installation_path = Some(path.to_string());
                tencent_usable = true;
            }
        }
    }

    // The original bails out of the whole registry pass (including the WeGame
    // key) when the LOL key exists but its value is unusable — kept, because
    // the drive scan below is what rescues that case anyway.
    if let Some(install) = detection.tencent_installation_path.as_deref() {
        let tcls = Path::new(install).join("Launcher").join("Client.exe");
        if tcls.is_file() {
            detection.tcls_executable_path = Some(tcls.display().to_string());
        }
        let launcher = Path::new(install)
            .join("WeGameLauncher")
            .join("launcher.exe");
        if launcher.is_file() {
            detection.we_game_launcher_executable_path = Some(launcher.display().to_string());
        }
    }

    if tencent_key.is_some() && !tencent_usable {
        return;
    }

    if let Some(content) = reg_export(WEGAME_DEFAULTICON_PATH) {
        // `item2.values[''].value` — the key's default value, shaped like
        // `"C:\...\wegame.exe",0`; only the first quoted run is the exe path
        if let Some(default_icon) = parse_reg_value(&content, "") {
            if let Some(path) = first_quoted(&default_icon) {
                if Path::new(&path).exists() {
                    detection.we_game_executable_path = Some(path);
                }
            }
        }
    }
}

/// `updateTencentPathsByFile` — scan fixed drives for `<drive>\WeGameApps\英雄联盟`.
#[cfg(windows)]
fn detect_tencent_by_drives(detection: &mut Detection) {
    for drive in logical_drives() {
        let installation = Path::new(&drive)
            .join(TENCENT_INSTALL_DIRNAME)
            .join(TENCENT_LOL_DIRNAME);
        if !installation.is_dir() {
            continue;
        }

        detection.tencent_installation_path = Some(installation.display().to_string());

        let tcls = installation.join("Launcher").join("Client.exe");
        if tcls.is_file() {
            detection.tcls_executable_path = Some(tcls.display().to_string());
        }
        let launcher = installation.join("WeGameLauncher").join("launcher.exe");
        if launcher.is_file() {
            detection.we_game_launcher_executable_path = Some(launcher.display().to_string());
        }

        if detection.tcls_executable_path.is_some()
            && detection.we_game_launcher_executable_path.is_some()
        {
            return;
        }
    }
}

/// `updateLeagueClientInstallationByFile` — `%ProgramData%\Riot Games\RiotClientInstalls.json`.
#[cfg(windows)]
fn detect_riot_installs_json(detection: &mut Detection) {
    let Some(program_data) = std::env::var_os("ProgramData") else {
        return;
    };
    let path = Path::new(&program_data)
        .join("Riot Games")
        .join("RiotClientInstalls.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return;
    };

    let Some(associated) = json.get("associated_client").and_then(Value::as_object) else {
        return;
    };

    // keys are LeagueClient install dirs, values the RiotClient executable
    for (install, riot_client) in associated {
        let client_exe = Path::new(install).join("LeagueClient.exe");
        if client_exe.is_file() {
            detection
                .league_client_executable_paths
                .push(client_exe.display().to_string());
        }

        // `_maybeOfficialRiotClient`: "Riot Games" in the path, no 英雄联盟
        let text = riot_client.as_str().unwrap_or_default();
        if text.contains("Riot Games") && !text.contains("英雄联盟") && Path::new(text).exists() {
            detection.official_riot_client_executable_path = Some(text.to_string());
            break;
        }
    }
}

/// `updateMacInstallationsByFile` — the fixed candidate list from the original.
#[cfg(not(windows))]
fn detect_mac_candidates() -> Detection {
    let mut detection = Detection::default();

    for candidate in [
        "/Applications/Riot Client.app",
        "/Users/Shared/Riot Games/Riot Client/Riot Client.app",
        "/Applications/Riot Client.app/Contents/MacOS/Riot Client",
        "/Users/Shared/Riot Games/Riot Client/Riot Client.app/Contents/MacOS/Riot Client",
    ] {
        if Path::new(candidate).exists() {
            detection.official_riot_client_executable_path = Some(candidate.to_string());
            break;
        }
    }

    for candidate in [
        "/Applications/League of Legends.app/Contents/LoL/LeagueClient.app/Contents/MacOS/LeagueClient",
        "/Applications/League of Legends.app/Contents/MacOS/LeagueofLegends",
        "/Users/Shared/Riot Games/League of Legends.app/Contents/LoL/LeagueClient.app/Contents/MacOS/LeagueClient",
        "/Users/Shared/Riot Games/League of Legends.app/Contents/MacOS/LeagueofLegends",
    ] {
        if Path::new(candidate).exists() {
            detection
                .league_client_executable_paths
                .push(candidate.to_string());
        }
    }

    detection
}

/// `tools.getDrives` equivalent — the original shells out to the same
/// PowerShell `Get-CimInstance` line. Device IDs are ASCII, so the OEM code
/// page cannot garble what we parse.
#[cfg(windows)]
fn logical_drives() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    let script = "Get-CimInstance -ClassName Win32_LogicalDisk | \
                  Where-Object {$_.DriveType -eq 3} | \
                  Select-Object -ExpandProperty DeviceID";
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| {
                line.len() == 2
                    && line.as_bytes()[0].is_ascii_alphabetic()
                    && line.as_bytes()[1] == b':'
            })
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// `regedit.list()` replacement: `reg export` writes a UTF-16LE `.reg` file,
/// which sidesteps both the OEM console code page and the fact that the
/// Tencent paths contain 英雄联盟.
#[cfg(windows)]
fn reg_export(key: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let file = std::env::temp_dir().join(format!(
        "riftflow-reg-{}-{}.reg",
        std::process::id(),
        REG_EXPORT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));

    let output = std::process::Command::new("reg")
        .creation_flags(0x0800_0000)
        .args(["export", key, &file.display().to_string(), "/y"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None; // `reg export` fails when the key does not exist
    }

    let bytes = std::fs::read(&file).ok()?;
    let _ = std::fs::remove_file(&file);

    if bytes.starts_with(&[0xff, 0xfe]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&units).ok()
    } else {
        Some(String::from_utf8_lossy(&bytes).to_string())
    }
}

#[cfg(windows)]
static REG_EXPORT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Pull one value out of `.reg` file text. `"Name"="..."`, `@="..."` for the
/// default value, and `hex(2):..,..` (UTF-16LE) for non-ASCII strings, with the
/// `\` line continuations the format uses for long hex runs.
#[cfg(windows)]
fn parse_reg_value(content: &str, name: &str) -> Option<String> {
    let prefix = if name.is_empty() {
        "@=".to_string()
    } else {
        format!("\"{name}\"=")
    };

    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        let line = line.trim_start();
        if !line.starts_with(&prefix) {
            continue;
        }

        let mut joined = line[prefix.len()..].to_string();
        while joined.ends_with('\\') {
            joined.truncate(joined.len() - 1);
            joined.push_str(lines.next()?.trim());
        }

        let joined = joined.trim();
        if let Some(hex) = joined.strip_prefix("hex") {
            let (_, data) = hex.split_once(':')?;
            let bytes: Vec<u8> = data
                .split(',')
                .filter_map(|token| {
                    let token = token.trim();
                    if token.is_empty() || token.starts_with('(') {
                        return None;
                    }
                    u8::from_str_radix(token, 16).ok()
                })
                .collect();

            // hex(2) is REG_SZ stored as UTF-16LE; other types are best-effort
            let is_utf16 = hex.starts_with("(2)");
            return Some(if is_utf16 {
                let units: Vec<u16> = bytes
                    .chunks(2)
                    .filter_map(|pair| {
                        if pair.len() == 2 {
                            Some(u16::from_le_bytes([pair[0], pair[1]]))
                        } else {
                            None
                        }
                    })
                    .collect();
                let mut text = String::from_utf16_lossy(&units);
                while text.ends_with('\0') {
                    text.pop();
                }
                text
            } else {
                String::from_utf8_lossy(&bytes).to_string()
            });
        }

        if joined.starts_with('"') && joined.ends_with('"') && joined.len() >= 2 {
            let inner = &joined[1..joined.len() - 1];
            return Some(
                inner
                    .replace("\\\\", "\\")
                    .replace("\\\"", "\""),
            );
        }

        return None; // DWORD/None etc. — not a path we can use
    }

    None
}

/// The launcher path lives inside the icon string as the first `"..."` run,
/// exactly what the original's `/"([^"]+)"/` grabbed.
#[cfg(windows)]
fn first_quoted(text: &str) -> Option<String> {
    let after = text.find('"')? + 1;
    let rest = &text[after..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn publish_detection(state_sync: &Arc<StateSync>, detection: &Detection) {
    let optional = |value: &Option<String>| {
        value
            .as_ref()
            .map(|path| Value::String(path.clone()))
            .unwrap_or(Value::Null)
    };

    let _ = state_sync.set(
        CLIENT_INSTALLATION_NAMESPACE,
        "state",
        "tencentInstallationPath",
        optional(&detection.tencent_installation_path),
    );
    let _ = state_sync.set(
        CLIENT_INSTALLATION_NAMESPACE,
        "state",
        "weGameExecutablePath",
        optional(&detection.we_game_executable_path),
    );
    let _ = state_sync.set(
        CLIENT_INSTALLATION_NAMESPACE,
        "state",
        "officialRiotClientExecutablePath",
        optional(&detection.official_riot_client_executable_path),
    );
    let _ = state_sync.set(
        CLIENT_INSTALLATION_NAMESPACE,
        "state",
        "tclsExecutablePath",
        optional(&detection.tcls_executable_path),
    );
    let _ = state_sync.set(
        CLIENT_INSTALLATION_NAMESPACE,
        "state",
        "weGameLauncherExecutablePath",
        optional(&detection.we_game_launcher_executable_path),
    );
    let _ = state_sync.set(
        CLIENT_INSTALLATION_NAMESPACE,
        "state",
        "leagueClientExecutablePaths",
        json!(detection.league_client_executable_paths),
    );
}

// ---------------------------------------------------------------------------
// game-client-main
// ---------------------------------------------------------------------------

fn register_game_client(
    ipc: &Arc<RiftFlowIpcMain>,
    league_client: &Arc<LeagueClientMain>,
) -> anyhow::Result<()> {
    ipc.on_call(
        GAME_CLIENT_NAMESPACE,
        "terminateGameClient",
        handler(|_ctx, _args| async move { Ok(terminate_game_client()) }),
    )?;

    let client = league_client.clone();
    ipc.on_call(
        GAME_CLIENT_NAMESPACE,
        "setSettingsFileReadonlyOrWritable",
        handler(move |_ctx, args| {
            let client = client.clone();
            async move {
                let mode = args
                    .get(0)
                    .and_then(Value::as_str)
                    .unwrap_or("readonly"); // `mode = 'readonly'` default
                let path = persisted_settings_path(&client).await?;

                // Node's `chmod 0o444/0o644` on Windows only toggles the
                // read-only attribute; `set_readonly` is the same effect.
                let mut permissions = std::fs::metadata(&path)
                    .map_err(|error| IpcError::plain(format!("{}: {error}", path.display())))?
                    .permissions();
                permissions.set_readonly(mode == "readonly");
                std::fs::set_permissions(&path, permissions)
                    .map_err(|error| IpcError::plain(format!("{}: {error}", path.display())))?;

                Ok(Value::Null)
            }
        }),
    )?;

    let client = league_client.clone();
    ipc.on_call(
        GAME_CLIENT_NAMESPACE,
        "getSettingsFileReadonlyOrWritable",
        handler(move |_ctx, _args| {
            let client = client.clone();
            async move {
                let path = persisted_settings_path(&client).await?;
                let readonly = std::fs::metadata(&path)
                    .map_err(|error| IpcError::plain(format!("{}: {error}", path.display())))?
                    .permissions()
                    .readonly();
                // `stats.mode & 0o222` → writable when the write bits exist
                Ok(json!(if readonly { "readonly" } else { "writable" }))
            }
        }),
    )?;

    Ok(())
}

/// `GameClientMain.terminateGameClient` — when the foreground check is
/// available (win32 + elevated, per `NATIVE_SUPPORT.isProcessForeground`) only
/// the foreground game process is killed; otherwise every matching pid dies.
fn terminate_game_client() -> Value {
    let foreground_gate = cfg!(windows) && riftflow_native::is_elevated();

    for pid in riftflow_native::pids_by_name(riftflow_native::GAME_CLIENT_PROCESS_NAME) {
        if foreground_gate && !riftflow_native::is_process_foreground(pid) {
            continue;
        }
        riftflow_native::terminate_process(pid);
    }

    Value::Null
}

/// `GameClientSettingsFileController._getConfigPathByLcuApi` + the settings
/// file. The TENCENT region resolves `<install>/../Game/Config` — the parent
/// hop mirrors Node's `join(dir, '..', …)`, same as
/// `riftflow_lcu::http::item_set_target_dir`.
async fn persisted_settings_path(league_client: &LeagueClientMain) -> Result<PathBuf, IpcError> {
    if league_client.state.auth_public().is_none() {
        return Err(IpcError::plain("LC Not connected"));
    }

    let http = league_client
        .http()
        .ok_or_else(|| IpcError::plain("LC Not connected"))?;

    let install = http
        .get("/data-store/v1/install-dir")
        .await
        .map_err(IpcError::plain)?;
    let install = install.as_str().unwrap_or_default().trim_matches('"').to_string();

    let base = Path::new(&install);
    let config_dir = if league_client.state.region() == "TENCENT" {
        base.parent().unwrap_or(base).join("Game").join("Config")
    } else {
        base.join("Config")
    };

    Ok(config_dir.join("PersistedSettings.json"))
}

// ---------------------------------------------------------------------------
// self-update-main
// ---------------------------------------------------------------------------

fn register_self_update(
    ipc: &Arc<RiftFlowIpcMain>,
    state_sync: &Arc<StateSync>,
    riftflow_api: &Arc<riftflow_api::state::RiftFlowApiRuntime>,
    version: &str,
    user_data_dir: &Path,
) -> anyhow::Result<()> {
    // `SelfUpdateController.checkLatestRelease()`: the flag transitions wrap the
    // fetch, and the answer is derived from the release document the riftflow-api
    // shard keeps, exactly as the original's computed `releaseInfo` does
    ipc.on_call(
        SELF_UPDATE_NAMESPACE,
        "checkUpdates",
        handler({
            let api = riftflow_api.clone();
            let sync = state_sync.clone();
            let version = version.to_string();
            move |_ctx, _args| {
                let api = api.clone();
                let sync = sync.clone();
                let version = version.clone();
                async move {
                    let _ = sync.set(SELF_UPDATE_NAMESPACE, "state", "isCheckingUpdates", json!(true));
                    let result = check_latest_release(&api, &sync, &version).await;
                    let _ = sync.set(SELF_UPDATE_NAMESPACE, "state", "isCheckingUpdates", json!(false));
                    result
                }
            }
        }),
    )?;

    let download_dir = user_data_dir.join(DOWNLOAD_DIR_NAME);
    ipc.on_call(
        SELF_UPDATE_NAMESPACE,
        "openNewUpdatesDir",
        handler(move |_ctx, _args| {
            let dir = download_dir.clone();
            async move {
                // `shell.openPath` answers '' when it worked and the OS error
                // string when the directory does not exist yet
                if dir.is_dir() && crate::file_io::reveal(&dir.to_string_lossy()) {
                    Ok(json!(""))
                } else {
                    Ok(json!("The system cannot find the file specified. (NewUpdates)"))
                }
            }
        }),
    )?;

    Ok(())
}

/// `isSupportedWin32X64Artifact`: only a win32/x64 7z archive can be applied.
fn is_supported_artifact(artifact: &Value) -> bool {
    super::portable_update::valid_artifact(artifact)
}

/// `resolveSelfUpdateReleaseInfo` from `self-update/release-info.ts`.
pub fn resolve_release_info(release: &Value, current_version: &str) -> Option<Value> {
    if release.is_null() {
        return None;
    }
    let version = release.get("version").and_then(Value::as_str)?;

    let artifact = release
        .get("artifacts")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find(|item| is_supported_artifact(item)))
        .cloned();

    Some(json!({
        "version": version,
        "currentVersion": current_version,
        "publishedAt": release.get("publishedAt").cloned().unwrap_or(Value::Null),
        "description": release.get("description").cloned().unwrap_or(Value::Null),
        "isNew": is_newer(version, current_version),
        "isUpdateSupported": artifact.is_some(),
        "artifact": artifact.unwrap_or(Value::Null),
    }))
}

/// `semver.gt(latest, current)`; an unparseable version is never newer.
fn is_newer(latest: &str, current: &str) -> bool {
    match (
        riftflow_api::semver::parse(latest),
        riftflow_api::semver::parse(current),
    ) {
        (Some(a), Some(b)) => riftflow_api::semver::gte(latest, current).unwrap_or(false) && a != b,
        _ => false,
    }
}

/// One release check: fetch, resolve, publish `releaseInfo`, answer.
pub(crate) async fn check_latest_release(
    api: &Arc<riftflow_api::state::RiftFlowApiRuntime>,
    state_sync: &Arc<StateSync>,
    version: &str,
) -> IpcResult<Value> {
    if !crate::self_update::lifecycle_supported() {
        return Ok(json!({ "result": "failed", "reason": "RiftFlow update source is not configured" }));
    }
    let language = riftflow_api_locale(state_sync);
    let outcome = api.refresh_latest_release(&language).await;

    let release = match outcome {
        Ok(release) => release,
        Err(error) => {
            return Ok(json!({ "result": "failed", "reason": error.to_string() }));
        }
    };

    let info = resolve_release_info(&release, version);
    let _ = state_sync.set(
        SELF_UPDATE_NAMESPACE,
        "state",
        "releaseInfo",
        info.clone().unwrap_or(Value::Null),
    );

    let new_and_supported = info.as_ref().and_then(|value| {
        Some(
            value.get("isNew").and_then(Value::as_bool)? && value.get("isUpdateSupported").and_then(Value::as_bool)?,
        )
    });

    Ok(match new_and_supported {
        Some(true) => json!({ "result": "new-updates" }),
        Some(false) => json!({ "result": "no-updates" }),
        None => json!({ "result": "failed", "reason": SELF_UPDATE_MISSING_REASON }),
    })
}

/// `SelfUpdateSettings.autoCheckUpdates`.
pub(crate) fn auto_check_enabled(settings: &Arc<SettingFactory>) -> bool {
    settings
        .get_setting(SELF_UPDATE_NAMESPACE, "autoCheckUpdates")
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

fn riftflow_api_locale(state_sync: &Arc<StateSync>) -> String {
    state_sync
        .get("app-common-main", "settings", "locale")
        .or_else(|| state_sync.get("app-common-main", "state", "locale"))
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "zh-CN".to_string())
}


#[cfg(windows)]
#[cfg(test)]
mod tests {
    use super::*;

    // Captured verbatim from `reg export` of a key holding a 英雄联盟 path,
    // an ASCII path and a DefaultIcon-style quoted default value (the file
    // itself is UTF-16LE; this is what `reg_export` hands back).
    const SAMPLE: &str = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Software\\RiftFlowPortTest]\r\n\"InstallPath\"=\"D:\\\\WeGameApps\\\\英雄联盟\"\r\n\"AsciiPath\"=\"D:\\\\WeGameApps\\\\lol\"\r\n@=\"\\\"C:\\\\Program Files (x86)\\\\WeGame\\\\wegame.exe\\\",0\"\r\n\r\n";

    #[test]
    fn reads_unicode_and_ascii_strings() {
        assert_eq!(
            parse_reg_value(SAMPLE, "InstallPath").as_deref(),
            Some(r"D:\WeGameApps\英雄联盟")
        );
        assert_eq!(
            parse_reg_value(SAMPLE, "AsciiPath").as_deref(),
            Some(r"D:\WeGameApps\lol")
        );
        assert_eq!(parse_reg_value(SAMPLE, "Missing"), None);
    }

    #[test]
    fn reads_the_default_value_and_extracts_the_icon_path() {
        let default = parse_reg_value(SAMPLE, "").unwrap();
        assert_eq!(default, r#""C:\Program Files (x86)\WeGame\wegame.exe",0"#);
        assert_eq!(
            first_quoted(&default).as_deref(),
            Some(r"C:\Program Files (x86)\WeGame\wegame.exe")
        );
    }

    #[test]
    fn decodes_hex2_continuations() {
        // older reg.exe builds emit non-ASCII REG_SZ as hex(2) runs
        let hex = "Windows Registry Editor Version 5.00\r\n\r\n[K]\r\n\
                   \"InstallPath\"=hex(2):44,00,3a,00,5c,00,57,00,\\\r\n5e,00,00,00\r\n";
        assert_eq!(parse_reg_value(hex, "InstallPath").as_deref(), Some(r"D:\W^"));
    }
}

#[cfg(test)]
mod self_update_tests {
    use super::*;

    fn artifact(platform: &str, arch: &str, content_type: &str, file_name: &str) -> Value {
        json!({ "platform": platform, "arch": arch, "contentType": content_type, "fileName": file_name, "size": 128, "sha256": "a".repeat(64), "downloadUrl":"https://files.example.invalid/RiftFlow.exe" })
    }

    #[test]
    fn only_a_verified_win32_x64_exe_can_be_applied() {
        assert!(is_supported_artifact(&artifact(
            "win32",
            "x64",
            "application/octet-stream",
            "RiftFlow.exe"
        )));
        assert!(is_supported_artifact(&artifact(
            "win32",
            "x64",
            "application/octet-stream",
            "RiftFlow.exe"
        )));
        assert!(!is_supported_artifact(&artifact(
            "win32",
            "arm64",
            "application/octet-stream",
            "RiftFlow.exe"
        )));
        assert!(!is_supported_artifact(&artifact(
            "darwin",
            "x64",
            "application/octet-stream",
            "RiftFlow.exe"
        )));
        assert!(!is_supported_artifact(&artifact(
            "win32",
            "x64",
            "application/octet-stream",
            "RiftFlow.zip"
        )));
    }

    #[test]
    fn the_release_info_answers_the_three_questions_the_ui_asks() {
        let release = json!({
            "version": "1.6.0",
            "publishedAt": "2026-10-20T00:00:00Z",
            "description": "notes",
            "artifacts": [artifact("win32", "x64", "application/octet-stream", "RiftFlow.exe")],
        });

        let info = resolve_release_info(&release, "1.5.1").expect("a release resolves");
        assert_eq!(info["isNew"], json!(true));
        assert_eq!(info["isUpdateSupported"], json!(true));
        assert_eq!(info["currentVersion"], json!("1.5.1"));
        assert_eq!(info["artifact"]["fileName"], json!("RiftFlow.exe"));

        let same = resolve_release_info(&release, "1.6.0").expect("still resolves");
        assert_eq!(same["isNew"], json!(false));

        let no_archive = json!({ "version": "9.9.9", "artifacts": [] });
        let unsupported = resolve_release_info(&no_archive, "1.5.1").expect("resolves");
        assert_eq!(unsupported["isUpdateSupported"], json!(false));
        assert_eq!(unsupported["isNew"], json!(true));

        assert!(resolve_release_info(&Value::Null, "1.5.1").is_none());
    }

    #[test]
    fn an_unparseable_version_is_never_newer() {
        assert!(!is_newer("not-a-version", "1.5.1"));
        assert!(!is_newer("1.6.0", "garbage"));
        assert!(is_newer("1.6.0", "1.5.1"));
        assert!(!is_newer("1.5.1", "1.5.1"));
    }
}
