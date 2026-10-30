//! Main-side LCU state hydration: `league-client/lc-state/index.ts` for the
//! sub-states, plus the builtin SGP config from `riftflow-api/builtin.ts`.
//!
//! This is what turns the booted shell into a usable app: the renderer's
//! player-tabs store keys a tab by `sgpServerId:puuid` and refuses to create one
//! without either, which is exactly why the window showed only its splash.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use serde_json::{Map, Value, json};

use riftflow_lcu::LeagueClientMain;
use riftflow_lcu::http::{LcuHttp, RequestConfig};
use riftflow_state::StateSync;

use crate::protocol::percent_decode;

pub const LEAGUE_CLIENT_NAMESPACE: &str = "league-client-main";
pub const SGP_NAMESPACE: &str = "sgp-main";
pub const AUTO_SELECT_NAMESPACE: &str = "auto-select-main";

/// The three `BUILTIN_*` consts, extracted verbatim from `builtin.ts` into
/// `assets/riftflow-builtin.json` so the values cannot drift from the original.
static BUILTIN: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/riftflow-builtin.json"))
        .expect("assets/riftflow-builtin.json should be valid JSON")
});

fn builtin(name: &str) -> Value {
    BUILTIN.get(name).cloned().unwrap_or(Value::Null)
}

/// Endpoints whose event payload is copied straight into a synced sub-state.
/// Only the ones the main window needs to draw its first screen are listed; the
/// rest of `lc-state` lands with the features that read them.
const SYNCED: &[(&str, &str, &str)] = &[
    ("/lol-summoner/v1/current-summoner", "summoner", "me"),
    (
        "/lol-summoner/v1/current-summoner/summoner-profile",
        "summoner",
        "profile",
    ),
    ("/lol-gameflow/v1/gameflow-phase", "gameflow", "phase"),
    ("/lol-gameflow/v1/session", "gameflow", "session"),
    ("/lol-lobby/v2/lobby", "lobby", "lobby"),
    (
        "/lol-lobby/v2/received-invitations",
        "lobby",
        "receivedInvitations",
    ),
    ("/lol-chat/v1/me", "chat", "me"),
    ("/lol-honor-v2/v1/ballot", "honor", "ballot"),
    ("/lol-champ-select/v1/session", "champSelect", "session"),
    ("/lol-matchmaking/v1/ready-check", "matchmaking", "readyCheck"),
    ("/lol-matchmaking/v1/search", "matchmaking", "search"),
    ("/lol-login/v1/login-queue-state", "login", "loginQueueState"),
    ("/entitlements/v1/token", "entitlements", "token"),
    ("/lol-league-session/v1/league-session-token", "leagueSession", "token"),
];

/// `GameDataState` loaders: every endpoint answers with an array that the original
/// keys by `id` — game-mode mutators by `MapId`, and `queues.json` keeps only the
/// first entry with id 0 (the duplicates after it are not custom queues).
const GAME_DATA: &[(&str, &str, &str)] = &[
    (
        "/lol-game-data/assets/v1/summoner-spells.json",
        "summonerSpells",
        "id",
    ),
    ("/lol-game-data/assets/v1/items.json", "items", "id"),
    ("/lol-game-data/assets/v1/queues.json", "queues", "id"),
    ("/lol-game-data/assets/v1/perks.json", "perks", "id"),
    (
        "/lol-game-data/assets/v1/champion-summary.json",
        "champions",
        "id",
    ),
    ("/lol-game-data/assets/v1/cherry-augments.json", "augments", "id"),
    ("/lol-game-data/assets/v1/maps.json", "maps", "id"),
    (
        "/lol-game-data/assets/v1/game-mode-mutators.json",
        "gameModeMutators",
        "MapId",
    ),
];

const PERKSTYLES_URL: &str = "/lol-game-data/assets/v1/perkstyles.json";

