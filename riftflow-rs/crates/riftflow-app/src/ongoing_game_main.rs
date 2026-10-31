//! `ongoing-game-main` — the running-game data plane, wired to the real client.
//!
//! The maths lives in `riftflow-ongoing-game`, which is what its own tests exercise.
//! This module is everything around that crate that a crate cannot own: the LCU/SGP
//! transports, the SQLite saved-player lookup, the persisted champ-select handoff,
//! the settings schema, the wake-up sources, and the two side effects of
//! `side-effects-controller.ts`.
//!
//! `RiftFlow/src/main/shards/ongoing-game/index.ts:222` `onInit()` maps onto:
//! * `_setupState` → [`register`]: the settings service — the fourteen keys of
//!   `index.ts:69-141` with their transforms and the `matchHistoryLoadCount`
//!   side effect — plus the prop mirror. The prop *surfaces* are declared by
//!   `state_bootstrap.rs`, which runs first, so this module only writes into them;
//!   a second `prop_sync` of a declared path is a hard error.
//! * `_champSelectHandoff.init()` → `OngoingGame::restore_handoff`
//! * `_ipcHandlers.register()` → `OngoingGame::register_ipc`: `getAll`, `setDraft`,
//!   `clearDraft`, `setMatchHistoryTagParams`, `reload`, `reloadPlayer`
//!   (`ipc-handlers.ts:25-71`), which is exactly the set
//!   `renderer-shared/shards/ongoing-game/index.ts` calls — every other renderer
//!   setter goes through `setting-utils` and lands on the settings surface above
//! * the seven `watch()`s → `OngoingGame::start`, the crate's one task that owns the
//!   `reaction` debounces, plus the wake-up sources added by [`start`]
//! * `_sideEffects.watch()` → the [`SideEffects`] task, the only behaviour of this
//!   shard that is not in the crate
//!
//! The events are the crate's, and they leave through the same
//! [`RiftFlowIpcMain::send_event`] path `auto_gameflow.rs` and `windows.rs` use:
//! `clear`, `summoner-loaded`/`-removed`, `ranked-stats-loaded`/`-removed`,
//! `saved-info-loaded`/`-removed`, `champion-mastery-loaded`/`-removed`,
//! `match-history-loaded`/`-removed`, `game-details-loaded`/`-removed` and
//! `additional-game-loaded` — the fifteen `store-event-handlers.ts` subscribes to.
//! `clear-player` is the one of those fifteen that no part of the original main
//! process ever emitted (`grep` over `src/main` finds only the subscription), so
//! this port does not invent a producer for it either.
//!
//! Named gaps, each one reported rather than faked:
//! * `analyzer: None`. `_computeAnalysis` calls `analyzeGames` / `analyzePlayers`
//!   from `shared/data-adapter/analysis/**` — roughly two thousand lines of per-game
//!   metric maths that no crate has ported. The `analysis` prop therefore keeps its
//!   declared boot value of `null`, while the premade-team inference, which *is*
//!   pure arithmetic and *is* ported, runs for real.
//! * `deobfuscator: NoDeobfuscation`. The obfuscated-puuid table came from the
//!   `riftflow-tools` addon; `riftflow-native` only exposes process probes, so hidden
//!   draft members stay hidden instead of being guessed at.
//! * SGP is reachable but not driven: `sgp-main` is still a placeholder namespace,
//!   so `availability.serversSupported.matchHistory` stays `false` and
//!   `state.apiShouldUse` resolves to `lcu`. That is the honest source today, and
//!   the crate's SGP path is exercised by its own tests.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;

use riftflow_ipc::RiftFlowIpcMain;
use riftflow_lcu::LeagueClientMain;
use riftflow_lcu::http::RequestConfig;
use riftflow_ongoing_game::{
    HandoffStorage, LcSnapshot, LcuSgpFetcher, NoDeobfuscation, OngoingGame, OngoingGameDeps,
    SavedPlayer,
    settings::{
        default_player_card_tags, side_effect_game_details_load_count,
        transform_game_details_load_count, transform_match_history_load_count,
    },
};
use riftflow_state::StateSync;
use riftflow_store::Database;
use riftflow_store::players;
use riftflow_store::settings::{
    SettingChangeContext, SettingDeclaration, SettingFactory, SettingsService, SideEffect,
    Transform, TransformContext, Validator, boolean, number, string_enum,
};

pub const NAMESPACE: &str = riftflow_ongoing_game::ONGOING_GAME_MAIN_NAMESPACE;

const LEAGUE: &str = "league-client-main";
const APP_COMMON: &str = "app-common-main";

/// `CHAMP_SELECT_HANDOFF_SNAPSHOT_KEY` (`champ-select-handoff-controller.ts:11`).
/// Not a declared setting: the original wrote it with
/// `settingService._saveToStorage`, so it is a bare row that no prop mirrors.
const HANDOFF_KEY: &str = "champSelectHandoffSnapshot";

/// Backstop for prop writes nothing here can subscribe to. `app-common-main`
/// installs no change sink, `sgp-main` availability is recomputed from inside the
/// shell's own poll loop, and the crate's `watch_lcu` covers the six endpoints this
/// shard subscribed to — so without a tick a `preferredLolSource` or token change
/// would wait for an unrelated LCU event. Every pass is memoized inside
/// `OngoingGame::run_pass`, so an idle tick evaluates and decides nothing.
const BACKSTOP: Duration = Duration::from_millis(2000);

/// The fourteen keys `index.ts:181-196` propSyncs, in that order.
/// `state_bootstrap.rs` declares this surface; this module's declarations have to
/// match it key for key or the first mirrored write fails.
const SETTING_KEYS: [&str; 14] = [
    "concurrency",
    "enabled",
    "matchHistoryLoadCount",
    "matchHistoryTagPreference",
    "gameDetailsLoadCount",
    "orderPlayerBy",
    "showChampionUsage",
    "showMatchHistoryItemBorder",
    "showJunglePathing",
    "showJunglePathingForAllPlayers",
    "autoRouteWhenGameStarts",
    "playerCardTags",
    "queryInLobbyPhase",
    "premadeTeamInferMatchCountThreshold",
];

/// `ongoGamePlayerCardTagsSchema`'s twenty-one keys (`setting-schemas.ts:4-27`).
/// `z.looseObject` demands every one of them and tolerates more, so the validator
/// says the same and the committed value keeps its extra keys.
const PLAYER_CARD_TAG_KEYS: [&str; 21] = [
    "showPremadeTeamTag",
    "showSuspiciousFlashPositionTag",
    "showWinningStreakTag",
    "showLosingStreakTag",
    "showSoloKillsTag",
    "showEasyGankTag",
    "showGreatPerformanceTag",
    "showAverageTeamDamageTag",
    "showAverageTeamDamageTakenTag",
    "showAverageTeamGoldTag",
    "showAverageCsPerMinuteTag",
    "showAverageDamageGoldEfficiencyTag",
    "showAverageEnemyMissingPingsTag",
    "showAverageVisionScoreTag",
    "showAverageKillDamageEfficiencyTag",
    "showSelfTag",
    "showMetTag",
    "showTaggedTag",
    "showWinRateTeamTag",
    "showPrivacyTag",
    "showRiftFlowScoreTag",
];

