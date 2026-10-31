//! `self-update-main`'s executor half: download the release archive, report
//! progress the way the original did, and run `riftflow-updater.exe` after this
//! process quits.
//!
//! Port of `src/main/shards/self-update/{update-executor.ts,updater-command.ts,
//! uninstaller.ts,context.ts}`. `checkUpdates` and the release comparison stay in
//! `launchers.rs`, next to the launcher detection that already had them.
//!
//! Two deliberate differences from the original, both because this is a Tauri shell
//! rather than an Electron one:
//! * the original hardcoded `RiftFlow.exe` as the executable the updater should
//!   relaunch; this passes the name of the binary that is actually running, which is
//!   correct for whichever exe ships.
//! * the "restart to finish" toast has no Tauri notification here, so the same
//!   message is logged instead of shown.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use riftflow_ipc::{RiftFlowIpcMain, IpcError, IpcResult, handler};
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;

pub const NAMESPACE: &str = "self-update-main";

const DOWNLOAD_DIR_NAME: &str = "NewUpdates";
const UPDATE_EXECUTABLE_NAME: &str = "riftflow-updater.exe";
const NEW_VERSION_FLAG: &str = "NEW_VERSION_FLAG";
const PLATFORM_UNSUPPORTED_REASON: &str = "platform-unsupported";
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

/// Automatic installation is disabled. A new updater must be built and validated
/// before enabling it; no legacy executable is embedded in this source tree.
const UPDATER_BYTES: &[u8] = &[];

const DEEP_LINK_PROTOCOLS: [&str; 2] = ["riftflow", "riftflow-dev"];

/// `createApplyUpdaterArguments`.
pub fn apply_updater_arguments(
    locale: &str,
    archive_path: &Path,
    target_path: &Path,
    executable_name: &str,
) -> Vec<String> {
    vec![
        "--lang".to_string(),
        locale.to_string(),
        "--executable".to_string(),
        executable_name.to_string(),
        "apply".to_string(),
        "--archive".to_string(),
        archive_path.to_string_lossy().to_string(),
        "--target".to_string(),
        target_path.to_string_lossy().to_string(),
        "--delete-archive".to_string(),
        "--launch".to_string(),
    ]
}

/// `createUninstallUpdaterArguments`.
pub fn uninstall_updater_arguments(
    locale: &str,
    app_ids: &[&str],
    app_path: &Path,
    data_path: &Path,
    executable_name: &str,
) -> Vec<String> {
    let mut args = vec![
        "--lang".to_string(),
        locale.to_string(),
        "--executable".to_string(),
        executable_name.to_string(),
        "uninstall".to_string(),
    ];
    for app_id in app_ids {
        args.push("--app-id".to_string());
        args.push(app_id.to_string());
    }
    args.push("--dirs-to-remove".to_string());
    args.push(app_path.to_string_lossy().to_string());
    args.push("--dirs-to-remove".to_string());
    args.push(data_path.to_string_lossy().to_string());
    args
}

/// `shouldRunSelfUpdateLifecycle()` — the original's guard is platform and arch,
/// not elevation or packaging.
pub fn lifecycle_supported() -> bool {
    cfg!(all(target_os = "windows", target_arch = "x86_64"))
}

struct PendingUpdate {
    updater_path: PathBuf,
    archive_path: PathBuf,
    new_version: String,
    args: Vec<String>,
}

static PENDING: OnceLock<Mutex<Option<PendingUpdate>>> = OnceLock::new();
static CANCEL: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static USER_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static TEMP_DIR: OnceLock<PathBuf> = OnceLock::new();

fn pending() -> &'static Mutex<Option<PendingUpdate>> {
    PENDING.get_or_init(|| Mutex::new(None))
}

fn cancel_flag() -> Arc<AtomicBool> {
    CANCEL
        .get_or_init(|| Arc::new(AtomicBool::new(false)))
        .clone()
}

fn progress(state_sync: &Arc<StateSync>, value: Value) {
    let _ = state_sync.set(NAMESPACE, "state", "updateProgressInfo", value);
}

fn setting(settings: &SettingFactory, key: &str) -> Value {
    settings.get_setting(NAMESPACE, key).unwrap_or(Value::Null)
}

fn locale(settings: &SettingFactory) -> String {
    settings
        .get_setting("app-common-main", "locale")
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "zh-CN".to_string())
}

