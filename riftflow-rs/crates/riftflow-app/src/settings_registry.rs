//! One home for every `*:settings` surface: the schema, validation, the debounced
//! write, the `update-state-prop` push, and a broadcast that feature shards react
//! to.
//!
//! In Electron these were observable objects, so a write automatically notified
//! both the renderers and main-side `reaction`s. Here one write has to do all
//! three, which is why they live behind a single `register_namespace` call.

use std::sync::Arc;

use riftflow_state::StateSync;
use riftflow_store::settings::{
    SettingDeclaration, SettingFactory, boolean, number, string, string_enum,
};
use serde_json::Value;
use tokio::sync::broadcast;

/// `(namespace, key, value)` after the value has been committed.
pub type SettingChange = (String, String, Value);

pub struct SettingsHub {
    changes: broadcast::Sender<SettingChange>,
}

impl SettingsHub {
    pub fn subscribe(&self) -> broadcast::Receiver<SettingChange> {
        self.changes.subscribe()
    }
}

fn setting(key: &str, default: Value, schema: Option<riftflow_store::settings::Validator>) -> SettingDeclaration {
    SettingDeclaration {
        key: key.to_string(),
        default,
        schema,
        transform: None,
        side_effect: None,
    }
}

fn flag(key: &str, default: bool) -> SettingDeclaration {
    setting(key, Value::Bool(default), Some(boolean()))
}

fn amount(key: &str, default: i64) -> SettingDeclaration {
    setting(key, Value::from(default), Some(number()))
}

fn choice(key: &str, default: &str, allowed: &[&str]) -> SettingDeclaration {
    setting(key, Value::String(default.to_string()), Some(string_enum(allowed)))
}

/// A record/array setting: stored as given, since the original validates those with
/// `z.record(...)` shapes that the renderer is responsible for building.
fn structured(key: &str, default: Value) -> SettingDeclaration {
    setting(key, default, None)
}

/// `auto-gameflow-main` — `AutoGameflowSettings` + its `register` schema.
fn auto_gameflow() -> Vec<SettingDeclaration> {
    vec![
        flag("autoAcceptEnabled", false),
        amount("autoAcceptDelaySeconds", 0),
        flag("autoHonorEnabled", false),
        choice(
            "autoHonorStrategy",
            "prefer-lobby-member",
            &[
                "prefer-lobby-member",
                "only-lobby-member",
                "all-member",
                "opt-out",
                "all-member-including-opponent",
            ],
        ),
        flag("autoMatchmakingEnabled", false),
        amount("autoMatchmakingDelaySeconds", 5),
        amount("autoMatchmakingMinimumMembers", 1),
        amount("autoMatchmakingMaximumMatchDuration", 0),
        choice(
            "autoMatchmakingRematchStrategy",
            "never",
            &["never", "fixed-duration", "estimated-duration"],
        ),
        amount("autoMatchmakingRematchFixedDuration", 2),
        flag("autoMatchmakingWaitForInvitees", true),
        flag("autoSkipLeaderEnabled", false),
        flag("playAgainEnabled", false),
        flag("autoHandleInvitationsEnabled", false),
        flag("autoReconnectEnabled", false),
        structured("invitationHandlingStrategies", serde_json::json!({})),
        flag("rejectInvitationWhenAway", false),
        flag("autoSendARAMTeamSideEnabled", false),
        flag("autoSendARAMTeamSideVisibleToTeam", false),
    ]
}

/// `respawn-timer-main` — `RespawnTimerSettings`.
fn respawn_timer() -> Vec<SettingDeclaration> {
    vec![flag("enabled", true)]
}

/// `auto-select-main` — `AutoSelectSettings`. Both are per-group records the
/// settings page builds, so they are stored as given.
fn auto_select() -> Vec<SettingDeclaration> {
    vec![
        structured("pickConfig", serde_json::json!({})),
        structured("banConfig", serde_json::json!({})),
    ]
}

/// `auto-misc-main` — `AutoMiscSettings`.
fn auto_misc() -> Vec<SettingDeclaration> {
    vec![
        flag("autoReplyEnabled", false),
        flag("autoReplyEnableOnAway", false),
        setting("autoReplyText", serde_json::json!(""), Some(string())),
        flag("lockOfflineStatus", false),
        flag("autoSetStatusMessageEnabled", false),
        setting("statusMessage", serde_json::json!(""), Some(string())),
        flag("autoSetRankedStatusEnabled", false),
        structured(
            "rankedStatus",
            serde_json::json!({ "queue": "RANKED_SOLO_5x5", "tier": "CHALLENGER", "division": "I" }),
        ),
    ]
}

