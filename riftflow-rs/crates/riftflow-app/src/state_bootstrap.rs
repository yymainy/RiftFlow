//! Declares every `propSync` surface the renderer shards subscribe to on boot.
//!
//! The list is extracted mechanically from `src/main/shards/**/index.ts`
//! (30 declarations) and mirrored here so a shard's
//! `subscribeAndGetInitialState` never fails for an un-ported namespace — in the
//! original, a missing state throws and stalls the whole renderer setup.
//!
//! Values fall into two kinds:
//! * **real** — computed here (`storage-main`, `app-common-main`, …)
//! * **placeholder** — typed-but-inert, listed in `PLACEHOLDER_NAMESPACES` and
//!   logged at startup, so nothing claims a feature works before its shard lands.

use std::sync::Arc;

use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;
use serde_json::{Value, json};

/// Namespaces whose state is declared but not yet driven by a ported shard.
/// Every one that used to be here (self-update, sgp, client-installation,
/// game-client, renderer-debug) has since been driven and verified against the
/// live client, so the list is empty again — keep it as the place to put a
/// namespace back if a shard is ever re-opened as inert.
pub const PLACEHOLDER_NAMESPACES: &[&str] = &[];

const NULL: Value = Value::Null;

/// `AdditionalResult`'s boot value: five empty records, not an empty list — the
/// cd-timer window does `Object.entries(ais.additional.selections)` unguarded.
fn additional_shape() -> Value {
    json!({
        "teams": {},
        "selections": {},
        "teamParticipantGroups": {},
        "spells": {},
        "positions": {},
    })
}

/// A `Record` prop: `{}` at boot, matching the TS field initializer.
fn empty_record_value() -> Value {
    json!({})
}

/// `shouldRunSelfUpdateLifecycle()`.
fn self_update_supported() -> bool {
    crate::self_update::lifecycle_supported()
}

