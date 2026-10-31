//! `index.ts` plus the `watch()` halves of every controller: one task, one pass at
//! a time, and one [`Reaction`] per MobX reaction.
//!
//! A `reaction(expr, body, { delay, equals, fireImmediately })` becomes a
//! [`Reaction`]: the expression is compared structurally to what was last seen, and
//! a changed expression arms a `delay` deadline instead of running at once — so a
//! burst of LCU events is still one load, and the first pass runs immediately like
//! `fireImmediately: true` did.
//!
//! The bodies never block the loop: each one spawns, exactly as the original called
//! its loaders without awaiting.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use riftflow_ipc::{RiftFlowIpcMain, IpcError, handler};
use riftflow_state::StateSync;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::analysis::{self, Analyzer};
use crate::champ_select::{Deobfuscate, HandoffSnapshot};
use crate::computed::{self, QueryStage};
use crate::ctx::{Ctx, SavedPlayer};
use crate::feature_gates::{self, FeatureGates};
use crate::fetch::{ApiSource, Fetcher, MatchHistoryQueryParams};
use crate::priority;
use crate::queue::TagMode;
use crate::record::{self, Pairs};
use crate::settings::{OngoingGameSettings, TagPreference};
use crate::snapshot::{LcSnapshot, SGP_NAMESPACE};
use crate::store::{Store, tag_params_from_value};
use crate::ONGOING_GAME_MAIN_NAMESPACE as NS;

const STATE_ID: &str = "state";
const SETTINGS_ID: &str = "settings";
const MOBX_DELAY: Duration = Duration::from_millis(300);

/// `settingService._getFromStorage` / `_saveToStorage` / `_removeFromStorage` for
/// the one key this shard persists, so the crate stays out of the database layer.
pub trait HandoffStorage: Send + Sync {
    fn get(&self) -> Option<Value>;
    fn save(&self, snapshot: &Value);
    fn remove(&self);
}

pub struct OngoingGameDeps {
    pub state_sync: Arc<StateSync>,
    pub ipc: Arc<RiftFlowIpcMain>,
    pub fetcher: Arc<dyn Fetcher>,
    pub saved_player: Option<Arc<dyn SavedPlayer>>,
    pub deobfuscator: Arc<dyn Deobfuscate>,
    pub storage: Option<Arc<dyn HandoffStorage>>,
    /// `SharedGlobalShard.global.platform` and `version`: the gate evaluator's inputs
    pub platform: String,
    pub version: String,
    pub analyzer: Option<Arc<dyn Analyzer>>,
}

/// one `reaction(expr, body, { delay, equals, fireImmediately })`
struct Reaction {
    seen: Option<Value>,
    due: Option<Instant>,
    delay: Duration,
}

impl Reaction {
    fn new(delay: Duration) -> Self {
        // an armed deadline from the start: `fireImmediately` means the first
        // evaluation runs, it does not mean it waits for a change first
        Self { seen: None, due: Some(Instant::now()), delay }
    }

    /// `true` when the body should run now
    fn tick(&mut self, inputs: Value, now: Instant) -> bool {
        if self.seen.as_ref() != Some(&inputs) {
            self.seen = Some(inputs);
            self.due = Some(now + self.delay);
        }

        match self.due {
            Some(at) if now >= at => {
                self.due = None;
                true
            }
            _ => false,
        }
    }

    fn next_due(&self) -> Option<Instant> {
        self.due
    }
}

struct Reactions {
    tag_params: Reaction,
    history: Reaction,
    player_data: Reaction,
    details: Reaction,
    additional: Reaction,
    analysis: Reaction,
    inference: Reaction,
    handoff: Reaction,
    invalidation: Reaction,
    concurrency: i64,
    was_unavailable: bool,
    history_game_id: Option<i64>,
}