/// `champSelect` list props. The renderer store holds these as a JS `Set` and
/// calls `.has()` on them, so they are marked with [`mark_as_set`] on the way out.
const CHAMP_SELECT_SETS: &[(&str, &str)] = &[
    (
        "/lol-champ-select/v1/pickable-champion-ids",
        "currentPickableChampionIds",
    ),
    (
        "/lol-champ-select/v1/bannable-champion-ids",
        "currentBannableChampionIds",
    ),
    (
        "/lol-champ-select/v1/disabled-champion-ids",
        "disabledChampionIds",
    ),
];

/// Single-value `champSelect` props, back to `null` when the endpoint disappears.
const CHAMP_SELECT_NULLABLE: &[(&str, &str)] = &[
    ("/lol-champ-select/v1/ongoing-champion-swap", "ongoingChampionSwap"),
    ("/lol-champ-select/v1/skin-selector-info", "skinSelectorInfo"),
    ("/lol-champ-select/v1/current-champion", "currentChampion"),
];

const SUBSET_LIST_URL: &str = "/lol-lobby-team-builder/champ-select/v1/subset-champion-list";
const ALL_GRID_CHAMPIONS_URL: &str = "/lol-champ-select/v1/all-grid-champions";

/// `list.reduce((prev, cur) => (prev[cur[field]] = cur, prev), {})`
fn key_by(list: &Value, field: &str, keep_first_zero_only: bool) -> Value {
    let mut out = Map::new();

    for entry in list.as_array().into_iter().flatten() {
        let Some(key) = entry.get(field).map(|value| match value {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            other => other.to_string(),
        }) else {
            continue;
        };

        if keep_first_zero_only && key == "0" && out.contains_key("0") {
            continue;
        }

        out.insert(key, entry.clone());
    }

    Value::Object(out)
}

/// `SgpState.leagueServers` is the builtin config until the riftflow-api fetch
/// replaces it; `supportedQueues` is the computed array, not the config object.
pub fn publish_builtin(state_sync: &Arc<StateSync>) -> anyhow::Result<()> {
    let champions: Value = serde_json::from_str(include_str!("../assets/champion-summary.json"))?;
    state_sync.set(
        LEAGUE_CLIENT_NAMESPACE,
        "gameData",
        "champions",
        key_by(&champions, "id", false),
    )?;
    state_sync.set(
        SGP_NAMESPACE,
        "state",
        "leagueServers",
        builtin("BUILTIN_SGP_LEAGUE_SERVERS_CONFIG"),
    )?;
    // `RiftFlowApiState.autoSelectGroups.groups` until the remote config replaces it;
    // the auto-select config UI lists exactly these groups
    let groups = builtin("BUILTIN_AUTO_SELECT_GROUPS")
        .get("groups")
        .cloned()
        .unwrap_or_else(|| json!([]));
    state_sync.set(AUTO_SELECT_NAMESPACE, "state", "groups", groups)?;
    sync_supported_queues(state_sync);
    Ok(())
}

/// `getSgpServerId()` from `shared/utils/sgp.ts`, aliases included.
pub fn sgp_server_id(region: &str, rso_platform_id: &str) -> String {
    let region = region.to_uppercase();
    let platform = rso_platform_id.to_uppercase();

    if region.is_empty() {
        return String::new();
    }

    if region == "TENCENT" {
        return format!("TENCENT_{platform}").to_uppercase();
    }

    let aliases: HashMap<&str, &str> = HashMap::from([
        ("NA", "NA1"),
        ("BR", "BR1"),
        ("TR", "TR1"),
        ("LAN", "LA1"),
        ("LAS", "LA2"),
        ("OCE", "OC1"),
        ("EUW1", "EUW"),
        ("JP1", "JP"),
    ]);

    aliases
        .get(region.as_str())
        .copied()
        .unwrap_or(region.as_str())
        .to_uppercase()
}