fn release_info(state_sync: &Arc<StateSync>) -> Option<Value> {
    state_sync
        .get(NAMESPACE, "state", "releaseInfo")
        .filter(|value| !value.is_null())
}

pub fn register(
    ipc: &Arc<RiftFlowIpcMain>,
    state_sync: &Arc<StateSync>,
    settings: &Arc<SettingFactory>,
    user_data_dir: &Path,
    temp_dir: &Path,
) -> anyhow::Result<()> {
    let _ = USER_DATA_DIR.set(user_data_dir.to_path_buf());
    let _ = TEMP_DIR.set(temp_dir.to_path_buf());

    for (method, force) in [("startUpdate", false), ("forceStartUpdate", true)] {
        let sync = state_sync.clone();
        let factory = settings.clone();
        ipc.on_call(
            NAMESPACE,
            method,
            handler(move |_ctx, _args| {
                let sync = sync.clone();
                let factory = factory.clone();
                async move { start_update(&sync, &factory, force).await }
            }),
        )?;
    }

    let sync = state_sync.clone();
    ipc.on_call(
        NAMESPACE,
        "cancelUpdate",
        handler(move |_ctx, _args| {
            let sync = sync.clone();
            async move { cancel_update(&sync) }
        }),
    )?;

    let sync = state_sync.clone();
    let factory = settings.clone();
    ipc.on_call(
        NAMESPACE,
        "uninstallApp",
        handler(move |_ctx, _args| {
            let sync = sync.clone();
            let factory = factory.clone();
            async move { uninstall_app(&sync, &factory).await }
        }),
    )?;

    Ok(())
}

/// `SelfUpdateExecutor.start()`. The returned value is the original's
/// `SelfUpdateActionResult`.
async fn start_update(
    state_sync: &Arc<StateSync>,
    _settings: &Arc<SettingFactory>,
    _force: bool,
) -> IpcResult<Value> {
    if !lifecycle_supported() {
        eprintln!("[self-update] 当前平台不支持自更新");
        return Ok(json!({ "result": "failed", "reason": PLATFORM_UNSUPPORTED_REASON }));
    }

    if state_sync.get(NAMESPACE,"state","updateProgressInfo").is_some_and(|v|v["phase"]=="downloading" || v["phase"]=="waiting-for-restart"){return Ok(json!({"result":"no-op"}));}
    let Some(info) = release_info(state_sync) else {
        return Ok(json!({ "result": "no-op" }));
    };

    let supported = info.get("isUpdateSupported").and_then(Value::as_bool) == Some(true);
    let is_new = info.get("isNew").and_then(Value::as_bool) == Some(true);
    if !supported || !is_new {
        return Ok(json!({ "result": "no-op" }));
    }

    let result=super::portable_update::start(state_sync,&info,&user_data_dir().join(DOWNLOAD_DIR_NAME),&cancel_flag()).await;
    match result {
        Ok(value)=>Ok(value),
        Err(error)=>{
            progress(state_sync,if cancel_flag().load(Ordering::SeqCst){Value::Null}else{json!({"phase":"download-failed","downloadingProgress":0,"averageDownloadSpeed":0,"downloadTimeLeft":-1,"fileSize":0})});
            Ok(json!({"result":"failed","reason":error.to_string()}))
        }
    }
}

