//! `state.ts` `OngoingGameState` — the record maps the loaders fill.
//!
//! Only the *loading state* records and the derived props are propSync'd in the
//! original; the seven data maps are not, and reach the renderer through `getAll`
//! plus one `<namespace>/<event>` push per item. That split is kept here: a write
//! goes to the map, then either a prop push (`StateSync::set`) or an event
//! (`RiftFlowIpcMain::send_event`) follows.
//!
//! Every ordered map here is a `Vec<(String, T)>` so a puuid's position survives a
//! replacement, the way a JS object keeps the key where it was inserted.

use std::sync::{Arc, Mutex};

use riftflow_ipc::RiftFlowIpcMain;
use riftflow_state::StateSync;
use serde_json::{Map, Value, json};

use crate::computed::Additional;
use crate::fetch::{ApiSource, MatchHistoryQueryParams};
use crate::record::{self, Pairs};
use crate::ONGOING_GAME_MAIN_NAMESPACE as NS;

pub const STATE_ID: &str = "state";

/// One entry of `state.matchHistory`.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchHistoryEntry {
    pub source: ApiSource,
    pub params: MatchHistoryQueryParams,
    pub data: Vec<Value>,
}

impl MatchHistoryEntry {
    /// the whole `params` object is stored, because `sameSgpQuery`/`sameLcuQuery`
    /// compare `startIndex` and `count` against it on the next pass
    pub fn to_value(&self) -> Value {
        json!({
            "source": self.source.as_str(),
            "params": {
                "startIndex": self.params.start_index,
                "count": self.params.count,
                "tag": self.params.tag,
                "tagsQueryType": self.params.tags_query_type,
            },
            "data": self.data,
        })
    }
}

/// `Pick<MatchHistoryQueryParams, 'tag' | 'tagsQueryType'>` off the synced prop.
pub fn tag_params_from_value(value: &Value) -> MatchHistoryQueryParams {
    MatchHistoryQueryParams {
        tag: value.get("tag").and_then(Value::as_str).map(str::to_string),
        tags_query_type: value
            .get("tagsQueryType")
            .and_then(Value::as_str)
            .map(str::to_string),
        ..Default::default()
    }
}

/// One wrapped game: `{ source, gameId, data }`, the `LcuOrSgpGameSummary` wrapper.
pub fn wrap_game(source: ApiSource, game_id: i64, data: Value) -> Value {
    json!({ "source": source.as_str(), "gameId": game_id, "data": data })
}

#[derive(Default)]
struct Records {
    analysis: Option<Value>,
    match_history: Pairs<Value>,
    match_history_loading: Pairs<String>,
    summoner: Pairs<Value>,
    summoner_loading: Pairs<String>,
    ranked_stats: Pairs<Value>,
    ranked_loading: Pairs<String>,
    champion_mastery: Pairs<Value>,
    champion_mastery_loading: Pairs<String>,
    saved_info: Pairs<Value>,
    saved_info_loading: Pairs<String>,
    game_details: Pairs<Value>,
    additional_game: Pairs<Value>,
    inferred_premade_teams: Vec<Vec<String>>,
    additional: Option<Value>,
    draft: Option<Value>,
    match_history_tag_params: Option<Value>,
    /// `champSelectHandoffSnapshot` — state only, never propSync'd
    handoff_snapshot: Option<Value>,
}

pub struct Store {
    records: Mutex<Records>,
    state_sync: Arc<StateSync>,
    ipc: Arc<RiftFlowIpcMain>,
}

impl Store {
    pub fn new(state_sync: Arc<StateSync>, ipc: Arc<RiftFlowIpcMain>) -> Arc<Self> {
        Arc::new(Self { records: Mutex::new(Records::default()), state_sync, ipc })
    }

    fn set_prop(&self, path: &str, value: Value) {
        let _ = self.state_sync.set(NS, STATE_ID, path, value);
    }