/// `SgpState.availability` — a computed getter in the original, pushed as a value
/// here whenever the auth changes.
pub fn sync_availability(state_sync: &Arc<StateSync>, league_client: &Arc<LeagueClientMain>) {
    let availability = match league_client.state.auth_public() {
        None => json!({
            "region": "",
            "rsoPlatform": "",
            "sgpServerId": "",
            "serversSupported": { "matchHistory": false, "common": false },
        }),
        Some(auth) => {
            let server_id = sgp_server_id(&auth.region, &auth.rso_platform_id);
            let supported = BUILTIN
                .get("BUILTIN_SGP_LEAGUE_SERVERS_CONFIG")
                .and_then(|config| config.get("servers"))
                .and_then(|servers| servers.get(&server_id))
                .cloned()
                .unwrap_or(json!({ "matchHistory": false, "common": false }));

            json!({
                "region": auth.region,
                "rsoPlatform": auth.rso_platform_id,
                "sgpServerId": server_id,
                "serversSupported": {
                    "matchHistory": supported.get("matchHistory").cloned().unwrap_or(Value::Bool(false)),
                    "common": supported.get("common").cloned().unwrap_or(Value::Bool(false)),
                },
            })
        }
    };

    let _ = state_sync.set(SGP_NAMESPACE, "state", "availability", availability);
}

/// `RiftFlowApiState.supportedQueues` — the list from the fetched config document.
/// `SgpState.supportedQueues` derives from it on every lobby/gameflow change, so it
/// has to live somewhere the watchers can read without holding the api runtime.
static FETCHED_SUPPORTED_QUEUES: LazyLock<Mutex<Value>> = LazyLock::new(|| Mutex::new(Value::Null));

/// Called by the riftflow-api refresh hook with the document's `queues` array.
pub fn set_fetched_supported_queues(value: Value) {
    let mut guard = FETCHED_SUPPORTED_QUEUES.lock().unwrap();
    *guard = value;
}

fn configured_supported_queues() -> Value {
    let guard = FETCHED_SUPPORTED_QUEUES.lock().unwrap();
    match guard.as_array() {
        Some(list) if !list.is_empty() => guard.clone(),
        _ => BUILTIN
            .get("BUILTIN_SUPPORTED_QUEUES")
            .and_then(|value| value.get("queues"))
            .cloned()
            .unwrap_or_else(|| json!([])),
    }
}

/// `SgpState.supportedQueues` — a computed getter: the configured list, with the
/// queue of the game in progress put in front when it is not already there.
pub fn sync_supported_queues(state_sync: &Arc<StateSync>) {
    let configured = configured_supported_queues();

    let lobby_queue = state_sync
        .get(LEAGUE_CLIENT_NAMESPACE, "lobby", "lobby")
        .and_then(|value| value.pointer("/gameConfig/queueId").and_then(Value::as_i64));
    let session_queue = state_sync
        .get(LEAGUE_CLIENT_NAMESPACE, "gameflow", "session")
        .and_then(|value| value.pointer("/gameData/queue/id").and_then(Value::as_i64));
    let current = lobby_queue.or(session_queue);

    let queues = match current.filter(|id| *id > 0) {
        Some(queue_id) => {
            let known = state_sync
                .get(LEAGUE_CLIENT_NAMESPACE, "gameData", "queues")
                .and_then(|queues| queues.get(&queue_id.to_string()).cloned())
                .is_some();
            let already_listed = configured
                .as_array()
                .is_some_and(|list| list.iter().any(|entry| entry.as_i64() == Some(queue_id)));

            if known && !already_listed {
                let mut merged = vec![json!(queue_id)];
                merged.extend(configured.as_array().cloned().unwrap_or_default());
                Value::Array(merged)
            } else {
                configured
            }
        }
        None => configured,
    };

    let _ = state_sync.set(SGP_NAMESPACE, "state", "supportedQueues", queues);
}

