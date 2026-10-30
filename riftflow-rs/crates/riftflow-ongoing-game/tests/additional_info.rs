//! The GSM backfill path end to end: which request, which parse, which writes.

use std::sync::{Arc, Mutex};

use riftflow_ipc::{RiftFlowIpcMain, RendererId, RendererTransport};
use riftflow_ongoing_game::additional_info;
use riftflow_ongoing_game::computed::{Additional, QueryStage};
use riftflow_ongoing_game::ctx::Ctx;
use riftflow_ongoing_game::feature_gates::FeatureGates;
use riftflow_ongoing_game::fetch::{FetchError, FetchRequest, Fetcher};
use riftflow_ongoing_game::queue::QueueKeeper;
use riftflow_ongoing_game::settings::OngoingGameSettings;
use riftflow_ongoing_game::snapshot::LcSnapshot;
use riftflow_ongoing_game::store::Store;
use riftflow_state::StateSync;
use serde_json::{Value, json};

const NS: &str = "ongoing-game-main";

#[derive(Clone)]
struct Gsm {
    urls: Arc<Mutex<Vec<String>>>,
    reply: Value,
}

impl RendererTransport for Gsm {
    fn send_to(&self, _r: RendererId, _e: &str, _a: Vec<Value>) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl Fetcher for Gsm {
    async fn fetch(&self, request: FetchRequest) -> Result<Value, FetchError> {
        self.urls.lock().unwrap().push(request.url.clone());

        if let Some(status) = self.reply.get("__status").and_then(Value::as_u64) {
            return Err(FetchError::with_status(status as u16));
        }

        Ok(self.reply.clone())
    }
}

fn harness(reply: Value) -> (Arc<Ctx>, Gsm) {
    let gsm = Gsm { urls: Arc::new(Mutex::new(Vec::new())), reply };
    let ipc = Arc::new(RiftFlowIpcMain::new(Arc::new(gsm.clone())));
    let state_sync = Arc::new(StateSync::new(ipc.clone()));

    state_sync
        .prop_sync(
            NS,
            "state",
            &[("additional", json!({})), ("draft", Value::Null)],
            false,
        )
        .unwrap();
    // the re-check at the end of `update` reads the live props, so an in-game
    // session has to be there for anything to be written at all
    state_sync
        .prop_sync("league-client-main", "gameflow", &[("phase", json!("InProgress")), ("session", in_game_session())], false)
        .unwrap();
    state_sync
        .prop_sync("league-client-main", "summoner", &[("me", json!({ "puuid": "self" }))], false)
        .unwrap();

    let ctx = Ctx::new(
        Store::new(state_sync.clone(), ipc.clone()),
        Arc::new(QueueKeeper::new(&["match-history", "misc"])),
        Arc::new(gsm.clone()),
        None,
        state_sync,
        ipc,
    );

    (ctx, gsm)
}

fn ledge_reply() -> Value {
    json!({ "game": {
        "gameMode": "CLASSIC",
        "teamOne": [{
            "puuid": "hidden-1", "championId": 11, "teamParticipantId": 4,
            "selectedPosition": "JUNGLE", "selectedRole": "JUNGLE.PRIMARY.JUNGLE.MIDDLE",
        }],
        "teamTwo": [{
            "puuid": "hidden-2", "championId": 22, "teamParticipantId": 0,
            "selectedPosition": "UTILITY", "selectedRole": "UTILITY.AUTOFILL.UTILITY.UTILITY",
        }],
        "playerChampionSelections": [
            { "puuid": "hidden-1", "spell1Id": 11, "spell2Id": 14 },
            { "puuid": "hidden-2", "spell1Id": 7, "spell2Id": 4 },
        ],
    } })
}

fn in_game() -> QueryStage {
    QueryStage::InGame { queue_id: 420, queue_type: "CLASSIC".to_string(), game_mode: "CLASSIC".to_string(), game_id: 15 }
}

fn in_game_session() -> Value {
    json!({
        "phase": "InProgress",
        "gameData": {
            "queue": { "id": 420, "type": "CLASSIC", "gameMode": "CLASSIC" },
            "gameId": 15,
            "playerChampionSelections": [],
            "teamOne": [{ "puuid": "self", "championId": 1, "selectedPosition": "TOP", "selectedRole": "TOP.PRIMARY.TOP.JUNGLE" }],
            "teamTwo": [],
        }
    })
}

fn snapshot() -> LcSnapshot {
    LcSnapshot::from_json(&json!({ "summoner": { "me": { "puuid": "self" } } }))
}

async fn drain(ctx: &Arc<Ctx>) {
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        if ctx.queues.task_ids().is_empty() {
            return;
        }
    }
}