    fn send(&self, event: &str, args: &[Value]) {
        self.ipc.send_event(NS, event, args);
    }

    pub fn draft(&self) -> Option<Value> {
        self.records.lock().unwrap().draft.clone()
    }

    pub fn handoff_snapshot(&self) -> Option<Value> {
        self.records.lock().unwrap().handoff_snapshot.clone()
    }

    pub fn set_handoff_snapshot(&self, snapshot: Option<Value>) {
        self.records.lock().unwrap().handoff_snapshot = snapshot;
    }

    pub fn set_draft(&self, draft: Option<Value>) {
        {
            let mut records = self.records.lock().unwrap();
            records.draft.clone_from(&draft);
        }
        self.set_prop("draft", draft.unwrap_or(Value::Null));
    }

    pub fn additional(&self) -> Additional {
        Additional::from_value(&self.additional_value())
    }

    pub fn additional_value(&self) -> Value {
        self.records
            .lock()
            .unwrap()
            .additional
            .clone()
            .unwrap_or_else(empty_additional)
    }

    /// `setAdditional` — plus the second `propSync('ongoing-game-main', 'additional', …)`
    /// the original registered so a renderer can subscribe to `additional` alone.
    pub fn set_additional(&self, additional: &Additional) {
        let value = additional.to_value();
        self.records.lock().unwrap().additional = Some(value.clone());
        self.publish_additional(value);
    }

    /// `clearAdditional`
    pub fn clear_additional(&self) {
        self.records.lock().unwrap().additional = None;
        self.publish_additional(empty_additional());
    }

    fn publish_additional(&self, value: Value) {
        self.set_prop("additional", value.clone());
        let _ = self.state_sync.set(NS, "additional", "additional", value);
    }

    pub fn match_history_tag_params(&self) -> MatchHistoryQueryParams {
        let stored = self.records.lock().unwrap().match_history_tag_params.clone();
        tag_params_from_value(stored.as_ref().unwrap_or(&Value::Null))
    }

    pub fn set_match_history_tag_params(&self, params: &MatchHistoryQueryParams) {
        let value = params.to_json();
        self.records.lock().unwrap().match_history_tag_params = Some(value.clone());
        self.set_prop("matchHistoryTagParams", value);
    }

    pub fn analysis(&self) -> Option<Value> {
        self.records.lock().unwrap().analysis.clone()
    }

    pub fn set_analysis(&self, analysis: Option<Value>) {
        {
            let mut records = self.records.lock().unwrap();
            records.analysis.clone_from(&analysis);
        }
        self.set_prop("analysis", analysis.unwrap_or(Value::Null));
    }

    pub fn inferred_premade_teams(&self) -> Vec<Vec<String>> {
        self.records.lock().unwrap().inferred_premade_teams.clone()
    }

    pub fn set_inferred_premade_teams(&self, teams: Vec<Vec<String>>) {
        {
            let mut records = self.records.lock().unwrap();
            records.inferred_premade_teams = teams.clone();
        }
        self.set_prop("inferredPremadeTeams", json!(teams));
    }

    pub fn match_history_records(&self) -> Pairs<Value> {
        self.records.lock().unwrap().match_history.clone()
    }

    pub fn match_history_for(&self, puuid: &str) -> Option<Value> {
        self.records
            .lock()
            .unwrap()
            .match_history
            .iter()
            .find(|(existing, _)| existing == puuid)
            .map(|(_, entry)| entry.clone())
    }

    pub fn game_details(&self) -> Pairs<Value> {
        self.records.lock().unwrap().game_details.clone()
    }

    pub fn game_details_for(&self, game_id: i64) -> Option<Value> {
        lookup(&self.records.lock().unwrap().game_details, &game_id.to_string())
    }

    pub fn additional_game(&self, game_id: i64) -> Option<Value> {
        lookup(&self.records.lock().unwrap().additional_game, &game_id.to_string())
    }