/// `SgpState.isTokenReady`, maintained by `SgpTokenStateController` from the two
/// token reactions.
fn sync_token_ready(state_sync: &Arc<StateSync>) {
    let ready = state_sync
        .get(LEAGUE_CLIENT_NAMESPACE, "entitlements", "token")
        .and_then(|token| {
            token
                .get("accessToken")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .is_some()
        && state_sync
            .get(LEAGUE_CLIENT_NAMESPACE, "leagueSession", "token")
            .and_then(|token| token.as_str().map(str::to_string))
            .is_some();

    let _ = state_sync.set(SGP_NAMESPACE, "state", "isTokenReady", json!(ready));
}

/// The `leagueClient.events.on(...)` half of `lc-state`: every event the socket
/// delivers for these endpoints becomes a prop push.
pub fn watch(state_sync: &Arc<StateSync>, league_client: &Arc<LeagueClientMain>) -> anyhow::Result<()> {
    let emitter = league_client.emitter();
    for (uri, state_id, path) in SYNCED {
        let sync = state_sync.clone();
        let state_id = (*state_id).to_string();
        let path = (*path).to_string();
        let drives_queues = matches!(state_id.as_str(), "lobby" | "gameflow");
        let drives_token_ready = matches!(state_id.as_str(), "entitlements" | "leagueSession");

        emitter.on(uri, move |payload, _params| {
            // the socket hands over the whole JsonApiEvent; the state stores `.data`,
            // and a `Delete` puts the prop back to its declared empty shape
            let data = if is_delete(&payload) {
                default_value(&state_id, &path)
            } else {
                payload.get("data").cloned().unwrap_or(Value::Null)
            };
            let _ = sync.set(LEAGUE_CLIENT_NAMESPACE, &state_id, &path, data);

            if drives_queues {
                sync_supported_queues(&sync);
            }
            if drives_token_ready {
                sync_token_ready(&sync);
            }
        })?;
    }

    for (uri, path) in CHAMP_SELECT_SETS {
        let sync = state_sync.clone();
        let path = (*path).to_string();

        emitter.on(uri, move |payload, _params| {
            let value = if is_delete(&payload) {
                json!([])
            } else {
                payload.get("data").cloned().unwrap_or_else(|| json!([]))
            };
            let _ = sync.set(LEAGUE_CLIENT_NAMESPACE, "champSelect", &path, value);
        })?;
    }

    for (uri, path) in CHAMP_SELECT_NULLABLE {
        let sync = state_sync.clone();
        let path = (*path).to_string();

        emitter.on(uri, move |payload, _params| {
            let value = if is_delete(&payload) {
                Value::Null
            } else {
                payload.get("data").cloned().unwrap_or(Value::Null)
            };
            let _ = sync.set(LEAGUE_CLIENT_NAMESPACE, "champSelect", &path, value);
        })?;
    }

    // one grid entry per event; the prop is the whole record, so it is merged
    let sync = state_sync.clone();
    emitter.on("/lol-champ-select/v1/grid-champions/:id", move |payload, _params| {
        let Some(entry) = payload.get("data").filter(|data| data.is_object()) else {
            return;
        };
        let Some(id) = entry.get("id").and_then(Value::as_i64) else {
            return;
        };

        let mut grid = sync
            .get(LEAGUE_CLIENT_NAMESPACE, "champSelect", "gridChampions")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();

        grid.insert(id.to_string(), entry.clone());
        let _ = sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            "champSelect",
            "gridChampions",
            Value::Object(grid),
        );
    })?;

    watch_chat_conversations(state_sync, league_client)?;

    let sync = state_sync.clone();
    emitter.on(SUBSET_LIST_URL, move |payload, _params| {
        let value = if is_delete(&payload) {
            json!([])
        } else {
            payload.get("data").cloned().unwrap_or_else(|| json!([]))
        };
        let _ = sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            "lobbyTeamBuilder",
            "champSelect.subsetChampionList",
            value,
        );
    })?;

    Ok(())
}


/// The three conversation types the sub-state keeps, in the order the original
/// switches on them.
const CONVERSATION_KINDS: &[&str] = &["championSelect", "postGame", "customGame"];