impl Default for Reactions {
    fn default() -> Self {
        Self {
            tag_params: Reaction::new(Duration::ZERO),
            history: Reaction::new(MOBX_DELAY),
            player_data: Reaction::new(MOBX_DELAY),
            details: Reaction::new(MOBX_DELAY),
            additional: Reaction::new(MOBX_DELAY),
            analysis: Reaction::new(MOBX_DELAY),
            inference: Reaction::new(MOBX_DELAY),
            handoff: Reaction::new(MOBX_DELAY),
            invalidation: Reaction::new(Duration::ZERO),
            concurrency: -1,
            was_unavailable: false,
            history_game_id: None,
        }
    }
}

pub struct OngoingGame {
    ctx: Arc<Ctx>,
    deobfuscator: Arc<dyn Deobfuscate>,
    analyzer: Option<Arc<dyn Analyzer>>,
    storage: Option<Arc<dyn HandoffStorage>>,
    platform: String,
    version: String,
    wake_tx: mpsc::UnboundedSender<()>,
    wake_rx: Mutex<Option<mpsc::UnboundedReceiver<()>>>,
    reactions: Mutex<Reactions>,
}

impl OngoingGame {
    pub fn new(deps: OngoingGameDeps) -> Arc<Self> {
        let store = Store::new(deps.state_sync.clone(), deps.ipc.clone());
        let ctx = Ctx::new(
            store,
            Arc::new(crate::queue::QueueKeeper::new(&["match-history", "misc"])),
            deps.fetcher,
            deps.saved_player,
            deps.state_sync.clone(),
            deps.ipc.clone(),
        );
        let (wake_tx, wake_rx) = mpsc::unbounded_channel();

        Arc::new(Self {
            ctx,
            deobfuscator: deps.deobfuscator,
            analyzer: deps.analyzer,
            storage: deps.storage,
            platform: deps.platform,
            version: deps.version,
            wake_tx,
            wake_rx: Mutex::new(Some(wake_rx)),
            reactions: Mutex::new(Reactions::default()),
        })
    }

    /// the second `propSync(ongoing-game-main, 'additional', state, ['additional'])`
    /// the original registered so a renderer can subscribe to that prop alone.
    pub fn declare_extra_props(&self) -> anyhow::Result<()> {
        self.ctx.state_sync.prop_sync(
            NS,
            "additional",
            &[("additional", crate::store::empty_additional())],
            false,
        )
    }

    pub fn ctx(&self) -> &Arc<Ctx> {
        &self.ctx
    }

    /// something one of the reactions reads has changed
    pub fn wake(&self) {
        let _ = self.wake_tx.send(());
    }

    /// `onInit` minus `_setupState`: the six IPC methods.
    pub fn register_ipc(self: &Arc<Self>) -> anyhow::Result<()> {
        let store = self.ctx.store.clone();
        self.ctx.ipc.on_call(NS, "getAll", handler(move |_ctx, _args| {
            let store = store.clone();
            async move { Ok(store.get_all()) }
        }))?;

        let this = self.clone();
        self.ctx.ipc.on_call(NS, "setDraft", handler(move |_ctx, args| {
            let this = this.clone();
            async move {
                let draft = args.first().cloned().filter(|value| !value.is_null());
                this.ctx.store.set_draft(draft);
                this.wake();
                Ok(Value::Null)
            }
        }))?;

        let this = self.clone();
        self.ctx.ipc.on_call(NS, "clearDraft", handler(move |_ctx, _args| {
            let this = this.clone();
            async move {
                this.ctx.store.set_draft(None);
                this.wake();
                Ok(Value::Null)
            }
        }))?;

        let this = self.clone();
        self.ctx.ipc.on_call(NS, "setMatchHistoryTagParams", handler(move |_ctx, args| {
            let this = this.clone();
            async move {
                let params = args.first().cloned().unwrap_or(Value::Null);
                this.ctx.store.set_match_history_tag_params(&tag_params_from_value(&params));
                this.wake();
                Ok(Value::Null)
            }
        }))?;

        let this = self.clone();
        self.ctx.ipc.on_call(NS, "reload", handler(move |_ctx, _args| {
            let this = this.clone();
            async move {
                this.clear_and_reload_all();
                Ok(Value::Null)
            }
        }))?;

        let this = self.clone();
        self.ctx.ipc.on_call(NS, "reloadPlayer", handler(move |_ctx, args| {
            let this = this.clone();
            async move {
                let Some(puuid) = args.first().and_then(Value::as_str) else {
                    return Err(IpcError::plain("reloadPlayer needs a puuid"));
                };
                let options = args.get(1).cloned().unwrap_or(Value::Null);
                this.reload_player(puuid, &options);
                Ok(Value::Null)
            }
        }))?;

        Ok(())
    }

