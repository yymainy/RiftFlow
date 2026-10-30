//! `player-data-loader.ts` — the four reads per participant.
//!
//! Faithful details worth knowing before changing anything:
//! * every `update*` prunes whoever left the teams list first, then starts a load
//!   per puuid in list order and returns without awaiting — the original never
//!   awaited them either, the queue is what paces the traffic.
//! * only `match-history` writes a loading state. `summonerLoadingState`,
//!   `rankedStatsLoadingState`, `savedInfoLoadingState` and
//!   `championMasteryLoadingState` are declared, propSync'd and cleared, but the
//!   original never sets them, so they stay empty here too.
//! * `loadSavedInfo` fetches the *tags'* `selfPuuid` summoners, not another copy of
//!   the queried player.

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::task::JoinHandle;

use crate::ctx::{Ctx, trace, warn};
use crate::fetch::{self, MatchHistoryQueryParams};
use crate::priority;
use crate::queue::{TagMode, TaskFailure, TaskOptions};
use crate::record::{self, Pairs};
use crate::snapshot::LcSnapshot;

const MISC: &str = "misc";

pub fn update_summoners(ctx: &Arc<Ctx>, puuids: &[String], force: bool) -> Vec<JoinHandle<()>> {
    ctx.store.retain_summoner(puuids);
    restart(ctx, puuids, "summoner", force, |ctx, puuid, force| {
        load_summoner(&ctx, &puuid, force, false)
    })
}

pub fn update_ranked_stats(ctx: &Arc<Ctx>, puuids: &[String], force: bool) -> Vec<JoinHandle<()>> {
    ctx.store.retain_ranked_stats(puuids);
    restart(ctx, puuids, "ranked-stats", force, |ctx, puuid, force| {
        load_ranked_stats(&ctx, &puuid, force)
    })
}

pub fn update_champion_masteries(ctx: &Arc<Ctx>, puuids: &[String], force: bool) -> Vec<JoinHandle<()>> {
    ctx.store.retain_champion_mastery(puuids);
    restart(ctx, puuids, "champion-mastery", force, |ctx, puuid, force| {
        load_champion_mastery(&ctx, &puuid, force)
    })
}

pub fn update_saved_info(
    ctx: &Arc<Ctx>,
    snapshot: &LcSnapshot,
    puuids: &[String],
    force: bool,
) -> Vec<JoinHandle<()>> {
    ctx.store.retain_saved_info(puuids);

    puuids
        .iter()
        .map(|puuid| {
            ctx.queues.cancel_by_tags(&[puuid.as_str(), "saved-info"], TagMode::And);
            let ctx = ctx.clone();
            let puuid = puuid.clone();
            let snapshot = snapshot.clone();
            tokio::spawn(async move { load_saved_info(&ctx, &snapshot, &puuid, force).await })
        })
        .collect()
}

/// `queueKeeper.cancelByTags([puuid, kind], 'and')` then a fresh load of that puuid.
fn restart<F>(ctx: &Arc<Ctx>, puuids: &[String], tag: &str, force: bool, load: F) -> Vec<JoinHandle<()>>
where
    F: Fn(Arc<Ctx>, String, bool) -> JoinHandle<()>,
{
    puuids
        .iter()
        .map(|puuid| {
            ctx.queues.cancel_by_tags(&[puuid.as_str(), tag], TagMode::And);
            load(ctx.clone(), puuid.clone(), force)
        })
        .collect()
}

/// `loadSummoner`. `is_additional` only adds a tag: the original never uses
/// `ADDITIONAL_SUMMONER`'s priority, so an additional summoner still queues at
/// `SUMMONER`.
pub fn load_summoner(ctx: &Arc<Ctx>, puuid: &str, force: bool, is_additional: bool) -> JoinHandle<()> {
    let ctx = ctx.clone();
    let puuid = puuid.to_string();

    tokio::spawn(async move {
        if !force && ctx.store.summoner_for(&puuid).is_some() {
            trace(&format!("summoner info already exists: {puuid}"));
            return;
        }

        let task_id = format!("summoner:{puuid}");
        if ctx.queues.has_task(&task_id) {
            trace(&format!("summoner already in queue: {puuid}"));
            return;
        }

        let mut tags = vec![puuid.clone(), "summoner".to_string()];
        if is_additional {
            tags.push("additional-summoner".to_string());
        }

        let options = TaskOptions::new()
            .priority(priority::SUMMONER)
            .tags(&tags.iter().map(String::as_str).collect::<Vec<_>>());
        let request = fetch::lcu_summoner(&puuid);
        let fetcher = ctx.fetcher.clone();

        match ctx.queues.add(MISC, &task_id, async move { fetcher.fetch(request).await.map_err(TaskFailure::from) }, options)
            .await
        {
            Ok(data) => ctx.store.set_summoner(&puuid, data),
            Err(error) if error.is_abort() => trace(&format!("summoner info loading aborted: {puuid}")),
            Err(error) => warn("error loading summoner info", &error.message),
        }
    })
}

