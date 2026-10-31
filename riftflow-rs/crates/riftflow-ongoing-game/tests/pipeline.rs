//! The pipeline as `riftflow-app` will drive it: synced LCU props in, props and events
//! out, and one HTTP call per read the original made.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use riftflow_ipc::{RiftFlowIpcMain, RendererId, RendererTransport};
use riftflow_ongoing_game::{
    FetchError, FetchRequest, Fetcher, NoDeobfuscation, OngoingGame, OngoingGameDeps,
};
use riftflow_state::StateSync;
use serde_json::{Value, json};

const NS: &str = "ongoing-game-main";
const LEAGUE: &str = "league-client-main";

#[derive(Default)]
struct Stub {
    urls: Mutex<Vec<String>>,
    responses: Mutex<HashMap<String, Value>>,
    /// any url with this prefix comes back as a transport error
    failing: Mutex<Option<String>>,
    events: Mutex<Vec<Vec<Value>>>,
}

impl RendererTransport for Stub {
    fn send_to(&self, _renderer: RendererId, _event: &str, args: Vec<Value>) -> anyhow::Result<()> {
        self.events.lock().unwrap().push(args);
        Ok(())
    }
}

#[async_trait::async_trait]
impl Fetcher for Stub {
    async fn fetch(&self, request: FetchRequest) -> Result<Value, FetchError> {
        let path = request.url.clone();

        if let Some(prefix) = self.failing.lock().unwrap().as_ref() {
            if path.contains(prefix.as_str()) {
                return Err(FetchError::new("boom"));
            }
        }
        self.urls.lock().unwrap().push(format!(
            "{path}?{}",
            request
                .query
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("&")
        ));

        let responses = self.responses.lock().unwrap();

        for (prefix, body) in responses.iter() {
            if path.starts_with(prefix) {
                return Ok(body.clone());
            }
        }

        Ok(json!({ "url": path }))
    }
}

struct Harness {
    stub: Arc<Stub>,
    sync: Arc<StateSync>,
    game: Arc<OngoingGame>,
}

impl Harness {
    fn new() -> Self {
        let stub = Arc::new(Stub::default());
        let ipc = Arc::new(RiftFlowIpcMain::new(stub.clone()));
        let sync = Arc::new(StateSync::new(ipc.clone()));
        ipc.renderer_register(1, None);

        sync.prop_sync(
            LEAGUE,
            "gameflow",
            &[("phase", json!("None")), ("session", Value::Null)],
            false,
        )
        .unwrap();
        sync.prop_sync(LEAGUE, "lobby", &[("lobby", Value::Null)], false).unwrap();
        sync.prop_sync(LEAGUE, "champSelect", &[("session", Value::Null)], false).unwrap();
        sync.prop_sync(LEAGUE, "summoner", &[("me", Value::Null)], false).unwrap();
        sync.prop_sync(LEAGUE, "state", &[("auth", Value::Null)], false).unwrap();
        sync.prop_sync(
            "sgp-main",
            "state",
            &[("availability", json!({ "sgpServerId": "" })), ("isTokenReady", json!(false))],
            false,
        )
        .unwrap();
        sync.prop_sync(
            NS,
            "state",
            &[
                ("championSelections", json!({})),
                ("positionAssignments", json!({})),
                ("analysis", Value::Null),
                ("queryStage", json!({ "phase": "unavailable", "gameInfo": Value::Null })),
                ("teams", json!({})),
                ("isInEog", json!(false)),
                ("matchHistoryLoadingState", json!({})),
                ("summonerLoadingState", json!({})),
                ("savedInfoLoadingState", json!({})),
                ("rankedStatsLoadingState", json!({})),
                ("championMasteryLoadingState", json!({})),
                ("teamParticipantGroups", json!({})),
                ("matchHistoryTagParams", json!({})),
                ("draft", Value::Null),
                ("additional", json!({})),
                ("mergedPremadeTeamMap", json!({})),
                ("inferredPremadeTeams", json!([])),
            ],
            false,
        )
        .unwrap();
        sync.prop_sync(
            "app-common-main",
            "settings",
            &[("preferredLolSource", json!("lcu"))],
            false,
        )
        .unwrap();
        sync.prop_sync(
            NS,
            "settings",
            &[
                ("enabled", json!(true)),
                ("queryInLobbyPhase", json!(true)),
                ("matchHistoryLoadCount", json!(20)),
                ("gameDetailsLoadCount", json!(1)),
                // one at a time: the queue's own pacing is covered by `queue.rs`,
                // and a shared game is only cached once the previous player is done
                ("concurrency", json!(1)),
                ("matchHistoryTagPreference", json!("current")),
                ("premadeTeamInferMatchCountThreshold", json!(5)),
            ],
            false,
        )
        .unwrap();

        let game = OngoingGame::new(OngoingGameDeps {
            state_sync: sync.clone(),
            ipc,
            fetcher: stub.clone(),
            saved_player: None,
            deobfuscator: Arc::new(NoDeobfuscation),
            storage: None,
            platform: "win32".to_string(),
            version: "1.5.1".to_string(),
            analyzer: None,
        });

        Self { stub, sync, game }
    }