#[tokio::test]
async fn an_in_game_ledge_answer_fills_the_additional_prop() {
    let (ctx, gsm) = harness(ledge_reply());

    additional_info::update(
        &ctx,
        &snapshot(),
        &OngoingGameSettings::default(),
        &FeatureGates::default(),
        &in_game(),
        None,
    )
    .await;
    drain(&ctx).await;

    assert_eq!(
        gsm.urls.lock().unwrap().first(),
        Some(&"/gsm/v1/ledge/region/@riftflow:sgpServerSubId@/puuid/self".to_string())
    );

    let additional = ctx.store.additional();
    assert_eq!(additional.teams[0].1, vec!["hidden-1".to_string()]);
    assert_eq!(additional.teams[1].1, vec!["hidden-2".to_string()]);
    assert_eq!(additional.selections, vec![("hidden-1".to_string(), 11), ("hidden-2".to_string(), 22)]);
    assert_eq!(additional.team_participant_groups, vec![("hidden-1".to_string(), 4)]);
    assert_eq!(additional.spells[0].1, json!({ "spell1Id": 11, "spell2Id": 14 }));
    assert_eq!(additional.positions[0].1["role"]["current"], json!("JUNGLE"));

    assert_eq!(
        ctx.state_sync.get(NS, "state", "additional"),
        Some(additional.to_value())
    );
}

#[tokio::test]
async fn a_ledge_404_is_no_members_rather_than_an_error() {
    let (ctx, _gsm) = harness(json!({ "__status": 404 }));

    additional_info::update(&ctx, &snapshot(), &OngoingGameSettings::default(), &FeatureGates::default(), &in_game(), None).await;
    drain(&ctx).await;

    assert_eq!(ctx.store.additional(), Additional::default());
}

#[tokio::test]
async fn a_transport_failure_is_logged_and_leaves_the_prop_empty() {
    let (ctx, _gsm) = harness(json!({ "__status": 500 }));

    additional_info::update(&ctx, &snapshot(), &OngoingGameSettings::default(), &FeatureGates::default(), &in_game(), None).await;
    drain(&ctx).await;

    assert_eq!(ctx.store.additional(), Additional::default());
}

#[tokio::test]
async fn outside_the_game_the_backfill_is_switched_off_and_cleared() {
    let (ctx, gsm) = harness(ledge_reply());
    let champ_select = QueryStage::ChampSelect {
        queue_id: 420,
        queue_type: "CLASSIC".to_string(),
        game_mode: "CLASSIC".to_string(),
        game_id: 15,
    };

    additional_info::update(&ctx, &snapshot(), &OngoingGameSettings::default(), &FeatureGates::default(), &champ_select, None).await;
    drain(&ctx).await;

    assert!(gsm.urls.lock().unwrap().is_empty(), "only an in-game session is queried");
    assert_eq!(ctx.store.additional(), Additional::default());

    // a draft is the other way out, and it clears what was already there
    ctx.store.set_additional(&Additional {
        teams: vec![("TEAM-100".to_string(), vec!["x".to_string()])],
        ..Default::default()
    });
    let draft = json!({ "queueId": 420, "teams": { "TEAM-100": ["x"] } });
    additional_info::update(
        &ctx,
        &snapshot(),
        &OngoingGameSettings::default(),
        &FeatureGates::default(),
        &QueryStage::Draft { queue_id: 420, queue_type: "CLASSIC".to_string() },
        Some(&draft),
    )
    .await;
    drain(&ctx).await;

    assert_eq!(ctx.store.additional(), Additional::default());
}

#[tokio::test]
async fn without_a_self_puuid_nothing_is_asked_for() {
    let (ctx, gsm) = harness(ledge_reply());

    additional_info::update(&ctx, &LcSnapshot::default(), &OngoingGameSettings::default(), &FeatureGates::default(), &in_game(), None).await;
    drain(&ctx).await;

    assert!(gsm.urls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_closed_feature_gate_skips_the_request() {
    let (ctx, gsm) = harness(ledge_reply());
    let gates = FeatureGates { gsm_by_puuid: false, ..FeatureGates::default() };

    additional_info::update(&ctx, &snapshot(), &OngoingGameSettings::default(), &gates, &in_game(), None).await;
    drain(&ctx).await;

    assert!(gsm.urls.lock().unwrap().is_empty());
}