/// `loadRankedStats`.
pub fn load_ranked_stats(ctx: &Arc<Ctx>, puuid: &str, force: bool) -> JoinHandle<()> {
    let ctx = ctx.clone();
    let puuid = puuid.to_string();

    tokio::spawn(async move {
        if !force && ctx.store.ranked_stats_for(&puuid).is_some() {
            trace(&format!("ranked stats already exists: {puuid}"));
            return;
        }

        let task_id = format!("ranked-stats:{puuid}");
        if ctx.queues.has_task(&task_id) {
            trace(&format!("ranked stats already in queue: {puuid}"));
            return;
        }

        let options = TaskOptions::new()
            .priority(priority::RANKED_STATS)
            .tags(&[puuid.as_str(), "ranked-stats"]);
        let request = fetch::lcu_ranked_stats(&puuid);
        let fetcher = ctx.fetcher.clone();

        match ctx.queues.add(MISC, &task_id, async move { fetcher.fetch(request).await.map_err(TaskFailure::from) }, options).await {
            Ok(data) => ctx.store.set_ranked_stats(&puuid, data),
            Err(error) if error.is_abort() => trace(&format!("ranked stats loading aborted: {puuid}")),
            Err(error) => warn("error loading ranked stats", &error.message),
        }
    })
}

/// `loadChampionMastery`.
pub fn load_champion_mastery(ctx: &Arc<Ctx>, puuid: &str, force: bool) -> JoinHandle<()> {
    let ctx = ctx.clone();
    let puuid = puuid.to_string();

    tokio::spawn(async move {
        if !force && ctx.store.champion_mastery_for(&puuid).is_some() {
            trace(&format!("champion mastery already exists: {puuid}"));
            return;
        }

        let task_id = format!("champion-mastery:{puuid}");
        if ctx.queues.has_task(&task_id) {
            trace(&format!("champion mastery already in queue: {puuid}"));
            return;
        }

        let options = TaskOptions::new()
            .priority(priority::CHAMPION_MASTERY)
            .tags(&[puuid.as_str(), "champion-mastery"]);
        let request = fetch::lcu_champion_mastery(&puuid);
        let fetcher = ctx.fetcher.clone();

        match ctx.queues.add(MISC, &task_id, async move { fetcher.fetch(request).await.map_err(TaskFailure::from) }, options).await {
            Ok(data) => {
                let simplified = simplify_champion_mastery(&data);
                ctx.store.set_champion_mastery(&puuid, record::object(&simplified));
            }
            Err(error) if error.is_abort() => trace(&format!("champion mastery loading aborted: {puuid}")),
            Err(error) => warn("error loading champion mastery", &error.message),
        }
    })
}

/// the `map(...).reduce(...)` of `loadChampionMastery`: six fields, keyed by champion
/// id, and a later duplicate replaces the earlier one.
pub fn simplify_champion_mastery(list: &Value) -> Pairs<Value> {
    const KEYS: [&str; 6] = [
        "championId",
        "championLevel",
        "championPoints",
        "championSeasonMilestone",
        "highestGrade",
        "lastPlayTime",
    ];

    let mut simplified: Pairs<Value> = Vec::new();

    for mastery in list.as_array().into_iter().flatten() {
        let champion_id = mastery.get("championId").and_then(Value::as_i64).unwrap_or_default().to_string();
        let entry: Value = KEYS
            .iter()
            .map(|key| ((*key).to_string(), mastery.get(*key).cloned().unwrap_or(Value::Null)))
            .collect::<serde_json::Map<_, _>>()
            .into();

        match simplified.iter_mut().find(|(existing, _)| *existing == champion_id) {
            Some((_, existing_entry)) => *existing_entry = entry,
            None => simplified.push((champion_id, entry)),
        }
    }

    simplified
}

