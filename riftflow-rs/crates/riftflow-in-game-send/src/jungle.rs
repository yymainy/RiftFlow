//! The 打野偏好 preset, ported from `presets/jungle.ts`.

use crate::options::{JunglePresetOptions, PresetTarget};
use crate::selection::main_items_take;
use crate::snapshot::{
    count_selected_champion_ids, finite_or_zero, finite_value, player_display_name,
    player_display_name_uses_champion_name, selected_players_by_puuids, GameSnapshot,
    JungleAnalysis, JungleCampCounts, JungleFirstClearCamp, PresetPlayer,
};
use crate::text::{self, Args, Locale, Punctuation};

const MAX_MAIN_CHAMPION_COUNT: usize = 3;
const MIN_MAIN_CHAMPION_MATCH_COUNT: f64 = 2.0;

/// A second lane counts as "also played" when it reaches this share of the first.
const LANE_DOUBLE_RATIO: f64 = 0.65;

/// `FirstClearTeamSide`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FirstClearSide {
    Blue,
    Red,
}

/// `FirstClearKind`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FirstClearKind {
    Normal,
    Invade,
}

/// `JungleStatsSource`
struct JungleStatsSource<'a> {
    champion_name: Option<String>,
    jungle: Option<&'a JungleAnalysis>,
    is_champion_stats: bool,
}

/// `buildJunglePresetLines`
pub fn build_jungle_lines(
    snapshot: &GameSnapshot,
    target: PresetTarget,
    options: &JunglePresetOptions,
    selected_puuids: &[String],
    locale: Locale,
) -> Vec<String> {
    let teams = snapshot.preset_teams(locale);
    let players: Vec<&PresetPlayer> =
        selected_players_by_puuids(target, selected_puuids, &teams, locale)
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
                build_jungle_stats(snapshot, options, player, display_name_uses_champion_name, locale);

            format!("{name}{line_separator}{stats}")
        })
        .collect()
}

/// `currentChampionName` — unlike the rating preset, an unpicked champion has no
/// name to talk about, and the caller reports 尚未选择英雄 instead.
fn current_champion_name(snapshot: &GameSnapshot, player: &PresetPlayer) -> Option<String> {
    if player.champion_id <= 0 {
        return None;
    }

    Some(snapshot.champion_name(player.champion_id))
}

/// `resolveJungleStatsSource`
fn resolve_jungle_stats_source<'a>(
    snapshot: &GameSnapshot,
    options: &JunglePresetOptions,
    player: &'a PresetPlayer,
) -> Option<JungleStatsSource<'a>> {
    let analysis = player.analysis.as_ref();

    if !options.show_current_champion {
        return Some(JungleStatsSource {
            champion_name: None,
            jungle: analysis.and_then(|analysis| analysis.jungle.as_ref()),
            is_champion_stats: false,
        });
    }

    let champion_name = current_champion_name(snapshot, player)?;

    Some(JungleStatsSource {
        champion_name: Some(champion_name),
        jungle: analysis
            .and_then(|analysis| analysis.champions.get(&player.champion_id))
            .and_then(|champion| champion.jungle.as_ref()),
        is_champion_stats: true,
    })
}

/// `buildJungleStats`
fn build_jungle_stats(
    snapshot: &GameSnapshot,
    options: &JunglePresetOptions,
    player: &PresetPlayer,
    display_name_uses_champion_name: bool,
    locale: Locale,
) -> String {
    if options.show_current_champion && player.champion_id <= 0 {
        return text::common(locale, "noChampionSelected", Args::new());
    }

    let Some(stats_source) = resolve_jungle_stats_source(snapshot, options, player) else {
        return text::common(locale, "noChampionSelected", Args::new());
    };

    if !stats_source.is_champion_stats {
        return build_global_jungle_stats(snapshot, options, player, &stats_source, locale);
    }

    build_champion_jungle_stats(options, &stats_source, display_name_uses_champion_name, locale)
}

