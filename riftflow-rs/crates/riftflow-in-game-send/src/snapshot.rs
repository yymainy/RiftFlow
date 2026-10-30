//! The ongoing-game / league-client state the presets read, ported from
//! `presets/helpers.ts` (`createPresetTeams`, `targetTeams`,
//! `selectedPlayersByPuuids`) and `presets/name-display.ts`.
//!
//! [`GameSnapshot`] is built from the JSON the app already keeps synced, with the
//! original key names, so a partial or cleared state degrades exactly the way the
//! TypeScript did: an absent summoner falls back to `displayName` then to the
//! puuid prefix, an absent champion selection is `0`, an absent premade group is
//! `None` rather than group zero.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::options::PresetTarget;
use crate::text::{self, Locale};

/// `SUMMONER_SPELL_SMITE_ID`
pub const SMITE_SPELL_ID: i64 = 11;

/// Everything the preset builders read out of the two synced shards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameSnapshot {
    /// `ongoingGame.state.queryStage.phase`
    pub phase: String,
    /// `leagueClient.data.summoner.me?.puuid`
    pub self_puuid: Option<String>,
    /// `ongoingGame.state.teams`
    pub teams: Vec<(String, Vec<String>)>,
    /// `ongoingGame.state.summoner`
    pub summoners: HashMap<String, SummonerEntry>,
    /// `ongoingGame.state.championSelections`
    pub champion_selections: HashMap<String, i64>,
    /// `ongoingGame.state.positionAssignments`
    pub position_assignments: HashMap<String, String>,
    /// `ongoingGame.state.mergedPremadeTeamMap`
    pub merged_premade_team_map: HashMap<String, i64>,
    /// `ongoingGame.state.additional.spells`
    pub spells: HashMap<String, SummonerSpells>,
    /// `ongoingGame.state.analysis.players`
    pub analysis_players: BTreeMap<String, PlayerAnalysis>,
    /// `leagueClient.data.chat.conversations.championSelect?.id`
    pub champion_select_conversation_id: Option<String>,
    /// `leagueClient.data.chat.conversations.customGame?.id`
    pub custom_game_conversation_id: Option<String>,
    /// `leagueClient.data.gameData.champions`
    pub champion_names: BTreeMap<i64, String>,
}

/// `ongoingGame.state.summoner[puuid]`, as far as the presets use it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SummonerEntry {
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
    pub display_name: Option<String>,
}

/// `ongoingGame.state.additional.spells[puuid]`
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SummonerSpells {
    pub spell1_id: i64,
    pub spell2_id: i64,
}