/// `auto-champ-config-main` — `AutoChampConfigSettings`.
fn auto_champ_config() -> Vec<SettingDeclaration> {
    vec![
        flag("enabled", false),
        structured("runesV2", serde_json::json!({})),
        structured("summonerSpells", serde_json::json!({})),
    ]
}

/// `in-game-send-main` — `InGameSendSettings`. The preset-option records are the
/// renderer's own concern (`createDefault*` in `@shared/shards/in-game-send`), but
/// they have to exist: the presets page reads `ratingPresetOptions.targetShortcuts`
/// during setup and throws if the prop is missing.
fn in_game_send() -> Vec<SettingDeclaration> {
    let target_shortcuts = serde_json::json!({ "friendly": null, "enemy": null, "all": null });
    vec![
        amount("sendInterval", 65),
        setting("cancelShortcut", Value::Null, None),
        structured(
            "ratingPresetOptions",
            serde_json::json!({
                "targetShortcuts": target_shortcuts.clone(),
                "kda": true,
                "winRate": true,
                "avgSoloKills": true,
                "avgVisionScore": false,
                "avgChampionDamage": false,
                "avgDamageTaken": false,
                "avgGold": false,
                "avgCsPerMinute": false,
                "avgKillParticipation": false,
                "avgDamageGoldEfficiency": false,
                "mainChampions": true,
                "mainPositions": true,
                "nameDisplayStrategy": "preferChampionName",
                "showCurrentChampion": false,
            }),
        ),
        structured(
            "junglePresetOptions",
            serde_json::json!({
                "targetShortcuts": target_shortcuts.clone(),
                "activityPreference": true,
                "firstClearDistribution": true,
                "earlyGank": true,
                "dragonControl": true,
                "monsterControl": true,
                "mainChampions": true,
                "nameDisplayStrategy": "preferChampionName",
                "showCurrentChampion": true,
            }),
        ),
        structured(
            "premadePresetOptions",
            serde_json::json!({
                "targetShortcuts": target_shortcuts.clone(),
                "nameDisplayStrategy": "preferChampionName",
            }),
        ),
        structured("fixedTextPresetItems", serde_json::json!([])),
    ]
}

/// Namespaces whose `:settings` surface this registry owns, so
/// `state_bootstrap::declare_all` must not declare them a second time.
pub const OWNED_NAMESPACES: &[&str] = &[
    "auto-gameflow-main",
    "auto-champ-config-main",
    "auto-misc-main",
    "auto-select-main",
    "in-game-send-main",
    "respawn-timer-main",
];

pub fn register_all(
    state_sync: &Arc<StateSync>,
    factory: &Arc<SettingFactory>,
) -> anyhow::Result<Arc<SettingsHub>> {
    let (changes, _) = broadcast::channel(128);
    let hub = Arc::new(SettingsHub { changes });

    // AppCommon is registered before this hub. Forward its language changes too,
    // so the notice refresh loop observes the same settings as the renderer.
    if let Some(service) = factory.get_service(crate::app_common::APP_COMMON_NAMESPACE) {
        let sync = state_sync.clone();
        let sender = hub.changes.clone();
        service.set_change_sink(Arc::new(move |namespace, key, value| {
            let _ = sync.set(namespace, "settings", key, value.clone());
            let _ = sender.send((namespace.to_string(), key.to_string(), value.clone()));
        }));
    }

    register_namespace(state_sync, factory, &hub, "auto-gameflow-main", auto_gameflow())?;
    register_namespace(state_sync, factory, &hub, "auto-select-main", auto_select())?;
    register_namespace(state_sync, factory, &hub, "auto-misc-main", auto_misc())?;
    register_namespace(state_sync, factory, &hub, "auto-champ-config-main", auto_champ_config())?;
    register_namespace(state_sync, factory, &hub, "in-game-send-main", in_game_send())?;
    register_namespace(state_sync, factory, &hub, "respawn-timer-main", respawn_timer())?;

    Ok(hub)
}

fn register_namespace(
    state_sync: &Arc<StateSync>,
    factory: &Arc<SettingFactory>,
    hub: &Arc<SettingsHub>,
    namespace: &str,
    declarations: Vec<SettingDeclaration>,
) -> anyhow::Result<()> {
    let service = factory.register(namespace, declarations)?;
    // `_getAllFromStorage` → the object the propSync surface mirrors
    service.apply_to_state()?;

    let sync = state_sync.clone();
    let sender = hub.changes.clone();
    service.set_change_sink(Arc::new(move |ns, key, value| {
        let _ = sync.set(ns, "settings", key, value.clone());
        let _ = sender.send((ns.to_string(), key.to_string(), value.clone()));
    }));

    let loaded = service.entries();
    let props: Vec<(&str, Value)> = loaded
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    state_sync.prop_sync(namespace, "settings", &props, true)?;

    Ok(())
}
