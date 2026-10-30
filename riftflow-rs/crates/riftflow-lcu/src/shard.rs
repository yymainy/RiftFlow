//! Port of the two shards that own the client connection:
//! `src/main/shards/league-client/index.ts` and
//! `src/main/shards/league-client-ux/index.ts`.
//!
//! Kept behavior, with the reasons it matters:
//! * the reconnect loop retries **only** on connection-refused; any other failure
//!   emits `error-connecting` and stops, so a broken credential does not spin.
//! * `_shouldHaveOneAttempt` lets a manual/forced connect try once even when the
//!   UX process is gone (the "UX closed but LeagueClient still running" case).
//! * renderer LCU subscriptions return incrementing `__<n>` ids and forward
//!   `extra-lcu-event` with `[subId, data, params]`.
//! * `lastConnectedClient` is persisted on success so a restart can resume.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use async_trait::async_trait;
use serde_json::{json, Value};

use riftflow_ipc::{handler, RiftFlowIpcMain, IpcError, IpcResult};
use riftflow_radix::{RadixEventEmitter, Subscription};
use riftflow_shard::RiftFlowShard;
use riftflow_state::StateSync;

use crate::discovery::{CommandLineReader, ProcessProbe};
use crate::http::{item_set_target_dir, LcuHttp, RequestConfig};
use crate::state::{
    ConnectionState, LeagueClientSettings, LeagueClientState, LeagueClientUxSettings,
    LeagueClientUxState,
};
use crate::ux_command_line::{UxCommandLine, UxCommandLinePublic};
use crate::ws::{self, LcuSocket};
use crate::{
    CLIENT_CMD_DEFAULT_POLL_INTERVAL_MS, CLIENT_CMD_LONG_POLL_INTERVAL_MS,
    CONNECT_TO_LC_RETRY_INTERVAL_MS, FIXED_ITEM_SET_PREFIX, LEAGUE_CLIENT_MAIN_NAMESPACE,
    LEAGUE_CLIENT_UX_MAIN_NAMESPACE,
};

/// Persistence for `league-client-main/lastConnectedClient`, implemented on top of
/// the settings store in the app shell.
pub trait CredentialStore: Send + Sync {
    fn save_last_connected_client(&self, value: &Value);
    fn get_last_connected_client(&self) -> Option<Value>;
    fn remove_last_connected_client(&self);
}

pub struct NoopCredentialStore;

impl CredentialStore for NoopCredentialStore {
    fn save_last_connected_client(&self, _value: &Value) {}
    fn get_last_connected_client(&self) -> Option<Value> {
        None
    }
    fn remove_last_connected_client(&self) {}
}

/// Opens the HTTP client and WebSocket for one credential set. Split out so the
/// reconnect loop is testable without a running game client.
#[async_trait]
pub trait LcuDialer: Send + Sync {
    async fn dial(
        &self,
        auth: &UxCommandLine,
        emitter: Arc<RadixEventEmitter>,
    ) -> anyhow::Result<(Arc<LcuHttp>, LcuSocket)>;
}

pub struct ProductionDialer;

#[async_trait]
impl LcuDialer for ProductionDialer {
    async fn dial(
        &self,
        auth: &UxCommandLine,
        emitter: Arc<RadixEventEmitter>,
    ) -> anyhow::Result<(Arc<LcuHttp>, LcuSocket)> {
        // `_initHttpInstance` throws on a ping failure only when there was no
        // response at all or the status is 5xx; 4xx means the client is there.
        let http = Arc::new(LcuHttp::new(auth.port, auth.basic_auth()));
        match http.ping().await {
            Ok(()) => {}
            Err(error) if error.to_string().contains("ECONNREFUSED") => return Err(error),
            Err(_) => {}
        }

        let socket = ws::connect(auth.port, &auth.auth_token, emitter).await?;
        Ok((http, socket))
    }
}

struct RendererSubscription {
    subscription: Subscription,
}

/// `LeagueClientMain`
pub struct LeagueClientMain {
    pub state: Arc<LeagueClientState>,
    pub settings: Arc<LeagueClientSettings>,

    ipc: Arc<RiftFlowIpcMain>,
    state_sync: Arc<StateSync>,
    store: Arc<dyn CredentialStore>,
    dialer: Arc<dyn LcuDialer>,

