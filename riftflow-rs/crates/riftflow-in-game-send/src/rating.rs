//! The 表现评分 preset, ported from `presets/rating.ts`.

use crate::options::{PresetTarget, RatingPresetOptions};
use crate::selection::main_items_take;
use crate::snapshot::{
    count_selected_champion_ids, finite_value, player_display_name,
    player_display_name_uses_champion_name, selected_players_by_puuids, GameSnapshot,
    PositionAnalysis, PresetPlayer, SummaryAnalysis, WinLossAnalysis,
};
use crate::text::{self, Args, Locale, Punctuation};

const MAX_MAIN_CHAMPION_COUNT: usize = 3;
const MAX_MAIN_POSITION_COUNT: usize = 2;
const MIN_MAIN_CHAMPION_MATCH_COUNT: f64 = 2.0;

const POSITION_ORDER: [&str; 5] = ["TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY"];

/// `RatingStatsSource` — which set of aggregated numbers a line is built from,
/// and whether those numbers are champion-specific.
struct RatingStatsSource<'a> {
    champion_name: Option<String>,
    match_count: Option<f64>,
    summary: Option<&'a SummaryAnalysis>,
    win_loss: Option<&'a WinLossAnalysis>,
    positions: Option<&'a PositionAnalysis>,
    is_champion_stats: bool,
}

/// `buildRatingPresetLines`
pub fn build_rating_lines(
    snapshot: &GameSnapshot,
    target: PresetTarget,
    options: &RatingPresetOptions,
    selected_puuids: &[String],
    locale: Locale,
) -> Vec<String> {
    let teams = snapshot.preset_teams(locale);
    let players: Vec<&PresetPlayer> = selected_players_by_puuids(
        target,
        selected_puuids,
        &teams,
        locale,
    )
    .into_iter()
    .map(|(_, player)| player)
    .collect();
    let selected_champion_id_counts = count_selected_champion_ids(&players);
    let line_separator = text::punctuation(locale, Punctuation::LineSeparator);

    players
        .iter()
        .map(|player| {
            let display_name_uses_champion_name = player_display_name_uses_champion_name(
                player,
                options.name_display_strategy,
                &selected_champion_id_counts,
            );
            let name = player_display_name(
                snapshot,
                player,
                options.name_display_strategy,
                &selected_champion_id_counts,
                locale,
            );
            let stats =
                build_rating_stats(snapshot, options, player, display_name_uses_champion_name, locale);

            format!("{name}{line_separator}{stats}")
        })
        .collect()
}

/// `currentChampionName`
fn current_champion_name(snapshot: &GameSnapshot, player: &PresetPlayer, locale: Locale) -> String {
    if player.champion_id == 0 {
        return text::common(locale, "currentChampion", Args::new());
    }

    snapshot.champion_name(player.champion_id)
}

/// `resolveRatingStatsSource`
fn resolve_rating_stats_source<'a>(
    snapshot: &GameSnapshot,
    options: &RatingPresetOptions,
    player: &'a PresetPlayer,
    locale: Locale,
) -> Option<RatingStatsSource<'a>> {
    let should_use_current_champion = options.show_current_champion && player.champion_id > 0;

    let Some(analysis) = player.analysis.as_ref() else {
        if !should_use_current_champion {
            return None;
        }

        return Some(RatingStatsSource {
            champion_name: Some(current_champion_name(snapshot, player, locale)),
            match_count: Some(0.0),
            summary: None,
            win_loss: None,
            positions: None,
            is_champion_stats: true,
        });
    };

    if !should_use_current_champion {
        return Some(RatingStatsSource {
            champion_name: None,
            match_count: analysis.count,
            summary: analysis.summary.as_ref(),
            win_loss: analysis.win_loss.as_ref().and_then(|map| map.all.as_ref()),
            positions: analysis.positions.as_ref(),
            is_champion_stats: false,
        });
    }

    let champion_name = Some(current_champion_name(snapshot, player, locale));

    match analysis.champions.get(&player.champion_id) {
        None => Some(RatingStatsSource {
            champion_name,
            match_count: Some(0.0),
            summary: None,
            win_loss: None,
            positions: None,
            is_champion_stats: true,
        }),
        Some(champion) => Some(RatingStatsSource {
            champion_name,
            match_count: champion.match_count(),
            summary: champion.summary.as_ref(),
            win_loss: champion.all_win_loss(),
            positions: champion.positions.as_ref(),
            is_champion_stats: true,
        }),
    }
}