/// `buildGlobalJungleStats`
fn build_global_jungle_stats(
    snapshot: &GameSnapshot,
    options: &JunglePresetOptions,
    player: &PresetPlayer,
    stats_source: &JungleStatsSource<'_>,
    locale: Locale,
) -> String {
    let Some(jungle) = stats_source.jungle else {
        return text::preset(locale, "jungle.noRecords", Args::new());
    };

    let mut parts = vec![sample_prefix(locale, stats_source, false)];

    if options.activity_preference {
        parts.push(activity_preference(jungle, locale));
    }

    push_gank_metrics(options, jungle, &mut parts, locale);
    push_dragon_metrics(options, jungle, &mut parts, locale);
    push_monster_metrics(options, jungle, &mut parts, locale);

    if options.main_champions {
        let names = main_jungle_champion_names(snapshot, player);
        parts.push(if names.is_empty() {
            text::common(locale, "noMainChampions", Args::new())
        } else {
            text::preset(
                locale,
                "jungle.metrics.mainChampions",
                Args::new().arg("champions", text::join_list(locale, &names)),
            )
        });
    }

    parts.join(" ")
}

/// `buildChampionJungleStats`
fn build_champion_jungle_stats(
    options: &JunglePresetOptions,
    stats_source: &JungleStatsSource<'_>,
    display_name_uses_champion_name: bool,
    locale: Locale,
) -> String {
    let Some(jungle) = stats_source.jungle else {
        return jungle_no_record_text(stats_source, display_name_uses_champion_name, locale);
    };

    let mut parts = vec![sample_prefix(
        locale,
        stats_source,
        display_name_uses_champion_name,
    )];

    if options.activity_preference {
        parts.push(activity_preference(jungle, locale));
    }

    if options.first_clear_distribution {
        if let Some(camps) = jungle.first_clear_camp.as_ref() {
            for (side, kind) in [
                (FirstClearSide::Blue, FirstClearKind::Normal),
                (FirstClearSide::Blue, FirstClearKind::Invade),
                (FirstClearSide::Red, FirstClearKind::Normal),
                (FirstClearSide::Red, FirstClearKind::Invade),
            ] {
                parts.push(format_first_clear_pattern(camps, side, kind, locale));
            }
        }
    }

    push_gank_metrics(options, jungle, &mut parts, locale);
    push_dragon_metrics(options, jungle, &mut parts, locale);
    push_monster_metrics(options, jungle, &mut parts, locale);

    parts.join(" ")
}

fn push_gank_metrics(
    options: &JunglePresetOptions,
    jungle: &JungleAnalysis,
    parts: &mut Vec<String>,
    locale: Locale,
) {
    if !options.early_gank {
        return;
    }

    let early_gank = jungle.early_gank.as_ref();
    // `jungle.earlyGank.levelNGankRate` — no gank data reads as 0%
    let level3 = early_gank.and_then(|gank| gank.level3_gank_rate).unwrap_or_default();
    let level4 = early_gank.and_then(|gank| gank.level4_gank_rate).unwrap_or_default();

    parts.push(text::preset(
        locale,
        "jungle.metrics.level3Gank",
        Args::new().arg("value", text::format_rate(level3)),
    ));
    parts.push(text::preset(
        locale,
        "jungle.metrics.level4Gank",
        Args::new().arg("value", text::format_rate(level4)),
    ));
}

fn push_dragon_metrics(
    options: &JunglePresetOptions,
    jungle: &JungleAnalysis,
    parts: &mut Vec<String>,
    locale: Locale,
) {
    if !options.dragon_control {
        return;
    }

    let objectives = jungle.objectives.as_ref();
    let value = objectives.and_then(|objectives| objectives.first_dragon_rate);
    let time = objectives.and_then(|objectives| objectives.avg_first_dragon_time);

    parts.push(text::preset(
        locale,
        "jungle.metrics.firstDragon",
        Args::new()
            .arg("value", text::format_rate(value.unwrap_or_default()))
            .arg("time", first_dragon_time_text(time, locale)),
    ));
    parts.push(text::preset(
        locale,
        "jungle.metrics.avgDragons",
        Args::new().arg(
            "value",
            text::to_fixed(objectives.and_then(|o| o.avg_dragons).unwrap_or_default(), 1),
        ),
    ));
}

