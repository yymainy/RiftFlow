//! Connection-layer behavior spec, ported from the observable semantics of
//! `LeagueClientMain` / `LeagueClientUxMain`. No live game client is required:
//! the dialer and the process probe are injected.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use riftflow_ipc::{RiftFlowIpcMain, CallOutcome, RendererTransport};
use riftflow_lcu::discovery::{CommandLineReader, ProcessProbe};
use riftflow_lcu::http::LcuHttp;
use riftflow_lcu::ws::stub_socket;
use riftflow_lcu::{
    LeagueClientMain, LeagueClientUxMain, NoopCredentialStore, UxCommandLine, UxCommandLinePublic,
    LEAGUE_CLIENT_MAIN_NAMESPACE, LEAGUE_CLIENT_UX_MAIN_NAMESPACE,
};
use riftflow_state::StateSync;
use async_trait::async_trait;
use serde_json::{json, Value};

#[derive(Clone, Default)]
struct RecordingTransport {
    sent: Arc<Mutex<Vec<(u64, String, Vec<Value>)>>>,
}

impl RendererTransport for RecordingTransport {
    fn send_to(&self, renderer: u64, event: &str, args: Vec<Value>) -> anyhow::Result<()> {
        self.sent.lock().unwrap().push((renderer, event.to_string(), args));
        Ok(())
    }
}

struct Harness {
    ipc: Arc<RiftFlowIpcMain>,
    state_sync: Arc<StateSync>,
    transport: RecordingTransport,
}

impl Harness {
    fn new() -> Self {
        let transport = RecordingTransport::default();
        let ipc = Arc::new(RiftFlowIpcMain::new(Arc::new(transport.clone())));
        let state_sync = Arc::new(StateSync::new(ipc.clone()));
        Self {
            ipc,
            state_sync,
            transport,
        }
    }

    fn frames(&self) -> Vec<(u64, String, Vec<Value>)> {
        self.transport.sent.lock().unwrap().clone()
    }
}

fn auth(port: i64, pid: i64, region: &str) -> UxCommandLine {
    UxCommandLine {
        port,
        pid,
        auth_token: "abc-_123".to_string(),
        rso_platform_id: "HN1".to_string(),
        region: region.to_string(),
        certificate: String::new(),
        riot_client_port: 51100,
        riot_client_auth_token: "xyz".to_string(),
    }
}

fn public_auth(port: i64, pid: i64) -> UxCommandLinePublic {
    auth(port, pid, "TENCENT").without_certificate()
}

/// Fails the configured number of times with ECONNREFUSED, then succeeds.
struct ScriptedDialer {
    refusals: usize,
    fail_with: Option<String>,
    attempts: AtomicUsize,
}

impl ScriptedDialer {
    fn refusing(refusals: usize) -> Arc<Self> {
        Arc::new(Self {
            refusals,
            fail_with: None,
            attempts: AtomicUsize::new(0),
        })
    }

    fn failing(message: &str) -> Arc<Self> {
        Arc::new(Self {
            refusals: 0,
            fail_with: Some(message.to_string()),
            attempts: AtomicUsize::new(0),
        })
    }

    fn immediate() -> Arc<Self> {
        Self::refusing(0)
    }

    fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl riftflow_lcu::shard::LcuDialer for ScriptedDialer {
    async fn dial(
        &self,
        auth: &UxCommandLine,
        _emitter: Arc<riftflow_radix::RadixEventEmitter>,
    ) -> anyhow::Result<(Arc<LcuHttp>, riftflow_lcu::ws::LcuSocket)> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);

        if let Some(message) = &self.fail_with {
            let message = message.clone();
            anyhow::bail!(message);
        }
        if attempt < self.refusals {
            anyhow::bail!("ECONNREFUSED: connection refused");
        }

        Ok((
            Arc::new(LcuHttp::new(auth.port, auth.basic_auth())),
            stub_socket(),
        ))
    }
}

fn shard_with(harness: &Harness, dialer: Arc<ScriptedDialer>) -> Arc<LeagueClientMain> {
    Arc::new(LeagueClientMain::new(
        harness.ipc.clone(),
        harness.state_sync.clone(),
        Arc::new(NoopCredentialStore),
        dialer,
    ))
}

