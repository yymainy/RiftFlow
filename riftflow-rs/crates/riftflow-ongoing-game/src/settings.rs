//! `state.ts` `OngoingGameSettings` and the `index.ts` registration transforms.
//!
//! The values are read off the synced `ongoing-game-main` settings surface rather
//! than owned here, because `riftflow-store::settings::SettingFactory` is what the
//! app already writes through; this is the typed view of it, default-for-default.

use serde_json::Value;

use crate::record::{as_str, bool_field};

/// `OngoingGameMatchHistoryTagPreference`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagPreference {
    Current,
    All,
}

/// `OngoingGamePanelOrderPlayerBy` — the panel's own sort, mirrored so the enum
/// cannot silently drift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderPlayerBy {
    WinRate,
    Kda,
    Default,
    RiftFlowScore,
    Position,
    PremadeTeam,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OngoingGameSettings {
    pub enabled: bool,
    pub match_history_load_count: i64,
    pub game_details_load_count: i64,
    pub concurrency: i64,
    pub match_history_tag_preference: TagPreference,
    pub order_player_by: OrderPlayerBy,
    pub show_champion_usage: String,
    pub show_match_history_item_border: bool,
    pub show_jungle_pathing: bool,
    pub show_jungle_pathing_for_all_players: bool,
    pub auto_route_when_game_starts: bool,
    pub player_card_tags: Value,
    pub query_in_lobby_phase: bool,
    pub premade_team_infer_match_count_threshold: i64,
}

impl Default for OngoingGameSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            match_history_load_count: 50,
            game_details_load_count: 20,
            concurrency: 2,
            match_history_tag_preference: TagPreference::Current,
            order_player_by: OrderPlayerBy::Default,
            show_champion_usage: "recent".to_string(),
            show_match_history_item_border: false,
            show_jungle_pathing: true,
            show_jungle_pathing_for_all_players: false,
            auto_route_when_game_starts: false,
            player_card_tags: default_player_card_tags(),
            query_in_lobby_phase: true,
            premade_team_infer_match_count_threshold: 5,
        }
    }
}

impl OngoingGameSettings {
    /// Missing or unparseable props fall back to the class default, which is what
    /// a partially-declared `propSync` surface looked like in the original.
    pub fn from_value(value: &Value) -> Self {
        let defaults = Self::default();
        let number = |key: &str, default: i64| match value.get(key).and_then(Value::as_i64) {
            Some(read) => read,
            None => default,
        };

        Self {
            enabled: bool_field(value, "enabled", defaults.enabled),
            match_history_load_count: number(
                "matchHistoryLoadCount",
                defaults.match_history_load_count,
            ),
            game_details_load_count: number("gameDetailsLoadCount", defaults.game_details_load_count),
            concurrency: number("concurrency", defaults.concurrency),
            match_history_tag_preference: match as_str(value.get("matchHistoryTagPreference")) {
                Some("all") => TagPreference::All,
                Some("current") => TagPreference::Current,
                _ => defaults.match_history_tag_preference,
            },
            order_player_by: match as_str(value.get("orderPlayerBy")) {
                Some("win-rate") => OrderPlayerBy::WinRate,
                Some("kda") => OrderPlayerBy::Kda,
                Some("riftflow-score") => OrderPlayerBy::RiftFlowScore,
                Some("position") => OrderPlayerBy::Position,
                Some("premade-team") => OrderPlayerBy::PremadeTeam,
                _ => OrderPlayerBy::Default,
            },
            show_champion_usage: value
                .get("showChampionUsage")
                .and_then(Value::as_str)
                .filter(|usage| matches!(*usage, "recent" | "mastery" | "none"))
                .unwrap_or("recent")
                .to_string(),
            show_match_history_item_border: bool_field(
                value,
                "showMatchHistoryItemBorder",
                defaults.show_match_history_item_border,
            ),
            show_jungle_pathing: bool_field(value, "showJunglePathing", defaults.show_jungle_pathing),
            show_jungle_pathing_for_all_players: bool_field(
                value,
                "showJunglePathingForAllPlayers",
                defaults.show_jungle_pathing_for_all_players,
            ),
            auto_route_when_game_starts: bool_field(
                value,
                "autoRouteWhenGameStarts",
                defaults.auto_route_when_game_starts,
            ),
            player_card_tags: value
                .get("playerCardTags")
                .filter(|tags| tags.is_object())
                .cloned()
                .unwrap_or(defaults.player_card_tags),
            query_in_lobby_phase: bool_field(value, "queryInLobbyPhase", defaults.query_in_lobby_phase),
            premade_team_infer_match_count_threshold: number(
                "premadeTeamInferMatchCountThreshold",
                defaults.premade_team_infer_match_count_threshold,
            ),
        }
    }

    /// `concurrency` is a user number, so it can be zero or absurd; the queue is
    /// built with it as-is, which is why the keeper clamps rather than validates.
    pub fn concurrency(&self) -> usize {
        self.concurrency.max(1) as usize
    }
}