    emitter: Arc<RadixEventEmitter>,
    http: RwLock<Option<Arc<LcuHttp>>>,
    socket: RwLock<Option<LcuSocket>>,

    renderer_sub_increment: AtomicI64,
    renderer_subs: Mutex<HashMap<String, RendererSubscription>>,

    manually_disconnected: AtomicBool,
    should_have_one_attempt: AtomicBool,
    launched_clients: RwLock<Vec<UxCommandLinePublic>>,
}

/// `native/index.ts:183`'s `adjustLeagueClientWindowSize(clientZoom, config)`.
///
/// The Win32 side lives in `riftflow-native`, which sits above this crate, so the
/// shell injects the function at boot — and injects it **only** when
/// `NATIVE_SUPPORT.adjustLeagueClientWindowSize.available` (`isWindows &&
/// isElevated`) holds, which is what makes an un-elevated run a silent no-op like
/// the original instead of an error the renderer never used to see.
pub type WindowAdjuster = fn(f64, Option<(i32, i32)>) -> Result<bool, String>;

static WINDOW_ADJUSTER: OnceLock<WindowAdjuster> = OnceLock::new();

pub fn set_window_adjuster(adjuster: WindowAdjuster) {
    let _ = WINDOW_ADJUSTER.set(adjuster);
}