/// `chat.conversations.*` and `chat.participants.*`. Three ported features read
/// the champion-select conversation id — the auto-select local notices, the ARAM
/// side line, the matchmaking countdown — so this mirrors `lc-state/index.ts:533`
/// exactly, including the id-prefix check and the Create/Update/Delete split.
fn watch_chat_conversations(
    state_sync: &Arc<StateSync>,
    league_client: &Arc<LeagueClientMain>,
) -> anyhow::Result<()> {
    let sync = state_sync.clone();
    league_client.emitter().on(
        "/lol-chat/v1/conversations/:id",
        move |payload, params| {
            let event_type = payload
                .get("eventType")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let data = payload.get("data").cloned().unwrap_or(Value::Null);

            if event_type == "Delete" {
                let decoded = percent_decode(
                    params.get("id").and_then(Value::as_str).unwrap_or_default(),
                );

                for kind in CONVERSATION_KINDS {
                    let stored = conversation(sync.as_ref(), kind);
                    if stored.as_deref() == Some(decoded.as_str()) {
                        let _ = sync.set(LEAGUE_CLIENT_NAMESPACE, "chat", &format!("conversations.{kind}"), Value::Null);
                        let _ = sync.set(LEAGUE_CLIENT_NAMESPACE, "chat", &format!("participants.{kind}"), Value::Null);
                    }
                }
                return;
            }

            let Some(kind) = data.get("type").and_then(Value::as_str).map(str::to_string) else {
                return;
            };
            if !CONVERSATION_KINDS.contains(&kind.as_str()) {
                return;
            }
            // the champion-select conversation is identified twice over: by its
            // `type` and by the id prefix
            if kind == "championSelect"
                && !data
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .contains("lol-champ-select")
            {
                return;
            }

            if event_type == "Create" {
                let _ = sync.set(
                    LEAGUE_CLIENT_NAMESPACE,
                    "chat",
                    &format!("conversations.{kind}"),
                    data,
                );
                let _ = sync.set(
                    LEAGUE_CLIENT_NAMESPACE,
                    "chat",
                    &format!("participants.{kind}"),
                    json!([]),
                );
            } else if event_type == "Update" {
                let _ = sync.set(
                    LEAGUE_CLIENT_NAMESPACE,
                    "chat",
                    &format!("conversations.{kind}"),
                    data,
                );
            }
        },
    )?;

    // a `joined_room` message grows the participant list
    let sync = state_sync.clone();
    league_client.emitter().on(
        "/lol-chat/v1/conversations/:conversationId/messages/:messageId",
        move |payload, params| {
            let data = payload.get("data").cloned().unwrap_or(Value::Null);
            if data.get("body").and_then(Value::as_str) != Some("joined_room") {
                return;
            }

            let Some(summoner_id) = data.get("fromSummonerId").filter(|value| !value.is_null()) else {
                return;
            };
            let conversation_id = percent_decode(
                params.get("conversationId").and_then(Value::as_str).unwrap_or_default(),
            );

            let Some(kind) = CONVERSATION_KINDS
                .iter()
                .find(|kind| conversation(sync.as_ref(), kind).as_deref() == Some(conversation_id.as_str()))
                .copied()
            else {
                return;
            };

            let mut ids = sync
                .get(LEAGUE_CLIENT_NAMESPACE, "chat", &format!("participants.{kind}"))
                .and_then(|value| value.as_array().cloned())
                .unwrap_or_default();

            if !ids.iter().any(|existing| existing == summoner_id) {
                ids.push(summoner_id.clone());
                let _ = sync.set(
                    LEAGUE_CLIENT_NAMESPACE,
                    "chat",
                    &format!("participants.{kind}"),
                    Value::Array(ids),
                );
            }
        },
    )?;

    Ok(())
}

fn conversation(sync: &riftflow_state::StateSync, kind: &str) -> Option<String> {
    sync.get(LEAGUE_CLIENT_NAMESPACE, "chat", &format!("conversations.{kind}"))
        .and_then(|value| value.get("id").and_then(Value::as_str).map(str::to_string))
}