/// `_downloadUpdate`, including the 200 ms progress throttle the original used so
/// the UI is not flooded with prop pushes.
pub(super) async fn download(
    url: &str,
    dest: &Path,
    state_sync: &Arc<StateSync>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .https_only(!cfg!(test))
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(300))
        .user_agent(riftflow_api::client::user_agent(env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;

    let response=response.error_for_status().map_err(|error|error.to_string())?;
    let total = response
        .content_length()
        .and_then(|length| i64::try_from(length).ok())
        .unwrap_or(-1);

    progress(
        state_sync,
        json!({
            "phase": "downloading",
            "downloadingProgress": 0,
            "averageDownloadSpeed": 0,
            "downloadTimeLeft": -1,
            "fileSize": total,
        }),
    );

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|error| error.to_string())?;

    let started = std::time::Instant::now();
    let mut written: i64 = 0;
    let mut last_progress = started;
    let mut stream = response;

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("canceled".to_string());
        }
        let Some(chunk) = stream
            .chunk()
            .await
            .map_err(|error| error.to_string())?
        else {
            break;
        };
        if written.saturating_add(chunk.len() as i64)>200*1024*1024 {return Err("更新文件超过大小限制".into());}
        file.write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
        written += chunk.len() as i64;

        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            last_progress = std::time::Instant::now();
            let elapsed = started.elapsed().as_secs_f64().max(0.001);
            let speed = written as f64 / elapsed;
            let fraction = if total > 0 {
                (written as f64 / total as f64).min(1.0)
            } else {
                0.0
            };
            let time_left = if total > 0 && speed > 0.0 {
                ((total - written) as f64 / speed).max(0.0)
            } else {
                -1.0
            };
            progress(
                state_sync,
                json!({
                    "phase": "downloading",
                    "downloadingProgress": fraction,
                    "averageDownloadSpeed": speed,
                    "downloadTimeLeft": time_left,
                    "fileSize": total,
                }),
            );
        }
    }

    file.flush().await.map_err(|error| error.to_string())?;

    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    let speed = written as f64 / elapsed;
    progress(
        state_sync,
        json!({
            "phase": "downloading",
            "downloadingProgress": 1,
            "averageDownloadSpeed": speed,
            "downloadTimeLeft": 0,
            "fileSize": total,
        }),
    );
    Ok(())
}

/// `SelfUpdateExecutor.cancel()`.
fn cancel_update(state_sync: &Arc<StateSync>) -> IpcResult<Value> {
    cancel_flag().store(true, Ordering::SeqCst);
    super::portable_update::cancel();

    if let Some(pending) = pending().lock().unwrap().take() {
        let _ = std::fs::remove_file(&pending.updater_path);
        let _ = std::fs::remove_file(&pending.archive_path);
    }
    progress(state_sync, Value::Null);
    Ok(json!({ "result": "ok" }))
}

/// `SelfUpdateUninstaller.uninstallApp()`.
async fn uninstall_app(
    _state_sync: &Arc<StateSync>,
    settings: &Arc<SettingFactory>,
) -> IpcResult<Value> {
    if UPDATER_BYTES.is_empty() || !lifecycle_supported() {
        return Ok(json!({ "result": "failed", "reason": PLATFORM_UNSUPPORTED_REASON }));
    }

    let exe = std::env::current_exe()
        .map_err(|error| IpcError::plain(error.to_string()))?;
    let app_path = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
    let data_path = user_data_dir();
    let updater_path = temp_dir().join(UPDATE_EXECUTABLE_NAME);

    tokio::fs::write(&updater_path, UPDATER_BYTES)
        .await
        .map_err(|error| IpcError::plain(error.to_string()))?;

    let args = uninstall_updater_arguments(
        &locale(settings),
        &DEEP_LINK_PROTOCOLS,
        &app_path,
        &data_path,
        &exe.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
    );

    std::process::Command::new(&updater_path)
        .args(&args)
        .current_dir(temp_dir())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| IpcError::plain(error.to_string()))?;

    Ok(json!({ "result": "ok" }))
}

fn user_data_dir() -> PathBuf {
    USER_DATA_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| std::env::temp_dir())
}

fn temp_dir() -> PathBuf {
    TEMP_DIR.get().cloned().unwrap_or_else(std::env::temp_dir)
}

/// The quit hook: run the staged updater and leave the "new version" marker the
/// next launch reads. `spawn` rather than `spawn_wait` — the updater replaces this
/// exe while it is on disk, so it must outlive us.
pub fn spawn_pending_update() -> bool {
    // Both direct process exits and Tauri's RunEvent::Exit call this. The first
    // call consumes the pending update; later calls are no-ops.
    if super::portable_update::spawn_on_quit(&user_data_dir()) {
        return true;
    }
    let Some(pending) = pending().lock().unwrap().take() else {
        return false;
    };

    match std::process::Command::new(&pending.updater_path)
        .args(&pending.args)
        .current_dir(temp_dir())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            eprintln!(
                "[self-update] updater spawned: {} {:?}",
                pending.updater_path.display(),
                pending.args
            );
            let _ = child;
        }
        Err(error) => {
            eprintln!("[self-update] 无法启动更新程序: {error}");
            return false;
        }
    }

    let flag = user_data_dir().join(NEW_VERSION_FLAG);
    if let Err(error) = std::fs::write(&flag, format!("\"{}\"", pending.new_version)) {
        eprintln!("[self-update] 写 {NEW_VERSION_FLAG} 失败: {error}");
    }
    true
}