async fn wait_for<F: Fn() -> bool>(predicate: F) {
    // The paused test clock only advances as far as this task sleeps, so each step
    // has to be large enough to cross the shard's 2000 ms retry interval.
    for _ in 0..300 {
        if predicate() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("condition never became true within 30s of virtual time");
}

#[tokio::test(start_paused = true)]
async fn refused_connection_retries_until_it_succeeds() {
    let harness = Harness::new();
    let dialer = ScriptedDialer::refusing(2);
    let shard = shard_with(&harness, dialer.clone());
    shard.publish_initial_state().unwrap();
    shard.register_ipc().unwrap();
    shard.set_launched_clients(vec![public_auth(52611, 14264)]);

    shard.connect(auth(52611, 14264, "TENCENT"), false);

    let state = shard.state.clone();
    wait_for(|| state.connection_state() == riftflow_lcu::state::ConnectionState::Connected).await;

    assert_eq!(dialer.attempts(), 3, "two refusals then success");
    assert_eq!(shard.state.region(), "TENCENT");
    assert!(shard.http().is_some());

    // the IPC-visible credential bag must not carry the certificate
    let serialized = serde_json::to_value(shard.state.auth_public().unwrap()).unwrap();
    assert!(serialized.get("certificate").is_none(), "{serialized}");
    assert_eq!(shard.state.connecting_client(), None, "cleared after connecting");

    // the state sync pushed connecting -> connected to subscribers of `state`
    assert_eq!(
        harness
            .state_sync
            .get(LEAGUE_CLIENT_MAIN_NAMESPACE, "state", "connectionState")
            .unwrap()
            .as_str()
            .unwrap(),
        "connected"
    );
}

#[tokio::test(start_paused = true)]
async fn non_refused_failure_emits_error_connecting_and_stops() {
    let harness = Harness::new();
    let dialer = ScriptedDialer::failing("certificate rejected");
    let shard = shard_with(&harness, dialer.clone());
    shard.publish_initial_state().unwrap();
    shard.set_launched_clients(vec![public_auth(52611, 14264)]);
    harness.ipc.renderer_register(9, Some("register"));

    shard.connect(auth(52611, 14264, "TENCENT"), false);

    let frames = loop {
        let frames = harness.frames();
        if !frames.is_empty() {
            break frames;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    };

    assert_eq!(dialer.attempts(), 1, "must not retry a non-refusal error");
    assert_eq!(
        frames[0].2,
        vec![
            json!("league-client-main"),
            json!("error-connecting"),
            json!("certificate rejected"),
        ]
    );
    assert_eq!(
        shard.state.connection_state(),
        riftflow_lcu::state::ConnectionState::Disconnected
    );
}

#[tokio::test(start_paused = true)]
async fn force_attempt_is_allowed_without_a_launched_client() {
    let harness = Harness::new();
    let dialer = ScriptedDialer::immediate();
    let shard = shard_with(&harness, dialer.clone());
    shard.publish_initial_state().unwrap();
    // no launched clients at all

    shard.connect(auth(52611, 14264, "TENCENT"), true);

    let state = shard.state.clone();
    wait_for(|| state.connection_state() == riftflow_lcu::state::ConnectionState::Connected).await;
    assert!(dialer.attempts() >= 1);
}

#[tokio::test(start_paused = true)]
async fn target_vanishing_from_the_launch_list_cancels_the_attempt() {
    let harness = Harness::new();
    let dialer = ScriptedDialer::refusing(usize::MAX);
    let shard = shard_with(&harness, dialer.clone());
    shard.publish_initial_state().unwrap();
    shard.set_launched_clients(vec![public_auth(52611, 999)]);

    shard.connect(auth(52611, 14264, "TENCENT"), false);

    let state = shard.state.clone();
    wait_for(|| state.connecting_client().is_none()).await;
    assert_eq!(dialer.attempts(), 0, "never dialed a vanished target");
}

#[test]
fn subscribe_ids_increment_and_forward_the_exact_frame() {
    let harness = Harness::new();
    let dialer = ScriptedDialer::immediate();
    let shard = shard_with(&harness, dialer);
    shard.publish_initial_state().unwrap();
    harness.ipc.renderer_register(3, Some("register"));

    let first = shard.subscribe_lcu_endpoint("/lol-gameflow/v1/gameflow-phase").unwrap();
    let second = shard.subscribe_lcu_endpoint("/lol-summoner/v1/current-summoner").unwrap();
    assert_eq!(first, "__1");
    assert_eq!(second, "__2");

    shard
        .emitter()
        .emit("/lol-gameflow/v1/gameflow-phase", json!({ "data": "None" }));

    let frames = harness.frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].0, 3);
    assert_eq!(
        frames[0].2,
        vec![
            json!("league-client-main"),
            json!("extra-lcu-event"),
            json!("__1"),
            json!({ "data": "None" }),
            Value::Null,
        ]
    );

    assert!(shard.unsubscribe_lcu_endpoint("__1"));
    assert!(!shard.unsubscribe_lcu_endpoint("__1"), "second unsubscribe");
    assert!(!shard.unsubscribe_lcu_endpoint("__404"));
}