    pub fn set_match_history(&self, puuid: &str, entry: &MatchHistoryEntry) {
        let value = entry.to_value();
        upsert(&mut self.records.lock().unwrap().match_history, puuid.to_string(), value.clone());
        self.send("match-history-loaded", &[json!(puuid), value]);
    }

    /// `updateMatchHistories`'s prune: whoever left the teams list loses their record.
    pub fn retain_match_history(&self, puuids: &[String]) -> Vec<String> {
        let loading = {
            let mut records = self.records.lock().unwrap();
            let removed: Vec<String> = missing(&records.match_history, puuids);
            for puuid in &removed {
                remove(&mut records.match_history, puuid);
                remove(&mut records.match_history_loading, puuid);
            }
            (removed, records.match_history_loading.clone())
        };

        for puuid in &loading.0 {
            self.send("match-history-removed", &[json!(puuid)]);
        }
        self.set_prop("matchHistoryLoadingState", record::object(&loading.1));
        loading.0
    }

    pub fn set_match_history_loading(&self, puuid: &str, state: &str) {
        self.write_loading(puuid, state, LoadingKind::MatchHistory);
    }

    pub fn set_summoner_loading(&self, puuid: &str, state: &str) {
        self.write_loading(puuid, state, LoadingKind::Summoner);
    }

    pub fn set_ranked_stats_loading(&self, puuid: &str, state: &str) {
        self.write_loading(puuid, state, LoadingKind::RankedStats);
    }

    pub fn set_saved_info_loading(&self, puuid: &str, state: &str) {
        self.write_loading(puuid, state, LoadingKind::SavedInfo);
    }

    pub fn set_champion_mastery_loading(&self, puuid: &str, state: &str) {
        self.write_loading(puuid, state, LoadingKind::ChampionMastery);
    }

    fn write_loading(&self, puuid: &str, state: &str, kind: LoadingKind) {
        let (prop, snapshot) = {
            let mut records = self.records.lock().unwrap();
            let map = loading_map(&mut records, kind);
            upsert(map, puuid.to_string(), state.to_string());
            (kind.prop(), map.clone())
        };

        self.set_prop(prop, record::object(&snapshot));
    }

    pub fn summoner_for(&self, puuid: &str) -> Option<Value> {
        lookup(&self.records.lock().unwrap().summoner, puuid)
    }

    pub fn ranked_stats_for(&self, puuid: &str) -> Option<Value> {
        lookup(&self.records.lock().unwrap().ranked_stats, puuid)
    }

    pub fn saved_info_for(&self, puuid: &str) -> Option<Value> {
        lookup(&self.records.lock().unwrap().saved_info, puuid)
    }

    pub fn champion_mastery_for(&self, puuid: &str) -> Option<Value> {
        lookup(&self.records.lock().unwrap().champion_mastery, puuid)
    }

    pub fn set_summoner(&self, puuid: &str, value: Value) {
        upsert(&mut self.records.lock().unwrap().summoner, puuid.to_string(), value.clone());
        self.send("summoner-loaded", &[json!(puuid), value]);
    }

    pub fn retain_summoner(&self, puuids: &[String]) -> Vec<String> {
        let (removed, loading) = {
            let mut records = self.records.lock().unwrap();
            let removed = missing(&records.summoner, puuids);
            for puuid in &removed {
                remove(&mut records.summoner, puuid);
                remove(&mut records.summoner_loading, puuid);
            }
            (removed, records.summoner_loading.clone())
        };

        for puuid in &removed {
            self.send("summoner-removed", &[json!(puuid)]);
        }
        self.set_prop("summonerLoadingState", record::object(&loading));
        removed
    }

    pub fn set_ranked_stats(&self, puuid: &str, value: Value) {
        upsert(&mut self.records.lock().unwrap().ranked_stats, puuid.to_string(), value.clone());
        self.send("ranked-stats-loaded", &[json!(puuid), value]);
    }

