//! The decision core of `auto-select-main`, ported from
//! `src/main/shards/auto-select/computed-state.ts` and the group-support helper in
//! `shared/shards/riftflow-api/auto-select.ts`.
//!
//! Everything here is pure over LCU JSON so the rules — which champion is actually
//! pickable, which move the player is allowed to take, whether a timer still leaves
//! room for a delay — can be tested without a client.

use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub const NONE_CHAMPION_ID: i64 = -1;
pub const RANDOM_CHAMPION_ID: i64 = -2;
pub const CHEERY_BRAVERY_ID: i64 = -3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorrectedTimer {
    pub remaining_ms: f64,
    pub total_ms: f64,
    pub elapsed_ms: f64,
}

/// `getCorrectedTimer`: the LCU clock drifts against the local one, so the session
/// carries both an epoch stamp and the adjusted time left.
pub fn corrected_timer(timer: Option<&Value>, now_ms: i64) -> Option<CorrectedTimer> {
    let timer = timer?;
    if timer.get("isInfinite").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }

    let adjusted = timer.get("adjustedTimeLeftInPhase")?.as_f64()?;
    let internal_now = timer.get("internalNowInEpochMs")?.as_f64()?;
    let total = timer.get("totalTimeInPhase")?.as_f64()?;

    let remaining_ms = adjusted - (now_ms as f64 - internal_now);
    Some(CorrectedTimer {
        remaining_ms,
        total_ms: total,
        elapsed_ms: total - remaining_ms,
    })
}

/// `getPhaseCalibratedDelayMs`: never longer than the configured delay, never past
/// the target offset, never past the end of the phase.
pub fn phase_calibrated_delay_ms(configured_ms: f64, target_offset_ms: f64, timer: Option<CorrectedTimer>) -> f64 {
    let Some(timer) = timer else {
        return configured_ms;
    };

    configured_ms
        .min(target_offset_ms - timer.elapsed_ms)
        .min(timer.remaining_ms)
        .max(0.0)
}

/// `banConfig.ban.delaySeconds * 1e3`, doubled for the second half of a
/// `show-and-lock-in` ban or pick.
pub fn target_offset_ms(strategy: &str, is_complete_phase: bool, configured_ms: f64) -> f64 {
    if strategy == "show-and-lock-in" && is_complete_phase {
        configured_ms * 2.0
    } else {
        configured_ms
    }
}

/// Whether the ban reaction should finish the action in one call: `just-show` is
/// handled by the caller, which stops watching once a champion is chosen.
pub fn ban_completes(strategy: &str, is_complete_phase: bool) -> bool {
    strategy == "lock-in-immediately" || (strategy == "show-and-lock-in" && is_complete_phase)
}

/// The pick reaction's `completed`: a subset draft always finishes, and
/// `pick-intent` never does.
pub fn pick_completes(move_name: &str, strategy: &str) -> bool {
    move_name != "pick-intent"
        && (move_name == "show-subset-pick"
            || move_name == "complete-subset-pick"
            || strategy == "lock-in-immediately"
            || (strategy == "show-and-lock-in" && move_name == "complete-pick"))
}

/// The bench swap waits out the configured accumulated delay, but never longer
/// than what is left in the phase — and unlike the ban/pick delays it may go
/// negative, which the original lets fire immediately.
pub fn bench_swap_delay_ms(accumulated_ms: f64, first_seen_ms: i64, now_ms: i64, remaining_ms: Option<f64>) -> f64 {
    let delay = accumulated_ms - (now_ms - first_seen_ms) as f64;
    match remaining_ms {
        Some(remaining) => delay.min(remaining),
        None => delay,
    }
}

/// The trade reaction's countdown: the pick delay, measured from when this player
/// first *received* the request, clamped by whatever the phase still has.
pub fn champion_swap_delay_ms(
    configured_ms: f64,
    created_at_ms: i64,
    now_ms: i64,
    time_left_ms: Option<f64>,
    clamp_negative: bool,
) -> f64 {
    let delay = configured_ms - (now_ms - created_at_ms) as f64;
    let delay = if clamp_negative { delay.max(0.0) } else { delay };
    match time_left_ms {
        Some(left) => delay.min(left),
        None => delay,
    }
}

pub fn timer_phase(timer: Option<&Value>) -> Option<&str> {
    timer.and_then(|value| value.get("phase"))?.as_str()
}

/// `getScopedBenchChampions`: during the draft phase a subset draw limits the pool.
pub fn scoped_bench_champions(subset: &[i64], timer: Option<&Value>, bench: &[Value]) -> Vec<i64> {
    if !subset.is_empty() && timer_phase(timer) == Some("BAN_PICK") {
        return subset.to_vec();
    }

    bench
        .iter()
        .filter_map(|entry| entry.get("championId").and_then(Value::as_i64))
        .collect()
}