#[tokio::test]
async fn http_request_requires_a_connection() {
    let harness = Harness::new();
    let dialer = ScriptedDialer::immediate();
    let shard = shard_with(&harness, dialer);
    shard.publish_initial_state().unwrap();
    shard.register_ipc().unwrap();

    let outcome = harness
        .ipc
        .handle_call(
            1,
            LEAGUE_CLIENT_MAIN_NAMESPACE,
            "http-request",
            vec![json!({ "method": "get", "url": "/lol-gameflow/v1/gameflow-phase" })],
        )
        .await;

    match outcome {
        CallOutcome::Reply(envelope) => {
            let value = envelope.to_json();
            assert_eq!(value["success"], json!(false));
            assert_eq!(value["error"]["code"], json!("LCU_UNINITIALIZED"));
            assert_eq!(value["error"]["name"], json!("RiftFlowIpcError"));
        }
        CallOutcome::Rejected(message) => panic!("should reply with an envelope, got {message}"),
    }
}

#[tokio::test]
async fn unknown_league_client_method_is_rejected() {
    let harness = Harness::new();
    let dialer = ScriptedDialer::immediate();
    let shard = shard_with(&harness, dialer);
    shard.register_ipc().unwrap();

    let outcome = harness
        .ipc
        .handle_call(1, LEAGUE_CLIENT_MAIN_NAMESPACE, "noSuchMethod", vec![])
        .await;

    assert_eq!(
        outcome,
        CallOutcome::Rejected(
            "No function \"noSuchMethod\" in namespace \"league-client-main\"".to_string()
        )
    );
}

#[tokio::test]
async fn all_eight_league_client_methods_are_registered() {
    let harness = Harness::new();
    let dialer = ScriptedDialer::immediate();
    let shard = shard_with(&harness, dialer);
    shard.publish_initial_state().unwrap();
    shard.register_ipc().unwrap();

    for method in [
        "connect",
        "disconnect",
        "http-request",
        "subscribeLcuEndpoint",
        "unsubscribeLcuEndpoint",
        "writeItemSetsToDisk",
        "peekClient",
        "fixWindowMethodA",
    ] {
        let outcome = harness
            .ipc
            .handle_call(1, LEAGUE_CLIENT_MAIN_NAMESPACE, method, vec![json!({})])
            .await;
        assert!(
            !matches!(outcome, CallOutcome::Rejected(_)),
            "{method} should be registered"
        );
    }
}

#[tokio::test]
async fn fix_window_method_a_is_a_noop_without_the_injected_bridge() {
    let harness = Harness::new();
    let dialer = ScriptedDialer::immediate();
    let shard = shard_with(&harness, dialer);
    shard.register_ipc().unwrap();

    let value = match harness
        .ipc
        .handle_call(1, LEAGUE_CLIENT_MAIN_NAMESPACE, "fixWindowMethodA", vec![json!({})])
        .await
    {
        CallOutcome::Reply(envelope) => envelope.to_json(),
        CallOutcome::Rejected(message) => panic!("unexpected rejection: {message}"),
    };

    // `NATIVE_SUPPORT.adjustLeagueClientWindowSize.available` is `isWindows &&
    // isElevated`, and the original returns `undefined` in that case without even
    // reading `/riotclient/zoom-scale`. The shell only injects the adjuster when the
    // flag holds, so no adjuster means the same silent no-op — not an error.
    assert_eq!(value["data"], json!(null));
    assert_eq!(value.get("error"), None);
}

#[test]
fn resume_precondition_matches_the_js_process_shape() {
    let harness = Harness::new();
    let shard = shard_with(&harness, ScriptedDialer::immediate());

    assert!(shard.should_attempt_resume(&[], &[14264]));
    assert!(!shard.should_attempt_resume(&[1], &[14264]), "UX still alive");
    assert!(!shard.should_attempt_resume(&[], &[]), "no client");
    assert!(
        !shard.should_attempt_resume(&[], &[1, 2]),
        "more than one client is ambiguous"
    );
}

