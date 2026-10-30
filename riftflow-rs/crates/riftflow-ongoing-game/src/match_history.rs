//! `match-history-loader.ts` — summaries, game details and the tag that scopes them.
//!
//! Read the four pure decisions at the top first: they are what the reactions in
//! `watch()` computed, and they are where all the quirks live.

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::task::JoinHandle;

use crate::ctx::{Ctx, trace, warn};
use crate::fetch::{
    self, ApiSource, MatchHistoryQueryParams,
};
use crate::priority;
use crate::queue::{PRIORITY_NOW, TagMode, TaskFailure, TaskOptions};
use crate::settings::TagPreference;
use crate::store::{MatchHistoryEntry, wrap_game};

const HISTORY_QUEUE: &str = "match-history";
const MISC: &str = "misc";

/// `currentQueueTagInfo` — which queue the tag should describe, and where it came
/// from. The three checks are not symmetric: a `draft` and a `gameflow` session
/// with queue id `0` or `-1` mean "no tag", a *lobby* is trusted as it is.
pub fn current_queue_tag_info(
    draft: Option<&Value>,
    lobby: Option<&Value>,
    gameflow_session: Option<&Value>,
) -> Option<(String, i64)> {
    if let Some(draft) = draft {
        let queue_id = draft.get("queueId").and_then(Value::as_i64).unwrap_or_default();
        if queue_id == 0 || queue_id == -1 {
            return None;
        }
        return Some(("draft".to_string(), queue_id));
    }

    if let Some(lobby) = lobby {
        return Some((
            "lobby".to_string(),
            lobby.pointer("/gameConfig/queueId").and_then(Value::as_i64).unwrap_or_default(),
        ));
    }

    if let Some(session) = gameflow_session {
        let queue_id = session.pointer("/gameData/queue/id").and_then(Value::as_i64).unwrap_or_default();
        if queue_id == 0 || queue_id == -1 {
            return None;
        }
        return Some(("gameflow-session".to_string(), queue_id));
    }

    None
}

/// the tag params the second reaction writes: `q_<queueId>`, or nothing at all for
/// the `'all'` preference
pub fn match_history_tag(
    current: Option<&(String, i64)>,
    preference: TagPreference,
) -> MatchHistoryQueryParams {
    let Some((_, queue_id)) = current else {
        return MatchHistoryQueryParams::default();
    };

    match preference {
        TagPreference::Current => MatchHistoryQueryParams {
            tag: Some(format!("q_{queue_id}")),
            ..Default::default()
        },
        // no queue at all *or* the 'all' preference: `tag: undefined`
        TagPreference::All => MatchHistoryQueryParams::default(),
    }
}