    /// `_clearAndReloadAll`
    pub fn clear_and_reload_all(&self) {
        self.ctx.store.clear(true, true);
        self.ctx.ipc.send_event(NS, "clear", &[]);

        let (snapshot, settings) = self.read_inputs();
        let derived = self.derive(&snapshot, &settings);
        let puuids = derived.puuids.clone();
        let params = history_params(settings.match_history_load_count, &self.ctx.store.match_history_tag_params());
        let api_source = snapshot.api_should_use();
        let ctx = self.ctx.clone();

        crate::player_data::update_summoners(&ctx, &puuids, true);
        crate::player_data::update_ranked_stats(&ctx, &puuids, true);
        crate::player_data::update_saved_info(&ctx, &snapshot, &puuids, true);
        crate::player_data::update_champion_masteries(&ctx, &puuids, true);
        crate::match_history::update_match_histories(&ctx, &puuids, &params, api_source, true);

        let settings_for_update = settings.clone();
        let gates = self.gates();
        let stage = derived.stage.clone();
        tokio::spawn(async move {
            crate::additional_info::update(&ctx, &snapshot, &settings_for_update, &gates, &stage, None).await;
        });
    }

    /// `_reloadPlayer`
    pub fn reload_player(&self, puuid: &str, options: &Value) {
        let scopes = crate::reload::resolve_reload_scopes(options).unwrap_or_default();
        let (snapshot, settings) = self.read_inputs();
        let api_source = snapshot.api_should_use();

        if scopes.iter().any(|scope| scope == "matchHistory") {
            self.ctx.queues.cancel_by_tags(&[puuid, "match-history"], TagMode::And);
            self.ctx.queues.cancel_by_tags(&[puuid, "game-summary"], TagMode::And);

            crate::match_history::load_match_history(
                &self.ctx,
                puuid,
                history_params(settings.match_history_load_count, &self.ctx.store.match_history_tag_params()),
                true,
                api_source,
            );
        }

        crate::player_data::reload_player(&self.ctx, &snapshot, puuid, options);
    }

    /// `onInit`'s task. Only ever succeeds once.
    pub fn start(self: &Arc<Self>) -> anyhow::Result<()> {
        let receiver = self
            .wake_rx
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| anyhow::anyhow!("ongoing-game pipeline already started"))?;