/// `buildRatingStats`
fn build_rating_stats(
    snapshot: &GameSnapshot,
    options: &RatingPresetOptions,
    player: &PresetPlayer,
    display_name_uses_champion_name: bool,
    locale: Locale,
) -> String {
    if options.show_current_champion && player.champion_id <= 0 {
        return text::common(locale, "noChampionSelected", Args::new());
    }

    let Some(stats_source) = resolve_rating_stats_source(snapshot, options, player, locale) else {
        return text::preset(locale, "rating.noRecentMatches", Args::new());
    };

    let current_champion_subject = if display_name_uses_champion_name {
        Some(text::common(locale, "thisChampion", Args::new()))
    } else {
        stats_source.champion_name.clone()
    };

    let names_a_champion =
        stats_source.champion_name.is_some() && current_champion_subject.is_some();

    let no_record_text = if names_a_champion {
        text::preset(
            locale,
            "rating.noChampionMatches",
            Args::new().arg(
                "champion",
                current_champion_subject.clone().unwrap_or_default(),
            ),
        )
    } else {
        text::preset(locale, "rating.noRecentMatches", Args::new())
    };

    if let Some(match_count) = stats_source.match_count {
        if match_count <= 0.0 {
            return no_record_text;
        }
    }

    let summary = stats_source.summary;
    let mut parts: Vec<String> = Vec::new();

    if options.win_rate {
        parts.push(rate_metric(
            locale,
            "rating.metrics.winRate",
            stats_source.win_loss.and_then(|win_loss| win_loss.win_rate),
        ));
    }

    if options.kda {
        parts.push(number_metric(
            locale,
            "rating.metrics.kda",
            summary.and_then(|summary| summary.avg_kda),
            2,
        ));
    }

    // `avgSoloKills` is sgp-only: the metric is skipped, not dashed, when absent
    if options.avg_solo_kills {
        let avg_solo_kills = summary.and_then(|summary| summary.avg_solo_kills);
        if let Some(value) = finite_value(avg_solo_kills) {
            parts.push(text::preset(
                locale,
                "rating.metrics.avgSoloKills",
                Args::new().arg("value", text::to_fixed(value, 1)),
            ));
        }
    }

    if options.avg_vision_score && summary.is_some() {
        parts.push(number_metric(
            locale,
            "rating.metrics.avgVisionScore",
            summary.and_then(|summary| summary.avg_vision_score),
            1,
        ));
    }

    if let Some(summary) = summary {
        if options.avg_champion_damage {
            parts.push(rate_metric(
                locale,
                "rating.metrics.avgChampionDamage",
                summary.avg_champion_damage_percentage_of_team,
            ));
        }
        if options.avg_damage_taken {
            parts.push(rate_metric(
                locale,
                "rating.metrics.avgDamageTaken",
                summary.avg_damage_taken_percentage_of_team,
            ));
        }
        if options.avg_gold {
            parts.push(rate_metric(
                locale,
                "rating.metrics.avgGold",
                summary.avg_gold_percentage_of_team,
            ));
        }
        if options.avg_cs_per_minute {
            parts.push(number_metric(
                locale,
                "rating.metrics.avgCsPerMinute",
                summary.avg_cs_per_minute,
                1,
            ));
        }
        if options.avg_kill_participation {
            parts.push(rate_metric(
                locale,
                "rating.metrics.avgKillParticipation",
                summary.avg_kill_participation,
            ));
        }
        if options.avg_damage_gold_efficiency {
            parts.push(rate_metric(
                locale,
                "rating.metrics.avgDamageGoldEfficiency",
                summary.avg_damage_gold_efficiency,
            ));
        }
    }

    if options.main_champions && !stats_source.is_champion_stats {
        let names = main_champion_names(snapshot, player);
        parts.push(if names.is_empty() {
            text::common(locale, "noMainChampions", Args::new())
        } else {
            text::preset(
                locale,
                "rating.metrics.mainChampions",
                Args::new().arg("champions", text::join_list(locale, &names)),
            )
        });
    }

    if options.main_positions {
        let names = main_position_names(
            stats_source.positions,
            !stats_source.is_champion_stats,
            locale,
        );

        if !names.is_empty() {
            parts.push(text::preset(
                locale,
                "rating.metrics.mainPositions",
                Args::new().arg("positions", text::join_list(locale, &names)),
            ));
        }
    }

    let count = finite_value(stats_source.match_count);

    if let Some(count) = count {
        let prefix = if names_a_champion {
            text::preset_plural(
                locale,
                "rating.championMatchCount",
                count,
                Args::new().arg(
                    "champion",
                    current_champion_subject.clone().unwrap_or_default(),
                ),
            )
        } else {
            text::preset_plural(locale, "rating.matchCount", count, Args::new())
        };

        return if parts.is_empty() {
            prefix
        } else {
            text::preset(
                locale,
                "rating.countWithStats",
                Args::new()
                    .arg("countText", prefix)
                    .arg("stats", parts.join(" ")),
            )
        };
    }

    if parts.is_empty() {
        no_record_text
    } else {
        parts.join(" ")
    }
}