pub fn spawn_pending_on_quit(app: &tauri::AppHandle) {
    let _ = spawn_pending_update();
    let _ = app;
}

/// `UPDATE_CHECK_INTERVAL` — the original polls every four hours.
const UPDATE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(4 * 60 * 60);

/// `LastUpdateChecker`: the flag the updater wrote last time says which version we
/// were moving to; if this build is at least that, the update landed.
///
/// `None` means "no verdict" — the flag held something that is not a version.
pub fn last_update_outcome(current_version: &str, flagged_version: &str) -> Option<bool> {
    if !riftflow_api::semver::is_valid(flagged_version) {
        return None;
    }
    riftflow_api::semver::gte(current_version, flagged_version)
}

/// `LastUpdateChecker.check()`.
pub fn check_last_update(state_sync: &Arc<StateSync>, user_data_dir: &Path, current_version: &str) {
    if !lifecycle_supported() {
        return;
    }

    let flag_path = user_data_dir.join(NEW_VERSION_FLAG);
    let Ok(raw) = std::fs::read_to_string(&flag_path) else {
        return;
    };

    let flagged = raw.trim().trim_matches('"').to_string();
    match last_update_outcome(current_version, &flagged) {
        Some(succeeded) => {
            eprintln!(
                "[self-update] 上次自动更新{}（目标 {flagged}，当前 {current_version}）",
                if succeeded { "成功" } else { "失败" }
            );
            let _ = state_sync.set(
                NAMESPACE,
                "state",
                "lastUpdateSucceeded",
                json!(succeeded),
            );
        }
        None => eprintln!("[self-update] 更新标记不是正常版本号: {flagged}"),
    }

    if let Err(error) = std::fs::remove_file(&flag_path) {
        eprintln!("[self-update] 无法删除更新标记: {error}");
    }
}

/// `SelfUpdateController.watchLatestRelease()`: the immediate-and-every-four-hours
/// release poll, gated by the `autoCheckUpdates` setting.
pub fn start(
    state_sync: &Arc<StateSync>,
    settings: &Arc<SettingFactory>,
    riftflow_api: &Arc<riftflow_api::state::RiftFlowApiRuntime>,
    user_data_dir: &Path,
    version: &str,
) {
    if !lifecycle_supported() {
        eprintln!("[self-update] 自更新仅支持 Windows x64，已禁用");
        return;
    }

    check_last_update(state_sync, user_data_dir, version);

    let sync = state_sync.clone();
    let factory = settings.clone();
    let api = riftflow_api.clone();
    let version = version.to_string();
    tauri::async_runtime::spawn(async move {
        loop {
            let mut delay=UPDATE_CHECK_INTERVAL;
            if crate::launchers::auto_check_enabled(&factory) {
                let _=sync.set(NAMESPACE,"state","isCheckingUpdates",json!(true));
                let checked=crate::launchers::check_latest_release(&api, &sync, &version).await;
                let _=sync.set(NAMESPACE,"state","isCheckingUpdates",json!(false));
                if checked.as_ref().map_or(true,|v|v["result"]=="failed"){delay=std::time::Duration::from_secs(60);}
                if checked.ok().is_some_and(|v|v["result"]=="new-updates") && setting(&factory,"autoDownloadUpdates")==true {
                    if let Some(info)=release_info(&sync) {
                        if setting(&factory,"ignoreVersion")!=info["version"] { let _=start_update(&sync,&factory,false).await; }
                    }
                }
            }
            tokio::time::sleep(delay).await;
        }
    });
}