pub fn declare_all(
    state_sync: &Arc<StateSync>,
    settings: &SettingFactory,
    using_higher_version_db: bool,
    log_level: &str,
) -> anyhow::Result<Vec<String>> {
    let mut declared: Vec<String> = Vec::new();

    // `LoggerFactoryMain` syncs a single bare-string prop, not an array
    declare(
        state_sync,
        &mut declared,
        settings,
        "logger-factory-main",
        "state",
        &[("logLevel", json!(log_level))],
    )?;

    // `StorageMain` — real value, from the version ladder result
    state_sync.prop_sync(
        "storage-main",
        "state",
        &[("usingHigherVersionDb", json!(using_higher_version_db))],
        true,
    )?;
    declared.push("storage-main:state".to_string());

    // `RiftFlowApiMain.state` — remote notice / contact channels / feature gates.
    // `RiftFlowApiState` defaults all three to null (the fetch is what fills them),
    // so null is the faithful pre-load value, not an invented empty object.
    declare(
        state_sync,
        &mut declared,
        settings,
        "riftflow-api-main",
        "state",
        &[("featureGates", NULL), ("notice", NULL), ("contactChannels", NULL)],
    )?;

    // `SelfUpdateMain`
    declare(
        state_sync,
        &mut declared,
        settings,
        "self-update-main",
        "state",
        &[
            ("isUpdateSupportedOnCurrentPlatform", json!(self_update_supported())),
            ("isCheckingUpdates", json!(false)),
            ("releaseInfo", NULL),
            ("updateProgressInfo", NULL),
            ("lastUpdateSucceeded", NULL),
        ],
    )?;
    declare(
        state_sync,
        &mut declared,
        settings,
        "self-update-main",
        "settings",
        &[
            ("autoCheckUpdates", json!(true)),
            ("autoDownloadUpdates", json!(false)),
            ("ignoreVersion", NULL),
        ],
    )?;

    // `SgpMain.state` — availability is derived from league-servers config
    declare(
        state_sync,
        &mut declared,
        settings,
        "sgp-main",
        "state",
        &[
            (
                "availability",
                json!({
                    "region": "",
                    "rsoPlatform": "",
                    "sgpServerId": "",
                    "serversSupported": { "matchHistory": false, "common": false },
                }),
            ),
            ("isTokenReady", json!(false)),
            ("leagueServers", json!({ "servers": {}, "serverNames": {}, "updatedAt": "" })),
            ("supportedQueues", json!([])),
            ("connectionSuccessesCounted", json!(0)),
            ("connectionFailuresCounted", json!(0)),
        ],
    )?;

    // `OngoingGameMain`
    declare(
        state_sync,
        &mut declared,
        settings,
        "ongoing-game-main",
        "state",
        &[
            ("championSelections", empty_record_value()),
            ("positionAssignments", json!({})),
            ("analysis", NULL),
            ("queryStage", json!({ "phase": "unavailable", "gameInfo": null })),
            ("teams", empty_record_value()),
            ("isInEog", json!(false)),
            ("matchHistoryLoadingState", empty_record_value()),
            ("summonerLoadingState", empty_record_value()),
            ("savedInfoLoadingState", empty_record_value()),
            ("rankedStatsLoadingState", empty_record_value()),
            ("championMasteryLoadingState", empty_record_value()),
            ("teamParticipantGroups", empty_record_value()),
            ("matchHistoryTagParams", json!({})),
            ("draft", NULL),
            ("additional", additional_shape()),
            ("mergedPremadeTeamMap", empty_record_value()),
            ("inferredPremadeTeams", json!([])),
        ],
    )?;
    declare(
        state_sync,
        &mut declared,
        settings,
        "ongoing-game-main",
        "settings",
        &[
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
            ("playerCardTags", json!({})),
            ("queryInLobbyPhase", json!(true)),
            ("premadeTeamInferMatchCountThreshold", json!(5)),
        ],
    )?;

    // `AutoSelectMain`
    declare(
        state_sync,
        &mut declared,
        settings,
        "auto-select-main",
        "state",
        &[
            ("groups", json!([])),
            ("temporarilyDisabled", json!(false)),
            ("delayedBan", NULL),
            ("delayedPick", NULL),
            ("delayedBenchSwap", NULL),
            ("delayedChampionSwap", NULL),
            ("expectedPicks", json!([])),
            ("expectedBans", json!([])),
            ("expectedSwaps", json!([])),
            ("activeGroupConfigId", NULL),
        ],
    )?;
    declare(
        state_sync,
        &mut declared,
        settings,
        "auto-select-main",
        "settings",
        &[("pickConfig", json!({})), ("banConfig", json!({}))],
    )?;

    // `AutoGameflowMain`
    declare(
        state_sync,
        &mut declared,
        settings,
        "auto-gameflow-main",
        "state",
        &[
            ("willAcceptAt", json!(-1)),
            ("willSearchMatch", json!(false)),
            ("willSearchMatchAt", json!(-1)),
            ("activityStartStatus", json!("unavailable")),
            ("friendsToBeInvited", json!([])),
        ],
    )?;
    declare(
        state_sync,
        &mut declared,
        settings,
        "auto-gameflow-main",
        "settings",
        &[
            ("autoAcceptDelaySeconds", json!(0)),
            ("autoAcceptEnabled", json!(false)),
            ("autoHonorEnabled", json!(false)),
            ("autoHonorStrategy", json!("prefer-lobby-member")),
            ("autoMatchmakingDelaySeconds", json!(5)),
            ("autoMatchmakingEnabled", json!(false)),
            ("autoMatchmakingMinimumMembers", json!(1)),
            ("autoMatchmakingRematchFixedDuration", json!(2)),
            ("autoMatchmakingRematchStrategy", json!("never")),
            ("autoMatchmakingWaitForInvitees", json!(true)),
            ("autoSkipLeaderEnabled", json!(false)),
            ("playAgainEnabled", json!(false)),
            ("autoHandleInvitationsEnabled", json!(false)),
            ("autoReconnectEnabled", json!(false)),
            ("autoMatchmakingMaximumMatchDuration", json!(0)),
            ("invitationHandlingStrategies", json!({})),
            ("rejectInvitationWhenAway", json!(false)),
            ("autoSendARAMTeamSideEnabled", json!(false)),
            ("autoSendARAMTeamSideVisibleToTeam", json!(false)),
        ],
    )?;

    // `AutoMiscMain` and `AutoChampionConfigMain` are owned by
    // `settings_registry`, which declares their `:settings` surface.

    // `InGameSendMain`
    declare(
        state_sync,
        &mut declared,
        settings,
        "in-game-send-main",
        "state",
        &[
            ("ratingPuuids", json!([])),
            ("junglePuuids", json!([])),
            ("premadeIndices", json!([])),
        ],
    )?;
    declare(
        state_sync,
        &mut declared,
        settings,
        "in-game-send-main",
        "settings",
        &[
            ("sendInterval", json!(65)),
            ("cancelShortcut", NULL),
            ("ratingPresetOptions", json!({})),
            ("junglePresetOptions", json!({})),
            ("premadePresetOptions", json!({})),
            ("fixedTextPresetItems", json!([])),
        ],
    )?;

    // `ClientInstallationMain`
    declare(
        state_sync,
        &mut declared,
        settings,
        "client-installation-main",
        "state",
        &[
            ("leagueClientExecutablePaths", json!([])),
            ("tencentInstallationPath", NULL),
            ("weGameExecutablePath", NULL),
            ("officialRiotClientExecutablePath", NULL),
            ("tclsExecutablePath", NULL),
            ("weGameLauncherExecutablePath", NULL),
            ("detectedLiveStreamingClients", json!([])),
        ],
    )?;

    // `GameClientMain`
    declare(
        state_sync,
        &mut declared,
        settings,
        "game-client-main",
        "settings",
        &[
            ("terminateGameClientWithShortcut", json!(false)),
            ("terminateShortcut", NULL),
        ],
    )?;

    // `RendererDebugMain`
    declare(
        state_sync,
        &mut declared,
        settings,
        "renderer-debug-main",
        "state",
        &[
            ("sendAllNativeLcuEvents", json!(false)),
            ("logAllLcuEvents", json!(false)),
        ],
    )?;

    // `RespawnTimerMain` — `info` is an object with a real default, not null:
    // `SidebarFixed.vue` reads `rts.info.isDead` during setup
    declare(
        state_sync,
        &mut declared,
        settings,
        "respawn-timer-main",
        "state",
        &[(
            "info",
            json!({ "timeLeft": 0, "totalTime": 0, "isDead": false }),
        )],
    )?;
    declare(
        state_sync,
        &mut declared,
        settings,
        "respawn-timer-main",
        "settings",
        &[("enabled", json!(true))],
    )?;

    // `WindowManagerMain` — the base namespace plus one sub-namespace per window.
    // `getStatePropKeys()` additions are per-window and listed below.
    declare(
        state_sync,
        &mut declared,
        settings,
        "window-manager-main",
        "state",
        &[("supportsMica", json!(false)), ("downloadTasks", json!([]))],
    )?;
    declare(
        state_sync,
        &mut declared,
        settings,
        "window-manager-main",
        "settings",
        &[
            ("backgroundMaterial", json!("none")),
            ("contentProtection", json!(false)),
        ],
    )?;

    for (suffix, extra_state, extra_settings) in window_manager_windows() {
        let namespace = format!("window-manager-main/{suffix}");

        let mut state_props: Vec<(&str, Value)> = vec![
            // `BaseRiftFlowWindowBasicState`: focus and status are string unions,
            // not booleans — the renderer's store defaults mirror these
            ("focus", json!("focused")),
            ("ready", json!(false)),
            ("status", json!("normal")),
            // every riftflow window is created with `show: false`
            ("show", json!(false)),
            ("trackedBounds", NULL),
        ];
        state_props.extend(extra_state.iter().map(|(key, value)| (*key, value.clone())));

        declare(state_sync, &mut declared, settings, &namespace, "state", &state_props)?;

        let mut settings_props: Vec<(&str, Value)> = vec![
            ("opacity", json!(1)),
            (
                "pinned",
                json!(matches!(
                    suffix.as_str(),
                    "aux-window" | "opgg-window" | "ongoing-game-window" | "cd-timer-window"
                )),
            ),
        ];
        settings_props.extend(extra_settings.iter().map(|(key, value)| (*key, value.clone())));

        declare(
            state_sync,
            &mut declared,
            settings,
            &namespace,
            "settings",
            &settings_props,
        )?;
    }

    // `LeagueClientData` sub-states (`lc-state/*`) — each is its own sync surface,
    // and the prop paths are dotted (`participants.postGame`), which a
    // state-object scan would miss.
    for (state_id, props) in league_client_states() {
        let typed: Vec<(&str, Value)> = props
            .iter()
            .map(|(path, value)| (*path, value.clone()))
            .collect();
        declare(state_sync, &mut declared, settings, "league-client-main", &state_id, &typed)?;
    }

    // `ExtraAssetsMain`
    declare(
        state_sync,
        &mut declared,
        settings,
        "extra-assets-main",
        "gtimg",
        &[("heroList", json!([])), ("kiwiAugments", json!([]))],
    )?;
    declare(
        state_sync,
        &mut declared,
        settings,
        "extra-assets-main",
        "opgg",
        &[("aramBalance", NULL)],
    )?;

    // `OngoingGameMain.additional`
    declare(
        state_sync,
        &mut declared,
        settings,
        "ongoing-game-main",
        "additional",
        &[("additional", additional_shape())],
    )?;

    Ok(declared)
}