impl GameSnapshot {
    /// Reads one assembled object holding the keys above under their synced
    /// names — `phase`, `selfPuuid`, `teams`, `summoner`, `championSelections`,
    /// `positionAssignments`, `mergedPremadeTeamMap`, `additional.spells`,
    /// `analysis.players`, `chat`, `gameData.champions`.
    ///
    /// The app layer owns the assembly (`ongoing-game-main` and
    /// `league-client-main` are separate prop-sync namespaces); anything it does
    /// not supply stays empty here, which is what the original saw for a state
    /// that had not been fetched yet.
    pub fn from_json(value: &Value) -> Self {
        let teams = value
            .get("teams")
            .and_then(Value::as_object)
            .map(|teams| {
                teams
                    .iter()
                    .map(|(identifier, puuids)| {
                        (
                            identifier.clone(),
                            puuids
                                .as_array()
                                .map(|puuids| {
                                    puuids
                                        .iter()
                                        .filter_map(Value::as_str)
                                        .map(str::to_string)
                                        .collect()
                                })
                                .unwrap_or_default(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        let summoners = value
            .get("summoner")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(puuid, entry)| {
                        serde_json::from_value::<SummonerEntry>(entry.clone())
                            .ok()
                            .map(|entry| (puuid.clone(), entry))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let champion_selections = string_keyed_ints(value.get("championSelections"));
        let merged_premade_team_map = string_keyed_ints(value.get("mergedPremadeTeamMap"));

        let position_assignments = value
            .get("positionAssignments")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(puuid, entry)| {
                        entry
                            .get("position")
                            .and_then(Value::as_str)
                            .map(|position| (puuid.clone(), position.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let spells = value
            .get("additional")
            .and_then(|additional| additional.get("spells"))
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(puuid, entry)| {
                        serde_json::from_value::<SummonerSpells>(entry.clone())
                            .ok()
                            .map(|entry| (puuid.clone(), entry))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let analysis_players = value
            .get("analysis")
            .and_then(|analysis| analysis.get("players"))
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(puuid, entry)| {
                        serde_json::from_value::<PlayerAnalysis>(entry.clone())
                            .ok()
                            .map(|entry| (puuid.clone(), entry))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let champion_names = value
            .get("gameData")
            .and_then(|game_data| game_data.get("champions"))
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(id, entry)| {
                        let id = id.parse::<i64>().ok()?;
                        let name = entry.get("name").and_then(Value::as_str)?;
                        Some((id, name.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let chat = value.get("chat");
        let conversation_id = |key: &str| {
            chat.and_then(|chat| chat.get("conversations"))
                .and_then(|conversations| conversations.get(key))
                .and_then(|conversation| conversation.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        };

        Self {
            phase: value
                .get("phase")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            self_puuid: value
                .get("selfPuuid")
                .and_then(Value::as_str)
                .map(str::to_string),
            teams,
            summoners,
            champion_selections,
            position_assignments,
            merged_premade_team_map,
            spells,
            analysis_players,
            champion_select_conversation_id: conversation_id("championSelect"),
            custom_game_conversation_id: conversation_id("customGame"),
            champion_names,
        }
    }

    /// `gameData.championName(id)` — the id itself when the champion list has no
    /// entry (or an empty name) for it.
    pub fn champion_name(&self, champion_id: i64) -> String {
        match self.champion_names.get(&champion_id) {
            Some(name) if !name.is_empty() => name.clone(),
            _ => champion_id.to_string(),
        }
    }

    /// Every puuid across every team, in `Object.values(teams).flat()` order.
    pub fn all_puuids(&self) -> Vec<String> {
        self.teams
            .iter()
            .flat_map(|(_, puuids)| puuids.iter())
            .cloned()
            .collect()
    }

    /// `createPresetTeams`: teams sorted self-team-first, then by side, with each
    /// player's identity resolved; teams without players are dropped.
    pub fn preset_teams(&self, locale: Locale) -> Vec<PresetTeam> {
        let self_team_identifier = self
            .self_puuid
            .as_deref()
            .and_then(|self_puuid| {
                self.teams
                    .iter()
                    .find(|(_, puuids)| puuids.iter().any(|puuid| puuid == self_puuid))
                    .map(|(identifier, _)| identifier.clone())
            });

        let mut ordered = self.teams.clone();
        ordered.sort_by(|(a, _), (b, _)| {
            compare_team_identifiers(a, b, self_team_identifier.as_deref())
        });

        ordered
            .into_iter()
            .map(|(team_identifier, puuids)| {
                let labels =
                    team_labels(&team_identifier, self_team_identifier.as_deref(), locale);
                let players = puuids
                    .iter()
                    .map(|puuid| self.preset_player(puuid))
                    .collect();

                PresetTeam {
                    team_identifier,
                    label: labels.0,
                    primary_label: labels.1,
                    players,
                }
            })
            .filter(|team| !team.players.is_empty())
            .collect()
    }

    fn preset_player(&self, puuid: &str) -> PresetPlayer {
        let summoner = self.summoners.get(puuid);

        PresetPlayer {
            puuid: puuid.to_string(),
            champion_id: self.champion_selections.get(puuid).copied().unwrap_or(0),
            game_name: summoner
                .and_then(|entry| entry.game_name.as_deref())
                .filter(|name| !name.is_empty())
                .or_else(|| summoner.and_then(|entry| entry.display_name.as_deref()).filter(|name| !name.is_empty()))
                .map(str::to_string)
                .unwrap_or_else(|| puuid.chars().take(6).collect()),
            tag_line: summoner
                .and_then(|entry| entry.tag_line.as_deref())
                .unwrap_or_default()
                .to_string(),
            position: self.position_assignments.get(puuid).cloned(),
            // `mergedPremadeTeamMap[puuid] || undefined` — group 0 is "no group"
            premade_group: self
                .merged_premade_team_map
                .get(puuid)
                .copied()
                .filter(|group| *group != 0),
            analysis: self.analysis_players.get(puuid).cloned(),
            spells: self.spells.get(puuid).cloned(),
        }
    }
}

/// `InGameSendPresetPlayer`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresetPlayer {
    pub puuid: String,
    pub champion_id: i64,
    pub game_name: String,
    pub tag_line: String,
    pub position: Option<String>,
    pub premade_group: Option<i64>,
    pub analysis: Option<PlayerAnalysis>,
    pub spells: Option<SummonerSpells>,
}

/// `InGameSendPresetTeam`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresetTeam {
    pub team_identifier: String,
    pub label: String,
    pub primary_label: String,
    pub players: Vec<PresetPlayer>,
}

/// `targetTeams`
pub fn target_teams<'a>(target: PresetTarget, teams: &'a [PresetTeam], locale: Locale) -> Vec<&'a PresetTeam> {
    if target == PresetTarget::All {
        return teams.iter().collect();
    }

    let primary_label = text::common(
        locale,
        match target {
            PresetTarget::Friendly => "team.friendly",
            _ => "team.enemy",
        },
        text::Args::new(),
    );

    let matched: Vec<&PresetTeam> = teams
        .iter()
        .filter(|team| team.primary_label == primary_label)
        .collect();

    if !matched.is_empty() {
        return matched;
    }

    // no side labels to match (an incomplete handoff): fall back to position
    match target {
        PresetTarget::Friendly => teams.first().into_iter().collect(),
        _ => teams.iter().skip(1).collect(),
    }
}

/// `selectedPlayersByPuuids`, keeping the team each player belongs to.
pub fn selected_players_by_puuids<'a>(
    target: PresetTarget,
    selected_puuids: &[String],
    teams: &'a [PresetTeam],
    locale: Locale,
) -> Vec<(&'a PresetTeam, &'a PresetPlayer)> {
    target_teams(target, teams, locale)
        .into_iter()
        .flat_map(|team| team.players.iter().map(move |player| (team, player)))
        .filter(|(_, player)| selected_puuids.iter().any(|puuid| puuid == &player.puuid))
        .collect()
}

fn team_labels(team_identifier: &str, self_team_identifier: Option<&str>, locale: Locale) -> (String, String) {
    let Some(self_team_identifier) = self_team_identifier else {
        return (team_identifier.to_string(), team_identifier.to_string());
    };

    let primary_label = text::common(
        locale,
        if team_identifier == self_team_identifier {
            "team.friendly"
        } else {
            "team.enemy"
        },
        text::Args::new(),
    );

    let label = text::common(
        locale,
        "team.labelWithName",
        text::Args::new()
            .arg("side", primary_label.clone())
            .arg("team", team_identifier.to_string()),
    );

    (label, primary_label)
}

/// `compareTeamIdentifiers`
fn compare_team_identifiers(a: &str, b: &str, self_team_identifier: Option<&str>) -> std::cmp::Ordering {
    if let Some(self_team_identifier) = self_team_identifier {
        if a == self_team_identifier {
            return std::cmp::Ordering::Less;
        }
        if b == self_team_identifier {
            return std::cmp::Ordering::Greater;
        }
    }

    let (sort_a, sort_b) = (team_identifier_sort_value(a), team_identifier_sort_value(b));
    if sort_a != sort_b {
        return sort_a.cmp(&sort_b);
    }

    // `localeCompare` over identifiers that are ASCII side names
    a.cmp(b)
}

/// `getTeamIdentifierSortValue`
fn team_identifier_sort_value(team_identifier: &str) -> i64 {
    match team_identifier {
        "TEAM-100" => 100,
        "TEAM-200" => 200,
        _ => match cherry_subteam_sort_value(team_identifier) {
            Some(value) => value,
            None => i64::MAX,
        },
    }
}

/// `teamIdentifier.match(/^CHERRY-(\d+)$/).map(m => 1000 + Number(m[1]))`
fn cherry_subteam_sort_value(team_identifier: &str) -> Option<i64> {
    let digits = team_identifier.strip_prefix("CHERRY-")?;

    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    digits.parse::<i64>().ok().map(|number| 1000 + number)
}

fn string_keyed_ints(value: Option<&Value>) -> HashMap<String, i64> {
    value
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, entry)| entry.as_i64().map(|number| (key.clone(), number)))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// aggregated analysis, as far as the presets read it (`AggregatedAnalysis`)
// ---------------------------------------------------------------------------

/// `AggregatedAnalysis` — only the fields the rating and jungle presets use.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayerAnalysis {
    /// nullable in the source data: a missing count suppresses the count prefix
    pub count: Option<f64>,
    pub summary: Option<SummaryAnalysis>,
    pub win_loss: Option<WinLossAnalysisMap>,
    pub positions: Option<PositionAnalysis>,
    #[serde(deserialize_with = "deserialize_champions", default)]
    pub champions: BTreeMap<i64, ChampionAnalysis>,
    pub jungle: Option<JungleAnalysis>,
}

/// `AggregatedSummaryAnalysis` (used subset)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SummaryAnalysis {
    pub avg_kda: Option<f64>,
    /// sgp-only: absent means the metric is not printed at all
    pub avg_solo_kills: Option<f64>,
    pub avg_vision_score: Option<f64>,
    pub avg_champion_damage_percentage_of_team: Option<f64>,
    pub avg_damage_taken_percentage_of_team: Option<f64>,
    pub avg_gold_percentage_of_team: Option<f64>,
    pub avg_cs_per_minute: Option<f64>,
    pub avg_kill_participation: Option<f64>,
    pub avg_damage_gold_efficiency: Option<f64>,
}

/// `AggregatedWinLossAnalysisMap` (used subset)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WinLossAnalysisMap {
    pub all: Option<WinLossAnalysis>,
}

/// `AggregatedWinLossAnalysis` (used subset)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WinLossAnalysis {
    pub count: Option<f64>,
    pub win_rate: Option<f64>,
}

/// `AggregatedPositionAnalysis` — position to match count.
pub type PositionAnalysis = BTreeMap<String, i64>;

/// `AggregatedChampionAnalysis` (used subset)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChampionAnalysis {
    pub champion_id: i64,
    pub summary: Option<SummaryAnalysis>,
    pub win_loss: Option<WinLossAnalysisMap>,
    pub positions: Option<PositionAnalysis>,
    pub jungle: Option<JungleAnalysis>,
}

impl ChampionAnalysis {
    /// `champion.winLoss.all`
    pub fn all_win_loss(&self) -> Option<&WinLossAnalysis> {
        self.win_loss.as_ref().and_then(|map| map.all.as_ref())
    }

    /// `champion.winLoss.all.count`, the number the presets use as "matches played
    /// on this champion"
    pub fn match_count(&self) -> Option<f64> {
        self.all_win_loss().and_then(|all| all.count)
    }
}

/// `AggregatedJungleAnalysis` (used subset)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JungleAnalysis {
    pub games_analyzed: Option<f64>,
    pub avg_top_zone_percentage: Option<f64>,
    pub avg_mid_zone_percentage: Option<f64>,
    pub avg_bot_zone_percentage: Option<f64>,
    pub objectives: Option<JungleObjectives>,
    pub first_clear_camp: Option<JungleFirstClearCamp>,
    pub early_gank: Option<JungleEarlyGank>,
}

impl JungleAnalysis {
    fn zone_percentage(value: Option<f64>) -> f64 {
        value.unwrap_or(0.0)
    }

    pub fn top_zone_percentage(&self) -> f64 {
        Self::zone_percentage(self.avg_top_zone_percentage)
    }

    pub fn mid_zone_percentage(&self) -> f64 {
        Self::zone_percentage(self.avg_mid_zone_percentage)
    }

    pub fn bot_zone_percentage(&self) -> f64 {
        Self::zone_percentage(self.avg_bot_zone_percentage)
    }

    pub fn games_analyzed_count(&self) -> i64 {
        finite_or_zero(self.games_analyzed)
    }
}

/// `AggregatedJungleObjectives` (used subset)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JungleObjectives {
    pub first_dragon_rate: Option<f64>,
    pub avg_dragons: Option<f64>,
    pub avg_first_dragon_time: Option<f64>,
    pub avg_voidgrubs: Option<f64>,
    pub avg_heralds: Option<f64>,
    pub avg_barons: Option<f64>,
}

/// `AggregatedJungleEarlyGank` (used subset)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JungleEarlyGank {
    pub level3_gank_rate: Option<f64>,
    pub level4_gank_rate: Option<f64>,
}

/// `AggregatedJungleFirstClearCamp`
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JungleFirstClearCamp {
    pub blue: Option<JungleCampCounts>,
    pub red: Option<JungleCampCounts>,
    pub blue_invade: Option<JungleCampCounts>,
    pub red_invade: Option<JungleCampCounts>,
    pub blue_games: Option<f64>,
    pub red_games: Option<f64>,
}

/// `Record<JungleCamp, number>` from `emptyCampCount()`. Field order is the
/// original object's key order, because the distribution list is a *stable* sort
/// by count and ties therefore keep it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JungleCampCounts {
    pub red: Option<f64>,
    pub blue: Option<f64>,
    pub wolves: Option<f64>,
    pub raptors: Option<f64>,
}

impl JungleCampCounts {
    /// `Object.entries(record)` in the order the record was built.
    pub fn entries(&self) -> [(&'static str, f64); 4] {
        [
            ("red", self.red.unwrap_or(0.0)),
            ("blue", self.blue.unwrap_or(0.0)),
            ("wolves", self.wolves.unwrap_or(0.0)),
            ("raptors", self.raptors.unwrap_or(0.0)),
        ]
    }

    /// `sumRecord(record)`
    pub fn total(&self) -> f64 {
        self.entries().into_iter().map(|(_, count)| count).sum()
    }
}

/// `AggregatedAnalysis.champions` is a `Record<number, …>`; the synced JSON keys
/// those entries by string, and the presets iterate them in numeric-key order.
fn deserialize_champions<'de, D>(deserializer: D) -> Result<BTreeMap<i64, ChampionAnalysis>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = BTreeMap::<String, ChampionAnalysis>::deserialize(deserializer)?;

