//! Live smoke run against a real League client. Not a test — it needs the user's
//! 国服 client connected at least once by RiftFlow, because the auth token is
//! read back from the app's own `lastConnectedClient` row (an unelevated process
//! cannot read the elevated client's command line).
//!
//!     cargo run -p riftflow-lcu --example live_connect
//!
//! It exercises the production path end to end: ProductionDialer → reqwest/rustls
//! HTTPS with the self-signed cert → tokio-tungstenite wss subscription → radix
//! fan-out → IPC event frame.

use std::path::PathBuf;
use std::sync::Arc;

use riftflow_ipc::{RiftFlowIpcMain, RendererTransport};
use riftflow_lcu::{
    ConnectionState, LeagueClientMain, NoopCredentialStore, ProductionDialer, UxCommandLine,
};
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;
use riftflow_store::Database;

use serde_json::Value;

#[derive(Clone, Default)]
struct PrintingTransport;

impl RendererTransport for PrintingTransport {
    fn send_to(&self, renderer: u64, event: &str, args: Vec<Value>) -> anyhow::Result<()> {
        let namespace = args.first().and_then(Value::as_str).unwrap_or("?");
        let name = args.get(1).and_then(Value::as_str).unwrap_or("?");
        let detail = match name {
            "extra-lcu-event" => {
                let sub_id = args.get(2).and_then(Value::as_str).unwrap_or("?");
                let uri = args
                    .get(3)
                    .and_then(|payload| payload.get("uri"))
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                format!("subId={sub_id} uri={uri}")
            }
            _ => format!("{args:?}"),
        };
        println!("EVENT renderer={renderer} channel={event} ns={namespace} :: {detail}");
        Ok(())
    }
}

fn db_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(appdata).join("riftflow").join("RiftFlow.db"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = db_path().ok_or_else(|| anyhow::anyhow!("APPDATA is not set"))?;
    if !path.exists() {
        anyhow::bail!("no RiftFlow.db at {}", path.display());
    }

    // read-only use of the real database: open() does not run the version ladder
    let db = Arc::new(std::sync::Mutex::new(Database::open(&path)?));
    let factory = SettingFactory::new(db.clone());

    let stored = factory
        .get_raw("league-client-main", "lastConnectedClient")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no stored credentials; connect RiftFlow to the client once first"
            )
        })?;

    let auth: UxCommandLine = serde_json::from_value(stored.clone())?;
    println!(
        "USING pid={} port={} region={} rsoPlatformId={} tokenLen={}",
        auth.pid,
        auth.port,
        auth.region,
        auth.rso_platform_id,
        auth.auth_token.len()
    );

    let ipc = Arc::new(RiftFlowIpcMain::new(Arc::new(PrintingTransport)));
    let state_sync = Arc::new(StateSync::new(ipc.clone()));
    let shard = Arc::new(LeagueClientMain::new(
        ipc.clone(),
        state_sync.clone(),
        Arc::new(NoopCredentialStore),
        Arc::new(ProductionDialer),
    ));

    shard.publish_initial_state()?;
    shard.register_ipc()?;
    // events only reach *registered* renderers, exactly like the JS router
    ipc.renderer_register(1, Some("register"));
    shard.set_launched_clients(vec![auth.without_certificate()]);
    shard.subscribe_lcu_endpoint("/lol-gameflow/v1/gameflow-phase")?;
    shard.subscribe_lcu_endpoint("/lol-chat/v1/conversations")?;
    // radix wildcard: catch every inbound frame so an idle client cannot mask a
    // broken websocket path
    shard.subscribe_lcu_endpoint("/**")?;

    shard.connect(auth.clone(), true);

    for step in 0..40 {
        if shard.state.connection_state() == ConnectionState::Connected {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        if step == 39 {
            anyhow::bail!(
                "never connected; state={:?}",
                shard.state.connection_state()
            );
        }
    }

    println!("STATE connected, region={}", shard.current_region());
    println!(
        "DIAG routes on /** = {}",
        shard.emitter().listener_count("/**")
    );

    // count frames straight off the emitter, independent of the IPC forwarding, so
    // a quiet client can be told apart from a broken fan-out
    let raw_frames = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = raw_frames.clone();
    shard
        .emitter()
        .on("/**", move |_data, _params| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })
        .unwrap();

    let http = shard.http().ok_or_else(|| anyhow::anyhow!("no http handle"))?;
    let summoner = http.get("/lol-summoner/v1/current-summoner").await?;
    println!(
        "SUMMONER gameName={:?} puuidLen={:?} iconId={:?} hasPsvLevel={:?}",
        summoner.get("gameName").and_then(Value::as_str),
        summoner
            .get("puuid")
            .and_then(Value::as_str)
            .map(|puuid| puuid.len()),
        summoner.get("profileIconId").and_then(Value::as_i64),
        summoner.get("psvLevel").is_some(),
    );

    let phase = http.get("/lol-gameflow/v1/gameflow-phase").await?;
    println!("GAMEFLOW phase={phase}");

    println!("waiting 12s for websocket events…");
    tokio::time::sleep(std::time::Duration::from_secs(12)).await;
    println!(
        "DIAG emitter frames seen in window = {}",
        raw_frames.load(std::sync::atomic::Ordering::SeqCst)
    );

    shard.disconnect();
    println!("STATE after disconnect={:?}", shard.state.connection_state());
    Ok(())
}