    pub fn retain_ranked_stats(&self, puuids: &[String]) -> Vec<String> {
        let (removed, loading) = {
            let mut records = self.records.lock().unwrap();
            let removed = missing(&records.ranked_stats, puuids);
            for puuid in &removed {
                remove(&mut records.ranked_stats, puuid);
                remove(&mut records.ranked_loading, puuid);
            }
            (removed, records.ranked_loading.clone())
        };

        for puuid in &removed {
            self.send("ranked-stats-removed", &[json!(puuid)]);
        }
        self.set_prop("rankedStatsLoadingState", record::object(&loading));
        removed
    }

    pub fn set_champion_mastery(&self, puuid: &str, value: Value) {
        upsert(&mut self.records.lock().unwrap().champion_mastery, puuid.to_string(), value.clone());
        self.send("champion-mastery-loaded", &[json!(puuid), value]);
    }

    pub fn retain_champion_mastery(&self, puuids: &[String]) -> Vec<String> {
        let (removed, loading) = {
            let mut records = self.records.lock().unwrap();
            let removed = missing(&records.champion_mastery, puuids);
            for puuid in &removed {
                remove(&mut records.champion_mastery, puuid);
                remove(&mut records.champion_mastery_loading, puuid);
            }
            (removed, records.champion_mastery_loading.clone())
        };

        for puuid in &removed {
            self.send("champion-mastery-removed", &[json!(puuid)]);
        }
        self.set_prop("championMasteryLoadingState", record::object(&loading));
        removed
    }

    pub fn set_saved_info(&self, puuid: &str, value: Value) {
        upsert(&mut self.records.lock().unwrap().saved_info, puuid.to_string(), value.clone());
        self.send("saved-info-loaded", &[json!(puuid), value]);
    }

    /// `savedInfo` is the one map whose prune does not touch a loading state.
    pub fn retain_saved_info(&self, puuids: &[String]) -> Vec<String> {
        let removed = {
            let mut records = self.records.lock().unwrap();
            let removed = missing(&records.saved_info, puuids);
            for puuid in &removed {
                remove(&mut records.saved_info, puuid);
            }
            removed
        };

        for puuid in &removed {
            self.send("saved-info-removed", &[json!(puuid)]);
        }
        removed
    }

    pub fn set_game_details(&self, game_id: i64, value: Value) {
        upsert(&mut self.records.lock().unwrap().game_details, game_id.to_string(), value.clone());
        self.send("game-details-loaded", &[json!(game_id), value]);
    }

    pub fn remove_game_details(&self, game_id: i64) {
        let removed = remove(&mut self.records.lock().unwrap().game_details, &game_id.to_string());
        if removed.is_some() {
            self.send("game-details-removed", &[json!(game_id)]);
        }
    }

    pub fn set_additional_game(&self, game_id: i64, value: Value) {
        upsert(&mut self.records.lock().unwrap().additional_game, game_id.to_string(), value.clone());
        self.send("additional-game-loaded", &[json!(game_id), value]);
    }

    /// `state.clear(options)` — everything except what the caller keeps.
    pub fn clear(&self, keep_tag_params: bool, keep_additional_info: bool) {
        {
            let mut records = self.records.lock().unwrap();
            records.analysis = None;
            records.match_history = Pairs::new();
            records.summoner = Pairs::new();
            records.saved_info = Pairs::new();
            records.ranked_stats = Pairs::new();
            records.champion_mastery = Pairs::new();
            records.game_details = Pairs::new();
            records.additional_game = Pairs::new();
            records.inferred_premade_teams = Vec::new();
            records.match_history_loading = Pairs::new();
            records.summoner_loading = Pairs::new();
            records.saved_info_loading = Pairs::new();
            records.ranked_loading = Pairs::new();
            records.champion_mastery_loading = Pairs::new();
        }

        self.set_prop("analysis", Value::Null);
        self.set_prop("inferredPremadeTeams", json!([]));
        self.set_prop("matchHistoryLoadingState", json!({}));
        self.set_prop("summonerLoadingState", json!({}));
        self.set_prop("savedInfoLoadingState", json!({}));
        self.set_prop("rankedStatsLoadingState", json!({}));
        self.set_prop("championMasteryLoadingState", json!({}));

        if !keep_additional_info {
            self.clear_additional();
        }

        if !keep_tag_params {
            self.records.lock().unwrap().match_history_tag_params = None;
            self.set_prop("matchHistoryTagParams", json!({}));
        }
    }