        let this = self.clone();
        tokio::spawn(this.run(receiver));
        Ok(())
    }

    /// LCU pushes reach the synced props through `riftflow-app`'s `lc_state::watch`;
    /// this is the shard's own subscription, whose only job is to wake the pipeline.
    pub fn watch_lcu(self: &Arc<Self>, league_client: &Arc<riftflow_lcu::LeagueClientMain>) -> anyhow::Result<()> {
        let emitter = league_client.emitter();

        for uri in [
            "/lol-gameflow/v1/session",
            "/lol-gameflow/v1/gameflow-phase",
            "/lol-lobby/v2/lobby",
            "/lol-champ-select/v1/session",
            "/lol-summoner/v1/current-summoner",
        ] {
            let wake = self.wake_tx.clone();
            emitter.on(uri, move |_payload, _params| {
                let _ = wake.send(());
            })?;
        }

        Ok(())
    }

    async fn run(self: Arc<Self>, mut wake_rx: mpsc::UnboundedReceiver<()>) {
        loop {
            match self.run_pass() {
                Some(at) => {
                    tokio::select! {
                        _ = wake_rx.recv() => {}
                        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(at)) => {}
                    }
                }
                None => {
                    if wake_rx.recv().await.is_none() {
                        return;
                    }
                }
            }
        }
    }

    fn read_inputs(&self) -> (LcSnapshot, OngoingGameSettings) {
        let mut settings = Map::new();
        for key in [
            "enabled",
            "queryInLobbyPhase",
            "matchHistoryLoadCount",
            "gameDetailsLoadCount",
            "concurrency",
            "matchHistoryTagPreference",
            "orderPlayerBy",
            "showChampionUsage",
            "showMatchHistoryItemBorder",
            "showJunglePathing",
            "showJunglePathingForAllPlayers",
            "autoRouteWhenGameStarts",
            "playerCardTags",
            "premadeTeamInferMatchCountThreshold",
        ] {
            if let Some(value) = self.ctx.state_sync.get(NS, SETTINGS_ID, key) {
                settings.insert(key.to_string(), value);
            }
        }

        (LcSnapshot::from_state(&self.ctx.state_sync), OngoingGameSettings::from_value(&Value::Object(settings)))
    }

    fn gates(&self) -> FeatureGates {
        let sgp_server_id = self
            .ctx
            .state_sync
            .get(SGP_NAMESPACE, "state", "availability")
            .and_then(|availability| availability.get("sgpServerId").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();

        let config = self.ctx.state_sync.get(feature_gates::RIFTFLOW_API_NAMESPACE, "state", "featureGates");
        FeatureGates::new(config.as_ref(), &self.platform, &self.version, &sgp_server_id)
    }

    /// everything `state.ts` computed, from the current props
    fn derive(&self, snapshot: &LcSnapshot, settings: &OngoingGameSettings) -> Derived {
        let gates = self.gates();
        // `state.draft` is shard state, not an LCU prop, so the store's copy is it
        let draft = self.ctx.store.draft();
        let additional = self.ctx.store.additional();
        let handoff = self.ctx.store.handoff_snapshot().as_ref().and_then(HandoffSnapshot::from_value);
        let stage = computed::get_query_stage(snapshot, settings, draft.as_ref());
        let teams = computed::get_teams(
            snapshot,
            settings,
            &stage,
            &additional,
            draft.as_ref(),
            handoff.as_ref(),
            gates.deobfuscation,
            self.deobfuscator.as_ref(),
        );
        let selections = computed::get_champion_selections(
            snapshot,
            &stage,
            &additional,
            draft.as_ref(),
            handoff.as_ref(),
            gates.deobfuscation,
            self.deobfuscator.as_ref(),
        );
        let positions = computed::get_position_assignments(
            snapshot,
            &stage,
            &additional,
            draft.as_ref(),
            handoff.as_ref(),
            gates.deobfuscation,
            self.deobfuscator.as_ref(),
        );
        let groups = computed::get_team_participant_groups(snapshot, &additional, draft.as_ref());
        let premade = computed::get_merged_premade_team_map(&teams, &groups, &self.ctx.store.inferred_premade_teams());

        let puuids = computed::team_puuids(&teams);

        Derived { stage, teams, selections, positions, groups, premade, puuids, additional, handoff }
    }

    /// One full evaluation of every computed, then every reaction whose input moved.
    /// `start` loops on this; the app may also call it itself if it would rather own
    /// the scheduling. The next deadline it returns is when something is due.
    pub fn run_pass(&self) -> Option<Instant> {
        let now = Instant::now();
        let (snapshot, settings) = self.read_inputs();
        let gates = self.gates();
        let derived = self.derive(&snapshot, &settings);
        let mut reactions = self.reactions.lock().unwrap();

        if settings.concurrency != reactions.concurrency {
            reactions.concurrency = settings.concurrency;
            drop(reactions);
            let concurrency = settings.concurrency();
            let _ = self.ctx.queues.set_concurrency("match-history", concurrency);
            let _ = self.ctx.queues.set_concurrency("misc", concurrency);
            reactions = self.reactions.lock().unwrap();
        }

        let sync = &self.ctx.state_sync;
        let _ = sync.set(NS, STATE_ID, "queryStage", derived.stage.to_value());
        let _ = sync.set(NS, STATE_ID, "teams", record::object(&derived.teams));
        let _ = sync.set(NS, STATE_ID, "championSelections", record::object(&derived.selections));
        let _ = sync.set(NS, STATE_ID, "positionAssignments", record::object(&derived.positions));
        let _ = sync.set(NS, STATE_ID, "teamParticipantGroups", record::object(&derived.groups));
        let _ = sync.set(NS, STATE_ID, "isInEog", json!(computed::is_in_eog(&snapshot)));
        let _ = sync.set(NS, STATE_ID, "mergedPremadeTeamMap", record::object(&derived.premade));

        // `_watchUnavailableState`: only the transition into unavailable does anything
        let unavailable = derived.stage.phase() == "unavailable";
        if unavailable && !reactions.was_unavailable {
            reactions.was_unavailable = true;
            drop(reactions);

            self.ctx.queues.cancel_all();
            self.ctx.store.clear(false, false);
            self.ctx.ipc.send_event(NS, "clear", &[]);

            reactions = self.reactions.lock().unwrap();
        } else if !unavailable {
            reactions.was_unavailable = false;
        }

        let api_source = snapshot.api_should_use();
        let draft = self.ctx.store.draft();

        if reactions.tag_params.tick(
            json!([
                crate::match_history::current_queue_tag_info(draft.as_ref(), snapshot.lobby.as_ref(), snapshot.gameflow_session.as_ref()),
                settings.match_history_tag_preference == TagPreference::Current,
            ]),
            now,
        ) {
            drop(reactions);
            let current = crate::match_history::current_queue_tag_info(draft.as_ref(), snapshot.lobby.as_ref(), snapshot.gameflow_session.as_ref());
            self.ctx
                .store
                .set_match_history_tag_params(&crate::match_history::match_history_tag(current.as_ref(), settings.match_history_tag_preference));
            reactions = self.reactions.lock().unwrap();
        }

        if reactions.history.tick(
            json!({
                "teams": record::object(&derived.teams),
                "apiShouldUse": api_source.as_str(),
                "sgpTokenReady": snapshot.sgp_token_ready,
                "count": settings.match_history_load_count,
                "gameId": derived.stage.game_id(),
                "params": self.ctx.store.match_history_tag_params().to_json(),
            }),
            now,
        ) {
            let history_game_id = derived.stage.game_id();
            let force_history = history_game_id.is_some() && history_game_id != reactions.history_game_id;
            if api_source != ApiSource::Sgp || snapshot.sgp_token_ready {
                if history_game_id.is_some() { reactions.history_game_id = history_game_id; }
            }
            drop(reactions);
            if api_source != ApiSource::Sgp || snapshot.sgp_token_ready {
                let ctx = self.ctx.clone();
                let puuids = derived.puuids.clone();
                let params = history_params(settings.match_history_load_count, &self.ctx.store.match_history_tag_params());

                tokio::spawn(async move {
                    crate::match_history::update_match_histories(&ctx, &puuids, &params, api_source, force_history);
                });
            }
            reactions = self.reactions.lock().unwrap();
        }

        if reactions.player_data.tick(json!(record::object(&derived.teams)), now) {
            drop(reactions);
            let ctx = self.ctx.clone();
            let snapshot = snapshot.clone();
            let puuids = derived.puuids.clone();

            tokio::spawn(async move {
                crate::player_data::update_summoners(&ctx, &puuids, false);
                crate::player_data::update_ranked_stats(&ctx, &puuids, false);
                crate::player_data::update_saved_info(&ctx, &snapshot, &puuids, false);
                crate::player_data::update_champion_masteries(&ctx, &puuids, false);
            });
            reactions = self.reactions.lock().unwrap();
        }

        let records: Pairs<Value> = self.ctx.store.match_history_records();
        let game_ids = crate::match_history::detail_whitelist(
            &records.iter().map(|(puuid, record)| (puuid.clone(), games_of(record))).collect::<Vec<_>>(),
            settings.game_details_load_count,
        );

        if reactions.details.tick(json!({ "apiSource": api_source.as_str(), "gameIds": game_ids }), now) {
            drop(reactions);
            let ctx = self.ctx.clone();
            let held = ctx.store.game_details();

            for game_id in crate::match_history::details_to_prune(&held, &game_ids) {
                ctx.store.remove_game_details(game_id);
                let key = game_id.to_string();
                ctx.queues.cancel_by_tags(&[key.as_str(), "game-details"], TagMode::And);
            }

            crate::match_history::load_game_details(&ctx, &game_ids, false, api_source, Some(priority::GAME_DETAILS));
            reactions = self.reactions.lock().unwrap();
        }

        if reactions.additional.tick(
            json!({ "stage": derived.stage.to_value(), "selfPuuid": snapshot.self_puuid(), "draft": draft, "gsm": gates.gsm_by_puuid }),
            now,
        ) {
            drop(reactions);
            let ctx = self.ctx.clone();
            let snapshot = snapshot.clone();
            let settings = settings.clone();
            let gates = gates.clone();
            let stage = derived.stage.clone();

            tokio::spawn(async move {
                crate::additional_info::update(&ctx, &snapshot, &settings, &gates, &stage, ctx.store.draft().as_ref()).await;
            });
            reactions = self.reactions.lock().unwrap();
        }

        // `_watchGameIdInvalidation`, its own no-delay reaction: a snapshot from the
        // draft that just ended must not leak into the next game
        if reactions.invalidation.tick(
            json!([derived.stage.game_id(), derived.handoff.as_ref().map(|snapshot| snapshot.game_id)]),
            now,
        ) {
            drop(reactions);
            self.invalidate_handoff(&derived.stage);
            reactions = self.reactions.lock().unwrap();
        }

        if reactions.handoff.tick(handoff_inputs(&snapshot, &derived.stage, &gates, draft.as_ref(), self.deobfuscator.as_ref()), now) {
            drop(reactions);
            self.record_handoff(&snapshot, &derived.stage, &gates, draft.as_ref());
            self.invalidate_handoff(&derived.stage);
            reactions = self.reactions.lock().unwrap();
        }

        let record_pairs: Vec<(String, Vec<Value>)> = records
            .iter()
            .map(|(puuid, record)| (puuid.clone(), games_of(record)))
            .collect();
        let details = self.ctx.store.game_details();

        // the two analysis reactions overlap deliberately: the first one sets the
        // inferred teams too, so running only it is what the original did twice
        if reactions.analysis.tick(json!(analysis::analysis_keys(&record_pairs, &derived.teams, &details)), now) {
            drop(reactions);
            let inferred = analysis::infer_premade_teams(
                &record_pairs,
                &derived.puuids,
                settings.premade_team_infer_match_count_threshold,
            );
            self.ctx.store.set_inferred_premade_teams(inferred);
            self.ctx.store.set_analysis(analysis::compute_analysis(
                &records,
                &details,
                &derived.teams,
                self.ctx.store.analysis().as_ref(),
                self.analyzer.as_deref(),
            ));
            reactions = self.reactions.lock().unwrap();
        } else if reactions.inference.tick(
            json!({
                "summaryKeys": analysis::distinct_game_ids(&record_pairs),
                "threshold": settings.premade_team_infer_match_count_threshold,
            }),
            now,
        ) {
            drop(reactions);
            let inferred = analysis::infer_premade_teams(&record_pairs, &derived.puuids, settings.premade_team_infer_match_count_threshold);
            self.ctx.store.set_inferred_premade_teams(inferred);
            reactions = self.reactions.lock().unwrap();
        }

        let mut due = [
            reactions.tag_params.next_due(),
            reactions.history.next_due(),
            reactions.player_data.next_due(),
            reactions.details.next_due(),
            reactions.additional.next_due(),
            reactions.analysis.next_due(),
            reactions.inference.next_due(),
            reactions.handoff.next_due(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

        due.sort();
        due.first().copied()
    }

    /// `_watchSnapshotRecording`'s body: record what champ-select knows.
    fn record_handoff(&self, snapshot: &LcSnapshot, stage: &QueryStage, gates: &FeatureGates, draft: Option<&Value>) {
        if !gates.deobfuscation || draft.is_some() || !matches!(stage, QueryStage::ChampSelect { .. }) {
            return;
        }

        let Some(session) = snapshot.champ_select_session.as_ref() else {
            return;
        };
        let Some(built) = HandoffSnapshot::build(session, gates.deobfuscation, self.deobfuscator.as_ref()) else {
            return;
        };

        let value = built.to_value();
        if self.ctx.store.handoff_snapshot().as_ref() == Some(&value) {
            return;
        }

        self.ctx.store.set_handoff_snapshot(Some(value.clone()));
        if let Some(storage) = &self.storage {
            storage.save(&value);
        }
    }

    /// `_watchGameIdInvalidation`: the snapshot belongs to the draft, not to a new game
    fn invalidate_handoff(&self, stage: &QueryStage) {
        let Some(active) = stage.game_id() else { return };
        let Some(snapshot) = self.ctx.store.handoff_snapshot().as_ref().and_then(HandoffSnapshot::from_value) else {
            return;
        };

        if active != snapshot.game_id {
            self.ctx.store.set_handoff_snapshot(None);
            if let Some(storage) = &self.storage {
                storage.remove();
            }
        }
    }

    /// `init()` — the persisted snapshot comes back, unless the gate says no.
    pub fn restore_handoff(&self) {
        if !self.gates().deobfuscation {
            self.ctx.store.set_handoff_snapshot(None);
            if let Some(storage) = &self.storage {
                storage.remove();
            }
            return;
        }

        let Some(stored) = self.storage.as_ref().and_then(|storage| storage.get()) else {
            return;
        };

        match HandoffSnapshot::from_value(&stored) {
            Some(snapshot) => self.ctx.store.set_handoff_snapshot(Some(snapshot.to_value())),
            None => {
                if let Some(storage) = &self.storage {
                    storage.remove();
                }
            }
        }
    }
}

/// `{ startIndex: 0, count, ...matchHistoryTagParams }`
pub fn history_params(count: i64, tag_params: &MatchHistoryQueryParams) -> MatchHistoryQueryParams {
    MatchHistoryQueryParams {
        start_index: Some(0),
        count: Some(count),
        tag: tag_params.tag.clone(),
        tags_query_type: tag_params.tags_query_type.clone(),
    }
}

/// everything `state.ts` computed for one pass
pub struct Derived {
    pub stage: QueryStage,
    pub teams: Pairs<Vec<String>>,
    pub selections: Pairs<i64>,
    pub positions: Pairs<Value>,
    pub groups: Pairs<Vec<String>>,
    pub premade: Pairs<i64>,
    pub puuids: Vec<String>,
    pub additional: crate::computed::Additional,
    pub handoff: Option<HandoffSnapshot>,
}

/// the expression of `_watchSnapshotRecording`: `null` when there is nothing to record
fn handoff_inputs(
    snapshot: &LcSnapshot,
    stage: &QueryStage,
    gates: &FeatureGates,
    draft: Option<&Value>,
    magic: &dyn Deobfuscate,
) -> Value {
    if !gates.deobfuscation || draft.is_some() || !matches!(stage, QueryStage::ChampSelect { .. }) {
        return Value::Null;
    }

    snapshot
        .champ_select_session
        .as_ref()
        .and_then(|session| HandoffSnapshot::build(session, gates.deobfuscation, magic))
        .map(|snapshot| snapshot.to_value())
        .unwrap_or(Value::Null)
}

fn games_of(record: &Value) -> Vec<Value> {
    record.get("data").and_then(Value::as_array).cloned().unwrap_or_default()
}