fn push_monster_metrics(
    options: &JunglePresetOptions,
    jungle: &JungleAnalysis,
    parts: &mut Vec<String>,
    locale: Locale,
) {
    if !options.monster_control {
        return;
    }

    let objectives = jungle.objectives.as_ref();
    let average = |get: fn(&crate::snapshot::JungleObjectives) -> Option<f64>| {
        objectives
            .and_then(get)
            .unwrap_or_default()
    };

    parts.push(text::preset(
        locale,
        "jungle.metrics.monsterControl",
        Args::new()
            .arg(
                "voidgrubs",
                text::to_fixed(average(|o| o.avg_voidgrubs), 1),
            )
            .arg("heralds", text::to_fixed(average(|o| o.avg_heralds), 1))
            .arg("barons", text::to_fixed(average(|o| o.avg_barons), 1)),
    ));
}

/// `formatJungleNoRecordText`
fn jungle_no_record_text(
    stats_source: &JungleStatsSource<'_>,
    display_name_uses_champion_name: bool,
    locale: Locale,
) -> String {
    if !stats_source.is_champion_stats || stats_source.champion_name.is_none() {
        return text::preset(locale, "jungle.noRecords", Args::new());
    }

    let subject = if display_name_uses_champion_name {
        text::common(locale, "thisChampion", Args::new())
    } else {
        stats_source.champion_name.clone().unwrap_or_default()
    };

    text::preset(
        locale,
        "jungle.noChampionRecords",
        Args::new().arg("champion", subject),
    )
}

/// `formatJungleSamplePrefix`
fn sample_prefix(
    locale: Locale,
    stats_source: &JungleStatsSource<'_>,
    display_name_uses_champion_name: bool,
) -> String {
    let count = stats_source
        .jungle
        .map(|jungle| jungle.games_analyzed_count())
        .unwrap_or(0);

    if !stats_source.is_champion_stats || stats_source.champion_name.is_none() {
        return text::preset_plural(locale, "jungle.sampleCount", count as f64, Args::new());
    }

    let subject = if display_name_uses_champion_name {
        text::common(locale, "thisChampion", Args::new())
    } else {
        stats_source.champion_name.clone().unwrap_or_default()
    };

    text::preset_plural(
        locale,
        "jungle.championSampleCount",
        count as f64,
        Args::new().arg("champion", subject),
    )
}

/// `formatActivityPreference`
fn activity_preference(jungle: &JungleAnalysis, locale: Locale) -> String {
    text::preset(
        locale,
        "jungle.metrics.activityPreference",
        Args::new()
            .arg("preference", lane_preference(jungle, locale))
            .arg(
                "top",
                text::format_rate(jungle.top_zone_percentage()),
            )
            .arg("mid", text::format_rate(jungle.mid_zone_percentage()))
            .arg("bot", text::format_rate(jungle.bot_zone_percentage())),
    )
}

/// `lanePreference`
fn lane_preference(jungle: &JungleAnalysis, locale: Locale) -> String {
    let mut lanes: Vec<(String, f64)> = [
        ("top", jungle.top_zone_percentage()),
        ("mid", jungle.mid_zone_percentage()),
        ("bot", jungle.bot_zone_percentage()),
    ]
    .into_iter()
    .map(|(key, rate)| (text::preset(locale, &format!("jungle.lanes.{key}"), Args::new()), rate))
    .collect();

    // `toSorted((a, b) => b.rate - a.rate)` keeps equal rates in top/mid/bot order
    lanes.sort_by(|a, b| b.1.total_cmp(&a.1));

    let (first, second) = (&lanes[0], &lanes[1]);

    if first.1 == 0.0 {
        return text::preset(locale, "jungle.lanePreference.unclear", Args::new());
    }

    if second.1 >= first.1 * LANE_DOUBLE_RATIO {
        return text::preset(
            locale,
            "jungle.lanePreference.double",
            Args::new()
                .arg("first", first.0.clone())
                .arg("second", second.0.clone()),
        );
    }

    text::preset(
        locale,
        "jungle.lanePreference.single",
        Args::new().arg("lane", first.0.clone()),
    )
}