#[tokio::test(start_paused = true)]
async fn item_sets_are_written_to_the_tencent_game_path_and_old_ones_pruned() {
    let harness = Harness::new();
    let shard = shard_with(&harness, ScriptedDialer::immediate());
    shard.publish_initial_state().unwrap();
    shard.set_launched_clients(vec![public_auth(52611, 14264)]);
    shard.connect(auth(52611, 14264, "TENCENT"), false);

    let state = shard.state.clone();
    wait_for(|| state.connection_state() == riftflow_lcu::state::ConnectionState::Connected).await;

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("riftflow-itemsets-{stamp}"));
    let install_dir = root.join("LeagueClient");
    std::fs::create_dir_all(&install_dir).unwrap();

    // 国服: install_dir/../Game/Config/Global/Recommended
    let recommended = root.join("Game").join("Config").join("Global").join("Recommended");
    std::fs::create_dir_all(&recommended).unwrap();
    std::fs::write(recommended.join("riftflow1-old.json"), b"stale").unwrap();
    std::fs::write(recommended.join("manual-set.json"), b"keep").unwrap();

    let written = shard
        .write_item_sets(
            install_dir.to_str().unwrap(),
            Some(vec![json!({ "uid": "riftflow1-abc", "name": "runes" })]),
            true,
        )
        .unwrap();

    assert_eq!(written.len(), 1);
    assert!(recommended.join("riftflow1-abc.json").exists());
    assert!(!recommended.join("riftflow1-old.json").exists(), "prefixed files pruned");
    assert!(recommended.join("manual-set.json").exists(), "others kept");

    let _ = std::fs::remove_dir_all(&root);
}

struct FakeProbe {
    ux_pids: Vec<u32>,
    lines: Vec<String>,
}

impl ProcessProbe for FakeProbe {
    fn pids_by_name(&self, _name: &str) -> Vec<u32> {
        self.ux_pids.clone()
    }

    fn command_line(&self, _pid: u32) -> anyhow::Result<String> {
        // one line per probe call; pids are matched by count, not by value
        self.lines
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unreadable"))
    }
}

const REAL_CMD: &str = r#""D:\WeGameApps\LOL\LeagueClient\LeagueClientUx.exe" --app-port=52611 --remoting-auth-token=abc-_123 --app-pid=14264 --rso_platform_id=HN1 --region=TENCENT"#;

#[test]
fn ux_poll_publishes_launched_clients_and_the_no_commandline_flag() {
    let harness = Harness::new();
    let probe = Arc::new(FakeProbe {
        ux_pids: vec![1],
        lines: vec![REAL_CMD.to_string()],
    });
    let ux = Arc::new(LeagueClientUxMain::new(
        harness.ipc.clone(),
        harness.state_sync.clone(),
        probe,
        true,
    ));
    ux.publish_initial_state().unwrap();
    harness.ipc.renderer_register(5, Some("register"));

    let clients = ux.update();
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].port, 52611);

    let initial = harness
        .state_sync
        .subscribe_and_get_initial_state(5, LEAGUE_CLIENT_UX_MAIN_NAMESPACE, "state")
        .unwrap();
    assert_eq!(initial["launchedClients"]["value"].as_array().unwrap().len(), 1);
    assert_eq!(initial["hasClientButNoCommandLine"]["value"], json!(false));
}

#[test]
fn ux_poll_flags_unreadable_command_lines_after_five_rounds() {
    let probe = Arc::new(FakeProbe {
        ux_pids: vec![1],
        lines: vec![],
    });
    let reader = CommandLineReader::new(probe, false);

    for _ in 0..4 {
        assert_eq!(reader.read(false).1, None);
    }
    assert_eq!(reader.read(false).1, Some(true));
}

#[tokio::test]
async fn ux_ipc_surface_matches_the_original() {
    let harness = Harness::new();
    let probe = Arc::new(FakeProbe {
        ux_pids: vec![1],
        lines: vec![REAL_CMD.to_string()],
    });
    let ux = Arc::new(LeagueClientUxMain::new(
        harness.ipc.clone(),
        harness.state_sync.clone(),
        probe,
        false,
    ));
    let league_client = Arc::new(LeagueClientMain::new(
        harness.ipc.clone(),
        harness.state_sync.clone(),
        Arc::new(NoopCredentialStore),
        ScriptedDialer::immediate(),
    ));
    ux.publish_initial_state().unwrap();
    ux.register_ipc(league_client.clone()).unwrap();

    let updated = harness
        .ipc
        .handle_call(1, LEAGUE_CLIENT_UX_MAIN_NAMESPACE, "update", vec![])
        .await;
    assert!(matches!(updated, CallOutcome::Reply(envelope) if envelope.success));

    let published = harness
        .state_sync
        .get(LEAGUE_CLIENT_UX_MAIN_NAMESPACE, "state", "launchedClients")
        .unwrap();
    assert_eq!(
        published.as_array().map(|arr| arr.len()),
        Some(1),
        "update() publishes the launched clients"
    );

    let value = match harness
        .ipc
        .handle_call(1, LEAGUE_CLIENT_UX_MAIN_NAMESPACE, "rebuildWmi", vec![])
        .await
    {
        CallOutcome::Reply(envelope) => envelope.to_json(),
        CallOutcome::Rejected(message) => panic!("{message}"),
    };
    assert_eq!(value["error"]["code"], json!("NativeNotSupported"));
}
