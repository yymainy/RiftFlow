//! The loader-level decisions: which tag, which window, what to skip, what to prune
//! and what to cache — the pure half of `match-history-loader.ts` plus the LCU/SGP
//! payload shaping.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use riftflow_ipc::{RiftFlowIpcMain, RendererId, RendererTransport};
use riftflow_ongoing_game::ctx::Ctx;
use riftflow_ongoing_game::fetch::{
    self, ApiSource, FetchError, FetchRequest, Fetcher, MatchHistoryQueryParams,
};
use riftflow_ongoing_game::match_history;
use riftflow_ongoing_game::queue::{QueueKeeper, TagMode};
use riftflow_ongoing_game::settings::TagPreference;
use riftflow_ongoing_game::store::{MatchHistoryEntry, Store, wrap_game};
use riftflow_state::StateSync;
use serde_json::{Value, json};

const NS: &str = "ongoing-game-main";

#[derive(Default)]
struct Stub {
    summary_delay: std::sync::atomic::AtomicU64,
    urls: Mutex<Vec<String>>,
    responses: Mutex<HashMap<String, Value>>,
    /// any url containing this substring comes back as a 500
    fail_containing: Mutex<Option<String>>,
}

impl RendererTransport for Stub {
    fn send_to(&self, _renderer: RendererId, _event: &str, _args: Vec<Value>) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl Fetcher for Stub {
    async fn fetch(&self, request: FetchRequest) -> Result<Value, FetchError> {
        let url = request.url.clone();
        self.urls.lock().unwrap().push(url.clone());
        if url.contains("/games/") {
            tokio::time::sleep(std::time::Duration::from_millis(self.summary_delay.load(std::sync::atomic::Ordering::Relaxed))).await;
        }

        if let Some(prefix) = self.fail_containing.lock().unwrap().as_ref() {
            if url.contains(prefix.as_str()) {
                return Err(FetchError::with_status(500));
            }
        }

        for (prefix, body) in self.responses.lock().unwrap().iter() {
            if url.contains(prefix.as_str()) {
                return Ok(body.clone());
            }
        }

        if url.contains("/matches") {
            return Ok(json!({ "games": { "games": [
                { "gameId": 7, "fromList": true },
                { "gameId": 8, "fromList": true },
            ] } }));
        }

        if url.contains("/games/") {
            let game_id = url.rsplit('/').next().unwrap_or_default().to_string();
            return Ok(json!({ "gameId": game_id.parse::<i64>().unwrap_or_default(), "detailed": true,
                "participants": [{"participantId":1},{"participantId":2}],
                "participantIdentities": [{"participantId":1,"player":{"puuid":"a"}},{"participantId":2,"player":{"puuid":"b"}}]
            }));
        }

        Ok(json!({ "url": url }))
    }
}

fn harness_with(stub: Arc<Stub>) -> Arc<Ctx> {
    let ipc = Arc::new(RiftFlowIpcMain::new(stub.clone()));
    let state_sync = Arc::new(StateSync::new(ipc.clone()));

    state_sync
        .prop_sync(
            NS,
            "state",
            &[
                ("matchHistoryLoadingState", json!({})),
                ("analysis", Value::Null),
                ("inferredPremadeTeams", json!([])),
                ("additional", json!({})),
                ("matchHistoryTagParams", json!({})),
                ("draft", Value::Null),
            ],
            false,
        )
        .unwrap();

    Ctx::new(
        Store::new(state_sync.clone(), ipc.clone()),
        Arc::new(QueueKeeper::new(&["match-history", "misc"])),
        stub,
        None,
        state_sync,
        ipc,
    )
}

fn harness() -> (Arc<Ctx>, Arc<Stub>) {
    let stub = Arc::new(Stub::default());
    let ctx = harness_with(stub.clone());
    (ctx, stub)
}

fn failing(prefix: &str) -> Arc<Ctx> {
    let stub = Arc::new(Stub::default());
    *stub.fail_containing.lock().unwrap() = Some(prefix.to_string());
    harness_with(stub)
}

/// the loads are fire-and-forget and a cache hit never reaches the queue, so this
/// waits for the spawned work rather than for a queue that stays empty
async fn drain(ctx: &Arc<Ctx>) {
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        if ctx.queues.task_ids().is_empty() {
            return;
        }
    }
}