fn rate_metric(locale: Locale, key: &str, value: Option<f64>) -> String {
    text::preset(
        locale,
        key,
        Args::new().arg("value", format_nullable_rate(value)),
    )
}

fn number_metric(locale: Locale, key: &str, value: Option<f64>, digits: usize) -> String {
    text::preset(
        locale,
        key,
        Args::new().arg("value", format_nullable_number(value, digits)),
    )
}

/// `formatNullableNumber(value, digits)`
fn format_nullable_number(value: Option<f64>, digits: usize) -> String {
    match finite_value(value) {
        Some(value) => text::to_fixed(value, digits),
        None => "-".to_string(),
    }
}

/// `formatNullableRate(value)`
fn format_nullable_rate(value: Option<f64>) -> String {
    match finite_value(value) {
        Some(value) => text::format_rate(value),
        None => "-".to_string(),
    }
}

/// `mainChampionNames`
fn main_champion_names(snapshot: &GameSnapshot, player: &PresetPlayer) -> Vec<String> {
    let Some(analysis) = player.analysis.as_ref() else {
        return Vec::new();
    };

    let mut candidates: Vec<(i64, f64)> = analysis
        .champions
        .values()
        .map(|champion| (champion.champion_id, champion.match_count().unwrap_or(0.0)))
        .filter(|(champion_id, count)| {
            *champion_id > 0 && *count >= MIN_MAIN_CHAMPION_MATCH_COUNT
        })
        .collect();

    candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));

    let counts: Vec<f64> = candidates.iter().map(|(_, count)| *count).collect();
    let take = main_items_take(&counts, MAX_MAIN_CHAMPION_COUNT);

    candidates[..take]
        .iter()
        .map(|(champion_id, _)| snapshot.champion_name(*champion_id))
        .collect()
}

/// `mainPositionNames`
fn main_position_names(
    positions: Option<&PositionAnalysis>,
    use_dominance: bool,
    locale: Locale,
) -> Vec<String> {
    let Some(positions) = positions else {
        return Vec::new();
    };

    let mut candidates: Vec<(&str, i64)> = positions
        .iter()
        .filter(|(position, count)| POSITION_ORDER.contains(&position.as_str()) && **count > 0)
        .map(|(position, count)| (position.as_str(), *count))
        .collect();

    // most played first; an equal count keeps the order the type declares
    candidates.sort_by(|(position_a, count_a), (position_b, count_b)| {
        count_b.cmp(count_a).then_with(|| {
            position_order_index(position_a).cmp(&position_order_index(position_b))
        })
    });

    let counts: Vec<f64> = candidates.iter().map(|(_, count)| *count as f64).collect();
    let take = if use_dominance {
        main_items_take(&counts, MAX_MAIN_POSITION_COUNT)
    } else {
        candidates.len()
    };

    candidates[..take]
        .iter()
        .map(|(position, _)| text::position_name(locale, position))
        .collect()
}