impl LeagueClientMain {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ipc: Arc<RiftFlowIpcMain>,
        state_sync: Arc<StateSync>,
        store: Arc<dyn CredentialStore>,
        dialer: Arc<dyn LcuDialer>,
    ) -> Self {
        Self {
            state: Arc::new(LeagueClientState::default()),
            settings: Arc::new(LeagueClientSettings::default()),
            ipc,
            state_sync,
            store,
            dialer,
            emitter: Arc::new(RadixEventEmitter::new()),
            http: RwLock::new(None),
            socket: RwLock::new(None),
            renderer_sub_increment: AtomicI64::new(1),
            renderer_subs: Mutex::new(HashMap::new()),
            manually_disconnected: AtomicBool::new(false),
            should_have_one_attempt: AtomicBool::new(false),
            launched_clients: RwLock::new(Vec::new()),
        }
    }

    pub fn emitter(&self) -> Arc<RadixEventEmitter> {
        self.emitter.clone()
    }

    pub fn namespace() -> &'static str {
        LEAGUE_CLIENT_MAIN_NAMESPACE
    }

    /// `_setupState` — declare the synced props with their initial values.
    pub fn publish_initial_state(&self) -> anyhow::Result<()> {
        self.state_sync.prop_sync(
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "state",
            &[
                ("auth", Value::Null),
                ("connectionState", json!("disconnected")),
                ("connectingClient", Value::Null),
            ],
            true,
        )?;
        self.state_sync.prop_sync(
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "settings",
            &[("autoConnect", json!(self.settings.auto_connect()))],
            true,
        )
    }

    fn publish_state(&self, changed: &[(&str, Value)]) {
        for (path, value) in changed {
            let _ = self
                .state_sync
                .set(LEAGUE_CLIENT_MAIN_NAMESPACE, "state", path, value.clone());
        }
    }

    pub fn http(&self) -> Option<Arc<LcuHttp>> {
        // `get http()` throws LeagueClientLcuUninitializedError in JS
        self.http.read().unwrap().clone()
    }

    pub fn current_region(&self) -> String {
        self.state.region()
    }

    /// `setConnectingClient` + `_doConnectingLoop` trigger.
    pub fn set_connecting_client(self: &Arc<Self>, client: Option<UxCommandLine>) {
        let published = self.state.set_connecting_client(client.clone());
        self.publish_state(&[(
            "connectingClient",
            serde_json::to_value(&published).unwrap_or(Value::Null),
        )]);

        if let Some(client) = client {
            self.spawn_connect_loop(client);
        }
    }

    fn spawn_connect_loop(self: &Arc<Self>, target: UxCommandLine) {
        let shard = self.clone();
        tokio::spawn(async move { shard.run_connect_loop(target).await });
    }

    /// `_doConnectingLoop` — retry only on connection-refused.
    async fn run_connect_loop(self: &Arc<Self>, mut target: UxCommandLine) {
        loop {
            if self.state.connecting_client().is_none() {
                break;
            }

            if !self.should_have_one_attempt.load(Ordering::SeqCst)
                && !self
                    .launched_clients
                    .read()
                    .unwrap()
                    .iter()
                    .any(|client| client.pid == target.pid)
            {
                // the target is no longer among the launched clients
                self.set_connecting_client(None);
                break;
            }

            match self.try_connect(&target).await {
                Ok(()) => {
                    self.should_have_one_attempt.store(false, Ordering::SeqCst);
                    self.set_connecting_client(None);
                    break;
                }
                Err(error) => {
                    let message = error.to_string();
                    if !message.contains("ECONNREFUSED") {
                        self.ipc.send_event(
                            LEAGUE_CLIENT_MAIN_NAMESPACE,
                            "error-connecting",
                            &[Value::String(message.clone())],
                        );
                        self.set_connecting_client(None);
                        break;
                    }

                    if self.should_have_one_attempt.swap(false, Ordering::SeqCst) {
                        self.set_connecting_client(None);
                        break;
                    }

                    tokio::time::sleep(std::time::Duration::from_millis(
                        CONNECT_TO_LC_RETRY_INTERVAL_MS,
                    ))
                    .await;

                    let Some(current) = self.state.connecting_client() else {
                        break;
                    };
                    target = current;
                }
            }
        }
    }

    /// `_connectToLcu` — one attempt.
    async fn try_connect(self: &Arc<Self>, auth: &UxCommandLine) -> anyhow::Result<()> {
        if self.state.connection_state() != ConnectionState::Disconnected {
            return Ok(());
        }

        self.state.set_connecting();
        self.publish_state(&[("connectionState", json!("connecting"))]);

        match self.dialer.dial(auth, self.emitter.clone()).await {
            Ok((http, socket)) => {
                *self.http.write().unwrap() = Some(http);
                let watcher = socket.closed_watcher();
                *self.socket.write().unwrap() = Some(socket);

                let published = self.state.set_connected(auth);
                self.publish_state(&[
                    ("connectionState", json!("connected")),
                    (
                        "auth",
                        serde_json::to_value(&published.1).unwrap_or(Value::Null),
                    ),
                ]);

                // `_saveToStorage('lastConnectedClient', cmd)`
                if let Ok(serialized) = serde_json::to_value(auth) {
                    self.store.save_last_connected_client(&serialized);
                }

                self.watch_disconnect(watcher);
                Ok(())
            }
            Err(error) => {
                let (state, auth_public) = self.state.set_disconnected();
                self.publish_state(&[
                    ("connectionState", to_json_state(state)),
                    ("auth", serde_json::to_value(&auth_public).unwrap_or(Value::Null)),
                ]);
                *self.http.write().unwrap() = None;
                Err(error)
            }
        }
    }

    /// `this._webSocket.on('close', ...)`
    fn watch_disconnect(self: &Arc<Self>, mut watcher: tokio::sync::watch::Receiver<bool>) {
        let shard = self.clone();
        tokio::spawn(async move {
            while !*watcher.borrow_and_update() {
                if watcher.changed().await.is_err() {
                    break;
                }
            }

            let (state, auth) = shard.state.set_disconnected();
            *shard.http.write().unwrap() = None;
            *shard.socket.write().unwrap() = None;
            shard.publish_state(&[
                ("connectionState", to_json_state(state)),
                ("auth", serde_json::to_value(&auth).unwrap_or(Value::Null)),
            ]);
        });
    }

    /// `connect({ force })`
    pub fn connect(self: &Arc<Self>, auth: UxCommandLine, force: bool) {
        if self.state.connection_state() == ConnectionState::Connected {
            self.disconnect();
        }
        if force {
            self.should_have_one_attempt.store(true, Ordering::SeqCst);
        }
        self.manually_disconnected.store(false, Ordering::SeqCst);
        self.set_connecting_client(Some(auth));
    }

    /// `disconnect()`
    pub fn disconnect(&self) {
        self.manually_disconnected.store(true, Ordering::SeqCst);
        let (state, auth) = self.state.set_disconnected();
        *self.http.write().unwrap() = None;
        self.publish_state(&[
            ("connectionState", to_json_state(state)),
            ("auth", serde_json::to_value(&auth).unwrap_or(Value::Null)),
        ]);

        if let Some(socket) = self.socket.write().unwrap().take() {
            tokio::spawn(async move { socket.close().await });
        }
    }

    /// `subscribeLcuEndpoint(uri)` → `__<n>`
    pub fn subscribe_lcu_endpoint(&self, uri: &str) -> anyhow::Result<String> {
        let id = self.renderer_sub_increment.fetch_add(1, Ordering::SeqCst);
        let sub_id = format!("__{id}");
        let ipc = self.ipc.clone();
        let namespace = LEAGUE_CLIENT_MAIN_NAMESPACE;
        let forwarded = sub_id.clone();

        let subscription = self.emitter.on(uri, move |data, params| {
            ipc.send_event(
                namespace,
                "extra-lcu-event",
                &[json!(forwarded), data, params],
            );
        })?;

        self.renderer_subs.lock().unwrap().insert(
            sub_id.clone(),
            RendererSubscription { subscription },
        );

        Ok(sub_id)
    }

    /// `unsubscribeLcuEndpoint(subId)`
    pub fn unsubscribe_lcu_endpoint(&self, sub_id: &str) -> bool {
        let removed = self.renderer_subs.lock().unwrap().remove(sub_id);
        match removed {
            Some(entry) => {
                self.emitter.off(&entry.subscription);
                true
            }
            None => false,
        }
    }

    /// The `_tryResumeConnection` precondition: UX gone, exactly one LeagueClient.
    pub fn should_attempt_resume(
        &self,
        ux_pids: &[u32],
        league_client_pids: &[u32],
    ) -> bool {
        ux_pids.is_empty() && league_client_pids.len() == 1
    }

    pub fn set_launched_clients(&self, clients: Vec<UxCommandLinePublic>) {
        *self.launched_clients.write().unwrap() = clients;
    }

    /// `writeItemSetsToDisk`, with the filesystem effects kept separate from the
    /// LCU install-dir lookup so it can be tested directly.
    pub fn write_item_sets(
        &self,
        install_dir: &str,
        item_sets: Option<Vec<Value>>,
        clear_previous: bool,
    ) -> anyhow::Result<Vec<std::path::PathBuf>> {
        let target = item_set_target_dir(install_dir, &self.current_region());

        if target.exists() && !target.is_dir() {
            anyhow::bail!("The path {} is not a directory", target.display());
        }
        if !target.exists() {
            std::fs::create_dir_all(&target)
                .map_err(|e| anyhow::anyhow!("create {}: {e}", target.display()))?;
        }

        if clear_previous {
            for entry in std::fs::read_dir(&target)? {
                let path = entry?.path();
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                if name.starts_with(FIXED_ITEM_SET_PREFIX) {
                    std::fs::remove_file(&path)?;
                }
            }
        }

        let mut written = Vec::new();
        if let Some(item_sets) = item_sets {
            for item_set in item_sets {
                let Some(uid) = item_set.get("uid").and_then(Value::as_str) else {
                    continue;
                };
                let path = target.join(format!("{uid}.json"));
                std::fs::write(&path, item_set.to_string())?;
                written.push(path);
            }
        }

        Ok(written)
    }

    /// The eight `league-client-main` handlers.
    pub fn register_ipc(self: &Arc<Self>) -> anyhow::Result<()> {
        let shard = self.clone();
        self.ipc.on_call(
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "connect",
            handler(move |_ctx, args| {
                let shard = shard.clone();
                async move {
                    let auth = parse_auth(&args).map_err(IpcError::plain)?;
                    let force = args
                        .first()
                        .and_then(|v| v.get("force"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    shard.connect(auth, force);
                    Ok(Value::Null)
                }
            }),
        )?;

        let shard = self.clone();
        self.ipc.on_call(
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "disconnect",
            handler(move |_ctx, _args| {
                let shard = shard.clone();
                async move {
                    shard.disconnect();
                    Ok(Value::Null)
                }
            }),
        )?;

        let shard = self.clone();
        self.ipc.on_call(
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "http-request",
            handler(move |_ctx, args| {
                let shard = shard.clone();
                async move { shard.http_request(args).await }
            }),
        )?;

        let shard = self.clone();
        self.ipc.on_call(
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "subscribeLcuEndpoint",
            handler(move |_ctx, args| {
                let shard = shard.clone();
                async move {
                    let uri = args
                        .first()
                        .and_then(Value::as_str)
                        .ok_or_else(|| IpcError::plain("uri is required"))?;
                    shard
                        .subscribe_lcu_endpoint(uri)
                        .map(Value::String)
                        .map_err(IpcError::plain)
                }
            }),
        )?;

        let shard = self.clone();
        self.ipc.on_call(
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "unsubscribeLcuEndpoint",
            handler(move |_ctx, args| {
                let shard = shard.clone();
                async move {
                    let sub_id = args
                        .first()
                        .and_then(Value::as_str)
                        .ok_or_else(|| IpcError::plain("subId is required"))?;
                    Ok(Value::Bool(shard.unsubscribe_lcu_endpoint(sub_id)))
                }
            }),
        )?;

        let shard = self.clone();
        self.ipc.on_call(
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "writeItemSetsToDisk",
            handler(move |_ctx, args| {
                let shard = shard.clone();
                async move { shard.write_item_sets_handler(args).await }
            }),
        )?;

        let shard = self.clone();
        self.ipc.on_call(
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "peekClient",
            handler(move |_ctx, args| {
                let shard = shard.clone();
                async move { shard.peek_client(args).await }
            }),
        )?;

        let shard = self.clone();
        self.ipc.on_call(
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "fixWindowMethodA",
            handler(move |_ctx, args| {
                let shard = shard.clone();
                async move { shard.fix_window_method_a(args).await }
            }),
        )
    }

    async fn http_request(&self, args: Vec<Value>) -> IpcResult<Value> {
        if self.state.connection_state() != ConnectionState::Connected {
            return Err(IpcError::business(
                "LeagueClient is not connected",
                "LCU_UNINITIALIZED",
            ));
        }

        let Some(http) = self.http.read().unwrap().clone() else {
            return Err(IpcError::business(
                "LeagueClient is not connected",
                "LCU_UNINITIALIZED",
            ));
        };

        let config = RequestConfig::from_json(args.first().unwrap_or(&Value::Null))
            .map_err(IpcError::plain)?;

        http.request_for_renderer(config)
            .await
            .map_err(|error| classify_request_error(&error))
    }

    /// `writeItemSetsToDisk(itemSets, clearPrevious)` — the install dir comes from
    /// `/data-store/v1/install-dir`, as in the original.
    async fn write_item_sets_handler(&self, args: Vec<Value>) -> IpcResult<Value> {
        let http = self
            .http
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| IpcError::business("LeagueClient is not connected", "LCU_UNINITIALIZED"))?;

        let install_dir = http
            .get("/data-store/v1/install-dir")
            .await
            .map_err(|error| classify_request_error(&error))?;
        let install_dir = install_dir
            .as_str()
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();

        let item_sets = args
            .first()
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let clear_previous = args.get(1).and_then(Value::as_bool).unwrap_or(true);

        let written = self
            .write_item_sets(
                &install_dir,
                if item_sets.is_empty() && args.first().is_none() {
                    None
                } else {
                    Some(item_sets)
                },
                clear_previous,
            )
            .map_err(IpcError::plain)?;

        Ok(json!({ "written": written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>() }))
    }

    /// `peekClient` — summoner plus a base64 profile icon, used before connecting.
    async fn peek_client(&self, args: Vec<Value>) -> IpcResult<Value> {
        let auth = parse_auth(&args).map_err(IpcError::plain)?;
        let http = LcuHttp::new(auth.port, auth.basic_auth());

        let summoner = http
            .get("/lol-summoner/v1/current-summoner")
            .await
            .map_err(|error| classify_request_error(&error))?;

        // 国服 current-summoner has no `psvLevel`, and `profileIconId` is the only
        // icon reference; a missing id simply yields no icon.
        let Some(icon_id) = summoner.get("profileIconId").and_then(Value::as_i64) else {
            return Ok(json!({ "summoner": summoner, "profileIcon": Value::Null }));
        };

        let icon = http
            .request(RequestConfig {
                method: "GET".to_string(),
                url: format!("/lol-game-data/assets/v1/profile-icons/{icon_id}.jpg"),
                response_type: Some("arraybuffer".to_string()),
                ..Default::default()
            })
            .await
            .map_err(|error| classify_request_error(&error))?;

        let content_type = icon
            .headers
            .get("content-type")
            .and_then(Value::as_str)
            .unwrap_or("image/jpeg");
        let base64_body = icon.body.as_str().unwrap_or_default().to_string();

        Ok(json!({
            "summoner": summoner,
            "profileIcon": format!("data:{content_type};base64,{base64_body}"),
        }))
    }

    /// Needs the `riftflow-tools` window handle path, which arrives with the
    /// `windows`-crate addon replacement; reported honestly rather than no-op'd.
    /// `fixWindowMethodA(config?)` — `league-client/index.ts:673`. The availability
    /// gate is the injected adjuster's presence (see `set_window_adjuster`); when it
    /// is absent the original returns `undefined` **without** even reading the zoom,
    /// so this stays a silent no-op instead of inventing an error.
    async fn fix_window_method_a(&self, args: Vec<Value>) -> IpcResult<Value> {
        let Some(adjust) = WINDOW_ADJUSTER.get().copied() else {
            return Ok(Value::Null);
        };

        let http = self
            .http
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| IpcError::business("LeagueClient is not connected", "LCU_UNINITIALIZED"))?;

        let zoom = http
            .get("/riotclient/zoom-scale")
            .await
            .map_err(|error| classify_request_error(&error))?
            .as_f64()
            .ok_or_else(|| IpcError::plain("zoom-scale is not a number"))?;

        // `{ baseWidth = 1280, baseHeight = 720 }`, taken as a pair or not at all,
        // which is what the addon's `config ?? {}` defaults do
        let base = args.first().and_then(|config| {
            let width = config.get("baseWidth").and_then(Value::as_i64)?;
            let height = config.get("baseHeight").and_then(Value::as_i64)?;
            Some((width as i32, height as i32))
        });

        let adjusted = adjust(zoom, base).map_err(IpcError::plain)?;

        Ok(json!(adjusted))
    }
}