/// `loadSavedInfo`.
pub async fn load_saved_info(ctx: &Arc<Ctx>, snapshot: &LcSnapshot, puuid: &str, force: bool) {
    let (Some((region, rso_platform_id)), Some(self_puuid)) = (snapshot.auth(), snapshot.self_puuid()) else {
        return;
    };

    let query = json!({
        "puuid": puuid,
        "selfPuuid": self_puuid,
        "region": region,
        "rsoPlatformId": rso_platform_id,
    });

    if !force && ctx.store.saved_info_for(puuid).is_some() {
        return;
    }

    let task_id = format!("saved-info:{puuid}");
    if ctx.queues.has_task(&task_id) {
        trace(&format!("saved info already in queue: {puuid}"));
        return;
    }

    let Some(saved_player) = ctx.saved_player.clone() else {
        return;
    };

    let options = TaskOptions::new().priority(priority::SAVED_INFO).tags(&[puuid, "saved-info"]);
    let result = ctx
        .queues
        .add(
            MISC,
            &task_id,
            async move {
                saved_player
                    .query_saved_player_with_games(query)
                    .await
                    .map(|found| found.unwrap_or(Value::Null))
                    .map_err(|error| TaskFailure::new(error.to_string()))
            },
            options,
        )
        .await;

    let data = match result {
        Ok(Value::Null) => return,
        Ok(data) => data,
        Err(error) if error.is_abort() => {
            trace(&format!("saved info loading aborted: {puuid}"));
            return;
        }
        Err(error) => {
            warn("error loading saved info", &error.message);
            return;
        }
    };

    let game_ids: Vec<i64> = data
        .pointer("/encounteredGames/data")
        .and_then(Value::as_array)
        .map(|games| {
            games
                .iter()
                .filter_map(|game| game.get("gameId").and_then(Value::as_i64))
                .collect()
        })
        .unwrap_or_default();

    crate::match_history::load_additional_game(ctx, &game_ids, force, snapshot.api_should_use());

    for tag in data.get("tags").and_then(Value::as_array).into_iter().flatten() {
        if let Some(tag_self_puuid) = tag.get("selfPuuid").and_then(Value::as_str) {
            load_summoner(ctx, tag_self_puuid, force, false);
        }
    }

    ctx.store.set_saved_info(puuid, data.clone());
    trace(&format!("saved info loaded: {puuid}"));
}

/// `reloadPlayer` — the four non-history scopes of `resolveOngoingGamePlayerReloadScopes`.
pub fn reload_player(
    ctx: &Arc<Ctx>,
    snapshot: &LcSnapshot,
    puuid: &str,
    options: &Value,
) -> Vec<JoinHandle<()>> {
    let scopes = crate::reload::resolve_reload_scopes(options).unwrap_or_default();
    let mut handles = Vec::new();

    if crate::reload::includes(&scopes, "summoner") {
        ctx.queues.cancel_by_tags(&[puuid, "summoner"], TagMode::And);
        handles.push(load_summoner(ctx, puuid, true, false));
    }

    if crate::reload::includes(&scopes, "rankedStats") {
        ctx.queues.cancel_by_tags(&[puuid, "ranked-stats"], TagMode::And);
        handles.push(load_ranked_stats(ctx, puuid, true));
    }

    if crate::reload::includes(&scopes, "savedInfo") {
        ctx.queues.cancel_by_tags(&[puuid, "saved-info"], TagMode::And);
        let ctx = ctx.clone();
        let snapshot = snapshot.clone();
        let puuid = puuid.to_string();
        handles.push(tokio::spawn(async move { load_saved_info(&ctx, &snapshot, &puuid, true).await }));
    }

    if crate::reload::includes(&scopes, "championMastery") {
        ctx.queues.cancel_by_tags(&[puuid, "champion-mastery"], TagMode::And);
        handles.push(load_champion_mastery(ctx, puuid, true));
    }

    handles
}