/// Prop shapes follow `lc-state/*.ts` field defaults exactly: a `Record` is `{}`,
/// a nullable object is `null`, and only real arrays are `[]`. Getting this wrong
/// is not cosmetic — `n.supportedQueues.map is not a function` came from declaring
/// a computed array as an object.
#[allow(clippy::type_complexity)]
fn league_client_states() -> Vec<(String, Vec<(&'static str, Value)>)> {
    let empty_record: fn() -> Value = || json!({});

    vec![
        ("champSelect".to_string(), vec![
            ("session", NULL),
            ("gridChampions", empty_record()),
            ("currentPickableChampionIds", json!([])),
            ("currentBannableChampionIds", json!([])),
            ("disabledChampionIds", json!([])),
            ("currentChampion", json!(0)),
            ("ongoingChampionSwap", NULL),
            ("skinSelectorInfo", NULL),
        ]),
        ("chat".to_string(), vec![
            ("me", NULL),
            ("participants.postGame", NULL),
            ("participants.customGame", NULL),
            ("participants.championSelect", NULL),
            ("conversations.postGame", NULL),
            ("conversations.customGame", NULL),
            ("conversations.championSelect", NULL),
        ]),
        ("entitlements".to_string(), vec![("token", NULL)]),
        ("gameData".to_string(), vec![
            ("champions", empty_record()),
            ("items", empty_record()),
            ("perks", empty_record()),
            ("perkstyles", json!({ "schemaVersion": 0, "styles": {} })),
            ("queues", empty_record()),
            ("summonerSpells", empty_record()),
            ("augments", empty_record()),
            ("gameModeMutators", empty_record()),
            ("maps", empty_record()),
        ]),
        ("gameflow".to_string(), vec![("phase", NULL), ("session", NULL)]),
        ("honor".to_string(), vec![("ballot", NULL)]),
        ("initialization".to_string(), vec![("progress", NULL)]),
        ("leagueSession".to_string(), vec![("token", NULL)]),
        ("lobby".to_string(), vec![
            ("lobby", NULL),
            ("receivedInvitations", json!([])),
        ]),
        ("lobbyTeamBuilder".to_string(), vec![
            ("champSelect.subsetChampionList", json!([])),
        ]),
        ("login".to_string(), vec![("loginQueueState", NULL)]),
        ("matchmaking".to_string(), vec![
            ("readyCheck", NULL),
            ("search", NULL),
        ]),
        ("summoner".to_string(), vec![("me", NULL), ("profile", NULL)]),
    ]
}

/// `getStatePropKeys()` / `getSettingPropKeys()` per window subclass.
#[allow(clippy::type_complexity)]
fn window_manager_windows(
) -> Vec<(String, Vec<(&'static str, Value)>, Vec<(&'static str, Value)>)> {
    vec![
        (
            "main-window".to_string(),
            vec![],
            vec![("closeAction", json!("ask"))],
        ),
        (
            "aux-window".to_string(),
            vec![],
            vec![
                ("enabled", json!(true)),
                ("autoShow", json!(true)),
                ("showSkinSelector", json!(false)),
            ],
        ),
        (
            "opgg-window".to_string(),
            vec![],
            vec![
                ("enabled", json!(true)),
                ("autoShow", json!(true)),
                ("showShortcut", Value::Null),
            ],
        ),
        (
            "ongoing-game-window".to_string(),
            vec![("fakeShow", json!(false))],
            vec![
                ("enabled", json!(false)),
                ("showShortcut", Value::Null),
            ],
        ),
        (
            "cd-timer-window".to_string(),
            vec![
                // `CdTimerWindowState.supportedGameModes` is a hardcoded default,
                // so this is the real value rather than a placeholder
                (
                    "supportedGameModes",
                    json!([
                        { "gameMode": "CLASSIC", "abilityHaste": 0 },
                        { "gameMode": "PRACTICETOOL", "abilityHaste": 0 },
                        { "gameMode": "ARAM", "abilityHaste": 70 },
                        { "gameMode": "URF", "abilityHaste": 300 },
                        { "gameMode": "ONEFORALL", "abilityHaste": 0 },
                        { "gameMode": "NEXUSBLITZ", "abilityHaste": 0 },
                        { "gameMode": "ULTBOOK", "abilityHaste": 0 },
                        { "gameMode": "KIWI", "abilityHaste": 70 },
                    ]),
                ),
                ("gameTime", NULL),
            ],
            vec![
                ("enabled", json!(false)),
                ("showShortcut", Value::Null),
                ("timerType", json!("countdown")),
                ("reverseAdjustmentDirection", json!(false)),
            ],
        ),
    ]
}

fn declare(
    state_sync: &Arc<StateSync>,
    declared: &mut Vec<String>,
    settings: &SettingFactory,
    namespace: &str,
    state_id: &str,
    props: &[(&str, Value)],
) -> anyhow::Result<()> {
    if state_id == "settings" {
        return declare_settings(state_sync, declared, settings, namespace, props);
    }
    state_sync.prop_sync(namespace, state_id, props, true)?;
    declared.push(format!("{namespace}:{state_id}"));
    Ok(())
}

/// `SettingsService.applyToState()`: a stored row wins over the declared default,
/// so a setting the user changed survives a restart instead of flashing back.
fn declare_settings(
    state_sync: &Arc<StateSync>,
    declared: &mut Vec<String>,
    settings: &SettingFactory,
    namespace: &str,
    props: &[(&str, Value)],
) -> anyhow::Result<()> {
    if crate::settings_registry::OWNED_NAMESPACES.contains(&namespace) {
        // registered with a real schema (validators + change push) there
        return Ok(());
    }
    let stored: Vec<(&str, Value)> = props
        .iter()
        .map(|(key, default)| {
            let value = settings
                .get_raw(namespace, key)
                .ok()
                .flatten()
                .unwrap_or_else(|| default.clone());
            (*key, value)
        })
        .collect();

    state_sync.prop_sync(namespace, "settings", &stored, true)?;
    declared.push(format!("{namespace}:settings"));
    Ok(())
}