/// `loadConversation`: GET the list, then GET each conversation's participants.
async fn hydrate_chat_conversations(state_sync: &Arc<StateSync>, http: &LcuHttp) {
    let Some(conversations) = get_or_none(http, "/lol-chat/v1/conversations").await else {
        return;
    };

    for conversation in conversations.as_array().into_iter().flatten() {
        let Some(kind) = conversation.get("type").and_then(Value::as_str) else {
            continue;
        };
        if !CONVERSATION_KINDS.contains(&kind) {
            continue;
        }
        if kind == "championSelect"
            && !conversation
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("lol-champ-select")
        {
            continue;
        }

        let _ = state_sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            "chat",
            &format!("conversations.{kind}"),
            conversation.clone(),
        );

        let id = conversation.get("id").and_then(Value::as_str).unwrap_or_default();
        let participants = get_or_none(http, &format!("/lol-chat/v1/conversations/{id}/participants")).await;

        let ids: Vec<Value> = participants
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|person| person.get("summonerId").cloned())
            .collect();

        let _ = state_sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            "chat",
            &format!("participants.{kind}"),
            Value::Array(ids),
        );
    }
}

/// A `GET` that distinguishes "answered" from "failed". A 404 here means there is
/// no chat yet, so the caller leaves the props alone rather than storing an error.
async fn get_or_none(http: &LcuHttp, url: &str) -> Option<Value> {
    let config = RequestConfig {
        method: "GET".to_string(),
        url: url.to_string(),
        ..Default::default()
    };

    match http.request(config).await {
        Ok(response) if response.status < 400 => Some(response.body),
        _ => None,
    }
}

fn is_delete(payload: &Value) -> bool {
    payload.get("eventType").and_then(Value::as_str) == Some("Delete")
}

/// `GET` whose failure means "the endpoint is not there" — the pattern every
/// `champSelect` loader in `lc-state` uses, where a 404 clears the prop.
async fn get_or(http: &riftflow_lcu::http::LcuHttp, url: &str, default: Value) -> Value {
    let config = RequestConfig {
        method: "GET".to_string(),
        url: url.to_string(),
        ..Default::default()
    };

    match http.request(config).await {
        Ok(response) if response.status < 400 && !response.body.is_null() => response.body,
        _ => default,
    }
}

/// `_stateInitializer`: read the current values once per connection, so the UI is
/// right immediately instead of waiting for the next event.
pub async fn hydrate(state_sync: &Arc<StateSync>, league_client: &Arc<LeagueClientMain>) {
    let Some(http) = league_client.http() else {
        return;
    };

    for (url, state_id, path) in SYNCED {
        let config = RequestConfig {
            method: "GET".to_string(),
            url: (*url).to_string(),
            ..Default::default()
        };

        match http.request(config).await {
            Ok(response) if response.status < 400 => {
                let _ = state_sync.set(LEAGUE_CLIENT_NAMESPACE, state_id, path, response.body);
            }
            // 404 means the endpoint is not there right now — no lobby, no draft —
            // which is exactly the case the original loaders clear
            Ok(response) if response.status == 404 => {
                let _ = state_sync.set(
                    LEAGUE_CLIENT_NAMESPACE,
                    state_id,
                    path,
                    default_value(state_id, path),
                );
            }
            Ok(response) => {
                if std::env::var("RIFTFLOW_LCU_TRACE").is_ok() {
                    eprintln!("[lc-state] {url} → {}", response.status);
                }
            }
            Err(error) => {
                if std::env::var("RIFTFLOW_LCU_TRACE").is_ok() {
                    eprintln!("[lc-state] {url} not available: {error}");
                }
            }
        }
    }

    for (url, path, field) in GAME_DATA {
        match http.get(url).await {
            Ok(list) => {
                let keyed = key_by(&list, field, *path == "queues");
                let _ = state_sync.set(LEAGUE_CLIENT_NAMESPACE, "gameData", path, keyed);
            }
            Err(error) => {
                if std::env::var("RIFTFLOW_LCU_TRACE").is_ok() {
                    eprintln!("[lc-state] {url} not available: {error}");
                }
            }
        }
    }

    if let Ok(perkstyles) = http.get(PERKSTYLES_URL).await {
        let styles = perkstyles
            .get("styles")
            .map(|styles| key_by(styles, "id", false))
            .unwrap_or_else(|| json!({}));
        let _ = state_sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            "gameData",
            "perkstyles",
            json!({
                "schemaVersion": perkstyles.get("schemaVersion").cloned().unwrap_or(json!(0)),
                "styles": styles,
            }),
        );
    }

    sync_supported_queues(state_sync);
    sync_token_ready(state_sync);
    sync_availability(state_sync, league_client);
    hydrate_champ_select(state_sync, &http).await;
    hydrate_chat_conversations(state_sync, &http).await;
}