    Ok(raw
        .into_iter()
        .filter_map(|(key, entry)| key.parse::<i64>().ok().map(|id| (id, entry)))
        .collect())
}

/// `Number.isFinite(value) ? value : 0` for the counts the original used in text.
pub(crate) fn finite_or_zero(value: Option<f64>) -> i64 {
    value
        .filter(|value| value.is_finite())
        .map(|value| value as i64)
        .unwrap_or(0)
}

/// `Number.isFinite(value) ? value : null`, i.e. the `formatNullable*` guards.
pub(crate) fn finite_value(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite())
}

/// `playerRiotId`
pub fn player_riot_id(player: &PresetPlayer) -> String {
    if player.tag_line.is_empty() {
        player.game_name.clone()
    } else {
        format!("{}#{}", player.game_name, player.tag_line)
    }
}

/// `countSelectedChampionIds`
pub fn count_selected_champion_ids(players: &[&PresetPlayer]) -> BTreeMap<i64, i64> {
    let mut counts = BTreeMap::new();

    for player in players {
        if player.champion_id > 0 {
            *counts.entry(player.champion_id).or_insert(0) += 1;
        }
    }

    counts
}

/// `playerDisplayNameUsesChampionName`
pub fn player_display_name_uses_champion_name(
    player: &PresetPlayer,
    strategy: crate::options::NameDisplayStrategy,
    selected_champion_id_counts: &BTreeMap<i64, i64>
) -> bool {
    use crate::options::NameDisplayStrategy as Strategy;

    if player.champion_id <= 0 {
        return false;
    }

    match strategy {
        Strategy::PreferName => false,
        Strategy::ChampionNameWithName => true,
        Strategy::PreferChampionName => {
            selected_champion_id_counts.get(&player.champion_id) == Some(&1)
        }
    }
}