/// `{ startIndex: 0, count, ...state.matchHistoryTagParams }`
pub fn history_params(count: i64, tag_params: &MatchHistoryQueryParams) -> MatchHistoryQueryParams {
    MatchHistoryQueryParams {
        start_index: Some(0),
        count: Some(count),
        tag: tag_params.tag.clone(),
        tags_query_type: tag_params.tags_query_type.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::SavedPlayer;
    use crate::fetch::{ApiSource, FetchError, FetchRequest, Fetcher};
    use crate::queue::QueueKeeper;
    use crate::store::Store;
    use riftflow_ipc::RiftFlowIpcMain;
    use riftflow_state::StateSync;
    use serde_json::json;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder {
        urls: Mutex<Vec<String>>,
    }

    impl riftflow_ipc::RendererTransport for Recorder {
        fn send_to(&self, _renderer: riftflow_ipc::RendererId, _event: &str, _args: Vec<Value>) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl Fetcher for Recorder {
        async fn fetch(&self, request: FetchRequest) -> Result<Value, FetchError> {
            self.urls.lock().unwrap().push(request.url.clone());

            if request.url.contains("champion-mastery") {
                return Ok(json!([
                    { "championId": 1, "championLevel": 5, "championPoints": 100,
                      "championSeasonMilestone": { "achievedLevel": 5 }, "highestGrade": "S", "lastPlayTime": 1 },
                    { "championId": 1, "championLevel": 7, "championPoints": 300,
                      "championSeasonMilestone": null, "highestGrade": "S+", "lastPlayTime": 2 },
                    { "championId": 2, "championLevel": 1 },
                ]));
            }

            if request.url.contains("games") {
                return Ok(json!({ "gameId": 7 }));
            }

            Ok(json!({ "puuid": "p", "url": request.url }))
        }
    }

    #[derive(Default)]
    struct FakeSaved {
        hits: Mutex<usize>,
    }

    #[async_trait::async_trait]
    impl SavedPlayer for FakeSaved {
        async fn query_saved_player_with_games(&self, _query: Value) -> anyhow::Result<Option<Value>> {
            *self.hits.lock().unwrap() += 1;
            Ok(Some(json!({
                "puuid": "p",
                "encounteredGames": { "data": [{ "gameId": 7 }, { "gameId": 8 }] },
                "tags": [{ "selfPuuid": "tagged-1" }],
            })))
        }
    }

    fn build_ctx(saved: Option<Arc<dyn SavedPlayer>>) -> (Arc<Ctx>, Arc<Recorder>) {
        let fetcher = Arc::new(Recorder::default());
        let ipc = Arc::new(RiftFlowIpcMain::new(fetcher.clone()));
        let state_sync = Arc::new(StateSync::new(ipc.clone()));

        let ctx = Ctx::new(
            Store::new(state_sync.clone(), ipc.clone()),
            Arc::new(QueueKeeper::new(&["match-history", "misc"])),
            fetcher.clone(),
            saved,
            state_sync,
            ipc,
        );

        (ctx, fetcher)
    }

    fn ctx() -> (Arc<Ctx>, Arc<Recorder>) {
        build_ctx(None)
    }

    async fn settle(ctx: &Arc<Ctx>) {
        while !ctx.queues.task_ids().is_empty() || ctx.queues.active_count(MISC) > 0 {
            tokio::task::yield_now().await;
        }
        tokio::task::yield_now().await;
    }

    async fn await_all(handles: Vec<JoinHandle<()>>) {
        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn one_summoner_per_puuid_and_nobody_is_fetched_twice() {
        let (ctx, recorder) = ctx();
        let puuids = vec!["a".to_string(), "b".to_string()];

        await_all(update_summoners(&ctx, &puuids, false)).await;
        settle(&ctx).await;
        assert_eq!(recorder.urls.lock().unwrap().len(), 2);

        // a second pass with the same list hits the `already exists` guard
        await_all(update_summoners(&ctx, &puuids, false)).await;
        settle(&ctx).await;
        assert_eq!(recorder.urls.lock().unwrap().len(), 2);

        // `force` goes to the wire again
        await_all(update_summoners(&ctx, &puuids, true)).await;
        settle(&ctx).await;
        assert_eq!(recorder.urls.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn a_player_that_left_takes_their_record_with_them() {
        let (ctx, _) = ctx();
        await_all(update_summoners(&ctx, &["a".to_string(), "b".to_string()], false)).await;
        settle(&ctx).await;
        assert!(ctx.store.summoner_for("a").is_some());

        await_all(update_summoners(&ctx, &["a".to_string()], false)).await;
        settle(&ctx).await;
        assert!(ctx.store.summoner_for("b").is_none());
    }

    #[tokio::test]
    async fn mastery_is_simplified_by_champion_id_with_the_last_entry_winning() {
        let (ctx, _) = ctx();
        await_all(update_champion_masteries(&ctx, &["a".to_string()], false)).await;
        settle(&ctx).await;

        let mastery = ctx.store.champion_mastery_for("a").expect("a mastery record");
        assert_eq!(mastery["1"]["championLevel"], json!(7), "the duplicate replaced the first");
        assert_eq!(mastery["1"]["championPoints"], json!(300));
        assert_eq!(
            mastery["1"].as_object().unwrap().keys().collect::<Vec<_>>(),
            vec![
                "championId",
                "championLevel",
                "championPoints",
                "championSeasonMilestone",
                "highestGrade",
                "lastPlayTime",
            ],
            "the six fields the original picks"
        );
        assert_eq!(mastery["2"]["championPoints"], json!(null), "a missing field is null, not absent");
    }

    #[tokio::test]
    async fn saved_info_pulls_the_encountered_games_and_the_tagged_summoners() {
        let (ctx, recorder) = build_ctx(Some(Arc::new(FakeSaved::default())));

        let snapshot = LcSnapshot::from_json(&json!({
            "summoner": { "me": { "puuid": "self" } },
            "state": { "auth": { "region": "HN1", "rsoPlatformId": "TENCENT" } },
        }));

        await_all(update_saved_info(&ctx, &snapshot, &["a".to_string()], false)).await;
        settle(&ctx).await;

        let urls = recorder.urls.lock().unwrap();
        assert!(urls.iter().any(|url| url == "/lol-match-history/v1/games/7"), "{urls:?}");
        assert!(urls.iter().any(|url| url == "/lol-match-history/v1/games/8"), "{urls:?}");
        assert!(
            urls.iter().any(|url| url == "/lol-summoner/v2/summoners/puuid/tagged-1"),
            "the tag's selfPuuid is fetched: {urls:?}"
        );
        assert!(ctx.store.saved_info_for("a").is_some());
    }

    #[tokio::test]
    async fn without_an_auth_or_a_self_puuid_nothing_is_queried() {
        let (ctx, recorder) = build_ctx(Some(Arc::new(FakeSaved::default())));

        let snapshot = LcSnapshot::from_json(&json!({ "summoner": { "me": { "puuid": "self" } } }));
        load_saved_info(&ctx, &snapshot, "a", false).await;
        settle(&ctx).await;

        assert!(recorder.urls.lock().unwrap().is_empty());
        assert!(ctx.store.saved_info_for("a").is_none());
    }

    #[tokio::test]
    async fn a_reload_scope_only_touches_the_scopes_it_names() {
        let (ctx, recorder) = ctx();

        let options = json!({ "includes": ["rankedStats"] });
        await_all(reload_player(&ctx, &LcSnapshot::default(), "a", &options)).await;
        settle(&ctx).await;

        assert_eq!(recorder.urls.lock().unwrap().clone(), vec!["/lol-ranked/v1/ranked-stats/a".to_string()]);
    }

    #[tokio::test]
    async fn a_second_update_for_the_same_puuid_cancels_and_restarts_nothing() {
        let (ctx, recorder) = ctx();
        ctx.queues.set_concurrency("misc", 1).unwrap();

        let first = update_summoners(&ctx, &["a".to_string()], true);
        // the same list again while the first is still queued: `hasTask` wins
        let second = update_summoners(&ctx, &["a".to_string()], true);
        await_all(first).await;
        await_all(second).await;
        settle(&ctx).await;

        assert_eq!(recorder.urls.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_history_params_start_at_zero_and_carry_the_tag() {
        let tag_params = MatchHistoryQueryParams { tag: Some("q_420".to_string()), ..Default::default() };
        let params = history_params(50, &tag_params);

        assert_eq!(params.start_index, Some(0));
        assert_eq!(params.count, Some(50));
        assert_eq!(params.tag.as_deref(), Some("q_420"));
        assert_eq!(params.tags_query_type, None);
    }

    #[test]
    fn simplification_of_a_non_list_answer_is_empty() {
        assert!(simplify_champion_mastery(&json!({ "error": "boom" })).is_empty());
        assert!(simplify_champion_mastery(&Value::Null).is_empty());
    }

    #[test]
    fn the_api_source_names_the_wire_value() {
        assert_eq!(ApiSource::Lcu.as_str(), "lcu");
        assert_eq!(ApiSource::Sgp.as_str(), "sgp");
    }
}