/// `createDefaultOngoingGamePanelPlayerCardTagSettings`.
pub fn default_player_card_tags() -> Value {
    serde_json::json!({
        "showPremadeTeamTag": true,
        "showSuspiciousFlashPositionTag": true,
        "showWinningStreakTag": true,
        "showLosingStreakTag": true,
        "showSoloKillsTag": true,
        "showEasyGankTag": true,
        "showGreatPerformanceTag": true,
        "showAverageTeamDamageTag": false,
        "showAverageTeamDamageTakenTag": false,
        "showAverageTeamGoldTag": false,
        "showAverageCsPerMinuteTag": false,
        "showAverageDamageGoldEfficiencyTag": false,
        "showAverageEnemyMissingPingsTag": false,
        "showAverageVisionScoreTag": false,
        "showAverageKillDamageEfficiencyTag": true,
        "showSelfTag": true,
        "showMetTag": true,
        "showTaggedTag": true,
        "showWinRateTeamTag": true,
        "showPrivacyTag": true,
        "showRiftFlowScoreTag": false,
    })
}

/// `matchHistoryLoadCount` transform: out of 1..=200 the previous value wins.
pub fn transform_match_history_load_count(value: i64, old_value: i64) -> i64 {
    if (1..=200).contains(&value) {
        value
    } else {
        old_value
    }
}

/// `gameDetailsLoadCount` transform: out of 0..=matchHistoryLoadCount it snaps to
/// `matchHistoryLoadCount` (not to the previous value).
pub fn transform_game_details_load_count(value: i64, match_history_load_count: i64) -> i64 {
    if value >= 0 && value <= match_history_load_count {
        value
    } else {
        match_history_load_count
    }
}

/// `matchHistoryLoadCount` side effect: `gameDetailsLoadCount` follows downwards.
pub fn side_effect_game_details_load_count(
    match_history_load_count: i64,
    current_game_details_load_count: i64,
) -> i64 {
    match_history_load_count.min(current_game_details_load_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_empty_surface_reads_the_class_defaults() {
        let settings = OngoingGameSettings::from_value(&json!({}));
        let defaults = OngoingGameSettings::default();

        assert_eq!(settings.enabled, defaults.enabled);
        assert_eq!(settings.match_history_load_count, 50);
        assert_eq!(settings.game_details_load_count, 20);
        assert_eq!(settings.concurrency, 2);
        assert_eq!(settings.match_history_tag_preference, TagPreference::Current);
        assert_eq!(settings.order_player_by, OrderPlayerBy::Default);
        assert!(settings.query_in_lobby_phase);
        assert_eq!(settings.premade_team_infer_match_count_threshold, 5);
        assert_eq!(
            settings.player_card_tags["showRiftFlowScoreTag"],
            defaults.player_card_tags["showRiftFlowScoreTag"]
        );
    }

    #[test]
    fn declared_preferences_are_read_verbatim() {
        let settings = OngoingGameSettings::from_value(&json!({
            "enabled": false,
            "matchHistoryLoadCount": 120,
            "gameDetailsLoadCount": 8,
            "matchHistoryTagPreference": "all",
            "orderPlayerBy": "premade-team",
            "showChampionUsage": "mastery",
            "queryInLobbyPhase": false,
            "premadeTeamInferMatchCountThreshold": 9,
        }));

        assert!(!settings.enabled);
        assert_eq!(settings.match_history_load_count, 120);
        assert_eq!(settings.game_details_load_count, 8);
        assert_eq!(settings.match_history_tag_preference, TagPreference::All);
        assert_eq!(settings.order_player_by, OrderPlayerBy::PremadeTeam);
        assert_eq!(settings.show_champion_usage, "mastery");
        assert!(!settings.query_in_lobby_phase);
        assert_eq!(settings.premade_team_infer_match_count_threshold, 9);
    }

    #[test]
    fn an_unknown_champion_usage_style_falls_back_to_recent() {
        let settings = OngoingGameSettings::from_value(&json!({ "showChampionUsage": "mood" }));
        assert_eq!(settings.show_champion_usage, "recent");
    }

    #[test]
    fn a_load_count_outside_one_to_two_hundred_keeps_the_old_value() {
        assert_eq!(transform_match_history_load_count(200, 50), 200);
        assert_eq!(transform_match_history_load_count(201, 50), 50);
        assert_eq!(transform_match_history_load_count(0, 50), 50);
    }

    #[test]
    fn a_details_count_above_the_history_count_snaps_to_the_history_count() {
        assert_eq!(transform_game_details_load_count(10, 50), 10);
        assert_eq!(transform_game_details_load_count(60, 50), 50);
        assert_eq!(transform_game_details_load_count(-1, 50), 50);
    }

    #[test]
    fn lowering_the_history_count_pulls_the_details_count_down_only() {
        assert_eq!(side_effect_game_details_load_count(10, 20), 10);
        assert_eq!(side_effect_game_details_load_count(40, 20), 20);
    }

    #[test]
    fn concurrency_never_reaches_the_queue_as_zero() {
        let settings = OngoingGameSettings {
            concurrency: 0,
            ..Default::default()
        };
        assert_eq!(settings.concurrency(), 1);
    }
}