static PIPELINE: OnceLock<Arc<OngoingGame>> = OnceLock::new();

/// The live pipeline, for a module that wants to nudge it or read its store.
#[allow(dead_code)] // the seam is public; `register` is what fills it
pub fn instance() -> Option<Arc<OngoingGame>> {
    PIPELINE.get().cloned()
}

/// `onInit()` minus the watchers: settings service, the crate's pipeline, the
/// restored handoff snapshot and the six IPC methods.
///
/// Call this before any window loads: an unregistered namespace *rejects* the
/// renderer's `riftflowCall`, and this shard's `getAll` is the first thing its
/// `onInit` awaits.
pub fn register(
    ipc: &Arc<RiftFlowIpcMain>,
    state_sync: &Arc<StateSync>,
    league_client: &Arc<LeagueClientMain>,
    settings: &Arc<SettingFactory>,
    sgp_gateway: &Arc<riftflow_sgp::SgpGateway>,
    version: &str,
) -> anyhow::Result<Arc<OngoingGame>> {
    if PIPELINE.get().is_some() {
        anyhow::bail!("{NAMESPACE} is already registered");
    }

    // The service has to be readable by its own transforms and side effects, which
    // are built before it exists — the original had the same cycle and solved it
    // with a class field (`this.settings`, `this._settingService`).
    let service_cell: Arc<OnceLock<Arc<SettingsService>>> = Arc::new(OnceLock::new());
    let service = match settings.register(NAMESPACE, declarations(&service_cell)) {
        Ok(service) => Some(service),
        Err(error) => {
            // another module owns this settings namespace; the pipeline still reads
            // whatever surface it declared, so say so instead of failing the boot
            eprintln!("[ongoing-game] {NAMESPACE} settings schema not owned here: {error}");
            None
        }
    };

    if let Some(service) = &service {
        let _ = service_cell.set(service.clone());

        // `applyToState()`, then the push half of what a MobX settings mutation did
        // implicitly. Mirrored by hand because the surface already exists:
        // `state_bootstrap.rs` read the raw rows, which skip every transform and
        // leave `playerCardTags` the empty object instead of the schema default.
        if let Err(error) = service.apply_to_state() {
            eprintln!("[ongoing-game] settings load failed: {error}");
        }
        for (key, value) in service.entries() {
            let _ = state_sync.set(NAMESPACE, "settings", key.as_str(), value);
        }
    }

    let game = build(
        ipc,
        state_sync,
        league_client,
        settings,
        sgp_gateway,
        version,
    )?;

    // the write path propSync had: commit → prop → re-evaluate
    let wake = game.clone();
    let sync = state_sync.clone();
    if let Some(service) = &service {
        service.set_change_sink(Arc::new(move |namespace, key, value| {
            if namespace == NAMESPACE {
                let _ = sync.set(namespace, "settings", key, value.clone());
                wake.wake();
            }
        }));
    }

    // `onInit`'s order: `_setupState`, `_champSelectHandoff.init()`, then the
    // handlers, so a restored snapshot is visible to the very first pass
    game.restore_handoff();
    game.register_ipc()?;

    let _ = PIPELINE.set(game.clone());

    Ok(game)
}

/// `OngoingGame::new` with this app's real seams.
///
/// Built inside the shared runtime because `QueueKeeper::new` spawns its two worker
/// tasks and `tokio::spawn` needs a task context, which `main()`'s body does not
/// have; `block_on` enters that context and returns as soon as the future does, and
/// this future awaits nothing.
#[allow(clippy::type_complexity)]
fn build(
    ipc: &Arc<RiftFlowIpcMain>,
    state_sync: &Arc<StateSync>,
    league_client: &Arc<LeagueClientMain>,
    settings: &Arc<SettingFactory>,
    sgp_gateway: &Arc<riftflow_sgp::SgpGateway>,
    version: &str,
) -> anyhow::Result<Arc<OngoingGame>> {
    let state_sync = state_sync.clone();
    let ipc = ipc.clone();
    let fetcher = LcuSgpFetcher::new(league_client.clone(), sgp_gateway.clone(), state_sync.clone());
    let saved_player = Arc::new(DbSavedPlayer {
        db: settings.db().clone(),
    });
    let storage = Arc::new(RowHandoffStorage {
        settings: settings.clone(),
    });
    let platform = node_platform();
    let version = version.to_string();

    Ok(tauri::async_runtime::block_on(async move {
        OngoingGame::new(OngoingGameDeps {
            state_sync,
            ipc,
            fetcher,
            saved_player: Some(saved_player),
            deobfuscator: Arc::new(NoDeobfuscation),
            storage: Some(storage),
            platform,
            version,
            // `analysis-controller.ts`'s statistical half, unported — see the header
            analyzer: None,
        })
    }))
}

/// `onInit`'s watchers: the crate's reaction loop, its LCU subscriptions, the
/// backstop tick and the side-effects task.
pub fn start(
    game: &Arc<OngoingGame>,
    league_client: &Arc<LeagueClientMain>,
    state_sync: &Arc<StateSync>,
    settings: &Arc<SettingFactory>,
) -> anyhow::Result<()> {
    game.watch_lcu(league_client)?;

    // `OngoingGame::start` spawns through `tokio::spawn`, so it needs the same
    // context as `build`; the task it starts outlives this call.
    let launching = {
        let game = game.clone();
        tauri::async_runtime::block_on(async move { game.start() })
    };
    launching?;

    let ticker = game.clone();
    let roster_client = league_client.clone();
    let roster_sync = state_sync.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(BACKSTOP);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut roster_game_id = None;
        let mut roster_attempts = 0;
        loop {
            interval.tick().await;
            let session = roster_sync.get(LEAGUE, "gameflow", "session").unwrap_or(Value::Null);
            let game_id = session.pointer("/gameData/gameId").and_then(Value::as_i64);
            if game_id != roster_game_id {
                roster_game_id = game_id;
                roster_attempts = 0;
            }
            let enabled = roster_sync.get(NAMESPACE, "settings", "enabled").and_then(|value| value.as_bool()).unwrap_or(true);
            let phase = roster_sync.get(LEAGUE, "gameflow", "phase").unwrap_or(Value::Null);
            if enabled && matches!(phase.as_str(), Some("InProgress" | "Reconnect")) && roster_attempts < 30 && roster_needs_refresh(&session) {
                if let Some(http) = roster_client.http() {
                    roster_attempts += 1;
                    if let Ok(fresh) = http.get("/lol-gameflow/v1/session").await {
                        let current = roster_sync.get(LEAGUE, "gameflow", "session").unwrap_or(Value::Null);
                        if current.pointer("/gameData/gameId").and_then(Value::as_i64) == game_id
                            && fresh.pointer("/gameData/gameId").and_then(Value::as_i64) == game_id
                            && roster_sync.get(LEAGUE, "gameflow", "phase").is_some_and(|value| matches!(value.as_str(), Some("InProgress" | "Reconnect")))
                            && matches!(current.get("phase").and_then(Value::as_str), Some("InProgress" | "Reconnect"))
                            && matches!(fresh.get("phase").and_then(Value::as_str), Some("InProgress" | "Reconnect")) {
                            let _ = roster_sync.set(LEAGUE, "gameflow", "session", fresh);
                        }
                    }
                }
            }
            ticker.wake();
        }
    });

    let mut side_effects = SideEffects::new(
        state_sync.clone(),
        league_client.clone(),
        game.clone(),
        settings.db().clone(),
        settings.clone(),
    );

    // the endpoints the two side-effect reactions track; `savedInfo` and `summoner`
    // are not props at all, so the backstop tick is what notices them filling in
    let (sender, wakes) = mpsc::unbounded_channel();
    let emitter = league_client.emitter();
    for uri in [
        "/lol-gameflow/v1/gameflow-phase",
        "/lol-gameflow/v1/session",
        "/lol-chat/v1/conversations/:id",
    ] {
        let sender = sender.clone();
        emitter.on(uri, move |_payload, _params| {
            let _ = sender.send(());
        })?;
    }
    drop(sender);

    tauri::async_runtime::spawn(async move { side_effects.run(wakes).await });

    Ok(())
}