fn held(source: ApiSource, params: &MatchHistoryQueryParams) -> MatchHistoryEntry {
    MatchHistoryEntry { source, params: params.clone(), data: vec![] }
}

fn list(ids: &[i64]) -> Vec<Value> {
    ids.iter().map(|id| wrap_game(ApiSource::Lcu, *id, json!({ "gameId": id }))).collect()
}

#[test]
fn a_draft_or_session_without_a_queue_has_no_tag() {
    assert_eq!(match_history::current_queue_tag_info(Some(&json!({ "queueId": 0 })), None, None), None);
    assert_eq!(match_history::current_queue_tag_info(Some(&json!({ "queueId": -1 })), None, None), None);
    assert_eq!(
        match_history::current_queue_tag_info(
            Some(&json!({ "queueId": 420 })),
            Some(&json!({ "gameConfig": { "queueId": 1 } })),
            None
        ),
        Some(("draft".to_string(), 420))
    );

    // the lobby is trusted as it is: a `0` queue still names a tag there
    assert_eq!(
        match_history::current_queue_tag_info(None, Some(&json!({ "gameConfig": { "queueId": 0 } })), None),
        Some(("lobby".to_string(), 0))
    );

    assert_eq!(
        match_history::current_queue_tag_info(None, None, Some(&json!({ "gameData": { "queue": { "id": 520 } } }))),
        Some(("gameflow-session".to_string(), 520))
    );
    assert_eq!(
        match_history::current_queue_tag_info(None, None, Some(&json!({ "gameData": { "queue": { "id": -1 } } }))),
        None
    );
    assert_eq!(match_history::current_queue_tag_info(None, None, None), None);
}

#[test]
fn the_current_preference_is_the_only_one_that_produces_a_tag() {
    let current = ("lobby".to_string(), 420);

    assert_eq!(match_history::match_history_tag(Some(&current), TagPreference::Current).tag.as_deref(), Some("q_420"));
    assert_eq!(match_history::match_history_tag(Some(&current), TagPreference::All).tag, None);
    assert_eq!(match_history::match_history_tag(None, TagPreference::Current).tag, None);
    assert_eq!(match_history::match_history_tag(None, TagPreference::Current).start_index, None);
}

#[test]
fn an_lcu_entry_ignores_the_tag_when_deciding_to_skip() {
    let params = MatchHistoryQueryParams::with_tag(0, 20, None);
    let entry = held(ApiSource::Lcu, &params);

    // `sameLcuQuery` never looks at `tag`, so a new tag does not re-query on LCU
    assert!(match_history::is_same_query(&entry, &MatchHistoryQueryParams::with_tag(0, 20, Some("q_420".to_string())), ApiSource::Lcu));
    assert!(!match_history::is_same_query(&entry, &MatchHistoryQueryParams::with_tag(0, 50, None), ApiSource::Lcu));
    assert!(!match_history::is_same_query(&entry, &MatchHistoryQueryParams::with_tag(1, 20, None), ApiSource::Lcu));

    // a different source is always a different query
    assert!(!match_history::is_same_query(&entry, &params, ApiSource::Sgp));
}

#[test]
fn an_sgp_entry_compares_the_tag_and_the_window() {
    let params = MatchHistoryQueryParams::with_tag(0, 20, Some("q_420".to_string()));
    let entry = held(ApiSource::Sgp, &params);

    assert!(match_history::is_same_query(&entry, &params, ApiSource::Sgp));
    assert!(!match_history::is_same_query(&entry, &MatchHistoryQueryParams::with_tag(0, 20, Some("q_430".to_string())), ApiSource::Sgp));
    assert!(!match_history::is_same_query(&entry, &MatchHistoryQueryParams::with_tag(0, 20, None), ApiSource::Sgp));
    assert!(!match_history::is_same_query(&entry, &params, ApiSource::Lcu));
}