/// The `_stateInitializer` loaders of `_syncLcuChampSelect` plus the subset list.
/// These are the inputs the auto-select decisions read, so a stale value from a
/// previous draft is worse than an empty one.
async fn hydrate_champ_select(state_sync: &Arc<StateSync>, http: &LcuHttp) {
    for (url, path) in CHAMP_SELECT_SETS {
        let value = get_or(http, url, json!([])).await;
        let _ = state_sync.set(LEAGUE_CLIENT_NAMESPACE, "champSelect", path, value);
    }

    for (url, path) in CHAMP_SELECT_NULLABLE {
        let value = get_or(http, url, Value::Null).await;
        let _ = state_sync.set(LEAGUE_CLIENT_NAMESPACE, "champSelect", path, value);
    }

    let grid = get_or(http, ALL_GRID_CHAMPIONS_URL, json!([])).await;
    let _ = state_sync.set(
        LEAGUE_CLIENT_NAMESPACE,
        "champSelect",
        "gridChampions",
        key_by(&grid, "id", false),
    );

    let subset = get_or(http, SUBSET_LIST_URL, json!([])).await;
    let _ = state_sync.set(
        LEAGUE_CLIENT_NAMESPACE,
        "lobbyTeamBuilder",
        "champSelect.subsetChampionList",
        subset,
    );
}

/// Mark the props the renderer keeps as a JS `Set`. Call after the surfaces are
/// declared, before anything pushes them.
pub fn mark_set_props(state_sync: &Arc<StateSync>) {
    for (_, path) in CHAMP_SELECT_SETS {
        let _ = state_sync.mark_as_set(LEAGUE_CLIENT_NAMESPACE, "champSelect", path);
    }
}

/// `_onLcuNotConnected`: the sub-states go back to nothing, so a client restart
/// cannot leave the UI showing the previous summoner.
pub fn clear(state_sync: &Arc<StateSync>) {
    for (state_id, path) in SYNCED.iter().map(|(_, state, path)| (*state, *path)) {
        let _ = state_sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            state_id,
            path,
            default_value(state_id, path),
        );
    }

    for (_, path) in CHAMP_SELECT_SETS {
        let _ = state_sync.set(LEAGUE_CLIENT_NAMESPACE, "champSelect", path, json!([]));
    }

    for kind in CONVERSATION_KINDS {
        let _ = state_sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            "chat",
            &format!("conversations.{kind}"),
            Value::Null,
        );
        let _ = state_sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            "chat",
            &format!("participants.{kind}"),
            Value::Null,
        );
    }

    for (_, path) in CHAMP_SELECT_NULLABLE {
        let _ = state_sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            "champSelect",
            path,
            Value::Null,
        );
    }

    let _ = state_sync.set(
        LEAGUE_CLIENT_NAMESPACE,
        "champSelect",
        "gridChampions",
        json!({}),
    );
    let _ = state_sync.set(
        LEAGUE_CLIENT_NAMESPACE,
        "lobbyTeamBuilder",
        "champSelect.subsetChampionList",
        json!([]),
    );

    sync_supported_queues(state_sync);
    sync_token_ready(state_sync);
    sync_availability_unset(state_sync);
}

fn sync_availability_unset(state_sync: &Arc<StateSync>) {
    let _ = state_sync.set(
        SGP_NAMESPACE,
        "state",
        "availability",
        json!({
            "region": "",
            "rsoPlatform": "",
            "sgpServerId": "",
            "serversSupported": { "matchHistory": false, "common": false },
        }),
    );
}