/// `formatFirstDragonTimeText`
fn first_dragon_time_text(seconds: Option<f64>, locale: Locale) -> String {
    let Some(seconds) = finite_value(seconds) else {
        return String::new();
    };

    let total_seconds = seconds.round() as i64;
    let minutes = total_seconds / 60;
    let remaining = format!("{:02}", total_seconds % 60);

    text::preset(
        locale,
        "jungle.firstDragonAverageTime",
        Args::new().arg("time", format!("{minutes}:{remaining}")),
    )
}

/// `createFirstClearPattern` + `formatFirstClearPattern`
fn format_first_clear_pattern(
    camps: &JungleFirstClearCamp,
    side: FirstClearSide,
    kind: FirstClearKind,
    locale: Locale,
) -> String {
    let record = camp_record(camps, side, kind);
    let side_games = match side {
        FirstClearSide::Blue => camps.blue_games.unwrap_or(0.0),
        FirstClearSide::Red => camps.red_games.unwrap_or(0.0),
    };
    let count = record.map(JungleCampCounts::total).unwrap_or(0.0);

    let rate = if side_games > 0.0 {
        Some(count / side_games)
    } else {
        None
    };

    let distribution: Vec<String> = if count > 0.0 {
        let mut entries: Vec<(&str, f64)> = record
            .map(JungleCampCounts::entries)
            .unwrap_or([("red", 0.0), ("blue", 0.0), ("wolves", 0.0), ("raptors", 0.0)])
            .into_iter()
            .filter(|(_, camp_count)| *camp_count > 0.0)
            .collect();

        entries.sort_by(|(_, a), (_, b)| b.total_cmp(a));

        entries
            .into_iter()
            .map(|(camp, camp_count)| {
                text::preset(
                    locale,
                    "jungle.campRate",
                    Args::new()
                        .arg("camp", camp_name(locale, camp))
                        .arg("rate", text::format_rate(camp_count / count)),
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    let rate_text = match rate {
        Some(rate) => text::format_rate(rate),
        None => "-".to_string(),
    };

    let distribution_text = if matches!(rate, Some(rate) if rate > 0.0) && !distribution.is_empty() {
        format!(
            "[{}]",
            text::join_list(locale, &distribution)
        )
    } else {
        String::new()
    };

    text::preset(
        locale,
        "jungle.firstClear.pattern",
        Args::new()
            .arg("side", side_label(side, locale))
            .arg("kind", kind_label(kind, locale))
            .arg("rate", rate_text)
            .arg("distribution", distribution_text),
    )
}

fn camp_record(
    camps: &JungleFirstClearCamp,
    side: FirstClearSide,
    kind: FirstClearKind,
) -> Option<&JungleCampCounts> {
    match (side, kind) {
        (FirstClearSide::Blue, FirstClearKind::Normal) => camps.blue.as_ref(),
        (FirstClearSide::Blue, FirstClearKind::Invade) => camps.blue_invade.as_ref(),
        (FirstClearSide::Red, FirstClearKind::Normal) => camps.red.as_ref(),
        (FirstClearSide::Red, FirstClearKind::Invade) => camps.red_invade.as_ref(),
    }
}

fn camp_name(locale: Locale, camp: &str) -> String {
    match camp {
        "blue" | "red" | "wolves" | "raptors" => {
            text::preset(locale, &format!("jungle.camps.{camp}"), Args::new())
        }
        // `formatCampName` echoes anything the camp table does not know
        other => other.to_string(),
    }
}

fn kind_label(kind: FirstClearKind, locale: Locale) -> String {
    text::preset(
        locale,
        match kind {
            FirstClearKind::Normal => "jungle.firstClear.normal",
            FirstClearKind::Invade => "jungle.firstClear.invade",
        },
        Args::new(),
    )
}

fn side_label(side: FirstClearSide, locale: Locale) -> String {
    text::preset(
        locale,
        match side {
            FirstClearSide::Blue => "jungle.firstClear.blue",
            FirstClearSide::Red => "jungle.firstClear.red",
        },
        Args::new(),
    )
}

/// `mainJungleChampionNames` — ranked by the champion's own jungle sample count.
fn main_jungle_champion_names(snapshot: &GameSnapshot, player: &PresetPlayer) -> Vec<String> {
    let Some(analysis) = player.analysis.as_ref() else {
        return Vec::new();
    };

    let mut candidates: Vec<(i64, f64)> = analysis
        .champions
        .values()
        .map(|champion| {
            (
                champion.champion_id,
                champion
                    .jungle
                    .as_ref()
                    .map(|jungle| finite_or_zero(jungle.games_analyzed) as f64)
                    .unwrap_or_default(),
            )
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn jungle_json(overrides: Value) -> Value {
        let base = json!({
            "gamesAnalyzed": 20,
            "avgTopZonePercentage": 0.28,
            "avgMidZonePercentage": 0.41,
            "avgBotZonePercentage": 0.31,
            "objectives": {
                "firstDragonRate": 0.58,
                "avgFirstDragonTime": 390,
                "avgDragons": 1.7,
                "avgVoidgrubs": 2.1,
                "avgHeralds": 0.4,
                "avgBarons": 0.2
            },
            "firstClearCamp": {
                "blue": { "red": 8, "blue": 0, "wolves": 0, "raptors": 0 },
                "red": { "red": 0, "blue": 8, "wolves": 0, "raptors": 0 },
                "blueInvade": { "red": 1, "blue": 1, "wolves": 0, "raptors": 0 },
                "redInvade": { "red": 2, "blue": 0, "wolves": 0, "raptors": 0 },
                "blueGames": 10,
                "redGames": 10
            },
            "earlyGank": { "level3GankRate": 0.24, "level4GankRate": 0.43 }
        });

        let mut merged = base.as_object().unwrap().clone();
        for (key, value) in overrides.as_object().unwrap() {
            merged.insert(key.clone(), value.clone());
        }
        Value::Object(merged)
    }

    fn snapshot() -> GameSnapshot {
        GameSnapshot::from_json(&json!({
            "selfPuuid": "p1",
            "teams": { "TEAM-100": ["p1"] },
            "summoner": { "p1": { "gameName": "RIP董事长", "tagLine": "81406" } },
            "championSelections": { "p1": 103 },
            "gameData": { "champions": {
                "103": { "name": "无极剑圣" }, "64": { "name": "盲僧" }, "111": { "name": "深海泰坦" }
            } },
            "analysis": { "players": { "p1": {
                "jungle": jungle_json(json!({})),
                "champions": {
                    "103": { "championId": 103, "jungle": jungle_json(json!({
                        "gamesAnalyzed": 16,
                        "avgTopZonePercentage": 0.22,
                        "avgMidZonePercentage": 0.46,
                        "avgBotZonePercentage": 0.32,
                        "objectives": {
                            "firstDragonRate": 0.625, "avgFirstDragonTime": 372, "avgDragons": 1.9,
                            "avgVoidgrubs": 2.4, "avgHeralds": 0.5, "avgBarons": 0.3
                        },
                        "firstClearCamp": {
                            "blue": { "red": 4, "blue": 2, "wolves": 0, "raptors": 0 },
                            "red": { "red": 2, "blue": 6, "wolves": 0, "raptors": 0 },
                            "blueInvade": { "red": 0, "blue": 2, "wolves": 0, "raptors": 0 },
                            "redInvade": { "red": 0, "blue": 0, "wolves": 0, "raptors": 0 },
                            "blueGames": 8, "redGames": 8
                        },
                        "earlyGank": { "level3GankRate": 0.31, "level4GankRate": 0.5 }
                    })) },
                    "64": { "championId": 64, "jungle": jungle_json(json!({ "gamesAnalyzed": 12 })) },
                    "111": { "championId": 111, "jungle": jungle_json(json!({ "gamesAnalyzed": 8 })) },
                    "222": { "championId": 222, "jungle": jungle_json(json!({ "gamesAnalyzed": 1 })) }
                }
            } } }
        }))
    }

    fn puuids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn builds_current_champion_jungle_lines_by_default() {
        let lines = build_jungle_lines(
            &snapshot(),
            PresetTarget::Friendly,
            &JunglePresetOptions::default(),
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(
            lines,
            vec![
                "无极剑圣：本英雄打野样本16场 前期偏中下，上22%中46%下32% 蓝方常规开75%[红Buff67%，蓝Buff33%] 蓝方入侵开25%[蓝Buff100%] 红方常规开100%[蓝Buff75%，红Buff25%] 红方入侵开0% 3级抓31% 4级抓50% 一龙率63%，首龙均时6:12 场均小龙1.9 野怪资源巢虫2.4/先锋0.5/大龙0.3"
                    .to_string()
            ]
        );
    }

    #[test]
    fn builds_global_jungle_lines_without_the_current_champion_filter() {
        let options = JunglePresetOptions {
            show_current_champion: false,
            ..Default::default()
        };

        let lines = build_jungle_lines(
            &snapshot(),
            PresetTarget::Friendly,
            &options,
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(
            lines[0],
            "无极剑圣：打野样本20场 前期偏中下，上28%中41%下31% 3级抓24% 4级抓43% 一龙率58%，首龙均时6:30 场均小龙1.7 野怪资源巢虫2.1/先锋0.4/大龙0.2 主玩英雄[无极剑圣，盲僧，深海泰坦]"
        );
    }

    #[test]
    fn builds_english_jungle_lines_with_player_names() {
        let mut snapshot = snapshot();
        snapshot.champion_names.clear();
        for (id, name) in [(103, "Master Yi"), (64, "Lee Sin"), (111, "Nautilus")] {
            snapshot.champion_names.insert(id, name.to_string());
        }

        let options = JunglePresetOptions {
            name_display_strategy: crate::options::NameDisplayStrategy::PreferName,
            ..Default::default()
        };

        let lines = build_jungle_lines(
            &snapshot,
            PresetTarget::Friendly,
            &options,
            &puuids(&["p1"]),
            Locale::En,
        );

        assert_eq!(
            lines[0],
            "RIP董事长#81406: 16 jungle samples on Master Yi Early favors mid/bot, top 22% mid 46% bot 32% blue-side standard 75%[Red Buff 67%, Blue Buff 33%] blue-side invade 25%[Blue Buff 100%] red-side standard 100%[Blue Buff 75%, Red Buff 25%] red-side invade 0% Lv3 gank 31% Lv4 gank 50% First dragon 63%, first dragon avg 6:12 Avg dragons 1.9 Objectives voidgrubs 2.4/heralds 0.5/barons 0.3"
        );
    }

    #[test]
    fn reports_when_jungle_samples_have_no_clear_main_champion() {
        let mut snapshot = snapshot();
        let analysis = snapshot.analysis_players.get_mut("p1").expect("analysis");
        for champion in analysis.champions.values_mut() {
            if let Some(jungle) = champion.jungle.as_mut() {
                jungle.games_analyzed = Some(1.0);
            }
        }

        let options = JunglePresetOptions {
            show_current_champion: false,
            activity_preference: false,
            early_gank: false,
            dragon_control: false,
            monster_control: false,
            ..Default::default()
        };

        let lines = build_jungle_lines(
            &snapshot,
            PresetTarget::Friendly,
            &options,
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(lines[0], "无极剑圣：打野样本20场 无明显主玩英雄");
    }

    #[test]
    fn a_champion_without_jungle_records_names_itself() {
        let mut snapshot = snapshot();
        let analysis = snapshot.analysis_players.get_mut("p1").expect("analysis");
        analysis.champions.get_mut(&103).expect("103").jungle = None;

        let lines = build_jungle_lines(
            &snapshot,
            PresetTarget::Friendly,
            &JunglePresetOptions::default(),
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        // the champion name is used for the subject because the line is about
        // 本英雄 only when the display name itself is the champion
        assert_eq!(lines[0], "无极剑圣：没有本英雄打野明细记录");
    }

    #[test]
    fn no_jungle_records_at_all_without_champion_stats() {
        let mut snapshot = snapshot();
        let analysis = snapshot.analysis_players.get_mut("p1").expect("analysis");
        analysis.jungle = None;

        let options = JunglePresetOptions {
            show_current_champion: false,
            ..Default::default()
        };

        let lines = build_jungle_lines(
            &snapshot,
            PresetTarget::Friendly,
            &options,
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(lines[0], "无极剑圣：没有打野明细记录");
    }

    #[test]
    fn an_unpicked_champion_is_reported_instead_of_stats() {
        let mut snapshot = snapshot();
        snapshot.champion_selections.insert("p1".to_string(), 0);

        let lines = build_jungle_lines(
            &snapshot,
            PresetTarget::Friendly,
            &JunglePresetOptions::default(),
            &puuids(&["p1"]),
            Locale::ZhCn,
        );

        assert_eq!(lines[0], "RIP董事长#81406：尚未选择英雄");
    }

    #[test]
    fn an_unclear_lane_distribution_is_not_called_a_preference() {
        let jungle: JungleAnalysis =
            serde_json::from_value(jungle_json(json!({
                "avgTopZonePercentage": 0.0,
                "avgMidZonePercentage": 0.0,
                "avgBotZonePercentage": 0.0
            })))
            .unwrap();

        assert_eq!(lane_preference(&jungle, Locale::ZhCn), "分布不明显");

        let jungle: JungleAnalysis =
            serde_json::from_value(jungle_json(json!({
                "avgTopZonePercentage": 0.9,
                "avgMidZonePercentage": 0.1,
                "avgBotZonePercentage": 0.0
            })))
            .unwrap();

        assert_eq!(lane_preference(&jungle, Locale::ZhCn), "偏上");
    }

    #[test]
    fn a_missing_first_dragon_time_drops_the_average() {
        assert_eq!(first_dragon_time_text(None, Locale::ZhCn), "");
        assert_eq!(first_dragon_time_text(Some(f64::NAN), Locale::ZhCn), "");
        assert_eq!(
            first_dragon_time_text(Some(390.0), Locale::ZhCn),
            "，首龙均时6:30"
        );
        assert_eq!(
            first_dragon_time_text(Some(372.4), Locale::En),
            ", first dragon avg 6:12"
        );
    }

    #[test]
    fn an_unknown_side_game_count_prints_a_dash_and_no_distribution() {
        let camps: JungleFirstClearCamp =
            serde_json::from_value(json!({ "blue": { "red": 3 }, "redGames": 4.0 })).unwrap();

        let blue_normal =
            format_first_clear_pattern(&camps, FirstClearSide::Blue, FirstClearKind::Normal, Locale::ZhCn);
        assert_eq!(
            blue_normal, "蓝方常规开-",
            "with no games on that side the rate is unknown, so the distribution is dropped too"
        );

        let red_normal =
            format_first_clear_pattern(&camps, FirstClearSide::Red, FirstClearKind::Normal, Locale::ZhCn);
        assert_eq!(red_normal, "红方常规开0%", "a side with games but no record is zero");
    }
}
