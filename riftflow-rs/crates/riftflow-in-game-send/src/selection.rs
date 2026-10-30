//! The selection rules of `preset-selection-controller.ts`, plus the
//! `selectMainItems` drop-off both the rating and jungle presets share.

use serde_json::Value;

use crate::snapshot::{GameSnapshot, SMITE_SPELL_ID};

/// `[...new Set(puuids)]` — deduplicated, first occurrence wins.
pub fn unique_puuids(puuids: &[String]) -> Vec<String> {
    let mut unique = Vec::with_capacity(puuids.len());

    for puuid in puuids {
        if !unique.iter().any(|candidate| candidate == puuid) {
            unique.push(puuid.clone());
        }
    }

    unique
}

/// `_filterCurrentPuuids`: deduplicate, then keep only the puuids still in the
/// current teams. A player who left mid-game is dropped rather than carried over
/// from the previous selection.
pub fn filter_current_puuids(snapshot: &GameSnapshot, puuids: &[String]) -> Vec<String> {
    let current = snapshot.all_puuids();

    unique_puuids(puuids)
        .into_iter()
        .filter(|puuid| current.iter().any(|candidate| candidate == puuid))
        .collect()
}

/// `_defaultJunglePuuids`: every player whose assigned position is jungle, or who
/// is holding smite.
pub fn default_jungle_puuids(snapshot: &GameSnapshot) -> Vec<String> {
    snapshot
        .all_puuids()
        .into_iter()
        .filter(|puuid| is_jungler(snapshot, puuid))
        .collect()
}

fn is_jungler(snapshot: &GameSnapshot, puuid: &str) -> bool {
    // the original upper-cases the assignment before comparing
    if snapshot
        .position_assignments
        .get(puuid)
        .map(|position| position.to_ascii_uppercase() == "JUNGLE")
        .unwrap_or(false)
    {
        return true;
    }

    match snapshot.spells.get(puuid) {
        Some(spells) => spells.spell1_id == SMITE_SPELL_ID || spells.spell2_id == SMITE_SPELL_ID,
        None => false,
    }
}

/// The `premadeIndices` IPC argument: a JSON array of 1-based group indexes.
///
/// The original is typed `number[]` and checks `Number.isInteger` when filtering;
/// over JSON there is no such guarantee, so a non-integer (or non-number) entry is
/// dropped here instead of reaching the filter.
pub fn parse_premade_indices(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .map(|entries| entries.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default()
}

/// `_filterCurrentPremadeIndices`: deduplicate, then keep only the groups still
/// present in `mergedPremadeTeamMap`.
pub fn filter_current_premade_indices(snapshot: &GameSnapshot, indices: &[i64]) -> Vec<i64> {
    let current: Vec<i64> = snapshot
        .merged_premade_team_map
        .values()
        .copied()
        .collect();

    let mut unique: Vec<i64> = Vec::new();

    for index in indices {
        if unique.contains(index) {
            continue;
        }
        if current.contains(index) {
            unique.push(*index);
        }
    }

    unique
}

/// `selectMainItems` — given candidates already sorted by usage, how many of them
/// survive the "clearly a main pick" cut.
///
/// The original stops at the first index where `count[i] >= count[i + 1] * ratio`
/// and returns nothing when no such drop exists, which is how a list of evenly
/// played champions becomes 无明显主玩英雄.
pub(crate) fn main_items_take(counts: &[f64], max_count: usize) -> usize {
    const MAIN_USAGE_DROP_RATIO: f64 = 1.5;

    if counts.len() <= max_count {
        return counts.len();
    }

    // `Math.min(maxCount, length - 1) - 1`; a zero cap leaves nothing to keep
    let Some(start) = max_count.min(counts.len() - 1).checked_sub(1) else {
        return 0;
    };

    for index in (0..=start).rev() {
        if counts[index] >= counts[index + 1] * MAIN_USAGE_DROP_RATIO {
            return index + 1;
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot() -> GameSnapshot {
        GameSnapshot::from_json(&json!({
            "teams": {
                "TEAM-100": ["top", "jungle"],
                "TEAM-200": ["enemy-jungle", "enemy-mid"]
            },
            "positionAssignments": {
                "top": { "position": "TOP" },
                "jungle": { "position": "JUNGLE" },
                "enemy-mid": { "position": "MIDDLE" }
            },
            "additional": { "spells": {
                "enemy-jungle": { "spell1Id": 11, "spell2Id": 4 }
            } },
            "mergedPremadeTeamMap": { "top": 1, "jungle": 1, "enemy-mid": 2 }
        }))
    }

    fn puuids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn default_jungle_selection_picks_both_teams_junglers() {
        assert_eq!(
            default_jungle_puuids(&snapshot()),
            vec!["jungle".to_string(), "enemy-jungle".to_string()]
        );
    }

    #[test]
    fn an_assigned_jungle_position_is_matched_case_insensitively() {
        let mut snapshot = snapshot();
        snapshot
            .position_assignments
            .insert("enemy-mid".to_string(), "jungle".to_string());

        assert_eq!(
            default_jungle_puuids(&snapshot),
            vec![
                "jungle".to_string(),
                "enemy-jungle".to_string(),
                "enemy-mid".to_string()
            ],
            "the team order is kept"
        );
    }

    #[test]
    fn rating_selection_keeps_the_full_team_order_and_drops_departed_players() {
        let mut requested = puuids(&["top", "top", "gone"]);
        requested.extend(puuids(&["enemy-mid"]));

        assert_eq!(
            filter_current_puuids(&snapshot(), &requested),
            vec!["top".to_string(), "enemy-mid".to_string()]
        );
        assert_eq!(unique_puuids(&requested), puuids(&["top", "gone", "enemy-mid"]));
    }

    #[test]
    fn premade_indices_parse_and_filter() {
        let value = json!([1, 2, 2, 3, 1.5, "4", null]);
        let parsed = parse_premade_indices(&value);

        assert_eq!(parsed, vec![1, 2, 2, 3]);
        assert_eq!(filter_current_premade_indices(&snapshot(), &parsed), vec![1, 2]);
        assert_eq!(parse_premade_indices(&json!("not an array")), Vec::<i64>::new());
        assert_eq!(parse_premade_indices(&Value::Null), Vec::<i64>::new());
    }

    #[test]
    fn main_items_stop_at_the_first_clear_drop() {
        let counts = [16.0, 14.0, 12.0, 2.0];
        assert_eq!(main_items_take(&counts, 3), 3);

        let steep = [16.0, 10.0, 9.0, 8.0];
        assert_eq!(main_items_take(&steep, 3), 1, "only the leader clears the ratio");

        let flat = [10.0, 10.0, 10.0, 10.0];
        assert_eq!(main_items_take(&flat, 3), 0);

        assert_eq!(main_items_take(&[5.0, 4.0], 3), 2, "shorter than the cap: everything stays");
        assert_eq!(main_items_take(&[], 3), 0);
    }
}