fn to_json_state(state: ConnectionState) -> Value {
    match state {
        ConnectionState::Disconnected => json!("disconnected"),
        ConnectionState::Connecting => json!("connecting"),
        ConnectionState::Connected => json!("connected"),
    }
}

/// axios errors keep their response shape in the IPC envelope, so the renderer can
/// still read `error.response.data`.
fn classify_request_error(error: &anyhow::Error) -> IpcError {
    let message = error.to_string();
    if let Some(status) = message
        .split("status ")
        .nth(1)
        .and_then(|tail| tail.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|digits| digits.parse::<i64>().ok())
    {
        return IpcError::http(message, Some(status), None);
    }
    IpcError::plain(message)
}

fn parse_auth(args: &[Value]) -> anyhow::Result<UxCommandLine> {
    let Some(value) = args.first() else {
        anyhow::bail!("auth is required");
    };

    let port = value
        .get("port")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow::anyhow!("auth.port is required"))?;
    let auth_token = value
        .get("authToken")
        .or_else(|| value.get("remotingAuthToken"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("auth.authToken is required"))?
        .to_string();
    let pid = value
        .get("pid")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow::anyhow!("auth.pid is required"))?;

    let text = |key: &str| -> String {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };

    Ok(UxCommandLine {
        port,
        pid,
        auth_token,
        rso_platform_id: text("rsoPlatformId"),
        region: text("region"),
        certificate: text("certificate"),
        riot_client_port: value.get("riotClientPort").and_then(Value::as_i64).unwrap_or(0),
        riot_client_auth_token: text("riotClientAuthToken"),
    })
}