#[test]
fn the_whitelist_slices_each_player_separately_and_keeps_duplicates() {
    let records = vec![("a".to_string(), list(&[1, 2, 3])), ("b".to_string(), list(&[3, 4]))];

    // `flatMap(m => m.data.map(g => g.gameId).slice(0, n))` — a shared game id is
    // listed once per player, and the `Set` the reaction builds is what dedups it
    assert_eq!(match_history::detail_whitelist(&records, 2), vec![1, 2, 3, 4]);
    assert_eq!(match_history::detail_whitelist(&records, 1), vec![1, 3]);
    assert_eq!(match_history::detail_whitelist(&records, 3), vec![1, 2, 3, 3, 4]);
    assert!(match_history::detail_whitelist(&records, 0).is_empty());
}

#[test]
fn only_details_outside_the_whitelist_are_pruned() {
    let held: Vec<(String, Value)> = vec![
        ("1".to_string(), wrap_game(ApiSource::Lcu, 1, json!({}))),
        ("3".to_string(), wrap_game(ApiSource::Lcu, 3, json!({}))),
    ];

    assert_eq!(match_history::details_to_prune(&held, &[1, 2]), vec![3]);
    assert_eq!(match_history::details_to_prune(&held, &[1, 3]), Vec::<i64>::new());
    assert_eq!(match_history::details_to_prune(&held, &[]), vec![1, 3]);
}

#[test]
fn an_sgp_summary_without_its_json_half_is_dropped_in_order() {
    let entry = match_history::sgp_entry(
        &json!({ "games": [
            { "json": { "gameId": 7 } },
            { "notJson": true },
            { "json": { "gameId": 8 } },
        ] }),
        MatchHistoryQueryParams::with_tag(0, 20, Some("q_420".to_string())),
    );

    assert_eq!(entry.source, ApiSource::Sgp);
    assert_eq!(entry.params.tag.as_deref(), Some("q_420"));
    assert_eq!(
        entry.data.iter().map(|game| game["gameId"].as_i64()).collect::<Vec<_>>(),
        vec![Some(7), Some(8)]
    );
    assert_eq!(entry.data[0]["source"], json!("sgp"));
    assert_eq!(entry.data[0]["data"]["json"]["gameId"], json!(7), "the wrapper keeps the whole row");

    assert!(match_history::sgp_entry(&json!({}), MatchHistoryQueryParams::default()).data.is_empty());
}

#[tokio::test]
async fn an_lcu_history_completes_each_game_once_and_reuses_the_cache() {
    let (ctx, stub) = harness();
    let params = MatchHistoryQueryParams::with_tag(0, 20, None);
    let data = ctx.fetcher.fetch(fetch::lcu_match_history("a", &params)).await.unwrap();

    let first = match_history::lcu_entry(ctx.clone(), "a", &data, params.clone()).await;

    assert_eq!(first.data.len(), 2);
    assert_eq!(first.data[0]["data"]["detailed"], json!(true), "the list entry was replaced by the whole game");
    assert_eq!(stub.urls.lock().unwrap().iter().filter(|url| url.contains("/games/")).count(), 2);

    // the second player shares both games, so the cache answers without a request
    let data = ctx.fetcher.fetch(fetch::lcu_match_history("b", &params)).await.unwrap();
    let second = match_history::lcu_entry(ctx.clone(), "b", &data, params).await;

    assert_eq!(second.data[1]["gameId"], json!(8));
    assert_eq!(stub.urls.lock().unwrap().iter().filter(|url| url.contains("/games/")).count(), 2);
}

#[tokio::test]
async fn a_failed_summary_falls_back_to_the_list_entry() {
    let ctx = failing("/games/");
    let data = json!({ "games": { "games": [{ "gameId": 7, "fromList": true }] } });

    let entry = match_history::lcu_entry(ctx.clone(), "a", &data, MatchHistoryQueryParams::default()).await;

    assert_eq!(entry.data[0]["data"]["fromList"], json!(true), "the wrapper kept the list payload");
    assert_eq!(entry.source, ApiSource::Lcu);
}