fn roster_needs_refresh(session: &Value) -> bool {
    if !matches!(session.get("phase").and_then(Value::as_str), Some("InProgress" | "Reconnect")) {
        return false;
    }
    let data = session.get("gameData").unwrap_or(&Value::Null);
    let mode = data.pointer("/queue/gameMode").and_then(Value::as_str);
    if !matches!(mode, Some("CLASSIC" | "ARAM")) || data.get("isCustomGame").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    let expected = data.pointer("/queue/maximumParticipantListSize").and_then(Value::as_u64).unwrap_or(10) as usize / 2;
    ["teamOne", "teamTwo"].iter().any(|side| {
        let members = data.get(*side).and_then(Value::as_array);
        members.is_none_or(|members| members.len() < expected || members.iter().any(|member| {
            let id = member.get("puuid").and_then(Value::as_str).unwrap_or_default();
            id.is_empty() || id == "00000000-0000-0000-0000-000000000000"
                || member.get("championId").and_then(Value::as_i64).unwrap_or_default() <= 0
        }))
    })
}

// --------------------------------------------------------------- settings schema

fn declarations(service_cell: &Arc<OnceLock<Arc<SettingsService>>>) -> Vec<SettingDeclaration> {
    let flag = |key: &str, default: bool| setting(key, json!(default), Some(boolean()));
    let amount = |key: &str, default: i64| setting(key, json!(default), Some(number()));
    let choice = |key: &str, default: &str, allowed: &[&str]| {
        setting(key, json!(default), Some(string_enum(allowed)))
    };

    vec![
        amount("concurrency", 2),
        flag("enabled", true),
        // `index.ts:74-90`: out of range keeps the old value, and a commit drags
        // `gameDetailsLoadCount` down with it
        SettingDeclaration {
            key: "matchHistoryLoadCount".to_string(),
            default: json!(50),
            schema: Some(number()),
            transform: Some(clamp_load_count()),
            side_effect: Some(load_count_side_effect(service_cell)),
        },
        choice("matchHistoryTagPreference", "current", &["current", "all"]),
        SettingDeclaration {
            key: "gameDetailsLoadCount".to_string(),
            default: json!(20),
            schema: Some(number()),
            transform: Some(clamp_details_count(service_cell)),
            side_effect: None,
        },
        choice(
            "orderPlayerBy",
            "default",
            &["win-rate", "kda", "default", "riftflow-score", "position", "premade-team"],
        ),
        choice("showChampionUsage", "recent", &["recent", "mastery", "none"]),
        flag("showMatchHistoryItemBorder", false),
        flag("showJunglePathing", true),
        flag("showJunglePathingForAllPlayers", false),
        flag("autoRouteWhenGameStarts", false),
        SettingDeclaration {
            key: "playerCardTags".to_string(),
            default: default_player_card_tags(),
            schema: Some(player_card_tags_schema()),
            transform: None,
            side_effect: None,
        },
        flag("queryInLobbyPhase", true),
        amount("premadeTeamInferMatchCountThreshold", 5),
    ]
}

fn setting(key: &str, default: Value, schema: Option<Validator>) -> SettingDeclaration {
    SettingDeclaration {
        key: key.to_string(),
        default,
        schema,
        transform: None,
        side_effect: None,
    }
}

/// `transform: ({ value, oldValue }) => value >= 1 && value <= 200 ? value : oldValue`
/// — the range maths is the crate's, so the settings path and its tests agree.
fn clamp_load_count() -> Transform {
    Arc::new(|context: &TransformContext| {
        let old = context
            .old_value
            .clone()
            .unwrap_or_else(|| context.default_value.clone());

        match (context.value.as_i64(), old.as_i64()) {
            (Some(value), Some(old_value)) => json!(transform_match_history_load_count(value, old_value)),
            // `value >= 1` is false for anything that is not a number, and JS then
            // hands back `oldValue` itself — the row as it stands, not a coercion
            _ => old,
        }
    })
}

/// `transform: ({ value }) => value >= 0 && value <= settings.matchHistoryLoadCount
///   ? value : settings.matchHistoryLoadCount` — the cap is read live, as the
/// closure that captured `this.settings` did.
fn clamp_details_count(service_cell: &Arc<OnceLock<Arc<SettingsService>>>) -> Transform {
    let cell = service_cell.clone();

    Arc::new(move |context: &TransformContext| {
        // `settings.matchHistoryLoadCount`, the class default until a service exists
        let limit = cell
            .get()
            .map(|service| service.get("matchHistoryLoadCount"))
            .unwrap_or_else(|| context.default_value.clone());

        match (context.value.as_i64(), limit.as_i64()) {
            (Some(value), Some(limit)) => json!(transform_game_details_load_count(value, limit)),
            // `z.number()` rejects the value before the transform, so the cap wins
            _ => limit,
        }
    })
}