fn local_cell_id(session: &Value) -> Option<i64> {
    session.get("localPlayerCellId").and_then(Value::as_i64)
}

fn action_rows(session: &Value) -> Vec<Vec<Value>> {
    session
        .get("actions")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_array().cloned())
                .collect()
        })
        .unwrap_or_default()
}

/// `getCurrentActions`: the first row that still has work, narrowed to this player.
pub fn current_actions(session: Option<&Value>) -> Vec<Value> {
    let Some(session) = session else {
        return Vec::new();
    };
    let rows = action_rows(session);
    if rows.is_empty() {
        return Vec::new();
    }

    let Some(row) = rows.iter().find(|row| {
        !row.iter().all(|action| action.get("completed").and_then(Value::as_bool).unwrap_or(false))
    }) else {
        return Vec::new();
    };

    let cell = local_cell_id(session);
    row.iter()
        .filter(|action| action.get("actorCellId").and_then(Value::as_i64) == cell)
        .cloned()
        .collect()
}

/// `getActiveAction`.
pub fn active_action(current: &[Value]) -> Option<Value> {
    current
        .iter()
        .find(|action| !action.get("completed").and_then(Value::as_bool).unwrap_or(false))
        .cloned()
}

/// `getFirstUnfinishedPickAction`.
pub fn first_unfinished_pick_action(session: Option<&Value>) -> Option<Value> {
    let session = session?;
    let cell = local_cell_id(session)?;

    action_rows(session)
        .into_iter()
        .flatten()
        .filter(|action| action.get("actorCellId").and_then(Value::as_i64) == Some(cell))
        .find(|action| action.get("type").and_then(Value::as_str) == Some("pick")
            && !action.get("completed").and_then(Value::as_bool).unwrap_or(false))
}