    /// `getAll` — the seven maps that are not propSync'd.
    pub fn get_all(&self) -> Value {
        let records = self.records.lock().unwrap();

        json!({
            "matchHistory": record::object(&records.match_history),
            "summoner": record::object(&records.summoner),
            "rankedStats": record::object(&records.ranked_stats),
            "savedInfo": record::object(&records.saved_info),
            "championMastery": record::object(&records.champion_mastery),
            "gameDetails": record::object(&records.game_details),
            "additionalGames": record::object(&records.additional_game),
        })
    }
}

#[derive(Clone, Copy)]
enum LoadingKind {
    MatchHistory,
    Summoner,
    RankedStats,
    SavedInfo,
    ChampionMastery,
}

impl LoadingKind {
    fn prop(self) -> &'static str {
        match self {
            LoadingKind::MatchHistory => "matchHistoryLoadingState",
            LoadingKind::Summoner => "summonerLoadingState",
            LoadingKind::RankedStats => "rankedStatsLoadingState",
            LoadingKind::SavedInfo => "savedInfoLoadingState",
            LoadingKind::ChampionMastery => "championMasteryLoadingState",
        }
    }
}

fn loading_map<'a>(records: &'a mut Records, kind: LoadingKind) -> &'a mut Pairs<String> {
    match kind {
        LoadingKind::MatchHistory => &mut records.match_history_loading,
        LoadingKind::Summoner => &mut records.summoner_loading,
        LoadingKind::RankedStats => &mut records.ranked_loading,
        LoadingKind::SavedInfo => &mut records.saved_info_loading,
        LoadingKind::ChampionMastery => &mut records.champion_mastery_loading,
    }
}

fn lookup(pairs: &Pairs<Value>, key: &str) -> Option<Value> {
    pairs.iter().find(|(existing, _)| existing == key).map(|(_, value)| value.clone())
}

fn missing(pairs: &Pairs<Value>, keep: &[String]) -> Vec<String> {
    pairs
        .iter()
        .map(|(key, _)| key.clone())
        .filter(|key| !keep.iter().any(|kept| kept == key))
        .collect()
}

fn upsert<T: Clone>(pairs: &mut Pairs<T>, key: String, value: T) {
    match pairs.iter_mut().find(|(existing, _)| *existing == key) {
        Some((_, existing_value)) => *existing_value = value,
        None => pairs.push((key, value)),
    }
}

fn remove<T>(pairs: &mut Pairs<T>, key: &str) -> Option<T> {
    let index = pairs.iter().position(|(existing, _)| existing == key)?;
    Some(pairs.remove(index).1)
}