/// What a synced prop holds when there is nothing to read. Every `SYNCED` prop is
/// a nullable object except the phase string and the invitation list — writing
/// `{}` where the renderer expects `null` or `[]` is what made the sidebar crash
/// on `session.timer` in the first place.
fn default_value(state_id: &str, path: &str) -> Value {
    match (state_id, path) {
        ("gameflow", "phase") => json!("None"),
        ("lobby", "receivedInvitations") => json!([]),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tencent_regions_carry_the_platform_id() {
        assert_eq!(sgp_server_id("TENCENT", "HN1"), "TENCENT_HN1");
        assert_eq!(sgp_server_id("tencent", "tj101"), "TENCENT_TJ101");
    }

    #[test]
    fn riot_region_codes_are_aliased() {
        assert_eq!(sgp_server_id("NA", ""), "NA1");
        assert_eq!(sgp_server_id("OCE", ""), "OC1");
        assert_eq!(sgp_server_id("EUW1", ""), "EUW");
        assert_eq!(sgp_server_id("BR1", ""), "BR1");
        assert_eq!(sgp_server_id("", "HN1"), "");
    }

    #[test]
    fn the_synced_props_clear_to_their_declared_shape() {
        assert_eq!(default_value("gameflow", "phase"), json!("None"));
        assert_eq!(default_value("lobby", "receivedInvitations"), json!([]));

        for (_, state_id, path) in SYNCED {
            let value = default_value(state_id, path);
            assert!(
                value.is_null() || value == json!("None") || value.is_array(),
                "{state_id}.{path} clears to {value}, which is not its declared shape"
            );
        }
    }

    #[test]
    fn supported_queues_follow_the_fetched_doc_and_prepend_the_live_queue() {
        use riftflow_ipc::{RiftFlowIpcMain, RendererId, RendererTransport};

        struct Quiet;
        impl RendererTransport for Quiet {
            fn send_to(
                &self,
                _renderer: RendererId,
                _event: &str,
                _args: Vec<Value>,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let ipc = Arc::new(RiftFlowIpcMain::new(Arc::new(Quiet)));
        let sync = Arc::new(StateSync::new(ipc));
        sync.prop_sync(SGP_NAMESPACE, "state", &[("supportedQueues", json!([]))], false)
            .unwrap();
        sync.prop_sync(
            LEAGUE_CLIENT_NAMESPACE,
            "gameData",
            &[("queues", json!({}))],
            false,
        )
        .unwrap();
        sync.prop_sync(
            LEAGUE_CLIENT_NAMESPACE,
            "gameflow",
            &[("session", Value::Null)],
            false,
        )
        .unwrap();

        // no document yet: the builtin list is what the renderer sees
        sync_supported_queues(&sync);
        let builtin = BUILTIN
            .get("BUILTIN_SUPPORTED_QUEUES")
            .and_then(|value| value.get("queues"))
            .cloned()
            .unwrap();
        assert_eq!(sync.get(SGP_NAMESPACE, "state", "supportedQueues"), Some(builtin));

        // the fetched document replaces it while no game is running
        set_fetched_supported_queues(json!([420, 440]));
        sync_supported_queues(&sync);
        assert_eq!(
            sync.get(SGP_NAMESPACE, "state", "supportedQueues"),
            Some(json!([420, 440]))
        );

        // a game in a queue the document does not list, known to game-data, is put
        // in front instead of dropped
        sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            "gameData",
            "queues",
            json!({ "1700": { "id": 1700 } }),
        )
        .unwrap();
        sync.set(
            LEAGUE_CLIENT_NAMESPACE,
            "gameflow",
            "session",
            json!({ "gameData": { "queue": { "id": 1700 } } }),
        )
        .unwrap();
        sync_supported_queues(&sync);
        assert_eq!(
            sync.get(SGP_NAMESPACE, "state", "supportedQueues"),
            Some(json!([1700, 420, 440]))
        );

        set_fetched_supported_queues(Value::Null);
    }
}