#[tokio::test]
async fn a_partial_shared_cache_never_replaces_the_requested_players_own_record() {
    let (ctx, _) = harness();
    ctx.cache.put_summary("lcu:7", &wrap_game(ApiSource::Lcu, 7, json!({
        "participants":[{"participantId":1}],
        "participantIdentities":[{"participantId":1,"player":{"puuid":"a"}}]
    })));
    let list = json!({"games":{"games":[{"gameId":7,"ownerRecord":true,
        "participants":[{"participantId":2}],
        "participantIdentities":[{"participantId":2,"player":{"puuid":"b"}}]
    }]}});
    let entry = match_history::lcu_entry(ctx, "b", &list, MatchHistoryQueryParams::with_tag(0,20,None)).await;
    assert_eq!(entry.data[0]["data"]["ownerRecord"],json!(true));
}

#[tokio::test]
async fn concurrent_teammates_wait_for_the_shared_match_instead_of_publishing_incomplete_lists() {
    let (ctx, stub) = harness();
    stub.summary_delay.store(30, std::sync::atomic::Ordering::Relaxed);
    let list = json!({"games":{"games":[{"gameId":7}]}});
    let params = MatchHistoryQueryParams::with_tag(0,20,None);
    let (a,b) = tokio::join!(match_history::lcu_entry(ctx.clone(),"a",&list,params.clone()),
        match_history::lcu_entry(ctx.clone(),"b",&list,params));
    assert_eq!(a.data[0]["data"]["detailed"],json!(true));
    assert_eq!(b.data[0]["data"]["detailed"],json!(true));
    assert_eq!(stub.urls.lock().unwrap().iter().filter(|url|url.contains("/games/7")).count(),1);
}

#[tokio::test]
async fn a_pruned_detail_leaves_the_store_and_its_queue() {
    let (ctx, _stub) = harness();
    ctx.store.set_game_details(9, wrap_game(ApiSource::Lcu, 9, json!({})));

    for game_id in match_history::details_to_prune(&ctx.store.game_details(), &[7]) {
        ctx.store.remove_game_details(game_id);
        let key = game_id.to_string();
        ctx.queues.cancel_by_tags(&[key.as_str(), "game-details"], TagMode::And);
    }

    assert!(ctx.store.game_details_for(9).is_none());
}

#[tokio::test]
async fn a_cached_timeline_is_pushed_without_a_request() {
    let (ctx, stub) = harness();
    let wrapper = wrap_game(ApiSource::Lcu, 42, json!({ "frames": [] }));
    ctx.cache.put_details("lcu:42", &wrapper);

    match_history::load_game_details(&ctx, &[42], false, ApiSource::Lcu, None);
    drain(&ctx).await;

    assert_eq!(ctx.store.game_details_for(42), Some(wrapper));
    assert!(
        stub.urls.lock().unwrap().iter().all(|url| !url.contains("42")),
        "{:?}",
        stub.urls.lock().unwrap()
    );
}

#[tokio::test]
async fn an_unchanged_source_skips_and_a_forced_one_does_not() {
    let (ctx, stub) = harness();
    ctx.store.set_game_details(7, wrap_game(ApiSource::Lcu, 7, json!({ "first": true })));

    match_history::load_game_details(&ctx, &[7], false, ApiSource::Lcu, None);
    drain(&ctx).await;
    assert_eq!(timeline_calls(&stub), 0);

    // `force` is the only way to re-read a timeline the store already holds
    match_history::load_game_details(&ctx, &[7], true, ApiSource::Lcu, None);
    drain(&ctx).await;
    assert_eq!(timeline_calls(&stub), 1);
}

fn timeline_calls(stub: &Arc<Stub>) -> usize {
    stub.urls.lock().unwrap().iter().filter(|url| url.contains("/game-timelines/7")).count()
}