    fn set(&self, namespace: &str, state_id: &str, path: &str, value: Value) {
        self.sync.set(namespace, state_id, path, value).unwrap();
    }

    /// pass, wait out the 300 ms debounce, let the spawned loads settle — until a
    /// round changes nothing, since a finished history arms the timeline reaction
    async fn settle(&self) {
        for _ in 0..4 {
            self.game.run_pass();
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;

            for _ in 0..200 {
                if self.game.ctx().queues.task_ids().is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }

            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        self.game.run_pass();
    }

    fn urls(&self) -> Vec<String> {
        let mut urls = self.stub.urls.lock().unwrap().clone();
        urls.sort();
        urls
    }

    fn prop(&self, path: &str) -> Value {
        self.sync.get(NS, "state", path).unwrap_or(Value::Null)
    }

    fn events(&self) -> Vec<Vec<Value>> {
        self.stub.events.lock().unwrap().clone()
    }
}

fn member(puuid: &str, champion_id: i64, participant_id: i64) -> Value {
    json!({
        "puuid": puuid,
        "championId": champion_id,
        "selectedPosition": "JUNGLE",
        "selectedRole": "JUNGLE.PRIMARY.JUNGLE.TOP",
        "teamParticipantId": participant_id,
    })
}

fn in_game_session() -> Value {
    json!({
        "phase": "InProgress",
        "gameData": {
            "queue": { "id": 420, "type": "CLASSIC", "gameMode": "CLASSIC" },
            "gameId": 15,
            "playerChampionSelections": [
                { "puuid": "a", "championId": 1 },
                { "puuid": "b", "championId": 2 },
                { "puuid": "c", "championId": 3 },
                { "puuid": "d", "championId": 4 }
            ],
            "teamOne": [member("a", 1, 1), member("b", 2, 1)],
            "teamTwo": [member("c", 3, 0), member("d", 4, 0)],
        }
    })
}

#[tokio::test]
async fn the_same_roster_in_a_new_game_refreshes_history_instead_of_reusing_the_previous_query() {
    let harness = Harness::new();
    harness.set(LEAGUE, "gameflow", "phase", json!("InProgress"));
    harness.set(LEAGUE, "gameflow", "session", in_game_session());
    harness.settle().await;
    let before = harness.urls().iter().filter(|url| url.contains("/matches")).count();
    assert!(before > 0);
    let mut session = in_game_session();
    session["gameData"]["gameId"] = json!(16);
    harness.set(LEAGUE, "gameflow", "session", session);
    harness.settle().await;
    let after = harness.urls().iter().filter(|url| url.contains("/matches")).count();
    assert!(after > before, "a new match must refresh histories even with the same players");
}

/// every player shares the same one game, so the summary must be fetched once
fn shared_history(game_id: i64) -> Value {
    json!({
        "games": {
            "games": [{
                "gameId": game_id,
                "participants": [
                    { "participantId": 1, "teamId": 100 },
                    { "participantId": 2, "teamId": 100 },
                ],
                "participantIdentities": [
                    { "participantId": 1, "player": { "puuid": "a" } },
                    { "participantId": 2, "player": { "puuid": "b" } },
                ],
            }]
        }
    })
}

#[tokio::test]
async fn an_in_game_session_loads_every_participant_once() {
    let harness = Harness::new();
    harness.stub.responses.lock().unwrap().insert(
        "/lol-match-history/v1/products/lol".to_string(),
        shared_history(7),
    );
    harness.stub.responses.lock().unwrap().insert("/lol-match-history/v1/games/".to_string(), json!({ "gameId": 7 }));
    harness.stub.responses.lock().unwrap().insert("/lol-match-history/v1/game-timelines/".to_string(), json!({ "gameId": 7, "frames": [] }));
    harness.set(LEAGUE, "gameflow", "phase", json!("InProgress"));
    harness.set(LEAGUE, "gameflow", "session", in_game_session());

    harness.settle().await;

    let urls = harness.urls();
    for puuid in ["a", "b", "c", "d"] {
        assert!(urls.iter().any(|url| url.starts_with(&format!("/lol-summoner/v2/summoners/puuid/{puuid}"))), "{urls:?}");
        assert!(urls.iter().any(|url| url.starts_with(&format!("/lol-ranked/v1/ranked-stats/{puuid}"))), "{urls:?}");
        assert!(urls.iter().any(|url| url.starts_with(&format!("/lol-champion-mastery/v1/{puuid}/champion-mastery"))), "{urls:?}");
        // `matchHistoryLoadCount: 20` becomes the closed range 0..19
        assert!(
            urls.iter().any(|url| url == &format!("/lol-match-history/v1/products/lol/{puuid}/matches?begIndex=0&endIndex=19")),
            "{urls:?}"
        );
    }

    // one game for all four players: the summary cache means nobody reads it twice
    // from the store, and the timeline is fetched exactly once
    assert_eq!(urls.iter().filter(|url| url.starts_with("/lol-match-history/v1/game-timelines/")).count(), 1, "{urls:?}");
    let summaries = urls.iter().filter(|url| url.starts_with("/lol-match-history/v1/games/")).count();
    assert!(summaries < 4, "the summaries are shared, not one per player: {summaries}");

    assert_eq!(
        harness.prop("teams"),
        json!({ "TEAM-100": ["a", "b"], "TEAM-200": ["c", "d"] })
    );
    assert_eq!(
        harness.prop("queryStage"),
        json!({
            "phase": "in-game",
            "gameInfo": { "queueId": 420, "queueType": "CLASSIC", "gameMode": "CLASSIC", "gameId": 15 },
        })
    );
    assert_eq!(
        harness.prop("championSelections"),
        json!({ "a": 1, "b": 2, "c": 3, "d": 4 })
    );
    assert_eq!(harness.prop("isInEog"), json!(false));
    assert_eq!(
        harness.prop("teamParticipantGroups"),
        json!({ "1": ["a", "b"] }),
        "a zero teamParticipantId is skipped, like the original's falsy check"
    );

    let loading = harness.prop("matchHistoryLoadingState");
    for puuid in ["a", "b", "c", "d"] {
        assert_eq!(loading[puuid], json!("loaded"), "{loading}");
    }

    let pushed = harness.events();
    assert!(pushed.iter().any(|event| event.get(1).and_then(Value::as_str) == Some("match-history-loaded")));
    assert!(pushed.iter().any(|event| event.get(1).and_then(Value::as_str) == Some("summoner-loaded")));
    assert!(pushed.iter().any(|event| event.get(1).and_then(Value::as_str) == Some("game-details-loaded")));

    let all = harness.game.ctx().store.get_all();
    assert_eq!(all.as_object().unwrap().keys().len(), 7);
    assert_eq!(all["matchHistory"].as_object().unwrap().len(), 4);
    assert_eq!(all["gameDetails"]["7"]["gameId"], json!(7));
}

#[tokio::test]
async fn an_lcu_history_carrying_no_tag_still_asks_for_the_queue_tag() {
    let harness = Harness::new();
    harness.set(
        LEAGUE,
        "lobby",
        "lobby",
        json!({ "gameConfig": { "queueId": 420, "gameMode": "CLASSIC" }, "members": [{ "puuid": "a" }] }),
    );

    harness.settle().await;

    assert_eq!(harness.prop("matchHistoryTagParams"), json!({ "tag": "q_420", "tagsQueryType": null }));
    assert_eq!(harness.prop("queryStage"), json!({ "phase": "lobby", "gameInfo": { "queueId": 420, "queueType": "CLASSIC" } }));
    assert_eq!(harness.prop("teams"), json!({ "LOBBY": ["a"] }));

    // the LCU endpoint has no tag parameter, so the tag never reaches its query
    assert!(
        harness
            .urls()
            .iter()
            .any(|url| url == "/lol-match-history/v1/products/lol/a/matches?begIndex=0&endIndex=19")
    );
}

#[tokio::test]
async fn a_losing_tag_preference_clears_the_params_and_the_history_follows() {
    let harness = Harness::new();
    harness.set(NS, "settings", "matchHistoryTagPreference", json!("all"));
    harness.set(
        LEAGUE,
        "lobby",
        "lobby",
        json!({ "gameConfig": { "queueId": 420, "gameMode": "CLASSIC" }, "members": [] }),
    );

    harness.settle().await;
    assert_eq!(harness.prop("matchHistoryTagParams"), json!({ "tag": null, "tagsQueryType": null }));
}

#[tokio::test]
async fn a_draft_replaces_the_live_read_and_loads_its_players() {
    let harness = Harness::new();
    harness.game.ctx().store.set_draft(Some(json!({
        "gameModeKind": "cherry",
        "queueId": 1300,
        "puuid": "self",
        "teams": { "TEAM-ALL": ["x", "y"] },
        "championSelections": { "x": 5, "y": 6 },
        "positions": { "x": { "selected": "jungle", "primary": "JUNGLE", "secondary": "TOP" } },
    })));

    harness.settle().await;

    assert_eq!(harness.prop("queryStage"), json!({ "phase": "draft", "gameInfo": { "queueId": 1300, "queueType": "CHERRY" } }));
    assert_eq!(harness.prop("teams"), json!({ "TEAM-ALL": ["x", "y"] }));
    assert_eq!(harness.prop("positionAssignments"), json!({ "x": { "position": "JUNGLE", "role": null } }));
    assert_eq!(harness.prop("teamParticipantGroups"), json!({}));

    let urls = harness.urls();
    assert!(urls.iter().any(|url| url.starts_with("/lol-summoner/v2/summoners/puuid/x")), "{urls:?}");
    assert!(urls.iter().any(|url| url.starts_with("/lol-summoner/v2/summoners/puuid/y")), "{urls:?}");
    assert_eq!(harness.prop("matchHistoryTagParams"), json!({ "tag": "q_1300", "tagsQueryType": null }));
}

#[tokio::test]
async fn nothing_going_on_clears_the_state_and_says_so() {
    let harness = Harness::new();
    harness.set(LEAGUE, "gameflow", "phase", json!("InProgress"));
    harness.set(LEAGUE, "gameflow", "session", in_game_session());
    harness.settle().await;
    assert!(!harness.prop("teams").as_object().unwrap().is_empty());

    harness.stub.events.lock().unwrap().clear();
    harness.set(LEAGUE, "gameflow", "phase", json!("None"));
    harness.set(LEAGUE, "gameflow", "session", Value::Null);

    harness.game.run_pass();
    harness.game.run_pass();

    let pushed = harness.events();
    assert!(
        pushed.iter().any(|event| event.get(1).and_then(Value::as_str) == Some("clear")),
        "the unavailable transition sends 'clear'"
    );
    assert_eq!(harness.prop("teams"), json!({}));
    assert_eq!(harness.prop("matchHistoryLoadingState"), json!({}));
    assert_eq!(harness.prop("analysis"), Value::Null);
    assert_eq!(harness.game.ctx().store.get_all()["matchHistory"], json!({}));
}

#[tokio::test]
async fn a_second_pass_for_the_same_teams_does_not_re_query_them() {
    let harness = Harness::new();
    harness.set(LEAGUE, "gameflow", "phase", json!("InProgress"));
    harness.set(LEAGUE, "gameflow", "session", in_game_session());
    harness.settle().await;

    let before = harness.urls().len();
    harness.game.run_pass();
    harness.game.run_pass();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    harness.game.run_pass();

    assert_eq!(harness.urls().len(), before, "a settled pipeline queries nothing more");
}

#[tokio::test]
async fn sgp_is_only_used_once_its_token_is_ready() {
    let harness = Harness::new();
    harness.set(LEAGUE, "gameflow", "phase", json!("InProgress"));
    harness.set(LEAGUE, "gameflow", "session", in_game_session());
    harness.set("app-common-main", "settings", "preferredLolSource", json!("sgp"));
    harness.set("sgp-main", "state", "availability", json!({ "sgpServerId": "EUW", "serversSupported": { "matchHistory": true } }));

    // no token yet: the history reaction returns before it queues anything
    harness.settle().await;
    assert!(harness.urls().iter().all(|url| !url.contains("match-history-query")), "{:?}", harness.urls());

    harness.set("sgp-main", "state", "isTokenReady", json!(true));
    harness.settle().await;

    let urls = harness.urls();
    assert!(urls.iter().any(|url| url.starts_with("/match-history-query/v1/products/lol/player/a/SUMMARY")), "{urls:?}");
    // the tag is a real query parameter on SGP, unlike the LCU path
    assert!(
        urls.iter().any(|url| url.contains("/SUMMARY?") && url.contains("tag=q_420")),
        "{urls:?}"
    );
}

#[tokio::test]
async fn the_sgp_history_drops_games_without_their_json_half() {
    let harness = Harness::new();
    harness.stub.responses.lock().unwrap().insert(
        "/match-history-query/v1/products/lol/player".to_string(),
        json!({ "games": [
            { "json": { "gameId": 7 } },
            { "noJson": true },
        ] }),
    );
    harness.set(LEAGUE, "gameflow", "phase", json!("InProgress"));
    harness.set(LEAGUE, "gameflow", "session", in_game_session());
    harness.set("sgp-main", "state", "isTokenReady", json!(true));
    harness.set("sgp-main", "state", "availability", json!({ "sgpServerId": "EUW", "serversSupported": { "matchHistory": true } }));
    harness.set("app-common-main", "settings", "preferredLolSource", json!("sgp"));

    harness.settle().await;

    let all = harness.game.ctx().store.get_all();
    let entry = &all["matchHistory"]["a"];
    assert_eq!(entry["source"], json!("sgp"));
    assert_eq!(entry["data"].as_array().unwrap().len(), 1);
    assert_eq!(entry["data"][0]["gameId"], json!(7));
    assert_eq!(harness.game.ctx().store.game_details_for(7).is_some(), true);
}

#[tokio::test]
async fn a_failed_history_marks_the_player_as_error() {
    let harness = Harness::new();
    harness.set(LEAGUE, "gameflow", "phase", json!("InProgress"));
    harness.set(LEAGUE, "gameflow", "session", in_game_session());

    *harness.stub.failing.lock().unwrap() = Some("/matches".to_string());
    harness.settle().await;

    let loading = harness.prop("matchHistoryLoadingState");
    assert_eq!(loading["a"], json!("error"), "{loading}");
}

#[tokio::test]
async fn the_ipc_surface_answers_the_six_methods() {
    let harness = Harness::new();
    harness.set(LEAGUE, "gameflow", "session", in_game_session());
    harness.set(LEAGUE, "gameflow", "phase", json!("InProgress"));
    harness.game.register_ipc().unwrap();
    harness.game.declare_extra_props().unwrap();

    let ipc = harness.game.ctx().ipc.clone();
    let get_all = ipc.handle_call(1, NS, "getAll", vec![]).await;
    let outcome = format!("{get_all:?}");
    assert!(outcome.contains("matchHistory"), "{outcome}");

    ipc.handle_call(1, NS, "setDraft", vec![json!({ "queueId": 1 })]).await;
    assert_eq!(harness.prop("draft"), json!({ "queueId": 1 }));

    ipc.handle_call(1, NS, "clearDraft", vec![]).await;
    assert_eq!(harness.prop("draft"), Value::Null);

    ipc.handle_call(1, NS, "setMatchHistoryTagParams", vec![json!({ "tag": "q_1" })]).await;
    assert_eq!(harness.prop("matchHistoryTagParams"), json!({ "tag": "q_1", "tagsQueryType": null }));
    assert_eq!(
        harness.game.ctx().store.match_history_tag_params().tag.as_deref(),
        Some("q_1")
    );

    ipc.handle_call(1, NS, "reloadPlayer", vec![json!("a"), json!({ "includes": ["summoner"] })]).await;
    let unknown = ipc.handle_call(1, NS, "reloadPlayer", vec![]).await;
    assert!(format!("{unknown:?}").contains("reloadPlayer needs a puuid"));

    ipc.handle_call(1, NS, "reload", vec![]).await;
    assert!(
        harness
            .events()
            .iter()
            .any(|event| event.get(1).and_then(Value::as_str) == Some("clear")),
        "a reload clears first, like `_clearAndReloadAll`"
    );
}