/// `LeagueClientUxMain` — the credential poller.
pub struct LeagueClientUxMain {
    pub state: Arc<LeagueClientUxState>,
    pub settings: Arc<LeagueClientUxSettings>,
    ipc: Arc<RiftFlowIpcMain>,
    state_sync: Arc<StateSync>,
    reader: Arc<CommandLineReader>,
    poll_interval_ms: std::sync::atomic::AtomicU64,
}

impl LeagueClientUxMain {
    pub fn new(
        ipc: Arc<RiftFlowIpcMain>,
        state_sync: Arc<StateSync>,
        probe: Arc<dyn ProcessProbe>,
        elevated: bool,
    ) -> Self {
        Self {
            state: Arc::new(LeagueClientUxState::default()),
            settings: Arc::new(LeagueClientUxSettings::default()),
            ipc,
            state_sync,
            reader: Arc::new(CommandLineReader::new(probe, elevated)),
            poll_interval_ms: std::sync::atomic::AtomicU64::new(CLIENT_CMD_DEFAULT_POLL_INTERVAL_MS),
        }
    }

    /// `CLIENT_CMD_LONG_POLL_INTERVAL` once connected, default while not.
    pub fn set_poll_interval(&self, interval_ms: u64) {
        self.poll_interval_ms
            .store(interval_ms, Ordering::SeqCst);
    }