/// `playerDisplayName`
pub fn player_display_name(
    snapshot: &GameSnapshot,
    player: &PresetPlayer,
    strategy: crate::options::NameDisplayStrategy,
    selected_champion_id_counts: &BTreeMap<i64, i64>,
    locale: Locale
) -> String {
    use crate::options::NameDisplayStrategy as Strategy;

    let riot_id = player_riot_id(player);
    let uses_champion_name =
        player_display_name_uses_champion_name(player, strategy, selected_champion_id_counts);

    if uses_champion_name {
        let champion_name = snapshot.champion_name(player.champion_id);

        if strategy == Strategy::ChampionNameWithName {
            return text::common(
                locale,
                "championWithPlayer",
                text::Args::new()
                    .arg("champion", champion_name)
                    .arg("player", riot_id),
            );
        }

        return champion_name;
    }

    riot_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::NameDisplayStrategy;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
            "phase": "in-game",
            "selfPuuid": "p1",
            "teams": { "TEAM-100": ["p1", "p2"] },
            "summoner": {
                "p1": { "gameName": "RIP董事长", "tagLine": "81406" },
                "p2": { "displayName": "Only Display" }
            },
            "championSelections": { "p1": 103, "p2": 64 },
            "positionAssignments": { "p1": { "position": "MIDDLE" }, "p2": { "position": "JUNGLE" } },
            "mergedPremadeTeamMap": { "p1": 1, "p2": 0 },
            "additional": { "spells": { "p2": { "spell1Id": 11, "spell2Id": 4 } } },
            "analysis": { "players": {} },
            "chat": { "conversations": { "championSelect": { "id": "champ-select-chat" }, "customGame": null } },
            "gameData": { "champions": { "103": { "name": "无极剑圣" }, "64": { "name": "盲僧" } } }
        })
    }

    #[test]
    fn reads_the_synced_key_names() {
        let snapshot = GameSnapshot::from_json(&fixture());

        assert_eq!(snapshot.phase, "in-game");
        assert_eq!(snapshot.self_puuid.as_deref(), Some("p1"));
        assert_eq!(snapshot.champion_select_conversation_id.as_deref(), Some("champ-select-chat"));
        assert_eq!(
            snapshot.custom_game_conversation_id, None,
            "a null conversation stays absent"
        );
        assert_eq!(snapshot.champion_name(103), "无极剑圣");
        assert_eq!(snapshot.champion_name(9999), "9999");
        assert_eq!(snapshot.all_puuids(), vec!["p1".to_string(), "p2".to_string()]);
    }

    #[test]
    fn players_fall_back_the_way_the_original_did() {
        let snapshot = GameSnapshot::from_json(&fixture());
        let teams = snapshot.preset_teams(Locale::ZhCn);
        let players = &teams[0].players;

        assert_eq!(players[0].game_name, "RIP董事长");
        assert_eq!(players[0].tag_line, "81406");
        assert_eq!(player_riot_id(&players[0]), "RIP董事长#81406");

        assert_eq!(players[1].game_name, "Only Display", "displayName is the fallback");
        assert_eq!(players[1].tag_line, "");
        assert_eq!(player_riot_id(&players[1]), "Only Display");

        assert_eq!(players[1].premade_group, None, "group 0 is not a group");
        assert_eq!(players[0].premade_group, Some(1));
        assert_eq!(players[1].spells.as_ref().map(|spells| spells.spell1_id), Some(11));
    }

    #[test]
    fn an_unknown_puuid_keeps_the_puuid_prefix() {
        let mut snapshot = GameSnapshot::from_json(&fixture());
        snapshot.teams.push(("TEAM-200".to_string(), vec!["abcdef123456".to_string()]));
        snapshot.self_puuid = None;

        let teams = snapshot.preset_teams(Locale::En);
        let enemy = teams.iter().find(|team| team.team_identifier == "TEAM-200").unwrap();

        assert_eq!(enemy.players[0].game_name, "abcdef");
        assert_eq!(
            enemy.primary_label, enemy.label,
            "without a self team the label is the raw identifier"
        );
        assert_eq!(enemy.label, "TEAM-200");
    }

    #[test]
    fn self_team_sorts_first_and_labels_are_localized() {
        let mut snapshot = GameSnapshot::from_json(&fixture());
        snapshot.self_puuid = Some("p9".to_string());
        snapshot.teams = vec![
            ("TEAM-200".to_string(), vec!["p9".to_string()]),
            ("TEAM-100".to_string(), vec!["p1".to_string()]),
        ];

        let teams = snapshot.preset_teams(Locale::ZhCn);
        assert_eq!(teams[0].team_identifier, "TEAM-200");
        assert_eq!(teams[0].primary_label, "我方");
        assert_eq!(teams[0].label, "我方 · TEAM-200");
        assert_eq!(teams[1].primary_label, "敌方");

        let teams = snapshot.preset_teams(Locale::En);
        assert_eq!(teams[0].primary_label, "Allied");
        assert_eq!(teams[1].label, "Enemy · TEAM-100");
    }

    #[test]
    fn cherry_subteams_sort_after_regular_teams_by_number() {
        let mut snapshot = GameSnapshot::from_json(&fixture());
        snapshot.self_puuid = None;
        snapshot.teams = vec![
            ("CHERRY-2".to_string(), vec!["a".to_string()]),
            ("TEAM-200".to_string(), vec!["b".to_string()]),
            ("CHERRY-10".to_string(), vec!["c".to_string()]),
            ("CHERRY-1".to_string(), vec!["d".to_string()]),
            ("TEAM-100".to_string(), vec!["e".to_string()]),
        ];

        let ordered: Vec<String> = snapshot
            .preset_teams(Locale::En)
            .into_iter()
            .map(|team| team.team_identifier)
            .collect();

        assert_eq!(
            ordered,
            vec![
                "TEAM-100".to_string(),
                "TEAM-200".to_string(),
                "CHERRY-1".to_string(),
                "CHERRY-2".to_string(),
                "CHERRY-10".to_string()
            ]
        );
    }

    #[test]
    fn empty_teams_are_dropped_and_targets_select_sides() {
        let mut snapshot = GameSnapshot::from_json(&fixture());
        snapshot.teams.push(("TEAM-300".to_string(), vec![]));

        let teams = snapshot.preset_teams(Locale::En);
        assert_eq!(teams.len(), 1);

        assert_eq!(target_teams(PresetTarget::All, &teams, Locale::En).len(), 1);
        assert_eq!(target_teams(PresetTarget::Friendly, &teams, Locale::En).len(), 1);
        assert!(
            target_teams(PresetTarget::Enemy, &teams, Locale::En).is_empty(),
            "`teams.slice(1)` of a single team is empty"
        );
    }

    #[test]
    fn selection_filters_to_the_target_side_and_the_selected_puuids() {
        let snapshot = GameSnapshot::from_json(&fixture());
        let teams = snapshot.preset_teams(Locale::En);

        let selected = selected_players_by_puuids(
            PresetTarget::Friendly,
            &["p2".to_string(), "gone".to_string()],
            &teams,
            Locale::En,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].1.puuid, "p2");

        assert!(
            selected_players_by_puuids(PresetTarget::Enemy, &["p1".to_string(), "p2".to_string()], &teams, Locale::En)
                .is_empty()
        );
    }

    #[test]
    fn champion_name_strategy_decides_the_display_name() {
        let snapshot = GameSnapshot::from_json(&fixture());
        let teams = snapshot.preset_teams(Locale::ZhCn);
        let both: Vec<&PresetPlayer> = teams.iter().flat_map(|t| &t.players).collect();

        let unique = count_selected_champion_ids(&both);
        assert_eq!(unique.get(&103), Some(&1));

        assert_eq!(
            player_display_name(&snapshot, &teams[0].players[0], NameDisplayStrategy::PreferChampionName, &unique, Locale::ZhCn),
            "无极剑圣"
        );
        assert_eq!(
            player_display_name(&snapshot, &teams[0].players[0], NameDisplayStrategy::PreferName, &unique, Locale::ZhCn),
            "RIP董事长#81406"
        );
        assert_eq!(
            player_display_name(&snapshot, &teams[0].players[0], NameDisplayStrategy::ChampionNameWithName, &unique, Locale::ZhCn),
            "无极剑圣（RIP董事长#81406）"
        );

        // a duplicated champion is never shown by name
        let mut duplicate = teams[0].players.clone();
        duplicate[1].champion_id = 103;
        let counts = count_selected_champion_ids(&[&duplicate[0], &duplicate[1]]);
        assert_eq!(
            player_display_name(&snapshot, &duplicate[0], NameDisplayStrategy::PreferChampionName, &counts, Locale::ZhCn),
            "RIP董事长#81406"
        );
    }

    #[test]
    fn analysis_deserializes_partial_json() {
        let mut fixture = fixture();
        fixture["analysis"]["players"]["p1"] = json!({
            "count": null,
            "winLoss": { "all": { "winRate": 0.59 } },
            "champions": {
                "103": { "championId": 103, "winLoss": { "all": { "count": 16 } } },
                "64": { "championId": 64 }
            },
            "jungle": {
                "gamesAnalyzed": 20,
                "firstClearCamp": { "blue": { "red": 8, "blue": 0 }, "blueGames": 10 }
            }
        });

        let snapshot = GameSnapshot::from_json(&fixture);
        let analysis = snapshot.analysis_players.get("p1").expect("analysis for p1");

        assert_eq!(analysis.count, None, "an explicit null count stays absent");
        assert_eq!(analysis.win_loss.as_ref().unwrap().all.as_ref().unwrap().win_rate, Some(0.59));
        assert_eq!(analysis.champions[&103].win_loss.as_ref().unwrap().all.as_ref().unwrap().count, Some(16.0));
        assert_eq!(analysis.champions[&64].champion_id, 64);

        let jungle = analysis.jungle.as_ref().unwrap();
        assert_eq!(jungle.games_analyzed_count(), 20);
        let camps = jungle.first_clear_camp.as_ref().unwrap();
        assert_eq!(camps.blue.as_ref().unwrap().entries()[0], ("red", 8.0));
        assert_eq!(camps.blue.as_ref().unwrap().total(), 8.0);
        assert_eq!(camps.red_games, None);
    }

    #[test]
    fn an_unparsable_analysis_entry_is_skipped_rather_than_fabricated() {
        let mut fixture = fixture();
        fixture["analysis"]["players"]["p1"] = json!("not an analysis");

        let snapshot = GameSnapshot::from_json(&fixture);
        assert!(snapshot.analysis_players.is_empty());
    }
}