/// `getCurrentSessionChampionId` — `|| null` in the original, so champion 0 is
/// treated as "no champion" too.
pub fn current_session_champion_id(session: Option<&Value>) -> Option<i64> {
    let session = session?;
    let cell = local_cell_id(session)?;

    session
        .get("myTeam")
        .and_then(Value::as_array)?
        .iter()
        .find(|member| member.get("cellId").and_then(Value::as_i64) == Some(cell))
        .and_then(|member| member.get("championId").and_then(Value::as_i64))
        .filter(|id| *id != 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    PickIntent,
    ShowPick,
    CompletePick,
    ShowBan,
    CompleteBan,
    Vote,
    ShowSubsetPick,
    CompleteSubsetPick,
    SubsetBenchSwap,
    BenchSwap,
}

impl Move {
    pub fn as_str(&self) -> &'static str {
        match self {
            Move::PickIntent => "pick-intent",
            Move::ShowPick => "show-pick",
            Move::CompletePick => "complete-pick",
            Move::ShowBan => "show-ban",
            Move::CompleteBan => "complete-ban",
            Move::Vote => "vote",
            Move::ShowSubsetPick => "show-subset-pick",
            Move::CompleteSubsetPick => "complete-subset-pick",
            Move::SubsetBenchSwap => "subset-bench-swap",
            Move::BenchSwap => "bench-swap",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MoveInputs<'a> {
    pub is_pick_intenting: bool,
    pub active_action: Option<&'a Value>,
    pub is_picking_now: bool,
    pub is_banning_now: bool,
    pub is_voting_now: bool,
    pub allow_subset_champion_picks: bool,
    pub bench_enabled: bool,
    pub current_session_champion_id: Option<i64>,
    pub timer: Option<&'a Value>,
}

/// `getMove`.
pub fn get_move(inputs: MoveInputs<'_>) -> Option<Move> {
    if inputs.is_pick_intenting {
        return Some(Move::PickIntent);
    }

    let champion_shown = inputs
        .active_action
        .and_then(|action| action.get("championId"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        != 0;

    if inputs.is_picking_now {
        return Some(if inputs.allow_subset_champion_picks {
            if champion_shown { Move::CompleteSubsetPick } else { Move::ShowSubsetPick }
        } else if champion_shown {
            Move::CompletePick
        } else {
            Move::ShowPick
        });
    }

    if inputs.is_banning_now {
        return Some(if champion_shown { Move::CompleteBan } else { Move::ShowBan });
    }

    if inputs.is_voting_now {
        return Some(Move::Vote);
    }

    if inputs.bench_enabled && inputs.current_session_champion_id.is_some() {
        return Some(
            if inputs.allow_subset_champion_picks && timer_phase(inputs.timer) == Some("BAN_PICK") {
                Move::SubsetBenchSwap
            } else {
                Move::BenchSwap
            },
        );
    }

    None
}

/// `isAutoSelectGroupSupportedOnSgpServer`.
pub fn group_supported_on_sgp_server(group: &Value, sgp_server_id: &str, league_servers: &Value) -> bool {
    let supported = group
        .get("supportedSgpServers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if supported.iter().any(|entry| entry.as_str() == Some("*")) {
        return true;
    }

    !sgp_server_id.is_empty()
        && league_servers.get(sgp_server_id).is_some()
        && supported.iter().any(|entry| entry.as_str() == Some(sgp_server_id))
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct PositionChampions(#[serde(default)] pub HashMap<String, Vec<i64>>);

impl PositionChampions {
    /// `config.champions[position || 'default']`.
    pub fn for_position(&self, position: Option<&str>) -> Vec<i64> {
        let key = position.filter(|value| !value.is_empty()).unwrap_or("default");
        self.0.get(key).cloned().unwrap_or_default()
    }
}

fn default_strategy() -> String {
    "show-and-lock-in".to_string()
}

fn default_bench_swap_delay() -> f64 {
    2.9
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PickConfig {
    pub enabled: bool,
    pub champions: PositionChampions,
    pub delay_seconds: f64,
    pub ignore_intent: bool,
    pub strategy: String,
    pub show_intent: bool,
    pub bench_select_first_available_champion: bool,
    pub bench_swap_accumulated_delay_seconds: f64,
    pub bench_handle_trade_enabled: bool,
}

impl Default for PickConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            champions: PositionChampions::default(),
            delay_seconds: 0.0,
            ignore_intent: false,
            strategy: default_strategy(),
            show_intent: false,
            bench_select_first_available_champion: false,
            bench_swap_accumulated_delay_seconds: default_bench_swap_delay(),
            bench_handle_trade_enabled: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BanConfig {
    pub enabled: bool,
    pub champions: PositionChampions,
    pub strategy: String,
    pub delay_seconds: f64,
}

impl Default for BanConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            champions: PositionChampions::default(),
            strategy: default_strategy(),
            delay_seconds: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActiveGroupConfig {
    pub group_id: String,
    pub temporarily_disabled: bool,
    pub pick: PickConfig,
    pub ban: BanConfig,
}

/// `getActiveGroupConfig`. The original returns `undefined` when the mode is not
/// known yet and `null` when no group matches — the difference matters to the
/// callers, so it is modelled as `Option<Option<..>>`.
pub fn get_active_group_config(
    groups: &[Value],
    game_mode: Option<&str>,
    queue_type: Option<&str>,
    is_custom_game: bool,
    sgp_server_id: &str,
    league_servers: &Value,
    temporarily_disabled: bool,
    pick_config: &Map<String, Value>,
    ban_config: &Map<String, Value>,
) -> Option<Option<ActiveGroupConfig>> {
    let (game_mode, queue_type) = match (game_mode, queue_type) {
        (Some(mode), Some(queue)) => (mode, queue),
        _ => return None,
    };

    let matched = groups.iter().find(|group| {
        if !group_supported_on_sgp_server(group, sgp_server_id, league_servers) {
            return false;
        }
        if group.get("isCustom").and_then(Value::as_bool).unwrap_or(false) != is_custom_game {
            return false;
        }

        group
            .get("targetGameModes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|entry| {
                entry.get("gameMode").and_then(Value::as_str) == Some(game_mode)
                    && entry
                        .get("queueTypes")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .any(|queue| queue.as_str() == Some("*") || queue.as_str() == Some(queue_type))
            })
    });

    // "no group matches" is `null` in the original, which is not the `undefined`
    // returned above when the mode is still unknown
    let Some(matched) = matched else {
        return Some(None);
    };

    let Some(group_id) = matched.get("groupId").and_then(Value::as_str) else {
        return Some(None);
    };

    let group_id = group_id.to_string();

    let parse = |configs: &Map<String, Value>| configs.get(&group_id).cloned();

    Some(Some(ActiveGroupConfig {
        pick: parse(pick_config)
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default(),
        ban: parse(ban_config)
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default(),
        group_id,
        temporarily_disabled,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChampionStatus {
    Pickable,
    Unpickable,
    NotOwned,
    Banned,
    PickIntented,
    Picked,
    SubsetPickable,
    Swappable,
    Unswappable,
    SubsetSwappable,
    WaitingOnFinalization,
    Bannable,
    Unbannable,
    Unknown,
}

impl ChampionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChampionStatus::Pickable => "pickable",
            ChampionStatus::Unpickable => "unpickable",
            ChampionStatus::NotOwned => "not-owned",
            ChampionStatus::Banned => "banned",
            ChampionStatus::PickIntented => "pick-intented",
            ChampionStatus::Picked => "picked",
            ChampionStatus::SubsetPickable => "subset-pickable",
            ChampionStatus::Swappable => "swappable",
            ChampionStatus::Unswappable => "unswappable",
            ChampionStatus::SubsetSwappable => "subset-swappable",
            ChampionStatus::WaitingOnFinalization => "waiting-on-finalization",
            ChampionStatus::Bannable => "bannable",
            ChampionStatus::Unbannable => "unbannable",
            ChampionStatus::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpectedChampion {
    pub id: i64,
    pub status: ChampionStatus,
}

fn selection_flag(grid: Option<&Value>, name: &str) -> bool {
    grid.and_then(|grid| grid.get("selectionStatus"))
        .and_then(|status| status.get(name))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn grid_of<'a>(grids: &'a Value, champion_id: i64) -> Option<&'a Value> {
    grids.get(champion_id.to_string())
}

/// `getExpectedPicks`.
pub fn get_expected_picks(
    active: Option<&ActiveGroupConfig>,
    assigned_position: Option<&str>,
    game_mode: Option<&str>,
    grid_champions: &Value,
    pickable: &[i64],
    allow_duplicate_picks: bool,
    allow_subset_champion_picks: bool,
    subset_champion_list: &[i64],
) -> Option<Vec<ExpectedChampion>> {
    let active = active?;

    Some(
        active
            .pick
            .champions
            .for_position(assigned_position)
            .into_iter()
            .map(|id| ExpectedChampion {
                status: pick_status(
                    id,
                    game_mode,
                    grid_of(grid_champions, id),
                    pickable,
                    allow_duplicate_picks,
                    allow_subset_champion_picks,
                    subset_champion_list,
                ),
                id,
            })
            .collect(),
    )
}

fn pick_status(
    id: i64,
    game_mode: Option<&str>,
    grid: Option<&Value>,
    pickable: &[i64],
    allow_duplicate_picks: bool,
    allow_subset_champion_picks: bool,
    subset_champion_list: &[i64],
) -> ChampionStatus {
    // "勇敢举动" is not a champion; the LCU has no grid entry for it either
    if id == CHEERY_BRAVERY_ID && game_mode == Some("CHERRY") {
        return ChampionStatus::Pickable;
    }

    let Some(grid) = grid else {
        return ChampionStatus::Unknown;
    };

    if !pickable.contains(&id) {
        return if grid.get("owned").and_then(Value::as_bool).unwrap_or(false) {
            ChampionStatus::Unpickable
        } else {
            ChampionStatus::NotOwned
        };
    }

    if selection_flag(Some(grid), "isBanned") {
        return ChampionStatus::Banned;
    }

    if !allow_duplicate_picks {
        if selection_flag(Some(grid), "pickIntented") && !selection_flag(Some(grid), "pickIntentedByMe") {
            return ChampionStatus::PickIntented;
        }
        if selection_flag(Some(grid), "pickedByOtherOrBanned") && !selection_flag(Some(grid), "selectedByMe") {
            return ChampionStatus::Picked;
        }
    }

    if allow_subset_champion_picks {
        return if subset_champion_list.contains(&id) {
            ChampionStatus::SubsetPickable
        } else {
            ChampionStatus::Unpickable
        };
    }

    ChampionStatus::Pickable
}

/// `getExpectedSwaps`.
pub fn get_expected_swaps(
    active: Option<&ActiveGroupConfig>,
    bench_enabled: bool,
    scoped_bench: &[i64],
    assigned_position: Option<&str>,
    pickable: &[i64],
    allow_subset_champion_picks: bool,
    in_ban_pick_phase: bool,
    subset_champion_list: &[i64],
) -> Option<Vec<ExpectedChampion>> {
    let active = active.filter(|_| bench_enabled && !scoped_bench.is_empty())?;

    Some(
        active
            .pick
            .champions
            .for_position(assigned_position)
            .into_iter()
            .map(|id| ExpectedChampion {
                id,
                status: if !scoped_bench.contains(&id) || !pickable.contains(&id) {
                    ChampionStatus::Unswappable
                } else if allow_subset_champion_picks && in_ban_pick_phase {
                    if subset_champion_list.contains(&id) {
                        ChampionStatus::SubsetSwappable
                    } else {
                        ChampionStatus::WaitingOnFinalization
                    }
                } else {
                    ChampionStatus::Swappable
                },
            })
            .collect(),
    )
}

/// `getExpectedBans`.
pub fn get_expected_bans(
    active: Option<&ActiveGroupConfig>,
    assigned_position: Option<&str>,
    grid_champions: &Value,
    bannable: &[i64],
) -> Option<Vec<ExpectedChampion>> {
    let active = active?;

    Some(
        active
            .ban
            .champions
            .for_position(assigned_position)
            .into_iter()
            .map(|id| {
                let grid = grid_of(grid_champions, id);
                ExpectedChampion {
                    id,
                    status: match grid {
                        None => ChampionStatus::Unknown,
                        Some(_) if !bannable.contains(&id) => ChampionStatus::Unbannable,
                        Some(grid)
                            if selection_flag(Some(grid), "isBanned") && id != NONE_CHAMPION_ID =>
                        {
                            ChampionStatus::Banned
                        }
                        Some(grid)
                            if selection_flag(Some(grid), "pickIntented")
                                && !selection_flag(Some(grid), "pickIntentedByMe") =>
                        {
                            ChampionStatus::PickIntented
                        }
                        _ => ChampionStatus::Bannable,
                    },
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn timer_correction_uses_the_epoch_pair() {
        let timer = json!({
            "adjustedTimeLeftInPhase": 20_000,
            "internalNowInEpochMs": 1_000,
            "totalTimeInPhase": 30_000,
            "isInfinite": false
        });

        let corrected = corrected_timer(Some(&timer), 6_000).unwrap();
        // 20s left as of internalNow=1s, and 5s of wall clock has passed since
        assert_eq!(corrected.remaining_ms, 15_000.0);
        assert_eq!(corrected.elapsed_ms, 15_000.0);

        assert!(corrected_timer(Some(&json!({ "isInfinite": true })), 0).is_none());
        assert!(corrected_timer(None, 0).is_none());
    }

    #[test]
    fn calibrated_delay_never_exceeds_the_phase() {
        // past the target offset the clamp wins over the remaining time
        let past = CorrectedTimer { remaining_ms: 1_000.0, total_ms: 30_000.0, elapsed_ms: 29_000.0 };
        assert_eq!(phase_calibrated_delay_ms(5_000.0, 25_000.0, Some(past)), 0.0);

        // otherwise the end of the phase is the binding limit
        let ending = CorrectedTimer { remaining_ms: 1_000.0, total_ms: 30_000.0, elapsed_ms: 20_000.0 };
        assert_eq!(phase_calibrated_delay_ms(5_000.0, 25_000.0, Some(ending)), 1_000.0);

        // before the target offset the configured delay wins
        let early = CorrectedTimer { remaining_ms: 20_000.0, total_ms: 30_000.0, elapsed_ms: 1_000.0 };
        assert_eq!(phase_calibrated_delay_ms(3_000.0, 25_000.0, Some(early)), 3_000.0);

        // past the target offset it clamps to zero rather than going negative
        let late = CorrectedTimer { remaining_ms: 20_000.0, total_ms: 30_000.0, elapsed_ms: 26_000.0 };
        assert_eq!(phase_calibrated_delay_ms(3_000.0, 25_000.0, Some(late)), 0.0);

        assert_eq!(phase_calibrated_delay_ms(2_500.0, 0.0, None), 2_500.0);
    }

    #[test]
    fn subset_pool_only_applies_during_the_draft_phase() {
        let ban_pick = json!({ "phase": "BAN_PICK" });
        let finalization = json!({ "phase": "FINALIZATION" });
        let bench = vec![json!({ "championId": 7 }), json!({ "championId": 9 })];

        assert_eq!(scoped_bench_champions(&[3, 4], Some(&ban_pick), &bench), vec![3, 4]);
        assert_eq!(scoped_bench_champions(&[3, 4], Some(&finalization), &bench), vec![7, 9]);
        assert_eq!(scoped_bench_champions(&[], Some(&ban_pick), &bench), vec![7, 9]);
    }

    #[test]
    fn current_actions_walks_to_the_first_row_with_work() {
        let session = json!({
            "localPlayerCellId": 2,
            "actions": [
                [{ "actorCellId": 2, "completed": true, "type": "ban" }],
                [
                    { "actorCellId": 1, "completed": false, "type": "ban" },
                    { "actorCellId": 2, "completed": false, "type": "ban", "actionId": 5 }
                ]
            ]
        });

        let mine = current_actions(Some(&session));
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0]["actionId"], 5);
        assert_eq!(active_action(&mine).unwrap()["actionId"], 5);

        // every row finished → nothing to do
        let done = json!({ "localPlayerCellId": 2, "actions": [[{ "completed": true }]] });
        assert!(current_actions(Some(&done)).is_empty());
    }

    #[test]
    fn move_resolution_covers_draft_and_bench_cases() {
        let shown = json!({ "championId": 12 });
        let blank = json!({ "championId": 0 });

        let cases: Vec<(MoveInputs, Option<&str>)> = vec![
            (MoveInputs { is_pick_intenting: true, active_action: None, is_picking_now: false,
                is_banning_now: false, is_voting_now: false, allow_subset_champion_picks: false,
                bench_enabled: false, current_session_champion_id: None, timer: None },
                Some("pick-intent")),
            (MoveInputs { is_pick_intenting: false, active_action: Some(&blank), is_picking_now: true,
                is_banning_now: false, is_voting_now: false, allow_subset_champion_picks: false,
                bench_enabled: false, current_session_champion_id: None, timer: None },
                Some("show-pick")),
            (MoveInputs { is_pick_intenting: false, active_action: Some(&shown), is_picking_now: true,
                is_banning_now: false, is_voting_now: false, allow_subset_champion_picks: false,
                bench_enabled: false, current_session_champion_id: None, timer: None },
                Some("complete-pick")),
            (MoveInputs { is_pick_intenting: false, active_action: Some(&blank), is_picking_now: false,
                is_banning_now: true, is_voting_now: false, allow_subset_champion_picks: false,
                bench_enabled: false, current_session_champion_id: None, timer: None },
                Some("show-ban")),
            (MoveInputs { is_pick_intenting: false, active_action: Some(&blank), is_picking_now: false,
                is_banning_now: false, is_voting_now: true, allow_subset_champion_picks: false,
                bench_enabled: false, current_session_champion_id: None, timer: None },
                Some("vote")),
        ];

        for (inputs, expected) in cases {
            assert_eq!(get_move(inputs).map(|m| m.as_str().to_string()), expected.map(str::to_string));
        }

        // bench swap needs a champion already assigned, and the subset variant needs
        // the draft phase
        let timer = json!({ "phase": "BAN_PICK" });
        assert_eq!(
            get_move(MoveInputs { is_pick_intenting: false, active_action: None, is_picking_now: false,
                is_banning_now: false, is_voting_now: false, allow_subset_champion_picks: true,
                bench_enabled: true, current_session_champion_id: Some(3), timer: Some(&timer) }),
            Some(Move::SubsetBenchSwap)
        );
        assert_eq!(
            get_move(MoveInputs { is_pick_intenting: false, active_action: None, is_picking_now: false,
                is_banning_now: false, is_voting_now: false, allow_subset_champion_picks: false,
                bench_enabled: true, current_session_champion_id: Some(3), timer: None }),
            Some(Move::BenchSwap)
        );
        assert_eq!(
            get_move(MoveInputs { is_pick_intenting: false, active_action: None, is_picking_now: false,
                is_banning_now: false, is_voting_now: false, allow_subset_champion_picks: false,
                bench_enabled: true, current_session_champion_id: None, timer: None }),
            None
        );
    }

    fn group(group_id: &str, custom: bool, modes: &Value, servers: &Value) -> Value {
        json!({
            "groupId": group_id,
            "isCustom": custom,
            "targetGameModes": modes,
            "supportedSgpServers": servers
        })
    }

    #[test]
    fn group_matching_needs_mode_queue_and_server() {
        let groups = vec![group(
            "ranked-solo",
            false,
            &json!([{ "gameMode": "RANKED_SOLO_5x5", "queueTypes": ["420", "430"] }]),
            &json!(["*"]),
        )];
        let servers = json!({ "TENCENT_HN1": { "common": "https://x" } });
        let pick = Map::new();
        let ban = Map::new();

        let matched = get_active_group_config(
            &groups, Some("RANKED_SOLO_5x5"), Some("420"), false, "TENCENT_HN1", &servers, false, &pick, &ban,
        );
        assert_eq!(matched.unwrap().unwrap().group_id, "ranked-solo");

        // an unknown queue type does not match
        assert!(get_active_group_config(
            &groups, Some("RANKED_SOLO_5x5"), Some("999"), false, "TENCENT_HN1", &servers, false, &pick, &ban,
        )
        .unwrap()
        .is_none());

        // `*` queue types match anything
        let wildcard = vec![group("any", false, &json!([{ "gameMode": "ARAM", "queueTypes": ["*"] }]), &json!(["*"]))];
        assert_eq!(
            get_active_group_config(&wildcard, Some("ARAM"), Some("1900"), false, "NA1", &servers, false, &pick, &ban)
                .unwrap().unwrap().group_id,
            "any"
        );

        // a group limited to one SGP server is skipped when that server is unknown
        let regional = vec![group("cn", false, &json!([{ "gameMode": "ARAM", "queueTypes": ["*"] }]), &json!(["TENCENT_HN1"]))];
        assert!(get_active_group_config(&regional, Some("ARAM"), Some("1900"), false, "", &servers, false, &pick, &ban)
            .unwrap().is_none());
        assert!(get_active_group_config(
            &regional, Some("ARAM"), Some("1900"), false, "TENCENT_XX9", &servers, false, &pick, &ban)
            .unwrap().is_none());

        // mode/queue not known yet is `undefined`, which is not "no match"
        assert!(get_active_group_config(&groups, None, Some("420"), false, "TENCENT_HN1", &servers, false, &pick, &ban).is_none());
    }

    fn active_with(pick_ids: &[i64], ban_ids: &[i64]) -> ActiveGroupConfig {
        let mut pick_map = HashMap::new();
        pick_map.insert("default".to_string(), pick_ids.to_vec());
        pick_map.insert("middle".to_string(), vec![99]);

        let mut ban_map = HashMap::new();
        ban_map.insert("default".to_string(), ban_ids.to_vec());

        ActiveGroupConfig {
            group_id: "g".to_string(),
            temporarily_disabled: false,
            pick: PickConfig {
                champions: PositionChampions(pick_map),
                ..Default::default()
            },
            ban: BanConfig {
                champions: PositionChampions(ban_map),
                ..Default::default()
            },
        }
    }

    fn grid(owned: bool, flags: &Value) -> Value {
        json!({ "owned": owned, "selectionStatus": flags })
    }

    #[test]
    fn pick_status_follows_the_servers_own_view() {
        let active = active_with(&[1, 2, 3, 4, CHEERY_BRAVERY_ID], &[]);
        let grids = json!({
            "1": grid(true, &json!({})),
            "2": grid(true, &json!({ "isBanned": true })),
            "3": grid(true, &json!({ "pickIntented": true })),
            "4": grid(true, &json!({ "pickedByOtherOrBanned": true }))
            // 5 is absent from the grid entirely
        });
        let pickable = [1, 2, 3, 4, 5];

        // duplicate protection off: intent and other-people's-picks are reported
        let strict: Vec<ChampionStatus> = get_expected_picks(
            Some(&active), None, Some("CHERRY"), &grids, &pickable, false, false, &[],
        )
        .unwrap()
        .into_iter()
        .map(|entry| entry.status)
        .collect();
        assert_eq!(
            strict,
            vec![
                ChampionStatus::Pickable,
                ChampionStatus::Banned,
                ChampionStatus::PickIntented,
                ChampionStatus::Picked,
                // cherry's "bravery" vote is not a champion and has no grid entry
                ChampionStatus::Pickable,
            ]
        );

        // with duplicates allowed, those two are pickable again
        let loose: Vec<ChampionStatus> = get_expected_picks(
            Some(&active), None, Some("CHERRY"), &grids, &pickable, true, false, &[],
        )
        .unwrap()
        .into_iter()
        .map(|entry| entry.status)
        .collect();
        assert_eq!(loose[2], ChampionStatus::Pickable);
        assert_eq!(loose[3], ChampionStatus::Pickable);

        // outside cherry, bravery is looked up like any champion and is unknown
        let elsewhere = get_expected_picks(
            Some(&active), None, Some("CLASSIC"), &grids, &pickable, false, false, &[],
        )
        .unwrap();
        assert_eq!(elsewhere[4].status, ChampionStatus::Unknown);

        // not owned is distinguished from merely unpickable
        let unowned = json!({ "1": grid(false, &json!({})) });
        let one = active_with(&[1], &[]);
        let status = get_expected_picks(Some(&one), None, None, &unowned, &[], true, false, &[])
            .unwrap()[0]
            .status;
        assert_eq!(status, ChampionStatus::NotOwned);

        // an unknown grid entry is trusted as "do not pick"
        let missing = active_with(&[77], &[]);
        let status = get_expected_picks(Some(&missing), None, None, &json!({}), &[], true, false, &[])
            .unwrap()[0]
            .status;
        assert_eq!(status, ChampionStatus::Unknown);

        // the ranked position list wins over `default`
        let positioned = get_expected_picks(
            Some(&active), Some("middle"), None, &json!({ "99": grid(true, &json!({})) }), &[99], true, false, &[],
        )
        .unwrap();
        assert_eq!(positioned, vec![ExpectedChampion { id: 99, status: ChampionStatus::Pickable }]);
    }

    #[test]
    fn subset_draft_narrows_what_can_be_picked_and_swapped() {
        let active = active_with(&[1, 2], &[]);

        let subset = get_expected_picks(
            Some(&active), None, None, &json!({ "1": grid(true, &json!({})), "2": grid(true, &json!({})) }),
            &[1, 2], true, true, &[1],
        )
        .unwrap();
        assert_eq!(subset[0].status, ChampionStatus::SubsetPickable);
        assert_eq!(subset[1].status, ChampionStatus::Unpickable);

        let swaps = get_expected_swaps(Some(&active), true, &[1, 2], None, &[1, 2], true, true, &[1]).unwrap();
        assert_eq!(swaps[0].status, ChampionStatus::SubsetSwappable);
        assert_eq!(swaps[1].status, ChampionStatus::WaitingOnFinalization);

        // outside the draft phase, or without a bench, there is nothing to swap
        let plain = get_expected_swaps(Some(&active), true, &[1, 2], None, &[1, 2], false, true, &[]).unwrap();
        assert_eq!(plain[0].status, ChampionStatus::Swappable);
        assert!(get_expected_swaps(Some(&active), false, &[1], None, &[1], false, false, &[]).is_none());
        assert!(get_expected_swaps(Some(&active), true, &[], None, &[1], false, false, &[]).is_none());
    }

    #[test]
    fn empty_ban_can_be_repeated_but_a_banned_champion_cannot() {
        let active = active_with(&[], &[NONE_CHAMPION_ID, 8, 9]);
        let grids = json!({
            "-1": grid(true, &json!({ "isBanned": true })),
            "8": grid(true, &json!({ "isBanned": true })),
            "9": grid(true, &json!({ "pickIntented": true }))
        });

        let bans = get_expected_bans(Some(&active), None, &grids, &[-1, 8, 9, 10]).unwrap();
        assert_eq!(bans[0].status, ChampionStatus::Bannable, "空 ban 可重复");
        assert_eq!(bans[1].status, ChampionStatus::Banned);
        assert_eq!(bans[2].status, ChampionStatus::PickIntented);

        let mut with_ten = grids.as_object().unwrap().clone();
        with_ten.insert("10".to_string(), grid(true, &json!({})));
        let not_listed = active_with(&[], &[10]);
        let bans = get_expected_bans(Some(&not_listed), None, &Value::Object(with_ten), &[]).unwrap();
        assert_eq!(bans[0].status, ChampionStatus::Unbannable);
    }
}

#[cfg(test)]
mod delay_tests {
    use super::*;

    #[test]
    fn show_and_lock_in_doubles_the_second_phase_offset() {
        assert_eq!(target_offset_ms("show-and-lock-in", true, 5_000.0), 10_000.0);
        assert_eq!(target_offset_ms("show-and-lock-in", false, 5_000.0), 5_000.0);
        assert_eq!(target_offset_ms("just-show", true, 5_000.0), 5_000.0);

        assert!(ban_completes("lock-in-immediately", false));
        assert!(ban_completes("show-and-lock-in", true));
        assert!(!ban_completes("show-and-lock-in", false));
        assert!(!ban_completes("just-show", true));
    }

    #[test]
    fn subset_and_intent_picks_complete_differently() {
        assert!(pick_completes("show-subset-pick", "just-show"));
        assert!(pick_completes("complete-subset-pick", "just-show"));
        assert!(pick_completes("complete-pick", "show-and-lock-in"));
        assert!(!pick_completes("show-pick", "show-and-lock-in"));
        // an intent is never a completion, whatever the strategy says
        assert!(!pick_completes("pick-intent", "lock-in-immediately"));
    }

    #[test]
    fn bench_delay_counts_down_from_when_the_champion_appeared() {
        // 2.9s accumulated, seen 800ms ago → 2.1s left, and the phase has room
        assert_eq!(bench_swap_delay_ms(2_900.0, 10_000, 10_800, Some(9_000.0)), 2_100.0);
        // near the end of the phase the remaining time wins
        assert_eq!(bench_swap_delay_ms(2_900.0, 10_000, 10_800, Some(300.0)), 300.0);
        // already past the accumulated delay: negative, i.e. fire immediately
        assert_eq!(bench_swap_delay_ms(2_900.0, 10_000, 14_000, Some(9_000.0)), -1_100.0);
        assert_eq!(bench_swap_delay_ms(2_900.0, 10_000, 10_000, None), 2_900.0);
    }

    #[test]
    fn trade_decline_keeps_the_original_clamping() {
        // the "not in my list" decline path does not clamp the negative delay
        assert_eq!(champion_swap_delay_ms(3_000.0, 10_000, 14_000, None, false), -1_000.0);
        // both other paths clamp at zero and then take the phase limit
        assert_eq!(champion_swap_delay_ms(3_000.0, 10_000, 14_000, None, true), 0.0);
        assert_eq!(champion_swap_delay_ms(3_000.0, 10_000, 11_000, Some(500.0), true), 500.0);
        assert_eq!(champion_swap_delay_ms(3_000.0, 10_000, 11_000, Some(500.0), false), 500.0);
    }
}