/// `sideEffect: ({ value }) => settingService.set('gameDetailsLoadCount',
///   Math.min(value, settings.gameDetailsLoadCount))`, awaited before the commit —
/// so a failing inner write aborts the outer one, exactly as the awaited JS did.
fn load_count_side_effect(service_cell: &Arc<OnceLock<Arc<SettingsService>>>) -> SideEffect {
    let cell = service_cell.clone();

    Arc::new(move |context: &SettingChangeContext| {
        let cell = cell.clone();
        let value = context.value.clone();

        Box::pin(async move {
            let Some(service) = cell.get().cloned() else {
                return Err("ongoing-game-main settings service is not ready".to_string());
            };

            let next = side_effect_game_details_load_count(
                value.as_i64().unwrap_or_default(),
                service
                    .get("gameDetailsLoadCount")
                    .as_i64()
                    .unwrap_or_default(),
            );

            service
                .set("gameDetailsLoadCount", json!(next))
                .await
                .map_err(|error| error.to_string())
        })
    })
}

fn player_card_tags_schema() -> Validator {
    Arc::new(|value: &Value| {
        let Some(object) = value.as_object() else {
            return Err("Expected object".to_string());
        };

        for key in PLAYER_CARD_TAG_KEYS {
            if object.get(key).and_then(Value::as_bool).is_none() {
                return Err(format!("Invalid or missing field: {key}"));
            }
        }

        Ok(value.clone())
    })
}

// ----------------------------------------------------------------------- seams

/// `saved-player-main`'s one read, against the same SQLite file the rest of the app
/// uses. `querySavedPlayerWithGames` answers `null` for a player never met, which is
/// the `None` the crate documents.
struct DbSavedPlayer {
    db: Arc<Mutex<Database>>,
}

#[async_trait::async_trait]
impl SavedPlayer for DbSavedPlayer {
    async fn query_saved_player_with_games(&self, query: Value) -> anyhow::Result<Option<Value>> {
        let found = {
            let guard = self.db.lock().unwrap();
            players::query_saved_player_with_games(guard.connection(), &query)?
        };

        Ok(if found.is_null() { None } else { Some(found) })
    }
}

/// `settingService._getFromStorage` / `_saveToStorage` / `_removeFromStorage` for
/// `champSelectHandoffSnapshot`: the `Settings` row under this shard's prefix, which
/// is where the original kept it.
struct RowHandoffStorage {
    settings: Arc<SettingFactory>,
}

impl HandoffStorage for RowHandoffStorage {
    fn get(&self) -> Option<Value> {
        self.settings
            .get_raw(NAMESPACE, HANDOFF_KEY)
            .ok()
            .flatten()
            .filter(|value| !value.is_null())
    }

    fn save(&self, snapshot: &Value) {
        // `catch (error) { logger.warn(...) }`: a snapshot that will not persist is
        // still a snapshot for this game
        if let Err(error) = self.settings.save_raw(NAMESPACE, HANDOFF_KEY, snapshot) {
            eprintln!("[ongoing-game] failed to save champ-select handoff snapshot: {error}");
        }
    }

    fn remove(&self) {
        if let Err(error) = self.settings.remove_raw(NAMESPACE, HANDOFF_KEY) {
            eprintln!("[ongoing-game] failed to clear champ-select handoff snapshot: {error}");
        }
    }
}

/// `SharedGlobalShard.global.platform`, i.e. Node's `process.platform` — the spelling
/// the remote feature-gate `platforms` lists are written against.
fn node_platform() -> String {
    if cfg!(target_os = "windows") {
        "win32".to_string()
    } else if cfg!(target_os = "macos") {
        "darwin".to_string()
    } else {
        "linux".to_string()
    }
}

// ------------------------------------------------------------------ side effects

/// One entry of `_watchTaggedPlayerReminders`'s `playersToSend`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TaggedPlayer {
    puuid: String,
    name: String,
    tag: String,
}

/// The two reactions' memo slots. Both start already evaluated on the empty world,
/// which is what the originals saw at `onInit`: no draft data, no conversation.
#[derive(Debug)]
struct Reminders {
    /// `itsTimeToSend` the last time reaction one read it; `None` never read
    room: Option<Option<String>>,
    /// the last `[playersToSend, itsTimeToSend]` reaction two ran for
    seen: Option<(Option<String>, Vec<String>)>,
    reminded: Vec<String>,
}

impl Default for Reminders {
    fn default() -> Self {
        Self {
            room: Some(None),
            seen: Some((None, Vec::new())),
            reminded: Vec::new(),
        }
    }
}

impl Reminders {
    /// `reaction(() => itsTimeToSend)`: only the fall to nothing does anything, and
    /// only on a change.
    fn observe_room(&mut self, room: Option<String>) {
        if self.room.as_ref() == Some(&room) {
            return;
        }

        if room.is_none() {
            self.reminded.clear();
        }

        self.room = Some(room);
    }

    /// `reaction(() => [playersToSend, itsTimeToSend])`: runs when the puuid list
    /// changes — its computed's `equals` compares puuids and nothing else — or the
    /// room moved, and returns the players not reminded yet.
    fn due(&mut self, room: Option<&String>, players: &[TaggedPlayer]) -> Vec<TaggedPlayer> {
        if room.is_none() {
            return Vec::new();
        }

        let puuids: Vec<String> = players.iter().map(|player| player.puuid.clone()).collect();
        let signature = (room.cloned(), puuids);
        if self.seen.as_ref() == Some(&signature) {
            return Vec::new();
        }
        self.seen = Some(signature);

        players
            .iter()
            .filter(|player| !self.reminded.iter().any(|reminded| *reminded == player.puuid))
            .cloned()
            .collect()
    }

    fn mark_reminded(&mut self, puuid: &str) {
        if !self.reminded.iter().any(|reminded| reminded == puuid) {
            self.reminded.push(puuid.to_string());
        }
    }
}

/// `side-effects-controller.ts`: the two reactions that write to the world instead of
/// to state. Both read what the crate already loaded, so nothing here is a second
/// copy of any data.
struct SideEffects {
    state_sync: Arc<StateSync>,
    league_client: Arc<LeagueClientMain>,
    game: Arc<OngoingGame>,
    db: Arc<Mutex<Database>>,
    settings: Arc<SettingFactory>,
    was_in_eog: Option<bool>,
    reminders: Reminders,
}

impl SideEffects {
    fn new(
        state_sync: Arc<StateSync>,
        league_client: Arc<LeagueClientMain>,
        game: Arc<OngoingGame>,
        db: Arc<Mutex<Database>>,
        settings: Arc<SettingFactory>,
    ) -> Self {
        Self {
            state_sync,
            league_client,
            game,
            db,
            settings,
            was_in_eog: None,
            reminders: Reminders::default(),
        }
    }

    async fn run(&mut self, mut wakes: mpsc::UnboundedReceiver<()>) {
        let mut interval = tokio::time::interval(BACKSTOP);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = interval.tick() => {}
                Some(()) = wakes.recv() => {}
            }