#[tokio::test]
async fn an_additional_game_is_only_re_read_for_another_source() {
    let (ctx, stub) = harness();
    ctx.store.set_additional_game(8, wrap_game(ApiSource::Lcu, 8, json!({ "x": 1 })));

    match_history::load_additional_game(&ctx, &[8], false, ApiSource::Lcu);
    drain(&ctx).await;
    assert!(stub.urls.lock().unwrap().iter().all(|url| !url.contains("/games/8")), "{:?}", stub.urls.lock().unwrap());

    match_history::load_additional_game(&ctx, &[8], false, ApiSource::Sgp);
    drain(&ctx).await;
    assert!(
        stub.urls.lock().unwrap().iter().any(|url| url.ends_with("/SUMMARY") && url.contains("_8/")),
        "the stored entry is `lcu`, so the SGP placeholder path goes to the wire: {:?}",
        stub.urls.lock().unwrap()
    );
}

#[tokio::test]
async fn a_history_load_writes_the_record_and_the_loading_state() {
    let (ctx, stub) = harness();
    let params = MatchHistoryQueryParams::with_tag(0, 20, None);

    match_history::update_match_histories(&ctx, &["a".to_string(), "b".to_string()], &params, ApiSource::Lcu, false);
    drain(&ctx).await;

    assert_eq!(ctx.store.match_history_for("a").unwrap()["data"].as_array().unwrap().len(), 2);
    assert_eq!(ctx.store.match_history_for("b").unwrap()["source"], json!("lcu"));
    assert_eq!(
        ctx.state_sync.get(NS, "state", "matchHistoryLoadingState"),
        Some(json!({ "a": "loaded", "b": "loaded" }))
    );

    // a departed player takes their record and their loading state with them
    match_history::update_match_histories(&ctx, &["a".to_string()], &params, ApiSource::Lcu, false);
    drain(&ctx).await;
    assert!(ctx.store.match_history_for("b").is_none(), "b left the teams list");
    assert_eq!(ctx.state_sync.get(NS, "state", "matchHistoryLoadingState"), Some(json!({ "a": "loaded" })));

    // the window that goes to the wire is `matchHistoryLoadCount`, as a closed range
    let mut history_calls: Vec<String> = stub
        .urls
        .lock()
        .unwrap()
        .iter()
        .filter(|url| url.contains("/matches"))
        .cloned()
        .collect();
    history_calls.sort();
    assert_eq!(history_calls, vec![
        "/lol-match-history/v1/products/lol/a/matches".to_string(),
        "/lol-match-history/v1/products/lol/b/matches".to_string(),
    ], "the second update for `a` is skipped by `sameLcuQuery`, not re-queried");

    // a bigger window is a different query, even without `force`
    match_history::update_match_histories(&ctx, &["a".to_string()], &MatchHistoryQueryParams::with_tag(0, 50, None), ApiSource::Lcu, false);
    drain(&ctx).await;
    assert_eq!(
        stub.urls.lock().unwrap().iter().filter(|url| url.contains("/matches")).count(),
        3,
        "the window went from 20 to 50"
    );
}

#[tokio::test]
async fn a_failed_history_marks_the_player_as_error_and_an_abort_does_not() {
    let ctx = failing("/matches");
    let params = MatchHistoryQueryParams::with_tag(0, 20, None);

    match_history::update_match_histories(&ctx, &["a".to_string()], &params, ApiSource::Lcu, false);
    drain(&ctx).await;

    assert_eq!(ctx.state_sync.get(NS, "state", "matchHistoryLoadingState"), Some(json!({ "a": "error" })));
    assert!(ctx.store.match_history_for("a").is_none());
}

#[test]
fn the_lcu_window_is_a_closed_range_and_sgp_keeps_the_tag() {
    let params = MatchHistoryQueryParams::with_tag(0, 50, Some("q_420".to_string()));

    assert_eq!(fetch::lcu_match_history("abc", &params).query[1], ("endIndex".to_string(), "49".to_string()));
    assert!(fetch::sgp_match_history("abc", &params, None).query.contains(&("tag".to_string(), "q_420".to_string())));
}