/// the `gameDetailsLoadCount` slice of every player's history — the whitelist of
/// timelines that may stay in the store
pub fn detail_whitelist(records: &[(String, Vec<Value>)], count: i64) -> Vec<i64> {
    records
        .iter()
        .flat_map(|(_, games)| {
            games
                .iter()
                .map(|game| game.get("gameId").and_then(Value::as_i64).unwrap_or_default())
                .take(count.max(0) as usize)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// the reaction's delete pass: details the whitelist no longer names.
pub fn details_to_prune(held: &[(String, Value)], whitelist: &[i64]) -> Vec<i64> {
    held
        .iter()
        .filter_map(|(_, detail)| {
            let game_id = detail.get("gameId").and_then(Value::as_i64)?;
            (!whitelist.contains(&game_id)).then_some(game_id)
        })
        .collect()
}

/// `sameSgpQuery` / `sameLcuQuery`: an LCU entry ignores the tag entirely, because
/// the LCU endpoint has no tag parameter to compare.
pub fn is_same_query(
    held: &MatchHistoryEntry,
    params: &MatchHistoryQueryParams,
    api_source: ApiSource,
) -> bool {
    if api_source == ApiSource::Sgp {
        return held.source == ApiSource::Sgp
            && held.params.tag == params.tag
            && held.params.tags_query_type == params.tags_query_type
            && held.params.start_index == params.start_index
            && held.params.count == params.count;
    }

    api_source == ApiSource::Lcu
        && held.source == ApiSource::Lcu
        && held.params.start_index == params.start_index
        && held.params.count == params.count
}

pub fn update_match_histories(
    ctx: &Arc<Ctx>,
    puuids: &[String],
    params: &MatchHistoryQueryParams,
    api_source: ApiSource,
    force: bool,
) -> Vec<JoinHandle<()>> {
    ctx.store.retain_match_history(puuids);

    puuids
        .iter()
        .map(|puuid| {
            ctx.queues.cancel_by_tags(&[puuid.as_str(), "match-history"], TagMode::And);
            load_match_history(ctx, puuid, params.clone(), force, api_source)
        })
        .collect()
}

/// `loadMatchHistory`, spawned like the original's fire-and-forget call.
pub fn load_match_history(
    ctx: &Arc<Ctx>,
    puuid: &str,
    params: MatchHistoryQueryParams,
    force: bool,
    api_source: ApiSource,
) -> JoinHandle<()> {
    let ctx = ctx.clone();
    let puuid = puuid.to_string();

    tokio::spawn(async move {
        let current = ctx.store.match_history_for(&puuid).and_then(entry_from_value);

        if let Some(current) = current {
            if !force && is_same_query(&current, &params, api_source) {
                trace(&format!(
                    "player match history query condition not changed, skip ({}): {puuid}",
                    api_source.as_str()
                ));
                return;
            }
        }

        let source_tag = api_source.as_str();
        let task_id = format!("{source_tag}-match-history:{puuid}");
        if ctx.queues.has_task(&task_id) {
            trace(&format!("player match history already in queue: {source_tag} {puuid}"));
            return;
        }

        ctx.store.set_match_history_loading(&puuid, "loading");
        let options = TaskOptions::new()
            .priority(priority::MATCH_HISTORY)
            .tags(&[puuid.as_str(), "match-history", source_tag]);

        let request = match api_source {
            ApiSource::Sgp => fetch::sgp_match_history(&puuid, &params, None),
            ApiSource::Lcu => fetch::lcu_match_history(&puuid, &params),
        };
        let fetcher = ctx.fetcher.clone();

        let result = ctx
            .queues
            .add(
                HISTORY_QUEUE,
                &task_id,
                async move { fetcher.fetch(request).await.map_err(TaskFailure::from) },
                options,
            )
            .await;

        let data = match result {
            Ok(data) => data,
            // an abort leaves the loading state where it was: the original returns
            // before touching it
            Err(error) if error.is_abort() => {
                trace(&format!("player match history loading aborted: {puuid}"));
                return;
            }
            Err(error) => {
                warn("error loading player match history", &error.message);
                ctx.store.set_match_history_loading(&puuid, "error");
                return;
            }
        };

        let entry = match api_source {
            ApiSource::Sgp => sgp_entry(&data, params),
            ApiSource::Lcu => lcu_entry(ctx.clone(), &puuid, &data, params).await,
        };

        ctx.store.set_match_history(&puuid, &entry);
        ctx.store.set_match_history_loading(&puuid, "loaded");
        trace(&format!("player match history loaded: {} {puuid}", api_source.as_str()));
    })
}

/// `data.games.filter(g => g.json)` — an SGP summary without its `json` half is dropped.
pub fn sgp_entry(data: &Value, params: MatchHistoryQueryParams) -> MatchHistoryEntry {
    let games: Vec<Value> = data
        .get("games")
        .and_then(Value::as_array)
        .map(|games| {
            games
                .iter()
                .filter(|game| game.get("json").is_some_and(|json| !json.is_null()))
                .map(|game| {
                    let game_id = game
                        .get("json")
                        .and_then(|json| json.get("gameId"))
                        .and_then(Value::as_i64)
                        .unwrap_or_default();
                    json!({ "source": "sgp", "gameId": game_id, "data": game })
                })
                .collect()
        })
        .unwrap_or_default();

    MatchHistoryEntry { source: ApiSource::Sgp, params, data: games }
}

/// The LCU list has no team/statistics detail per game, so each game id is fetched
/// whole (`lcu-game-summary:<gameId>`) and only the ones that came back are used —
/// a game whose summary failed falls back to the list entry.
pub async fn lcu_entry(
    ctx: Arc<Ctx>,
    puuid: &str,
    data: &Value,
    params: MatchHistoryQueryParams,
) -> MatchHistoryEntry {
    let list: Vec<Value> = data
        .pointer("/games/games")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let completions: Vec<JoinHandle<()>> = list
        .iter()
        .map(|game| {
            let game_id = game.get("gameId").and_then(Value::as_i64).unwrap_or_default();
            tokio::spawn(complete_game(ctx.clone(), puuid.to_string(), game_id))
        })
        .collect();

    for completion in completions {
        let _ = completion.await;
    }

    let games = list
        .iter()
        .map(|game| {
            let game_id = game.get("gameId").and_then(Value::as_i64).unwrap_or_default();
            ctx.cache
                .summary(&format!("lcu:{game_id}"))
                .unwrap_or_else(|| wrap_game(ApiSource::Lcu, game_id, game.clone()))
        })
        .collect();

    MatchHistoryEntry { source: ApiSource::Lcu, params, data: games }
}

/// `completeGame`: cache, then queue, then `/lol-match-history/v1/games/<id>`.
async fn complete_game(ctx: Arc<Ctx>, puuid: String, game_id: i64) {
    let key = format!("lcu:{game_id}");

    if ctx.cache.summary(&key).is_some() {
        return;
    }

    let task_id = format!("lcu-game-summary:{game_id}");
    if ctx.queues.has_task(&task_id) {
        trace(&format!("game summary already in queue: lcu {game_id}"));
        return;
    }

    let part_of = format!("part-of:{game_id}");
    let options = TaskOptions::new().priority(PRIORITY_NOW).tags(&[
        puuid.as_str(),
        "game-summary",
        "lcu",
        part_of.as_str(),
    ]);
    let request = fetch::lcu_game(game_id);
    let fetcher = ctx.fetcher.clone();

    match ctx
        .queues
        .add(MISC, &task_id, async move { fetcher.fetch(request).await.map_err(TaskFailure::from) }, options)
        .await
    {
        Ok(data) => {
            let wrapper = wrap_game(ApiSource::Lcu, game_id, data);
            ctx.cache.put_summary(&key, &wrapper);
        }
        Err(error) if error.is_abort() => {
            trace(&format!("game summary loading aborted: {puuid} {game_id}"));
        }
        Err(error) => warn("error loading game summary", &error.message),
    }
}

/// `loadGameDetails`. Note the SGP branch queues on `match-history` while the LCU
/// branch queues on `misc`, which is why the two sources have different backlogs.
pub fn load_game_details(
    ctx: &Arc<Ctx>,
    game_ids: &[i64],
    force: bool,
    api_source: ApiSource,
    detail_priority: Option<i64>,
) -> Vec<JoinHandle<()>> {
    game_ids
        .iter()
        .map(|game_id| {
            let ctx = ctx.clone();
            let game_id = *game_id;
            let detail_priority = detail_priority.unwrap_or(priority::GAME_DETAILS);

            tokio::spawn(async move {
                if let Some(current) = ctx.store.game_details_for(game_id) {
                    let same_source = current.get("source").and_then(Value::as_str) == Some(api_source.as_str());
                    if !force && same_source {
                        trace(&format!("game timeline query condition not changed, skip: {game_id}"));
                        return;
                    }
                }

                let key = format!("{}:{game_id}", api_source.as_str());
                if let Some(cached) = ctx.cache.details(&key) {
                    trace(&format!("game details hit cache: {game_id}"));
                    ctx.store.set_game_details(game_id, cached.clone());
                    return;
                }

                let task_id = format!("{}-game-details:{game_id}", api_source.as_str());
                if ctx.queues.has_task(&task_id) {
                    trace(&format!("game details already in queue: {game_id}"));
                    return;
                }

                let request = match api_source {
                    ApiSource::Sgp => fetch::sgp_game_details(game_id, None),
                    ApiSource::Lcu => fetch::lcu_game_details(game_id),
                };
                let queue = match api_source {
                    ApiSource::Sgp => HISTORY_QUEUE,
                    ApiSource::Lcu => MISC,
                };
                let options = TaskOptions::new()
                    .priority(detail_priority)
                    .tags(&[game_id.to_string().as_str(), "game-details", api_source.as_str()]);
                let fetcher = ctx.fetcher.clone();

                match ctx
                    .queues
                    .add(
                        queue,
                        &task_id,
                        async move { fetcher.fetch(request).await.map_err(TaskFailure::from) },
                        options,
                    )
                    .await
                {
                    Ok(data) => {
                        let wrapper = wrap_game(api_source, game_id, data);
                        ctx.cache.put_details(&key, &wrapper);
                        ctx.store.set_game_details(game_id, wrapper);
                    }
                    Err(error) if error.is_abort() => {
                        trace(&format!("game details loading aborted: {game_id}"));
                    }
                    Err(error) => warn("error loading game details", &error.message),
                }
            })
        })
        .collect()
}

/// `loadAdditionalGame` — both sources queue on `misc` at `priority: Infinity`.
pub fn load_additional_game(
    ctx: &Arc<Ctx>,
    game_ids: &[i64],
    force: bool,
    api_source: ApiSource,
) -> Vec<JoinHandle<()>> {
    let source_tag = api_source.as_str();

    game_ids
        .iter()
        .map(|game_id| {
            let ctx = ctx.clone();
            let game_id = *game_id;

            tokio::spawn(async move {
                if let Some(current) = ctx.store.additional_game(game_id) {
                    let same_source = current.get("source").and_then(Value::as_str) == Some(source_tag);
                    if !force && same_source {
                        trace(&format!("additional game query condition not changed, skip: {game_id}"));
                        return;
                    }
                }

                let key = format!("{source_tag}:{game_id}");
                if let Some(cached) = ctx.cache.summary(&key) {
                    trace(&format!("additional game hit cache: {game_id}"));
                    ctx.store.set_additional_game(game_id, cached.clone());
                    return;
                }

                let task_id = format!("{source_tag}-additional-game-summary:{game_id}");
                if ctx.queues.has_task(&task_id) {
                    trace(&format!("additional game already in queue: {source_tag} {game_id}"));
                    return;
                }

                let request = match api_source {
                    ApiSource::Sgp => fetch::sgp_game_summary(game_id, None),
                    ApiSource::Lcu => fetch::lcu_game(game_id),
                };
                let options = TaskOptions::new()
                    .priority(PRIORITY_NOW)
                    .tags(&[game_id.to_string().as_str(), "additional-game-summary", source_tag]);
                let fetcher = ctx.fetcher.clone();

                match ctx
                    .queues
                    .add(
                        MISC,
                        &task_id,
                        async move { fetcher.fetch(request).await.map_err(TaskFailure::from) },
                        options,
                    )
                    .await
                {
                    Ok(data) => {
                        let wrapper = wrap_game(api_source, game_id, data);
                        ctx.cache.put_summary(&key, &wrapper);
                        ctx.store.set_additional_game(game_id, wrapper);
                    }
                    Err(error) if error.is_abort() => {
                        trace(&format!("additional game loading aborted: {game_id}"));
                    }
                    Err(error) => warn("error loading additional game", &error.message),
                }
            })
        })
        .collect()
}

fn entry_from_value(value: Value) -> Option<MatchHistoryEntry> {
    let data = value.get("data")?.as_array()?.clone();
    Some(MatchHistoryEntry {
        source: match value.get("source").and_then(Value::as_str) {
            Some("sgp") => ApiSource::Sgp,
            _ => ApiSource::Lcu,
        },
        params: MatchHistoryQueryParams {
            start_index: value.pointer("/params/startIndex").and_then(Value::as_i64),
            count: value.pointer("/params/count").and_then(Value::as_i64),
            tag: value.pointer("/params/tag").and_then(Value::as_str).map(str::to_string),
            tags_query_type: value
                .pointer("/params/tagsQueryType")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        data,
    })
}