            self.evaluate();
        }
    }

    fn evaluate(&mut self) {
        let phase = self
            .state_sync
            .get(LEAGUE, "gameflow", "phase")
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();

        // `leagueClient.data.gameflow.phase`, not the crate's `isInEog`, which also
        // counts `WaitingForStats`
        let in_eog = phase == "EndOfGame" || phase == "PreEndOfGame";
        if entered_end_of_game(self.was_in_eog.replace(in_eog), in_eog) {
            self.save_end_of_game();
        }

        self.remind_tagged_players();
    }

    // --- `_watchEndOfGameSave`

    /// `saveEncounteredGame` before `saveSavedPlayer`, per player, in team order,
    /// with myself excluded. A failure stops the run: the original awaited each call
    /// and let a rejection end the loop.
    fn save_end_of_game(&self) {
        let snapshot = LcSnapshot::from_state(&self.state_sync);
        let Some((region, rso_platform_id)) = snapshot.auth() else {
            return;
        };
        let Some(self_puuid) = snapshot.self_puuid() else {
            return;
        };

        let stage = self.prop(NAMESPACE, "state", "queryStage");
        let stage = stage
            .as_ref()
            .filter(|stage| stage.get("gameInfo").is_some_and(|info| !info.is_null()));
        let Some(stage) = stage else {
            return;
        };
        // `state.queryStage.phase !== 'in-game' || !gameInfo.gameId`
        if stage.get("phase").and_then(Value::as_str) != Some("in-game") {
            return;
        }
        let game_info = &stage["gameInfo"];
        let Some(game_id) = game_info.get("gameId").and_then(Value::as_i64) else {
            return;
        };
        if game_id == 0 {
            return;
        }
        let queue_type = game_info
            .get("queueType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let roster = self.team_roster();
        if !roster.iter().any(|puuid| *puuid == self_puuid) {
            eprintln!("[ongoing-game] current player not in this game, skip recording");
            return;
        }

        for puuid in roster.iter().filter(|puuid| **puuid != self_puuid) {
            let encountered = json!({
                "gameId": game_id,
                "puuid": puuid,
                "selfPuuid": self_puuid,
                "region": region,
                "rsoPlatformId": rso_platform_id,
                "queueType": queue_type,
            });
            let saved = json!({
                "encountered": true,
                "puuid": puuid,
                "selfPuuid": self_puuid,
                "region": region,
                "rsoPlatformId": rso_platform_id,
            });

            let outcome = {
                let Ok(guard) = self.db.lock() else {
                    eprintln!("[ongoing-game] end-of-game recording skipped: database is poisoned");
                    return;
                };

                match players::save_encountered_game(guard.connection(), &encountered) {
                    Ok(_) => players::save_saved_player(guard.connection(), &saved)
                        .map(|_| ())
                        .map_err(|error| error.to_string()),
                    Err(error) => Err(error.to_string()),
                }
            };

            match outcome {
                Ok(()) => {
                    eprintln!("[ongoing-game] save game info: {game_id}");
                    eprintln!("[ongoing-game] save player info: {puuid}");
                }
                Err(error) => {
                    eprintln!(
                        "[ongoing-game] failed to record {puuid} for game {game_id}: {error}"
                    );
                    return;
                }
            }
        }
    }

    /// `Object.values(state.teams || {}).flat()` — the prop the crate pushes.
    fn team_roster(&self) -> Vec<String> {
        self.prop(NAMESPACE, "state", "teams")
            .and_then(|teams| teams.as_object().cloned())
            .map(|teams| {
                teams
                    .values()
                    .filter_map(Value::as_array)
                    .flatten()
                    .filter_map(|puuid| puuid.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    // --- `_watchTaggedPlayerReminders`

    fn remind_tagged_players(&mut self) {
        let room = self.champ_select_room();
        self.reminders.observe_room(room.clone());

        let tagged = self.players_to_send();
        let due = self.reminders.due(room.as_ref(), &tagged);
        let Some(room) = room else {
            return;
        };

        for player in due {
            let label = tagged_player_label(self.locale_is_english());
            let message = reminder_message(label, &player.name, &player.tag);
            self.reminders.mark_reminded(&player.puuid);
            self.chat_send(room.clone(), message);
        }
    }

    /// `itsTimeToSend`: null while a draft is loaded, else the champion-select
    /// conversation's id.
    fn champ_select_room(&self) -> Option<String> {
        reminder_room(
            self.game.ctx().store.draft().as_ref(),
            self.prop(LEAGUE, "chat", "conversations.championSelect").as_ref(),
        )
    }

    /// `playersToSend` over the crate's two maps — the same two `getAll` returns.
    fn players_to_send(&self) -> Vec<TaggedPlayer> {
        let all = self.game.ctx().store.get_all();
        tagged_players(all.get("savedInfo"), all.get("summoner"))
    }

    fn locale_is_english(&self) -> bool {
        locale_is_english(&self.settings)
    }

    /// `leagueClient.api.chat.chatSend(roomId, text, 'celebration').catch(() => {})`:
    /// a reminder that will not send is not a failure the game should hear about.
    fn chat_send(&self, conversation_id: String, message: String) {
        let Some(http) = self.league_client.http() else {
            return;
        };
        let body = json!({
            "body": message,
            "fromPid": "",
            "fromSummonerId": 0,
            "id": conversation_id,
            "isHistorical": false,
            "timestamp": "",
            "type": "celebration",
        });
        let config = RequestConfig {
            method: "POST".to_string(),
            url: format!("/lol-chat/v1/conversations/{conversation_id}/messages"),
            data: Some(body),
            ..Default::default()
        };

        tauri::async_runtime::spawn(async move {
            if let Err(error) = http.request(config).await {
                eprintln!("[ongoing-game] tagged-player reminder not sent: {error}");
            }
        });
    }

    fn prop(&self, namespace: &str, state_id: &str, path: &str) -> Option<Value> {
        self.state_sync.get(namespace, state_id, path)
    }
}

// ----------------------------------------------------------------- pure decisions

/// `reaction(() => isInEndOfGame)`, which never runs for its first value: only the
/// rise into end of game records, and a `PreEndOfGame` → `EndOfGame` move is one run.
fn entered_end_of_game(previous: Option<bool>, current: bool) -> bool {
    matches!(previous, Some(false)) && current
}

fn locale_is_english(settings: &SettingFactory) -> bool {
    settings
        .get_setting(APP_COMMON, "locale")
        .ok()
        .and_then(|locale| locale.as_str().map(str::to_string))
        .is_some_and(|locale| locale.starts_with("en"))
}

/// `playersToSend`, as a pure function over the two `getAll` maps: a player counts
/// only with a non-empty tag *and* a summoner already loaded.
fn tagged_players(saved_info: Option<&Value>, summoners: Option<&Value>) -> Vec<TaggedPlayer> {
    let (Some(saved_info), Some(summoners)) = (saved_info, summoners) else {
        return Vec::new();
    };

    let mut out: Vec<TaggedPlayer> = Vec::new();

    for (puuid, info) in saved_info.as_object().into_iter().flat_map(|map| map.iter()) {
        let tag = info.get("tag").and_then(Value::as_str).unwrap_or_default();
        if tag.is_empty() {
            continue;
        }

        let Some(summoner) = summoners.get(puuid).filter(|value| !value.is_null()) else {
            continue;
        };
        let game_name = summoner.get("gameName").and_then(Value::as_str).unwrap_or_default();
        let tag_line = summoner.get("tagLine").and_then(Value::as_str).unwrap_or_default();

        out.push(TaggedPlayer {
            puuid: puuid.clone(),
            name: format!("{game_name}#{tag_line}"),
            tag: tag.to_string(),
        });
    }

    out
}

/// `itsTimeToSend`: `state.draft` wins, then the conversation's id.
fn reminder_room(draft: Option<&Value>, conversation: Option<&Value>) -> Option<String> {
    if draft.is_some() {
        return None;
    }

    conversation?
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// `i18next.t('ongoing-game-main.taggedPlayer')`, the two entries of
/// `shared/i18n/*/main.yaml:85` — the locale list is `en` and `zh-CN`.
fn tagged_player_label(english: bool) -> &'static str {
    if english {
        "Tagged Player"
    } else {
        "已标记的玩家"
    }
}

/// `` `[${label}: ${name}]: \n${tag}` ``, the template of
/// `side-effects-controller.ts:144` down to the space before the newline.
fn reminder_message(label: &str, name: &str, tag: &str) -> String {
    format!("[{label}: {name}]: \n{tag}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn incomplete_live_rosters_are_rechecked_but_complete_and_ended_matches_are_not() {
        let member = |id: i64| json!({"puuid":format!("p{id}"),"championId":id+1});
        let mut session = json!({"phase":"InProgress","gameData":{
            "queue":{"gameMode":"ARAM","maximumParticipantListSize":10},
            "teamOne":(0..5).map(member).collect::<Vec<_>>(),
            "teamTwo":(5..9).map(member).collect::<Vec<_>>()
        }});
        assert!(roster_needs_refresh(&session));
        session["gameData"]["teamTwo"].as_array_mut().unwrap().push(member(9));
        assert!(!roster_needs_refresh(&session));
        session["gameData"]["teamTwo"][4]["puuid"] = json!("");
        assert!(roster_needs_refresh(&session));
        session["phase"] = json!("EndOfGame");
        assert!(!roster_needs_refresh(&session));
    }
    use super::*;

    fn cell() -> Arc<OnceLock<Arc<SettingsService>>> {
        Arc::new(OnceLock::new())
    }

    fn context(value: Value, old: Option<Value>) -> TransformContext {
        TransformContext {
            value,
            old_value: old,
            default_value: json!(50),
        }
    }

    #[test]
    fn the_written_surface_is_the_declared_one() {
        let keys: Vec<String> = declarations(&cell())
            .into_iter()
            .map(|declaration| declaration.key)
            .collect();

        assert_eq!(
            keys,
            SETTING_KEYS.iter().map(|key| key.to_string()).collect::<Vec<_>>(),
            "every declared key must already be a prop `state_bootstrap.rs` declared"
        );
        assert_eq!(SETTING_KEYS.len(), 14);
    }

    #[test]
    fn the_defaults_are_the_ones_in_state_ts() {
        let defaults: Vec<(String, Value)> = declarations(&cell())
            .into_iter()
            .map(|declaration| (declaration.key, declaration.default))
            .collect();

        for (key, expected) in [
            ("concurrency", json!(2)),
            ("enabled", json!(true)),
            ("matchHistoryLoadCount", json!(50)),
            ("matchHistoryTagPreference", json!("current")),
            ("gameDetailsLoadCount", json!(20)),
            ("orderPlayerBy", json!("default")),
            ("showChampionUsage", json!("recent")),
            ("showMatchHistoryItemBorder", json!(false)),
            ("showJunglePathing", json!(true)),
            ("showJunglePathingForAllPlayers", json!(false)),
            ("autoRouteWhenGameStarts", json!(false)),
            ("queryInLobbyPhase", json!(true)),
            ("premadeTeamInferMatchCountThreshold", json!(5)),
        ] {
            let found = defaults
                .iter()
                .find(|(key_at_all, _)| key_at_all == key)
                .unwrap_or_else(|| panic!("{key} is declared"));
            assert_eq!(found.1, expected, "{key}");
        }

        let tags = &defaults[11].1;
        for key in PLAYER_CARD_TAG_KEYS {
            assert!(tags.get(key).is_some(), "{key} has no default");
            assert_eq!(tags[key], default_player_card_tags()[key], "{key}");
        }
    }

    #[test]
    fn an_out_of_range_load_count_keeps_the_old_value() {
        let transform = clamp_load_count();

        assert_eq!(transform(&context(json!(1), Some(json!(30)))), json!(1));
        assert_eq!(transform(&context(json!(200), Some(json!(30)))), json!(200));
        assert_eq!(transform(&context(json!(0), Some(json!(30)))), json!(30));
        assert_eq!(transform(&context(json!(201), Some(json!(30)))), json!(30));

        // a first write has no `oldValue`, so the class default is what stays
        let first = TransformContext {
            value: json!(999),
            old_value: None,
            default_value: json!(50),
        };
        assert_eq!(transform(&first), json!(50));
    }

    #[test]
    fn the_details_count_is_capped_by_the_live_load_count() {
        let cell = cell();
        let transform = clamp_details_count(&cell);
        let details = |value: i64, default: i64| {
            transform(&TransformContext {
                value: json!(value),
                old_value: None,
                default_value: json!(default),
            })
        };

        // no service yet, so the declared default is the cap
        assert_eq!(details(40, 20), json!(20));
        assert_eq!(details(-1, 20), json!(20));
        assert_eq!(details(12, 20), json!(12));

        // the cap the transform applies is the crate's own maths
        assert_eq!(transform_game_details_load_count(10, 8), 8, "over the load count");
        assert_eq!(transform_game_details_load_count(8, 8), 8);
        assert_eq!(transform_game_details_load_count(0, 8), 0, "zero is allowed here");
        assert_eq!(transform_game_details_load_count(-1, 8), 8);
        assert_eq!(transform_game_details_load_count(9, 0), 0, "a zero cap stays zero");
    }

    #[test]
    fn a_lower_load_count_pulls_the_details_count_down() {
        assert_eq!(side_effect_game_details_load_count(50, 20), 20, "already below: unchanged");
        assert_eq!(side_effect_game_details_load_count(10, 20), 10, "the new load count wins");
        assert_eq!(side_effect_game_details_load_count(30, 5), 5, "raising the load count keeps it");
    }

    #[test]
    fn the_card_tags_schema_demands_every_key_and_keeps_the_rest() {
        let schema = player_card_tags_schema();

        let complete = default_player_card_tags();
        assert_eq!(schema(&complete).unwrap(), complete);

        let mut with_extra = complete.clone();
        with_extra
            .as_object_mut()
            .unwrap()
            .insert("showACustomThing".to_string(), json!(true));
        assert!(schema(&with_extra).is_ok(), "looseObject keeps unknown keys");
        assert_eq!(schema(&with_extra).unwrap()["showACustomThing"], json!(true));

        let mut missing = complete.clone();
        missing.as_object_mut().unwrap().remove("showRiftFlowScoreTag");
        assert!(
            schema(&missing).unwrap_err().contains("showRiftFlowScoreTag"),
            "a required key is named in the failure"
        );

        let mut not_a_bool = complete.clone();
        not_a_bool.as_object_mut().unwrap().insert("showSelfTag".to_string(), json!("yes"));
        assert!(schema(&not_a_bool).is_err());

        assert!(schema(&json!({})).is_err());
        assert!(schema(&json!("nope")).is_err());
    }

    #[test]
    fn a_tagged_player_needs_a_tag_and_a_summoner() {
        let saved_info = json!({
            "p1": { "puuid": "p1", "tag": "very bad indeed" },
            "p2": { "puuid": "p2", "tag": "" },
            "p3": { "puuid": "p3" },
            "p4": { "puuid": "p4", "tag": "duo with p1" },
        });
        let summoners = json!({
            "p1": { "gameName": "ONE", "tagLine": "NA1" },
            "p4": { "gameName": "FOUR", "tagLine": "EUW" },
            "p5": { "gameName": "NOT", "tagLine": "LOADED" },
            "p6": null,
        });

        assert_eq!(
            tagged_players(Some(&saved_info), Some(&summoners)),
            vec![
                TaggedPlayer {
                    puuid: "p1".to_string(),
                    name: "ONE#NA1".to_string(),
                    tag: "very bad indeed".to_string()
                },
                TaggedPlayer {
                    puuid: "p4".to_string(),
                    name: "FOUR#EUW".to_string(),
                    tag: "duo with p1".to_string()
                },
            ],
            "an empty tag, a missing tag, an unmet player and a null summoner all drop out"
        );

        assert!(tagged_players(None, Some(&summoners)).is_empty());
        assert!(tagged_players(Some(&saved_info), None).is_empty());
        assert!(tagged_players(Some(&json!({})), Some(&json!({}))).is_empty());
    }

    #[test]
    fn a_draft_has_no_room_to_send_into() {
        let conversation = json!({ "id": "room-1" });

        assert_eq!(
            reminder_room(Some(&json!({ "queueId": 420 })), Some(&conversation)),
            None,
            "`state.draft` short-circuits `itsTimeToSend`"
        );
        assert_eq!(
            reminder_room(None, Some(&conversation)),
            Some("room-1".to_string())
        );
        assert_eq!(reminder_room(None, Some(&json!({}))), None);
        assert_eq!(reminder_room(None, Some(&json!({ "id": "" }))), None);
        assert_eq!(reminder_room(None, None), None);
    }

    #[test]
    fn only_the_rise_into_end_of_game_records() {
        assert!(!entered_end_of_game(None, true), "a reaction skips its first value");
        assert!(!entered_end_of_game(None, false));
        assert!(entered_end_of_game(Some(false), true));
        assert!(
            !entered_end_of_game(Some(true), true),
            "PreEndOfGame → EndOfGame must not record twice"
        );
        assert!(!entered_end_of_game(Some(true), false));
        assert!(!entered_end_of_game(Some(false), false));
    }

    #[test]
    fn losing_the_conversation_forgets_who_was_reminded() {
        let mut reminders = Reminders::default();
        reminders.mark_reminded("p1");

        reminders.observe_room(Some("room-1".to_string()));
        assert_eq!(reminders.reminded, vec!["p1".to_string()], "a new room keeps it");

        reminders.observe_room(Some("room-2".to_string()));
        assert_eq!(reminders.reminded, vec!["p1".to_string()]);

        reminders.observe_room(None);
        assert!(reminders.reminded.is_empty(), "`itsTimeToSend` going falsy resets");

        // and the same value twice changes nothing, so nothing resets
        reminders.mark_reminded("p2");
        reminders.observe_room(None);
        assert_eq!(reminders.reminded, vec!["p2".to_string()]);
    }

    #[test]
    fn a_reminder_is_sent_once_per_player_per_conversation() {
        let tagged = vec![
            TaggedPlayer { puuid: "p1".to_string(), name: "ONE#NA1".to_string(), tag: "a".to_string() },
            TaggedPlayer { puuid: "p2".to_string(), name: "TWO#NA1".to_string(), tag: "b".to_string() },
        ];
        let room = "room-1".to_string();
        let mut reminders = Reminders::default();

        // nothing to do without a conversation
        assert!(reminders.due(None, &tagged).is_empty());

        let first = reminders.due(Some(&room), &tagged);
        assert_eq!(first.len(), 2, "the default saw no players, so both are new");
        for player in &first {
            reminders.mark_reminded(&player.puuid);
        }

        assert!(
            reminders.due(Some(&room), &tagged).is_empty(),
            "the same puuid list is the same signature"
        );

        // a third player arriving re-runs the body, but the two done ones are skipped
        let mut grown = tagged.clone();
        grown.push(TaggedPlayer {
            puuid: "p3".to_string(),
            name: "THREE#NA1".to_string(),
            tag: "c".to_string(),
        });
        let second = reminders.due(Some(&room), &grown);
        assert_eq!(second.iter().map(|player| player.puuid.as_str()).collect::<Vec<_>>(), vec!["p3"]);
    }

    #[test]
    fn the_reminder_message_is_the_originals_template() {
        // `[${t('ongoing-game-main.taggedPlayer')}: ${name}]: \n${tag}`
        assert_eq!(
            reminder_message(tagged_player_label(true), "ONE#NA1", "feeds lane"),
            "[Tagged Player: ONE#NA1]: \nfeeds lane"
        );
        assert_eq!(
            reminder_message(tagged_player_label(false), "ONE#NA1", "feeds lane"),
            "[已标记的玩家: ONE#NA1]: \nfeeds lane"
        );
    }

    #[test]
    fn the_platform_string_is_the_one_gates_are_written_against() {
        assert!(["win32", "darwin", "linux"].contains(&node_platform().as_str()));
        if cfg!(target_os = "windows") {
            assert_eq!(node_platform(), "win32");
        }
    }

    /// The wiring, end to end, with no League Client: the surfaces
    /// `state_bootstrap::declare_all` declares, a real SQLite-backed settings
    /// factory, and the crate's pipeline over the app's seams. This is what proves
    /// the module and the declared prop set agree — everything else here is unit
    /// maths.
    #[test]
    fn the_registered_surface_answers_what_the_renderer_calls() {
        let effects = tauri_free_scenario();

        // the six methods exist and answer; anything else still rejects
        assert_eq!(
            effects.call("getAll", vec![]),
            json!({
                "matchHistory": {}, "summoner": {}, "rankedStats": {}, "savedInfo": {},
                "championMastery": {}, "gameDetails": {}, "additionalGames": {},
            }),
            "the seven maps, empty because no game is live — not an invented payload"
        );
        assert!(effects.call_raw("reloadPlayer", vec![json!("p1")]).is_ok());
        assert!(effects.call_raw("setDraft", vec![json!({ "queueId": 420 })]).is_ok());
        assert!(effects
            .call_raw("noSuchFunction", vec![])
            .is_err(),
            "an unknown method rejects, as Electron rethrew");

        // `getAll` after a draft is set is still the same seven keys, but the prop
        // the renderer's store reads moved
        assert_eq!(
            effects.sync.get(NAMESPACE, "state", "draft"),
            Some(json!({ "queueId": 420 }))
        );
        assert!(effects.call_raw("clearDraft", vec![]).is_ok());
        assert_eq!(effects.sync.get(NAMESPACE, "state", "draft"), Some(Value::Null));

        // the mirror replaced the empty object `declare_all` had to fall back to
        let tags = effects
            .sync
            .get(NAMESPACE, "settings", "playerCardTags")
            .expect("playerCardTags is a prop");
        assert_eq!(tags.as_object().map(|object| object.len()), Some(21));

        // and a write goes transform → side effect → commit → prop
        effects.set_setting("matchHistoryLoadCount", json!(300));
        assert_eq!(
            effects.sync.get(NAMESPACE, "settings", "matchHistoryLoadCount"),
            Some(json!(50)),
            "out of 1..=200 keeps the old value"
        );
        effects.set_setting("matchHistoryLoadCount", json!(10));
        assert_eq!(
            effects.sync.get(NAMESPACE, "settings", "matchHistoryLoadCount"),
            Some(json!(10))
        );
        assert_eq!(
            effects.sync.get(NAMESPACE, "settings", "gameDetailsLoadCount"),
            Some(json!(10)),
            "Math.min(10, 20): the details count followed down"
        );

        // one pass over the real surfaces: nothing live, so `unavailable`, and the
        // crate's own prop set is complete enough to write without failing. It goes
        // through the runtime because a reaction body spawns its loaders, exactly as
        // the pipeline task does.
        let changed = tauri::async_runtime::block_on(async { effects.game.run_pass() })
            .is_some();
        assert_eq!(
            effects.sync.get(NAMESPACE, "state", "queryStage"),
            Some(json!({ "phase": "unavailable", "gameInfo": null }))
        );
        assert_eq!(
            effects.sync.get(NAMESPACE, "state", "teams"),
            Some(json!({})),
            "changed={changed}"
        );

        // and the watchers come up: the six LCU subscriptions, the crate's reaction
        // task, the backstop tick and the side-effects task
        start(&effects.game, &effects.league_client, &effects.sync, &effects.settings)
            .expect("the watchers subscribe and launch");

        // let the loop take at least one turn; disconnected, so it must still be
        // `unavailable` and must not have invented anything
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            effects.sync.get(NAMESPACE, "state", "queryStage"),
            Some(json!({ "phase": "unavailable", "gameInfo": null })),
            "the running loop kept the honest empty state"
        );
        assert_eq!(
            effects.call("getAll", vec![]),
            json!({
                "matchHistory": {}, "summoner": {}, "rankedStats": {}, "savedInfo": {},
                "championMastery": {}, "gameDetails": {}, "additionalGames": {},
            }),
            "no loader filled anything without a client"
        );

        effects.finish();
    }

    struct Scenario {
        ipc: Arc<RiftFlowIpcMain>,
        sync: Arc<StateSync>,
        settings: Arc<SettingFactory>,
        league_client: Arc<LeagueClientMain>,
        game: Arc<OngoingGame>,
        dir: std::path::PathBuf,
    }

    impl Scenario {
        fn call(&self, method: &str, args: Vec<Value>) -> Value {
            self.call_raw(method, args).expect(method)
        }

        /// the reply's `data`, or an error for anything that did not succeed
        fn call_raw(&self, method: &str, args: Vec<Value>) -> Result<Value, String> {
            let ipc = self.ipc.clone();
            let namespace = NAMESPACE.to_string();
            let name = method.to_string();

            tauri::async_runtime::block_on(async move {
                match ipc.handle_call(1, &namespace, &name, args).await {
                    riftflow_ipc::CallOutcome::Reply(envelope) => envelope
                        .data
                        .filter(|_| envelope.success)
                        .map(Ok)
                        .unwrap_or_else(|| Err("envelope carried no data".to_string())),
                    riftflow_ipc::CallOutcome::Rejected(message) => Err(message),
                }
            })
        }

        fn set_setting(&self, key: &str, value: Value) {
            let settings = self.settings.clone();
            let key = key.to_string();

            tauri::async_runtime::block_on(async move {
                settings.set_setting(NAMESPACE, &key, value).await.unwrap();
            });
        }

        fn finish(self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    fn tauri_free_scenario() -> Scenario {
        let dir = std::env::temp_dir().join(format!(
            "riftflow-ongoing-game-wiring-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("a temp directory for the scenario");

        let (db, _) = riftflow_store::Database::initialize(&dir.join("riftflow.db")).expect("a database");
        let settings = Arc::new(SettingFactory::new(Arc::new(Mutex::new(db))));
        let ipc = Arc::new(RiftFlowIpcMain::new(Arc::new(NullTransport)));
        let sync = Arc::new(StateSync::new(ipc.clone()));

        // exactly what the shell declares before a shard's handlers go in
        crate::state_bootstrap::declare_all(&sync, &settings, false, "info").expect("the surfaces");

        let league_client = Arc::new(LeagueClientMain::new(
            ipc.clone(),
            sync.clone(),
            Arc::new(NoCredentials),
            Arc::new(riftflow_lcu::ProductionDialer),
        ));
        let game = register(
            &ipc,
            &sync,
            &league_client,
            &settings,
            &Arc::new(riftflow_sgp::SgpGateway::new()),
            "1.5.1",
        )
        .expect("the shard registers");

        Scenario {
            ipc,
            sync,
            settings,
            league_client,
            game,
            dir,
        }
    }

    #[derive(Default)]
    struct NullTransport;

    impl riftflow_ipc::RendererTransport for NullTransport {
        fn send_to(
            &self,
            _renderer: riftflow_ipc::RendererId,
            _event: &str,
            _args: Vec<Value>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// the trait's three methods, with nothing stored
    struct NoCredentials;

    impl riftflow_lcu::CredentialStore for NoCredentials {
        fn save_last_connected_client(&self, _value: &Value) {}

        fn get_last_connected_client(&self) -> Option<Value> {
            None
        }

        fn remove_last_connected_client(&self) {}
    }
}