/// `clearAdditional`'s shape, which is also the initial value of the prop.
pub fn empty_additional() -> Value {
    let mut map = Map::new();
    for key in ["teams", "selections", "teamParticipantGroups", "spells", "positions"] {
        map.insert(key.to_string(), json!({}));
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use riftflow_ipc::RendererTransport;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct Collector {
        events: StdMutex<Vec<Vec<Value>>>,
    }

    impl RendererTransport for Collector {
        fn send_to(&self, _renderer: riftflow_ipc::RendererId, _event: &str, args: Vec<Value>) -> anyhow::Result<()> {
            self.events.lock().unwrap().push(args);
            Ok(())
        }
    }

    /// every pushed `riftflow-event` payload, as `(eventName, ...args)`
    fn events(collector: &Arc<Collector>) -> Vec<(String, Vec<Value>)> {
        collector
            .events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|payload| {
                let name = payload.get(1)?.as_str()?.to_string();
                Some((name, payload[2..].to_vec()))
            })
            .collect()
    }

    fn store() -> (Arc<Store>, Arc<Collector>, Arc<StateSync>) {
        let collector = Arc::new(Collector::default());
        let ipc = Arc::new(RiftFlowIpcMain::new(collector.clone()));
        let state_sync = Arc::new(StateSync::new(ipc.clone()));

        // events only reach registered renderers, exactly as the router insists
        ipc.renderer_register(1, None);

        state_sync
            .prop_sync(
                NS,
                STATE_ID,
                &[
                    ("analysis", Value::Null),
                    ("draft", Value::Null),
                    ("additional", empty_additional()),
                    ("matchHistoryTagParams", json!({})),
                    ("inferredPremadeTeams", json!([])),
                    ("matchHistoryLoadingState", json!({})),
                    ("summonerLoadingState", json!({})),
                    ("rankedStatsLoadingState", json!({})),
                    ("savedInfoLoadingState", json!({})),
                    ("championMasteryLoadingState", json!({})),
                ],
                false,
            )
            .unwrap();

        (Store::new(state_sync.clone(), ipc), collector, state_sync)
    }

    #[test]
    fn a_replaced_puuid_keeps_its_place_in_the_map() {
        let (store, _, sync) = store();

        store.set_match_history("a", &entry("a", 1));
        store.set_match_history("b", &entry("b", 2));
        store.set_match_history("a", &entry("a", 3));

        let keys: Vec<String> = store.match_history_records().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(store.match_history_for("a").unwrap()["data"][0]["gameId"], json!(3));
        assert_eq!(sync.get(NS, STATE_ID, "analysis"), Some(Value::Null));
    }

    #[test]
    fn loading_states_are_pushed_as_whole_records() {
        let (store, _, sync) = store();

        store.set_match_history_loading("a", "loading");
        store.set_match_history_loading("b", "loaded");

        assert_eq!(
            sync.get(NS, STATE_ID, "matchHistoryLoadingState"),
            Some(json!({ "a": "loading", "b": "loaded" }))
        );
    }

    #[test]
    fn a_departed_player_loses_their_record_and_their_loading_state() {
        let (store, collector, sync) = store();

        store.set_match_history("a", &entry("a", 1));
        store.set_match_history("b", &entry("b", 2));
        store.set_match_history_loading("b", "loaded");
        collector.events.lock().unwrap().clear();

        let removed = store.retain_match_history(&["a".to_string()]);

        assert_eq!(removed, vec!["b".to_string()]);
        assert!(store.match_history_for("b").is_none());
        assert_eq!(sync.get(NS, STATE_ID, "matchHistoryLoadingState"), Some(json!({})));
        assert_eq!(
            events(&collector),
            vec![("match-history-removed".to_string(), vec![json!("b")])]
        );
    }

    #[test]
    fn a_loaded_record_is_pushed_with_its_payload() {
        let (store, collector, _) = store();

        store.set_summoner("a", json!({ "puuid": "a", "gameName": "NAME" }));
        store.set_game_details(7, wrap_game(ApiSource::Lcu, 7, json!({ "gameId": 7 })));
        store.set_additional_game(8, wrap_game(ApiSource::Sgp, 8, json!({ "gameId": 8 })));

        let pushed = events(&collector);
        assert_eq!(pushed[0].0, "summoner-loaded");
        assert_eq!(pushed[0].1, vec![json!("a"), json!({ "puuid": "a", "gameName": "NAME" })]);
        assert_eq!(pushed[1].0, "game-details-loaded");
        assert_eq!(pushed[2].0, "additional-game-loaded");

        collector.events.lock().unwrap().clear();
        store.remove_game_details(7);
        assert_eq!(events(&collector), vec![("game-details-removed".to_string(), vec![json!(7)])]);

        collector.events.lock().unwrap().clear();
        store.remove_game_details(404);
        assert!(events(&collector).is_empty(), "removing what is not there says nothing");
    }

    #[test]
    fn saved_info_pruning_does_not_touch_a_loading_state() {
        let (store, _, sync) = store();

        store.set_saved_info_loading("a", "loading");
        store.set_saved_info("a", json!({ "puuid": "a" }));
        store.retain_saved_info(&[]);

        assert_eq!(
            sync.get(NS, STATE_ID, "savedInfoLoadingState"),
            Some(json!({ "a": "loading" })),
            "the original only deletes the record and the loading state"
        );
    }

    #[test]
    fn clear_resets_everything_it_is_not_told_to_keep() {
        let (store, _, sync) = store();

        store.set_match_history("a", &entry("a", 1));
        store.set_summoner("a", json!({ "puuid": "a" }));
        store.set_game_details(7, wrap_game(ApiSource::Lcu, 7, json!({ "x": 1 })));
        store.set_analysis(Some(json!({ "players": {} })));
        store.set_inferred_premade_teams(vec![vec!["a".to_string()]]);
        store.set_summoner_loading("a", "loaded");
        store.set_match_history_tag_params(&MatchHistoryQueryParams::with_tag(0, 20, Some("q_420".to_string())));
        store.set_additional(&Additional::default());

        store.clear(true, true);

        assert_eq!(store.get_all(), json!({
            "matchHistory": {}, "summoner": {}, "rankedStats": {}, "savedInfo": {},
            "championMastery": {}, "gameDetails": {}, "additionalGames": {},
        }));
        assert_eq!(sync.get(NS, STATE_ID, "analysis"), Some(Value::Null));
        assert_eq!(sync.get(NS, STATE_ID, "inferredPremadeTeams"), Some(json!([])));
        assert_eq!(sync.get(NS, STATE_ID, "matchHistoryTagParams"), Some(json!({ "tag": "q_420", "tagsQueryType": null })));
        assert!(store.additional_value().is_object());

        store.clear(false, false);
        assert_eq!(sync.get(NS, STATE_ID, "matchHistoryTagParams"), Some(json!({})));
        assert_eq!(store.additional(), Additional::default());
    }

    #[test]
    fn the_draft_and_tag_params_round_trip_through_their_props() {
        let (store, _, sync) = store();

        store.set_draft(Some(json!({ "queueId": 420 })));
        assert_eq!(sync.get(NS, STATE_ID, "draft"), Some(json!({ "queueId": 420 })));
        assert_eq!(store.draft(), Some(json!({ "queueId": 420 })));

        store.set_draft(None);
        assert_eq!(sync.get(NS, STATE_ID, "draft"), Some(Value::Null));

        store.set_match_history_tag_params(&MatchHistoryQueryParams { tag: Some("q_420".to_string()), ..Default::default() });
        assert_eq!(store.match_history_tag_params().tag.as_deref(), Some("q_420"));
    }

    #[test]
    fn an_unwritten_map_reads_as_empty_rather_than_missing() {
        let (store, _, _) = store();
        assert_eq!(store.get_all(), json!({
            "matchHistory": {}, "summoner": {}, "rankedStats": {}, "savedInfo": {},
            "championMastery": {}, "gameDetails": {}, "additionalGames": {},
        }));
        assert!(store.game_details_for(1).is_none());
        assert!(store.additional_game(1).is_none());
    }

    fn entry(puuid: &str, game_id: i64) -> MatchHistoryEntry {
        MatchHistoryEntry {
            source: ApiSource::Lcu,
            params: MatchHistoryQueryParams::with_tag(0, 20, None),
            data: vec![wrap_game(ApiSource::Lcu, game_id, json!({ "gameId": game_id, "participants": [puuid] }))],
        }
    }
}
