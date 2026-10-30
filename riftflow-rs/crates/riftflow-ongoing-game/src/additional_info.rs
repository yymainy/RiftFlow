//! `additional-info-controller.ts` — the GSM backfill for participants the LCU
//! session still hides.
//!
//! `extractTeamMembers` is the whole maths of this module and is pure; `update`
//! drives the one request the original queued.

use std::sync::Arc;

use serde_json::Value;

use crate::computed::{Additional, QueryStage};
use crate::ctx::{Ctx, trace, warn};
use crate::feature_gates::FeatureGates;
use crate::fetch;
use crate::priority;
use crate::queue::{TagMode, TaskFailure, TaskOptions};
use crate::record::{self, Pairs};
use crate::settings::OngoingGameSettings;
use crate::snapshot::LcSnapshot;

const EMPTY_PUUID: &str = crate::champ_select::EMPTY_PUUID;

fn puuid_of(member: &Value) -> String {
    member.get("puuid").and_then(Value::as_str).unwrap_or_default().to_string()
}

fn is_real(member: &Value) -> bool {
    let puuid = puuid_of(member);
    !puuid.is_empty() && puuid != EMPTY_PUUID
}

/// `extractTeamMembers`.
///
/// CHERRY is not a copy of CLASSIC: it keeps the zero `teamParticipantId`s the
/// classic branch drops, and it maps every member into one `TEAM-ALL`.
pub fn extract_team_members(
    game_mode: &str,
    team_one: &[Value],
    team_two: &[Value],
    spells: &[Value],
) -> Additional {
    let all: Vec<&Value> = team_one.iter().chain(team_two.iter()).filter(|p| is_real(p)).collect();

    let selections: Pairs<i64> = all
        .iter()
        .map(|p| (puuid_of(p), p.get("championId").and_then(Value::as_i64).unwrap_or_default()))
        .collect();

    let spell_map: Pairs<Value> = spells
        .iter()
        .map(|spell| {
            (
                puuid_of(spell),
                Value::Object(
                    [
                        ("spell1Id".to_string(), spell.get("spell1Id").cloned().unwrap_or(Value::Null)),
                        ("spell2Id".to_string(), spell.get("spell2Id").cloned().unwrap_or(Value::Null)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            )
        })
        .collect();

    let positions: Pairs<Value> = all
        .iter()
        .map(|p| {
            (
                puuid_of(p),
                Value::Object(
                    [
                        (
                            "position".to_string(),
                            Value::String(p.get("selectedPosition").and_then(Value::as_str).unwrap_or_default().to_string()),
                        ),
                        (
                            "role".to_string(),
                            record::parse_selected_role(p.get("selectedRole").and_then(Value::as_str)),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            )
        })
        .collect();

    if game_mode == "CHERRY" {
        let groups: Pairs<i64> = all
            .iter()
            .map(|p| (puuid_of(p), p.get("teamParticipantId").and_then(Value::as_i64).unwrap_or_default()))
            .collect();

        return Additional {
            teams: vec![("TEAM-ALL".to_string(), all.iter().map(|p| puuid_of(p)).collect())],
            selections,
            team_participant_groups: groups,
            spells: spell_map,
            positions,
        };
    }

    let groups: Pairs<i64> = all
        .iter()
        // the classic branch has no `if (!p.teamParticipantId) return acc` twin in
        // cherry, so a zero id is skipped here only
        .filter(|p| p.get("teamParticipantId").and_then(Value::as_i64).unwrap_or_default() != 0)
        .map(|p| (puuid_of(p), p.get("teamParticipantId").and_then(Value::as_i64).unwrap_or_default()))
        .collect();

    Additional {
        teams: vec![
            ("TEAM-100".to_string(), team_one.iter().map(puuid_of).filter(|p| is_puuid(p)).collect()),
            ("TEAM-200".to_string(), team_two.iter().map(puuid_of).filter(|p| is_puuid(p)).collect()),
        ],
        selections,
        team_participant_groups: groups,
        spells: spell_map,
        positions,
    }
}

fn is_puuid(puuid: &str) -> bool {
    !puuid.is_empty() && puuid != EMPTY_PUUID
}

/// the `Promise.allSettled` merge of the (currently one) member queries
pub fn merge_partials(partials: &[Additional]) -> Additional {
    let mut merged = Additional::default();

    for partial in partials {
        for (identifier, members) in &partial.teams {
            match merged.teams.iter_mut().find(|(existing, _)| existing == identifier) {
                Some((_, existing_members)) => *existing_members = crate::member_merge::member_merge(existing_members, members),
                None => merged.teams.push((identifier.clone(), members.clone())),
            }
        }

        for (puuid, champion_id) in &partial.selections {
            record_upsert(&mut merged.selections, puuid.clone(), *champion_id);
        }
        for (puuid, group) in &partial.team_participant_groups {
            record_upsert(&mut merged.team_participant_groups, puuid.clone(), *group);
        }
        for (puuid, spell) in &partial.spells {
            record_upsert(&mut merged.spells, puuid.clone(), spell.clone());
        }
        for (puuid, position) in &partial.positions {
            record_upsert(&mut merged.positions, puuid.clone(), position.clone());
        }
    }

    merged
}

fn record_upsert<T: Clone>(pairs: &mut Pairs<T>, key: String, value: T) {
    match pairs.iter_mut().find(|(existing, _)| *existing == key) {
        Some((_, existing)) => *existing = value,
        None => pairs.push((key, value)),
    }
}

/// `update()`.
pub async fn update(
    ctx: &Arc<Ctx>,
    snapshot: &LcSnapshot,
    settings: &OngoingGameSettings,
    gates: &FeatureGates,
    stage: &QueryStage,
    draft: Option<&Value>,
) {
    // `state.draft || !queryStage.gameInfo || phase !== 'in-game' || !me?.puuid`
    if draft.is_some() || !matches!(stage, QueryStage::InGame { .. }) || snapshot.self_puuid().is_none() {
        ctx.queues.cancel_by_tags(&["gsm-gameflow"], TagMode::Or);
        ctx.store.clear_additional();
        return;
    }

    ctx.queues.cancel_by_tags(&["gsm-gameflow"], TagMode::Or);

    let mut partials: Vec<Additional> = Vec::new();

    if gates.gsm_by_puuid {
        if let Some(partial) = gsm_game_members(ctx, snapshot).await {
            partials.push(partial);
        }
    }

    // the re-check the original's `then(...)` made before writing anything: the
    // game can have ended while the request was in flight
    let fresh = LcSnapshot::from_state(&ctx.state_sync);
    let fresh_stage = crate::computed::get_query_stage(&fresh, settings, ctx.store.draft().as_ref());
    if ctx.store.draft().is_some() || !matches!(fresh_stage, QueryStage::InGame { .. }) {
        return;
    }

    ctx.store.set_additional(&merge_partials(&partials));
}

/// `_getGsmGameMembers`.
async fn gsm_game_members(ctx: &Arc<Ctx>, snapshot: &LcSnapshot) -> Option<Additional> {
    let puuid = snapshot.self_puuid()?;
    let task_id = format!("gsm-gameflow:{puuid}");

    // the original tests `hasTask('gsm-gameflow')` without the puuid suffix, so this
    // guard never fires there; kept identical, dedup falls back on the cancel above
    if ctx.queues.has_task("gsm-gameflow") {
        trace(&format!("game members already in queue: {puuid}"));
        return None;
    }

    let request = fetch::sgp_gsm_by_puuid(&puuid, None);
    let fetcher = ctx.fetcher.clone();
    let options = TaskOptions::new()
        .priority(priority::ADDITIONAL_INFO)
        .tags(&[puuid.as_str(), "gsm-gameflow"]);

    match ctx
        .queues
        .add(
            "misc",
            &task_id,
            async move { fetcher.fetch(request).await.map_err(TaskFailure::from) },
            options,
        )
        .await
    {
        Ok(data) => {
            trace("additional team info by gsm game");
            let game = data.get("game")?;
            let team_one = game.get("teamOne").and_then(Value::as_array).cloned().unwrap_or_default();
            let team_two = game.get("teamTwo").and_then(Value::as_array).cloned().unwrap_or_default();
            let spells = game
                .get("playerChampionSelections")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let game_mode = game.get("gameMode").and_then(Value::as_str).unwrap_or_default();

            Some(extract_team_members(game_mode, &team_one, &team_two, &spells))
        }
        Err(error) if error.is_abort() => {
            trace("abort error getting game members");
            None
        }
        Err(error) if error.is_status(404) => None,
        Err(error) => {
            warn("error getting game members", &error.message);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn member(puuid: &str, champion_id: i64, participant_id: i64, position: &str, role: &str) -> Value {
        json!({
            "puuid": puuid,
            "championId": champion_id,
            "teamParticipantId": participant_id,
            "selectedPosition": position,
            "selectedRole": role,
        })
    }

    #[test]
    fn classic_extraction_splits_teams_and_skips_zero_participant_ids() {
        let extracted = extract_team_members(
            "CLASSIC",
            &[
                member("p1", 1, 11, "TOP", "TOP.PRIMARY.TOP.JUNGLE"),
                member(EMPTY_PUUID, 2, 12, "JUNGLE", "JUNGLE.PRIMARY.JUNGLE.TOP"),
                member("p0", 8, 0, "MIDDLE", "MIDDLE.PRIMARY.MIDDLE.BOTTOM"),
            ],
            &[member("p2", 3, 21, "MIDDLE", "MIDDLE.PRIMARY.MIDDLE.BOTTOM")],
            &[json!({ "puuid": "p1", "spell1Id": 4, "spell2Id": 14 }), json!({ "puuid": "p2", "spell1Id": 7, "spell2Id": 4 })],
        );

        assert_eq!(extracted.teams[0], ("TEAM-100".to_string(), vec!["p1".to_string(), "p0".to_string()]));
        assert_eq!(extracted.teams[1], ("TEAM-200".to_string(), vec!["p2".to_string()]));
        assert_eq!(
            extracted.team_participant_groups,
            vec![("p1".to_string(), 11), ("p2".to_string(), 21)],
            "the zero id is filtered out"
        );
        assert_eq!(extracted.selections, vec![("p1".to_string(), 1), ("p0".to_string(), 8), ("p2".to_string(), 3)]);
        assert_eq!(extracted.spells[0].1, json!({ "spell1Id": 4, "spell2Id": 14 }));
        assert_eq!(extracted.positions[0].1["role"]["secondary"], json!("JUNGLE"));
    }

    #[test]
    fn cherry_extraction_keeps_zero_participant_ids_and_one_team() {
        let extracted = extract_team_members(
            "CHERRY",
            &[member("p1", 10, 1, "", "TOP.PRIMARY.TOP.JUNGLE"), member(EMPTY_PUUID, 99, 9, "", "TOP")],
            &[member("p2", 20, 0, "", "BOTTOM.SECONDARY.TOP.BOTTOM")],
            &[json!({ "puuid": "p1", "spell1Id": 32, "spell2Id": 4 })],
        );

        assert_eq!(extracted.teams, vec![("TEAM-ALL".to_string(), vec!["p1".to_string(), "p2".to_string()])]);
        assert_eq!(
            extracted.team_participant_groups,
            vec![("p1".to_string(), 1), ("p2".to_string(), 0)],
            "cherry does not filter the zero id"
        );
        assert_eq!(extracted.selections, vec![("p1".to_string(), 10), ("p2".to_string(), 20)]);
    }

    #[test]
    fn merging_partials_merges_team_members_and_overwrites_the_rest() {
        let first = Additional {
            teams: vec![("TEAM-100".to_string(), vec!["a".to_string(), "b".to_string()])],
            selections: vec![("a".to_string(), 1)],
            ..Default::default()
        };
        let second = Additional {
            teams: vec![("TEAM-100".to_string(), vec!["b".to_string(), "c".to_string()])],
            selections: vec![("a".to_string(), 2)],
            ..Default::default()
        };

        let merged = merge_partials(&[first, second]);
        assert_eq!(merged.teams[0].1, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        assert_eq!(merged.selections, vec![("a".to_string(), 2)]);
    }

    #[test]
    fn an_empty_partial_list_clears_every_field() {
        let merged = merge_partials(&[]);
        assert_eq!(merged, Additional::default());
    }

    #[test]
    fn a_member_row_without_a_puuid_still_produces_a_key() {
        // the spells map is built from the raw list, unfiltered, in both branches
        let extracted = extract_team_members("CLASSIC", &[], &[], &[json!({ "spell1Id": 1, "spell2Id": 2 })]);
        assert_eq!(extracted.spells[0].0, String::new());
        assert_eq!(extracted.spells[0].1, json!({ "spell1Id": 1, "spell2Id": 2 }));
    }
}