/// Whether the previous run left an update behind, for the next launch's notice.
pub fn new_version_flag(user_data_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(user_data_dir.join(NEW_VERSION_FLAG)).ok()?;
    let trimmed = raw.trim().trim_matches('"').to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub fn setting_value(settings: &Arc<SettingFactory>, key: &str) -> Value {
    setting(settings, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_update_flag_is_read_the_way_the_checker_does() {
        assert_eq!(last_update_outcome("1.5.1", "1.5.1"), Some(true));
        assert_eq!(last_update_outcome("1.6.0", "1.5.1"), Some(true));
        assert_eq!(last_update_outcome("1.5.1", "1.6.0"), Some(false));
        assert_eq!(last_update_outcome("1.5.1", "not-a-version"), None);
    }

    #[test]
    fn the_apply_command_line_matches_the_original() {
        let args = apply_updater_arguments(
            "zh-CN",
            Path::new(r"C:\Users\x\AppData\Roaming\NewUpdates\a.7z"),
            Path::new(r"C:\Program Files\RiftFlow"),
            "RiftFlow.exe",
        );
        assert_eq!(
            args,
            vec![
                "--lang",
                "zh-CN",
                "--executable",
                "RiftFlow.exe",
                "apply",
                "--archive",
                r"C:\Users\x\AppData\Roaming\NewUpdates\a.7z",
                "--target",
                r"C:\Program Files\RiftFlow",
                "--delete-archive",
                "--launch",
            ]
        );
    }

    /// Streams a canned HTTP response into a temp file and checks the bytes, the
    /// progress prop, and that cancellation is honoured.
    #[test]
    fn the_download_writes_what_the_server_sent() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let body = b"0123456789ab".to_vec();
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("local addr").port();
        let expected = body.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                let head = format!(
                    "HTTP/1.1 200 OK
Content-Length: {}

",
                    expected.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&expected);
                let _ = stream.flush();
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });

        let dir = std::env::temp_dir().join(format!("riftflow-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let dest = dir.join("payload.bin");
        let ipc = Arc::new(riftflow_ipc::RiftFlowIpcMain::new(Arc::new(Quiet)));
        let sync = Arc::new(StateSync::new(ipc));
        sync.prop_sync(NAMESPACE, "state", &[("updateProgressInfo", Value::Null)], false)
            .expect("the prop");
        let url = format!("http://127.0.0.1:{port}/update.7z");
        let cancel = AtomicBool::new(false);
        let result = tauri::async_runtime::block_on(download(&url, &dest, &sync, &cancel));
        assert!(result.is_ok(), "download failed: {:?}", result.err());
        assert_eq!(std::fs::read(&dest).expect("the file"), body);

        let progress = sync
            .get(NAMESPACE, "state", "updateProgressInfo")
            .expect("a progress prop");
        assert_eq!(progress["phase"], json!("downloading"));
        assert_eq!(progress["downloadingProgress"], json!(1));
        assert_eq!(progress["fileSize"], json!(12));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_cancelled_job_stops_the_stream() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("local addr").port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0u8; 1024];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"HTTP/1.1 200 OK
Content-Length: 1000000

");
                let _ = stream.write_all(&[7u8; 512]);
                let _ = stream.flush();
                std::thread::sleep(std::time::Duration::from_millis(600));
            }
        });

        let dir = std::env::temp_dir().join(format!("riftflow-dl-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let dest = dir.join("payload.bin");
        let ipc = Arc::new(riftflow_ipc::RiftFlowIpcMain::new(Arc::new(Quiet)));
        let sync = Arc::new(StateSync::new(ipc));
        sync.prop_sync(NAMESPACE, "state", &[("updateProgressInfo", Value::Null)], false)
            .expect("the prop");

        let url = format!("http://127.0.0.1:{port}/update.7z");
        let cancel = AtomicBool::new(true);
        let reason = tauri::async_runtime::block_on(download(&url, &dest, &sync, &cancel))
            .expect_err("a cancelled download must not succeed");
        assert_eq!(reason, "canceled");
        let _ = std::fs::remove_dir_all(dir);
    }

    struct Quiet;

    impl riftflow_ipc::RendererTransport for Quiet {
        fn send_to(
            &self,
            _renderer: riftflow_ipc::RendererId,
            _event: &str,
            _args: Vec<Value>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn every_app_id_becomes_its_own_flag_pair() {
        let args = uninstall_updater_arguments(
            "en",
            &DEEP_LINK_PROTOCOLS,
            Path::new("/apps"),
            Path::new("/data"),
            "RiftFlow.exe",
        );
        let text = args.join(" ");
        assert!(text.contains("--app-id riftflow --app-id riftflow-dev"));
        assert!(text.ends_with("--dirs-to-remove /apps --dirs-to-remove /data"));
        assert!(text.starts_with("--lang en --executable RiftFlow.exe uninstall"));
    }
}