fn position_order_index(position: &str) -> usize {
    POSITION_ORDER
        .iter()
        .position(|candidate| *candidate == position)
        .unwrap_or(POSITION_ORDER.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::NameDisplayStrategy;
    use serde_json::json;

    fn snapshot() -> GameSnapshot {
        GameSnapshot::from_json(&json!({
            "selfPuuid": "p1",
            "teams": { "TEAM-100": ["p1", "p2"] },
            "summoner": {
                "p1": { "gameName": "RIP董事长", "tagLine": "81406" },
                "p2": { "gameName": "Lee Player", "tagLine": "002" }
            },
            "championSelections": { "p1": 103, "p2": 64 },
            "gameData": { "champions": {
                "103": { "name": "无极剑圣" },
                "64": { "name": "盲僧" },
                "111": { "name": "深海泰坦" }
            } },
            "analysis": { "players": {
                "p1": {
                    "count": 50,
                    "winLoss": { "all": { "winRate": 0.59 } },
                    "summary": {
                        "avgKda": 2.34,
                        "avgSoloKills": 0.8,
                        "avgVisionScore": 26.4,
                        "avgChampionDamagePercentageOfTeam": 0.3,
                        "avgDamageTakenPercentageOfTeam": 0.27,
                        "avgGoldPercentageOfTeam": 0.24,
                        "avgCsPerMinute": 7.1,
                        "avgKillParticipation": 0.62,
                        "avgDamageGoldEfficiency": 1.18
                    },
                    "positions": { "TOP": 4, "JUNGLE": 15, "MIDDLE": 28, "BOTTOM": 2, "UTILITY": 1 },
                    "champions": {
                        "103": { "championId": 103,
                                 "winLoss": { "all": { "count": 16, "winRate": 0.667 } },
                                 "summary": { "avgKda": 4.321, "avgSoloKills": 1.2 },
                                 "positions": { "TOP": 1, "JUNGLE": 2, "MIDDLE": 12, "BOTTOM": 1, "UTILITY": 0 } },
                        "64": { "championId": 64, "winLoss": { "all": { "count": 14 } } },
                        "111": { "championId": 111, "winLoss": { "all": { "count": 12 } } }
                    }
                }
            } }
        }))
    }

    fn puuids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn builds_the_default_rating_lines() {
        let lines = build_rating_lines(
            &snapshot(),
            PresetTarget::Friendly,
            &RatingPresetOptions::default(),
            &puuids(&["p1", "p2"]),
            Locale::ZhCn,
        );

        assert_eq!(
            lines,
            vec![
                "无极剑圣：50场对局，胜率59% KDA2.34 场均单杀0.8 主玩英雄[无极剑圣，盲僧，深海泰坦] 主玩位置[中路，打野]".to_string(),
                "盲僧：近期没有对局记录".to_string()
            ]
        );
    }

    #[test]
    fn builds_the_same_lines_in_english() {
        let mut snapshot = snapshot();
        snapshot.champion_names.clear();
        for (id, name) in [
            (103, "Master Yi"),
            (64, "Lee Sin"),
            (111, "Nautilus"),
        ] {
            snapshot.champion_names.insert(id, name.to_string());
        }

        let lines = build_rating_lines(
            &snapshot,
            PresetTarget::Friendly,
            &RatingPresetOptions::default(),
            &puuids(&["p1", "p2"]),
            Locale::En,
        );

        assert_eq!(
            lines,
            vec![
                "Master Yi: 50 matches, Win Rate 59% KDA 2.34 Avg Solo Kills 0.8 Main Champions [Master Yi, Lee Sin, Nautilus] Main Positions [Middle, Jungle]".to_string(),
                "Lee Sin: No recent matches".to_string()
            ]
        );
    }

    #[test]
    fn adds_the_optional_metrics_after_average_vision_score() {
        let options = RatingPresetOptions {
            avg_vision_score: true,
            avg_champion_damage: true,
            avg_damage_taken: true,
            avg_gold: true,
            avg_cs_per_minute: true,
            avg_kill_participation: true,
            avg_damage_gold_efficiency: true,
            main_champions: false,
            main_positions: false,
            ..Default::default()
        };

        let lines = build_rating_lines(
            &snapshot(),
            PresetTarget::Friendly,
            &options,
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(
            lines[0],
            "无极剑圣：50场对局，胜率59% KDA2.34 场均单杀0.8 场均视野得分26.4 伤害30% 承伤27% 经济24% 补兵7.1/分 参团率62% 伤转比118%"
        );
    }

    #[test]
    fn shows_champion_and_player_together_when_asked() {
        let options = RatingPresetOptions {
            name_display_strategy: NameDisplayStrategy::ChampionNameWithName,
            avg_solo_kills: false,
            avg_vision_score: false,
            main_champions: false,
            main_positions: false,
            ..Default::default()
        };

        let lines = build_rating_lines(
            &snapshot(),
            PresetTarget::Friendly,
            &options,
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(
            lines[0],
            "无极剑圣（RIP董事长#81406）：50场对局，胜率59% KDA2.34"
        );
    }

    #[test]
    fn uses_current_champion_stats_when_enabled() {
        let options = RatingPresetOptions {
            show_current_champion: true,
            ..Default::default()
        };

        let lines = build_rating_lines(
            &snapshot(),
            PresetTarget::Friendly,
            &options,
            &puuids(&["p1", "p2"]),
            Locale::ZhCn,
        );

        assert_eq!(
            lines,
            vec![
                "无极剑圣：本英雄16场，胜率67% KDA4.32 场均单杀1.2 主玩位置[中路，打野，上路，下路]".to_string(),
                "盲僧：近期没有本英雄对局记录".to_string()
            ]
        );
    }

    #[test]
    fn reports_a_player_who_has_not_picked_yet() {
        let mut snapshot = snapshot();
        snapshot.champion_selections.insert("p1".to_string(), 0);

        let options = RatingPresetOptions {
            show_current_champion: true,
            ..Default::default()
        };

        let lines = build_rating_lines(
            &snapshot,
            PresetTarget::Friendly,
            &options,
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(lines, vec!["RIP董事长#81406：尚未选择英雄".to_string()]);
    }

    #[test]
    fn an_unavailable_match_count_drops_the_prefix() {
        let mut snapshot = snapshot();
        snapshot
            .analysis_players
            .get_mut("p1")
            .expect("analysis")
            .count = None;

        let lines = build_rating_lines(
            &snapshot,
            PresetTarget::Friendly,
            &RatingPresetOptions::default(),
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(
            lines[0],
            "无极剑圣：胜率59% KDA2.34 场均单杀0.8 主玩英雄[无极剑圣，盲僧，深海泰坦] 主玩位置[中路，打野]"
        );
    }

    #[test]
    fn an_unavailable_sgp_metric_is_not_printed() {
        let mut snapshot = snapshot();
        let analysis = snapshot.analysis_players.get_mut("p1").expect("analysis");
        let mut summary = analysis.summary.clone().expect("summary");
        summary.avg_solo_kills = None;
        analysis.summary = Some(summary);

        let options = RatingPresetOptions {
            main_champions: false,
            main_positions: false,
            ..Default::default()
        };

        let lines = build_rating_lines(
            &snapshot,
            PresetTarget::Friendly,
            &options,
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(lines[0], "无极剑圣：50场对局，胜率59% KDA2.34");
    }

    #[test]
    fn reports_when_no_champion_is_clearly_main() {
        let mut snapshot = snapshot();
        let analysis = snapshot.analysis_players.get_mut("p1").expect("analysis");
        analysis.count = Some(4.0);
        for champion in analysis.champions.values_mut() {
            champion.win_loss.as_mut().expect("winLoss").all.as_mut().expect("all").count =
                Some(1.0);
        }

        let options = RatingPresetOptions {
            win_rate: false,
            kda: false,
            avg_solo_kills: false,
            main_positions: false,
            ..Default::default()
        };

        let lines = build_rating_lines(
            &snapshot,
            PresetTarget::Friendly,
            &options,
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(lines, vec!["无极剑圣：4场对局，无明显主玩英雄".to_string()]);
    }

    #[test]
    fn no_lines_without_a_sendable_player() {
        assert!(build_rating_lines(
            &snapshot(),
            PresetTarget::Friendly,
            &RatingPresetOptions::default(),
            &[],
            Locale::ZhCn
        )
        .is_empty());

        assert!(build_rating_lines(
            &snapshot(),
            PresetTarget::Enemy,
            &RatingPresetOptions::default(),
            &puuids(&["p1", "p2"]),
            Locale::ZhCn
        )
        .is_empty());
    }

    #[test]
    fn nullable_formats_fall_back_to_a_dash() {
        assert_eq!(format_nullable_number(None, 2), "-");
        assert_eq!(format_nullable_number(Some(f64::NAN), 2), "-");
        assert_eq!(format_nullable_number(Some(2.341), 2), "2.34");
        assert_eq!(format_nullable_rate(None), "-");
        assert_eq!(format_nullable_rate(Some(0.59)), "59%");
    }

    #[test]
    fn position_names_order_by_played_count_then_declaration() {
        let positions: PositionAnalysis = [
            ("BOTTOM".to_string(), 5),
            ("TOP".to_string(), 5),
            ("JUNGLE".to_string(), 9),
            ("NONE".to_string(), 50),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            main_position_names(Some(&positions), false, Locale::ZhCn),
            vec!["打野".to_string(), "上路".to_string(), "下路".to_string()]
        );
        assert!(main_position_names(None, false, Locale::ZhCn).is_empty());
    }
}