    pub fn poll_interval(&self) -> u64 {
        self.poll_interval_ms.load(Ordering::SeqCst)
    }

    pub fn publish_initial_state(&self) -> anyhow::Result<()> {
        self.state_sync.prop_sync(
            LEAGUE_CLIENT_UX_MAIN_NAMESPACE,
            "settings",
            &[("useWmi", json!(self.settings.use_wmi()))],
            true,
        )?;
        self.state_sync.prop_sync(
            LEAGUE_CLIENT_UX_MAIN_NAMESPACE,
            "state",
            &[
                ("launchedClients", json!([])),
                ("hasClientButNoCommandLine", json!(false)),
            ],
            true,
        )
    }

    /// `update()` — one poll, publishing both props the renderer watches.
    pub fn update(&self) -> Vec<UxCommandLinePublic> {
        let (clients, flag) = self.reader.read(self.settings.use_wmi());
        let published = clients
            .iter()
            .map(UxCommandLine::without_certificate)
            .collect::<Vec<_>>();

        let _ = self.state_sync.set(
            LEAGUE_CLIENT_UX_MAIN_NAMESPACE,
            "state",
            "launchedClients",
            serde_json::to_value(&published).unwrap_or(json!([])),
        );

        if let Some(flag) = flag {
            if self.state.set_has_client_but_no_command_line(flag) {
                let _ = self.state_sync.set(
                    LEAGUE_CLIENT_UX_MAIN_NAMESPACE,
                    "state",
                    "hasClientButNoCommandLine",
                    json!(true),
                );
            }
        }

        published
    }

    /// `update()` also restarts the timer with the default interval in JS.
    pub fn restart_polling(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let shard = self.clone();
        tokio::spawn(async move {
            loop {
                let clients = shard.update();
                shard.notify_observers(clients);
                tokio::time::sleep(std::time::Duration::from_millis(shard.poll_interval())).await;
            }
        })
    }

    fn notify_observers(&self, clients: Vec<UxCommandLinePublic>) {
        let _ = clients;
    }

    pub fn register_ipc(self: &Arc<Self>, league_client: Arc<LeagueClientMain>) -> anyhow::Result<()> {
        let shard = self.clone();
        let client_for_update = league_client.clone();
        self.ipc.on_call(
            LEAGUE_CLIENT_UX_MAIN_NAMESPACE,
            "update",
            handler(move |_ctx, _args| {
                let shard = shard.clone();
                let league_client = client_for_update.clone();
                async move {
                    let clients = shard.update();
                    league_client.set_launched_clients(clients);
                    Ok(Value::Null)
                }
            }),
        )?;

        let ipc = self.ipc.clone();
        self.ipc.on_call(
            LEAGUE_CLIENT_UX_MAIN_NAMESPACE,
            "rebuildWmi",
            handler(move |_ctx, _args| {
                let ipc = ipc.clone();
                async move {
                    ipc.send_event(LEAGUE_CLIENT_UX_MAIN_NAMESPACE, "error-polling", &[]);
                    Err(IpcError::business(
                        "rebuildWmi requires the elevated resource path",
                        "NativeNotSupported",
                    ))
                }
            }),
        )
    }

    /// `error-polling` is emitted when a poll throws.
    pub fn emit_poll_error(&self) {
        self.ipc
            .send_event(LEAGUE_CLIENT_UX_MAIN_NAMESPACE, "error-polling", &[]);
    }
}

/// `IRiftFlowShardInitDispose` lifecycle for the UX shard, wiring `onInit`.
#[async_trait]
impl RiftFlowShard for LeagueClientUxMain {
    async fn on_init(&self) -> anyhow::Result<()> {
        self.publish_initial_state()
    }

    async fn on_dispose(&self) -> anyhow::Result<()> {
        self.state_sync.clear();
        Ok(())
    }
}

pub const UX_LONG_POLL_INTERVAL_MS: u64 = CLIENT_CMD_LONG_POLL_INTERVAL_MS;
