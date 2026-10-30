//! `computed-state.ts` — the four computeds the panel renders and the two loaders
//! react to: `queryStage`, `teams`, `championSelections`, `positionAssignments`,
//! plus `teamParticipantGroups` and `mergedPremadeTeamMap` from `state.ts`.
//!
//! Everything here is a pure function of a snapshot plus the state the loaders
//! wrote (`additional`, `draft`, `champSelectHandoffSnapshot`). A draft replaces
//! the live read entirely: `draft` non-null means every prop comes off the draft,
//! and `teamParticipantGroups` becomes `{}` — no gameflow, no participant groups.

use serde_json::{Map, Value, json};

use crate::champ_select::{
    EMPTY_PUUID, Deobfuscate, HandoffSnapshot, collect_visible_members,
};
use crate::member_merge::member_merge;
use crate::record::{Pairs, as_str, entries, parse_selected_role};
use crate::settings::OngoingGameSettings;
use crate::snapshot::LcSnapshot;

/// The `ongoing-game-main` `state.additional` prop, as `AdditionalResult`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Additional {
    pub teams: Pairs<Vec<String>>,
    pub selections: Pairs<i64>,
    pub team_participant_groups: Pairs<i64>,
    pub spells: Pairs<Value>,
    pub positions: Pairs<Value>,
}

impl Additional {
    pub fn from_value(value: &Value) -> Self {
        let string_lists = |key: &str| -> Pairs<Vec<String>> {
            entries(value.get(key).unwrap_or(&Value::Null))
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        v.as_array()
                            .map(|list| {
                                list.iter()
                                    .filter_map(|member| member.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    )
                })
                .collect()
        };

        let number_map = |key: &str| -> Pairs<i64> {
            entries(value.get(key).unwrap_or(&Value::Null))
                .into_iter()
                .filter_map(|(k, v)| v.as_i64().map(|number| (k, number)))
                .collect()
        };

        let value_map = |key: &str| -> Pairs<Value> {
            entries(value.get(key).unwrap_or(&Value::Null))
        };

        Self {
            teams: string_lists("teams"),
            selections: number_map("selections"),
            team_participant_groups: number_map("teamParticipantGroups"),
            spells: value_map("spells"),
            positions: value_map("positions"),
        }
    }

    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("teams".to_string(), crate::record::object(&self.teams));
        map.insert("selections".to_string(), crate::record::object(&self.selections));
        map.insert(
            "teamParticipantGroups".to_string(),
            crate::record::object(&self.team_participant_groups),
        );
        map.insert("spells".to_string(), crate::record::object(&self.spells));
        map.insert("positions".to_string(), crate::record::object(&self.positions));
        Value::Object(map)
    }
}

/// `QueryStage`.
#[derive(Clone, Debug, PartialEq)]
pub enum QueryStage {
    Unavailable,
    ChampSelect {
        queue_id: i64,
        queue_type: String,
        game_mode: String,
        game_id: i64,
    },
    InGame {
        queue_id: i64,
        queue_type: String,
        game_mode: String,
        game_id: i64,
    },
    Lobby {
        queue_id: i64,
        queue_type: String,
    },
    Draft {
        queue_id: i64,
        queue_type: String,
    },
}

impl QueryStage {
    /// `phase` on the wire: `champ-select` | `in-game` | `lobby` | `draft` | `unavailable`.
    pub fn phase(&self) -> &'static str {
        match self {
            QueryStage::ChampSelect { .. } => "champ-select",
            QueryStage::InGame { .. } => "in-game",
            QueryStage::Lobby { .. } => "lobby",
            QueryStage::Draft { .. } => "draft",
            QueryStage::Unavailable => "unavailable",
        }
    }

    pub fn game_id(&self) -> Option<i64> {
        match self {
            QueryStage::ChampSelect { game_id, .. } | QueryStage::InGame { game_id, .. } => {
                Some(*game_id)
            }
            _ => None,
        }
    }

    pub fn queue_type(&self) -> Option<&str> {
        match self {
            QueryStage::ChampSelect { queue_type, .. }
            | QueryStage::InGame { queue_type, .. }
            | QueryStage::Lobby { queue_type, .. }
            | QueryStage::Draft { queue_type, .. } => Some(queue_type),
            QueryStage::Unavailable => None,
        }
    }

    pub fn is_cherry(&self) -> bool {
        self.queue_type() == Some("CHERRY")
    }

    /// `gameInfo` — the champ-select/in-game record carries `gameMode`/`gameId`,
    /// the lobby and draft ones only `queueId`/`queueType`, unavailable is `null`.
    pub fn to_value(&self) -> Value {
        match self {
            QueryStage::Unavailable => json!({ "phase": "unavailable", "gameInfo": null }),
            QueryStage::ChampSelect { queue_id, queue_type, game_mode, game_id } => json!({
                "phase": "champ-select",
                "gameInfo": {
                    "queueId": queue_id, "queueType": queue_type,
                    "gameMode": game_mode, "gameId": game_id,
                },
            }),
            QueryStage::InGame { queue_id, queue_type, game_mode, game_id } => json!({
                "phase": "in-game",
                "gameInfo": {
                    "queueId": queue_id, "queueType": queue_type,
                    "gameMode": game_mode, "gameId": game_id,
                },
            }),
            QueryStage::Lobby { queue_id, queue_type } => json!({
                "phase": "lobby",
                "gameInfo": { "queueId": queue_id, "queueType": queue_type },
            }),
            QueryStage::Draft { queue_id, queue_type } => json!({
                "phase": "draft",
                "gameInfo": { "queueId": queue_id, "queueType": queue_type },
            }),
        }
    }

    /// `getDraftQueryStage`
    pub fn from_draft(draft: &Value) -> Self {
        let queue_type = if as_str(draft.get("gameModeKind")) == Some("cherry") {
            "CHERRY"
        } else {
            "CLASSIC"
        };

        QueryStage::Draft {
            queue_id: draft.get("queueId").and_then(Value::as_i64).unwrap_or_default(),
            queue_type: queue_type.to_string(),
        }
    }
}

/// `getLiveQueryStage`, with the draft branch of the `queryStage` getter folded in.
pub fn get_query_stage(
    snapshot: &LcSnapshot,
    settings: &OngoingGameSettings,
    draft: Option<&Value>,
) -> QueryStage {
    if let Some(draft) = draft {
        return QueryStage::from_draft(draft);
    }

    if !settings.enabled {
        return QueryStage::Unavailable;
    }

    let session_phase = snapshot
        .gameflow_session
        .as_ref()
        .and_then(|session| as_str(session.get("phase")))
        .unwrap_or_default();

    if session_phase == "ChampSelect" && snapshot.champ_select_session.is_some() {
        return gameflow_query_stage(snapshot, QueryStage::ChampSelect {
            queue_id: 0,
            queue_type: String::new(),
            game_mode: String::new(),
            game_id: 0,
        });
    }

    if is_in_game_phase(session_phase) {
        return gameflow_query_stage(snapshot, QueryStage::InGame {
            queue_id: 0,
            queue_type: String::new(),
            game_mode: String::new(),
            game_id: 0,
        });
    }

    if settings.query_in_lobby_phase {
        if let Some(lobby) = &snapshot.lobby {
            return QueryStage::Lobby {
                queue_id: lobby
                    .pointer("/gameConfig/queueId")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                queue_type: lobby
                    .pointer("/gameConfig/gameMode")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            };
        }
    }

    QueryStage::Unavailable
}

/// `getGameflowQueryStage` — fills the four gameInfo fields off the session.
fn gameflow_query_stage(snapshot: &LcSnapshot, mut stage: QueryStage) -> QueryStage {
    let Some(session) = &snapshot.gameflow_session else {
        return stage;
    };

    let queue_id = session.pointer("/gameData/queue/id").and_then(Value::as_i64).unwrap_or_default();
    let queue_type = session
        .pointer("/gameData/queue/type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let game_mode = session
        .pointer("/gameData/queue/gameMode")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let game_id = session
        .pointer("/gameData/gameId")
        .and_then(Value::as_i64)
        .unwrap_or_default();

    match &mut stage {
        QueryStage::ChampSelect {
            queue_id: q,
            queue_type: t,
            game_mode: m,
            game_id: g,
        }
        | QueryStage::InGame {
            queue_id: q,
            queue_type: t,
            game_mode: m,
            game_id: g,
        } => {
            *q = queue_id;
            *t = queue_type;
            *m = game_mode;
            *g = game_id;
        }
        _ => {}
    }

    stage
}

fn is_in_game_phase(phase: &str) -> bool {
    matches!(
        phase,
        "GameStart" | "InProgress" | "WaitingForStats" | "PreEndOfGame" | "EndOfGame" | "Reconnect"
    )
}

/// `state.isInEog` — the top-level `gameflow.phase` prop, not the session's phase.
pub fn is_in_eog(snapshot: &LcSnapshot) -> bool {
    matches!(
        snapshot.gameflow_phase.as_str(),
        "WaitingForStats" | "PreEndOfGame" | "EndOfGame"
    )
}

/// Every puuid of every team, in team order — what all four loaders react to.
pub fn team_puuids(teams: &[(String, Vec<String>)]) -> Vec<String> {
    teams.iter().flat_map(|(_, members)| members.clone()).collect()
}

/// `getLiveTeams` / `getDraftTeams`.
pub fn get_teams(
    snapshot: &LcSnapshot,
    settings: &OngoingGameSettings,
    stage: &QueryStage,
    additional: &Additional,
    draft: Option<&Value>,
    handoff: Option<&HandoffSnapshot>,
    deobfuscation_enabled: bool,
    magic: &dyn Deobfuscate,
) -> Pairs<Vec<String>> {
    if let Some(draft) = draft {
        return entries(draft.get("teams").unwrap_or(&Value::Null))
            .into_iter()
            .map(|(identifier, members)| {
                (
                    identifier,
                    members
                        .as_array()
                        .map(|list| {
                            list.iter()
                                .filter_map(|member| member.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                )
            })
            .collect();
    }

    match stage {
        QueryStage::ChampSelect { .. } => champ_select_teams(
            snapshot,
            stage,
            deobfuscation_enabled,
            magic,
        ),
        QueryStage::InGame { .. } => {
            let mut teams = in_game_teams(snapshot, stage, additional);
            merge_handoff_teams(&mut teams, stage, handoff.and_then(|snapshot| usable_handoff(stage, deobfuscation_enabled, Some(snapshot))));
            teams
        }
        QueryStage::Lobby { .. } | QueryStage::Draft { .. } | QueryStage::Unavailable => {
            if settings.query_in_lobby_phase && snapshot.lobby.is_some() {
                lobby_teams(snapshot)
            } else {
                Vec::new()
            }
        }
    }
}

fn champ_select_teams(
    snapshot: &LcSnapshot,
    stage: &QueryStage,
    deobfuscation_enabled: bool,
    magic: &dyn Deobfuscate,
) -> Pairs<Vec<String>> {
    let Some(session) = &snapshot.champ_select_session else {
        return Vec::new();
    };

    let members = collect_visible_members(session, deobfuscation_enabled, magic);

    if stage.is_cherry() {
        return vec![(
            "TEAM-ALL".to_string(),
            members.into_iter().map(|member| member.puuid).collect(),
        )];
    }

    let mut teams: Pairs<Vec<String>> = Vec::new();
    for member in members {
        match teams.iter_mut().find(|(identifier, _)| *identifier == member.team_identifier) {
            Some((_, puuids)) => puuids.push(member.puuid),
            None => teams.push((member.team_identifier, vec![member.puuid])),
        }
    }

    teams
}

fn in_game_teams(snapshot: &LcSnapshot, stage: &QueryStage, additional: &Additional) -> Pairs<Vec<String>> {
    let Some(session) = &snapshot.gameflow_session else {
        return Vec::new();
    };

    // `GameStart` still has no real participants to read
    if as_str(session.get("phase")) == Some("GameStart") {
        return Vec::new();
    }

    let game_data = session.get("gameData").unwrap_or(&Value::Null);
    let team_one = game_data
        .get("teamOne")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let team_two = game_data
        .get("teamTwo")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if stage.is_cherry() {
        let real_players: Vec<String> = game_data
            .get("playerChampionSelections")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|entry| entry.get("puuid").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let members = team_one
            .iter()
            .chain(team_two.iter())
            .filter(|member| is_real_puuid(member))
            .filter_map(|member| member.get("puuid").and_then(Value::as_str).map(str::to_string))
            .filter(|puuid| real_players.iter().any(|real| real == puuid))
            .collect();

        return vec![("TEAM-ALL".to_string(), members)];
    }

    let mut teams: Pairs<Vec<String>> = vec![
        ("TEAM-100".to_string(), Vec::new()),
        ("TEAM-200".to_string(), Vec::new()),
    ];

    for member in team_one.iter().filter(|member| is_real_puuid(member)) {
        push_team(&mut teams, "TEAM-100", member.get("puuid").and_then(Value::as_str).unwrap_or_default());
    }
    for member in team_two.iter().filter(|member| is_real_puuid(member)) {
        push_team(&mut teams, "TEAM-200", member.get("puuid").and_then(Value::as_str).unwrap_or_default());
    }

    for (identifier, members) in &additional.teams {
        match teams.iter_mut().find(|(existing, _)| existing == identifier) {
            // `teams[t] ? merge : members` — an empty array is truthy in JS, so a
            // known team is always merged, never replaced
            Some((_, existing)) => *existing = member_merge(existing, members),
            None => teams.push((identifier.clone(), members.clone())),
        }
    }

    teams
}

fn push_team(teams: &mut Pairs<Vec<String>>, identifier: &str, puuid: &str) {
    if let Some((_, members)) = teams.iter_mut().find(|(existing, _)| existing == identifier) {
        members.push(puuid.to_string());
    } else {
        teams.push((identifier.to_string(), vec![puuid.to_string()]));
    }
}

fn is_real_puuid(member: &Value) -> bool {
    match member.get("puuid").and_then(Value::as_str) {
        Some(puuid) => !puuid.is_empty() && puuid != EMPTY_PUUID,
        None => false,
    }
}

fn lobby_teams(snapshot: &LcSnapshot) -> Pairs<Vec<String>> {
    let mut members = Vec::new();

    if let Some(lobby) = &snapshot.lobby {
        for member in lobby.get("members").and_then(Value::as_array).into_iter().flatten() {
            if is_real_puuid(member) {
                members.push(member.get("puuid").and_then(Value::as_str).unwrap_or_default().to_string());
            }
        }
    }

    vec![("LOBBY".to_string(), members)]
}

/// `getLiveChampionSelections` / `getDraftChampionSelections`.
pub fn get_champion_selections(
    snapshot: &LcSnapshot,
    stage: &QueryStage,
    additional: &Additional,
    draft: Option<&Value>,
    handoff: Option<&HandoffSnapshot>,
    deobfuscation_enabled: bool,
    magic: &dyn Deobfuscate,
) -> Pairs<i64> {
    if let Some(draft) = draft {
        return entries(draft.get("championSelections").unwrap_or(&Value::Null))
            .into_iter()
            .filter_map(|(puuid, champion_id)| champion_id.as_i64().map(|id| (puuid, id)))
            .collect();
    }

    match stage {
        QueryStage::ChampSelect { .. } => {
            let Some(session) = &snapshot.champ_select_session else {
                return Vec::new();
            };

            collect_visible_members(session, deobfuscation_enabled, magic)
                .into_iter()
                .map(|member| (member.puuid, member.champion_id))
                .collect()
        }
        QueryStage::InGame { .. } => {
            let mut selections = in_game_champion_selections(snapshot, additional);
            if let Some(snapshot) = usable_handoff(stage, deobfuscation_enabled, handoff) {
                for (puuid, player) in &snapshot.players {
                    // `??=` — an in-game read always wins
                    if !selections.iter().any(|(existing, _)| existing == puuid) {
                        selections.push((puuid.clone(), player.champion_id));
                    }
                }
            }
            selections
        }
        _ => Vec::new(),
    }
}

fn in_game_champion_selections(snapshot: &LcSnapshot, additional: &Additional) -> Pairs<i64> {
    let mut selections: Pairs<i64> = Vec::new();

    let Some(session) = &snapshot.gameflow_session else {
        return selections;
    };
    let game_data = session.get("gameData").unwrap_or(&Value::Null);

    for entry in game_data
        .get("playerChampionSelections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let puuid = entry.get("puuid").and_then(Value::as_str).unwrap_or_default();
        if !puuid.is_empty() && puuid != EMPTY_PUUID {
            upsert(&mut selections, puuid.to_string(), entry.get("championId").and_then(Value::as_i64).unwrap_or_default());
        }
    }

    // `championId` truthiness only: a 0-champion entry does not overwrite
    for side in ["teamOne", "teamTwo"] {
        for entry in game_data.get(side).and_then(Value::as_array).into_iter().flatten() {
            let champion_id = entry.get("championId").and_then(Value::as_i64).unwrap_or_default();
            if champion_id != 0 {
                let puuid = entry.get("puuid").and_then(Value::as_str).unwrap_or_default();
                upsert(&mut selections, puuid.to_string(), champion_id);
            }
        }
    }

    for (puuid, champion_id) in &additional.selections {
        upsert(&mut selections, puuid.clone(), *champion_id);
    }

    selections
}

/// `Object.assign` onto an existing key keeps its position, so a replacement is
/// in-place and a new key is appended.
fn upsert<T: Clone>(pairs: &mut Pairs<T>, key: String, value: T) {
    match pairs.iter_mut().find(|(existing, _)| *existing == key) {
        Some((_, existing_value)) => *existing_value = value,
        None => pairs.push((key, value)),
    }
}

/// `getLivePositionAssignments` / `getDraftPositionAssignments`.
pub fn get_position_assignments(
    snapshot: &LcSnapshot,
    stage: &QueryStage,
    additional: &Additional,
    draft: Option<&Value>,
    handoff: Option<&HandoffSnapshot>,
    deobfuscation_enabled: bool,
    magic: &dyn Deobfuscate,
) -> Pairs<Value> {
    if let Some(draft) = draft {
        let positions = draft.get("positions").filter(|value| !value.is_null());
        let Some(positions) = positions else {
            return Vec::new();
        };

        return entries(positions)
            .into_iter()
            .map(|(puuid, assignment)| {
                let selected = assignment
                    .get("selected")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_uppercase();
                (puuid, json!({ "position": selected, "role": Value::Null }))
            })
            .collect();
    }

    match stage {
        QueryStage::ChampSelect { .. } => {
            let Some(session) = &snapshot.champ_select_session else {
                return Vec::new();
            };

            collect_visible_members(session, deobfuscation_enabled, magic)
                .into_iter()
                .map(|member| {
                    (member.puuid, json!({ "position": member.position, "role": Value::Null }))
                })
                .collect()
        }
        QueryStage::InGame { .. } => {
            let mut assignments = in_game_position_assignments(snapshot, additional);
            if let Some(snapshot) = usable_handoff(stage, deobfuscation_enabled, handoff) {
                for (puuid, player) in &snapshot.players {
                    if !assignments.iter().any(|(existing, _)| existing == puuid) {
                        assignments.push((
                            puuid.clone(),
                            json!({
                                "position": if player.position.is_empty() { "NONE" } else { player.position.as_str() },
                                "role": Value::Null,
                            }),
                        ));
                    }
                }
            }
            assignments
        }
        _ => Vec::new(),
    }
}

fn in_game_position_assignments(snapshot: &LcSnapshot, additional: &Additional) -> Pairs<Value> {
    let mut assignments: Pairs<Value> = Vec::new();

    let Some(session) = &snapshot.gameflow_session else {
        return assignments;
    };
    let game_data = session.get("gameData").unwrap_or(&Value::Null);

    for side in ["teamOne", "teamTwo"] {
        for member in game_data.get(side).and_then(Value::as_array).into_iter().flatten() {
            let puuid = member.get("puuid").and_then(Value::as_str).unwrap_or_default();
            if puuid.is_empty() || puuid == EMPTY_PUUID {
                continue;
            }

            upsert(
                &mut assignments,
                puuid.to_string(),
                json!({
                    "position": member.get("selectedPosition").and_then(Value::as_str).unwrap_or_default(),
                    "role": parse_selected_role(member.get("selectedRole").and_then(Value::as_str)),
                }),
            );
        }
    }

    for (puuid, position) in &additional.positions {
        upsert(&mut assignments, puuid.clone(), position.clone());
    }

    assignments
}

/// `getLiveTeamParticipantGroups` / the draft branch of the getter.
pub fn get_team_participant_groups(
    snapshot: &LcSnapshot,
    additional: &Additional,
    draft: Option<&Value>,
) -> Pairs<Vec<String>> {
    if draft.is_some() {
        return Vec::new();
    }

    // no session means no groups, even when `additional` has some
    let Some(session) = &snapshot.gameflow_session else {
        return Vec::new();
    };

    let mut groups: Pairs<Vec<String>> = Vec::new();

    for member in session
        .pointer("/gameData/teamOne")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            session
                .pointer("/gameData/teamTwo")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
    {
        // `if (!member.teamParticipantId) continue` — falsy, so `0` and `""` are
        // skipped too, not just a missing key
        let Some(identifier) = member.get("teamParticipantId").filter(|id| !is_falsy(id)) else {
            continue;
        };

        let key = js_key(identifier);
        let puuid = member.get("puuid").and_then(Value::as_str).unwrap_or_default().to_string();
        push_team(&mut groups, &key, &puuid);
    }

    for (puuid, identifier) in &additional.team_participant_groups {
        let key = identifier.to_string();
        match groups.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, members)) => {
                if !members.iter().any(|member| member == puuid) {
                    members.push(puuid.clone());
                }
            }
            None => groups.push((key, vec![puuid.clone()])),
        }
    }

    groups
}

/// `!value` for a JSON value.
fn is_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => number.as_i64().map(|n| n == 0).unwrap_or(false),
        Value::String(text) => text.is_empty(),
        _ => false,
    }
}

/// A JSON number or string used as an object key, the way JS coerces it.
fn js_key(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

/// `getUsableChampSelectHandoffSnapshot` — only for the game it was taken in.
fn usable_handoff<'a>(
    stage: &QueryStage,
    deobfuscation_enabled: bool,
    handoff: Option<&'a HandoffSnapshot>,
) -> Option<&'a HandoffSnapshot> {
    if !deobfuscation_enabled || !matches!(stage, QueryStage::InGame { .. }) {
        return None;
    }

    let handoff = handoff?;
    if handoff.game_id == stage.game_id()? {
        Some(handoff)
    } else {
        None
    }
}

fn merge_handoff_teams(
    teams: &mut Pairs<Vec<String>>,
    stage: &QueryStage,
    handoff: Option<&HandoffSnapshot>,
) {
    let Some(handoff) = handoff else {
        return;
    };

    let existing: Vec<String> = teams.iter().flat_map(|(_, members)| members.clone()).collect();

    if stage.is_cherry() {
        let members: Vec<String> = handoff
            .players
            .iter()
            .map(|(puuid, _)| puuid.clone())
            .filter(|puuid| !existing.iter().any(|known| known == puuid))
            .collect();

        if !members.is_empty() {
            match teams.iter_mut().find(|(identifier, _)| identifier == "TEAM-ALL") {
                Some((_, known)) => *known = member_merge(known, &members),
                None => teams.push(("TEAM-ALL".to_string(), members)),
            }
        }

        return;
    }

    let mut existing = existing;

    for (identifier, members) in &handoff.teams {
        let missing: Vec<String> = members
            .iter()
            .filter(|puuid| {
                handoff.players.iter().any(|(known, _)| known == *puuid)
                    && !existing.iter().any(|known| known == *puuid)
            })
            .cloned()
            .collect();

        if missing.is_empty() {
            continue;
        }

        match teams.iter_mut().find(|(existing_id, _)| existing_id == identifier) {
            Some((_, known)) => *known = member_merge(known, &missing),
            None => teams.push((identifier.clone(), missing.clone())),
        }

        existing.extend(missing);
    }
}

/// `state.mergedPremadeTeamMap` — `puuid -> 1-based premade index`.
pub fn get_merged_premade_team_map(
    teams: &[(String, Vec<String>)],
    participant_groups: &[(String, Vec<String>)],
    inferred: &[Vec<String>],
) -> Pairs<i64> {
    let mut team_of: Vec<(String, Option<String>)> = Vec::new();
    for (identifier, puuids) in teams {
        for puuid in puuids {
            match team_of.iter_mut().find(|(existing, _)| existing == puuid) {
                Some((_, known)) => *known = Some(identifier.clone()),
                None => team_of.push((puuid.clone(), Some(identifier.clone()))),
            }
        }
    }
    let team_of = team_of;

    let mut candidates: Vec<Vec<String>> = Vec::new();
    candidates.extend(participant_groups.iter().map(|(_, puuids)| puuids.clone()));
    candidates.extend(inferred.iter().cloned());
    let candidates: Vec<Vec<String>> = candidates.into_iter().filter(|team| team.len() > 1).collect();

    let simplified = remove_subsets(&candidates, |team| team.clone());

    let mut assigned_team_index = 0i64;
    let mut premade: Pairs<i64> = Vec::new();

    for puuids in simplified {
        let first = puuids.first().and_then(|puuid| lookup_team(&team_of, puuid));
        // a puuid outside `teams` reads as `undefined` in JS, and two unknowns
        // compare equal, so a group nobody is in still gets numbered
        if puuids
            .iter()
            .any(|puuid| lookup_team(&team_of, puuid) != first)
        {
            continue;
        }

        assigned_team_index += 1;
        for puuid in puuids {
            match premade.iter_mut().find(|(existing, _)| *existing == puuid) {
                Some((_, index)) => *index = assigned_team_index,
                None => premade.push((puuid, assigned_team_index)),
            }
        }
    }

    premade
}

fn lookup_team(team_of: &[(String, Option<String>)], puuid: &str) -> Option<String> {
    team_of
        .iter()
        .find(|(existing, _)| existing == puuid)
        .and_then(|(_, identifier)| identifier.clone())
}

/// `removeSubsets` from `shared/utils/team-up-calc.ts`, over puuid lists.
pub use crate::team_up::remove_subsets;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::champ_select::NoDeobfuscation;
    use serde_json::json;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn in_game_session(members: Vec<Value>) -> LcSnapshot {
        LcSnapshot::from_json(&json!({
            "gameflow": {
                "phase": "InProgress",
                "session": {
                    "phase": "InProgress",
                    "gameData": {
                        "queue": { "id": 420, "type": "CLASSIC", "gameMode": "CLASSIC" },
                        "gameId": 12345,
                        "playerChampionSelections": [],
                        "teamOne": members,
                        "teamTwo": [],
                    },
                },
            },
        }))
    }

    fn classic_member(puuid: &str, champion_id: i64, participant_id: Option<i64>) -> Value {
        json!({
            "puuid": puuid,
            "championId": champion_id,
            "selectedPosition": "JUNGLE",
            "selectedRole": "JUNGLE.PRIMARY.JUNGLE.TOP",
            "teamParticipantId": participant_id.map(Value::from).unwrap_or(Value::Null),
        })
    }

    struct Input {
        snapshot: LcSnapshot,
        settings: OngoingGameSettings,
        additional: Additional,
        draft: Option<Value>,
        handoff: Option<HandoffSnapshot>,
    }

    impl Input {
        fn new(snapshot: LcSnapshot) -> Self {
            Self {
                snapshot,
                settings: OngoingGameSettings::default(),
                additional: Additional::default(),
                draft: None,
                handoff: None,
            }
        }

        fn stage(&self) -> QueryStage {
            get_query_stage(&self.snapshot, &self.settings, self.draft.as_ref())
        }

        fn teams(&self) -> Pairs<Vec<String>> {
            get_teams(
                &self.snapshot,
                &self.settings,
                &self.stage(),
                &self.additional,
                self.draft.as_ref(),
                self.handoff.as_ref(),
                true,
                &NoDeobfuscation,
            )
        }

        fn selections(&self) -> Pairs<i64> {
            get_champion_selections(
                &self.snapshot,
                &self.stage(),
                &self.additional,
                self.draft.as_ref(),
                self.handoff.as_ref(),
                true,
                &NoDeobfuscation,
            )
        }

        fn positions(&self) -> Pairs<Value> {
            get_position_assignments(
                &self.snapshot,
                &self.stage(),
                &self.additional,
                self.draft.as_ref(),
                self.handoff.as_ref(),
                true,
                &NoDeobfuscation,
            )
        }
    }

    #[test]
    fn a_disabled_shard_or_nothing_going_on_is_unavailable() {
        let snapshot = LcSnapshot::default();
        let settings = OngoingGameSettings::default();
        assert_eq!(
            get_query_stage(&snapshot, &settings, None),
            QueryStage::Unavailable
        );

        let disabled = OngoingGameSettings {
            enabled: false,
            ..Default::default()
        };
        let live = in_game_session(vec![classic_member("real1", 2, None)]);
        assert_eq!(get_query_stage(&live, &disabled, None), QueryStage::Unavailable);
    }

    #[test]
    fn the_gameflow_phase_decides_between_champ_select_and_in_game() {
        let mut snapshot = in_game_session(vec![classic_member("real1", 2, None)]);
        snapshot.champ_select_session = None;
        let settings = OngoingGameSettings::default();
        assert_eq!(
            get_query_stage(&snapshot, &settings, None),
            QueryStage::InGame {
                queue_id: 420,
                queue_type: "CLASSIC".to_string(),
                game_mode: "CLASSIC".to_string(),
                game_id: 12345,
            }
        );

        let mut champ_select = in_game_session(vec![]);
        champ_select.gameflow_session = Some(json!({
            "phase": "ChampSelect",
            "gameData": {
                "queue": { "id": 400, "type": "RANKED", "gameMode": "RANKED" },
                "gameId": 99,
            },
        }));
        champ_select.champ_select_session = Some(json!({ "gameId": 99, "myTeam": [], "theirTeam": [] }));
        assert_eq!(
            get_query_stage(&champ_select, &settings, None),
            QueryStage::ChampSelect {
                queue_id: 400,
                queue_type: "RANKED".to_string(),
                game_mode: "RANKED".to_string(),
                game_id: 99,
            }
        );

        // ChampSelect without a champ-select session is neither: it falls through
        let no_cs_session = LcSnapshot {
            champ_select_session: None,
            ..champ_select
        };
        assert_eq!(get_query_stage(&no_cs_session, &settings, None), QueryStage::Unavailable);
    }

    #[test]
    fn a_lobby_is_only_a_stage_when_the_setting_allows_it() {
        let snapshot = LcSnapshot::from_json(&json!({
            "lobby": { "lobby": { "gameConfig": { "queueId": 420, "gameMode": "CLASSIC" }, "members": [] } },
        }));

        assert_eq!(
            get_query_stage(&snapshot, &OngoingGameSettings::default(), None),
            QueryStage::Lobby { queue_id: 420, queue_type: "CLASSIC".to_string() }
        );

        let off = OngoingGameSettings {
            query_in_lobby_phase: false,
            ..Default::default()
        };
        assert_eq!(get_query_stage(&snapshot, &off, None), QueryStage::Unavailable);
    }

    #[test]
    fn a_draft_overrides_the_live_read() {
        let draft = json!({
            "gameModeKind": "cherry",
            "queueId": 1300,
            "puuid": "me",
            "teams": { "TEAM-ALL": ["a", "b"] },
            "championSelections": { "a": 1, "b": 2 },
            "positions": { "a": { "selected": "jungle", "primary": "JUNGLE", "secondary": "TOP" } },
        });
        let snapshot = LcSnapshot::default();
        let settings = OngoingGameSettings::default();
        let stage = get_query_stage(&snapshot, &settings, Some(&draft));

        assert_eq!(
            stage,
            QueryStage::Draft { queue_id: 1300, queue_type: "CHERRY".to_string() }
        );
        assert_eq!(stage.to_value()["gameInfo"], json!({ "queueId": 1300, "queueType": "CHERRY" }));

        let input = Input {
            snapshot,
            settings,
            additional: Additional::default(),
            draft: Some(draft.clone()),
            handoff: None,
        };

        assert_eq!(input.teams(), vec![("TEAM-ALL".to_string(), list(&["a", "b"]))]);
        assert_eq!(input.selections(), vec![("a".to_string(), 1), ("b".to_string(), 2)]);
        assert_eq!(
            input.positions(),
            vec![("a".to_string(), json!({ "position": "JUNGLE", "role": null }))]
        );

        // a draft has no participant groups at all
        assert_eq!(get_team_participant_groups(&input.snapshot, &input.additional, Some(&draft),), Vec::new());
    }

    #[test]
    fn a_draft_without_positions_has_no_positions() {
        let draft = json!({
            "gameModeKind": "normal",
            "queueId": 420,
            "teams": {},
            "championSelections": {},
            "positions": null,
        });

        assert_eq!(
            get_position_assignments(
                &LcSnapshot::default(),
                &QueryStage::Draft { queue_id: 420, queue_type: "CLASSIC".to_string() },
                &Additional::default(),
                Some(&draft),
                None,
                true,
                &NoDeobfuscation
            ),
            Vec::new()
        );
    }

    #[test]
    fn in_game_teams_start_from_both_sides_and_drop_placeholders() {
        let snapshot = in_game_session(vec![
            classic_member("real1", 2, Some(1)),
            json!({ "puuid": EMPTY_PUUID, "championId": 5 }),
            json!({ "puuid": "", "championId": 6 }),
        ]);
        let input = Input::new(snapshot);

        assert_eq!(
            input.teams(),
            vec![("TEAM-100".to_string(), list(&["real1"])), ("TEAM-200".to_string(), list(&[]))]
        );
    }

    #[test]
    fn gamestart_sessions_have_no_teams_yet() {
        let mut snapshot = in_game_session(vec![classic_member("real1", 2, None)]);
        snapshot.gameflow_session = Some(json!({
            "phase": "GameStart",
            "gameData": { "queue": { "id": 420, "type": "CLASSIC", "gameMode": "CLASSIC" }, "gameId": 1, "teamOne": [], "teamTwo": [] },
        }));

        assert_eq!(Input::new(snapshot).teams(), Vec::new());
    }

    #[test]
    fn cherry_teams_are_one_list_filtered_to_the_real_participants() {
        let snapshot = LcSnapshot::from_json(&json!({
            "gameflow": {
                "phase": "InProgress",
                "session": {
                    "phase": "InProgress",
                    "gameData": {
                        "queue": { "id": 1300, "type": "CHERRY", "gameMode": "CHERRY" },
                        "gameId": 1,
                        "playerChampionSelections": [{ "puuid": "a", "championId": 1 }],
                        "teamOne": [
                            { "puuid": "a", "championId": 1 },
                            { "puuid": "bench", "championId": 2 },
                            { "puuid": EMPTY_PUUID, "championId": 3 },
                        ],
                        "teamTwo": [{ "puuid": "d", "championId": 4 }],
                    },
                },
            },
        }));

        assert_eq!(Input::new(snapshot).teams(), vec![("TEAM-ALL".to_string(), list(&["a"]))]);
    }

    #[test]
    fn additional_info_merges_into_known_teams_and_adds_unknown_ones() {
        let snapshot = in_game_session(vec![classic_member("a", 1, None), classic_member("b", 2, None)]);
        let input = Input {
            additional: Additional {
                teams: vec![
                    ("TEAM-100".to_string(), list(&["b", "c"])),
                    ("TEAM-900".to_string(), list(&["x"])),
                ],
                ..Default::default()
            },
            snapshot,
            ..Input::new(LcSnapshot::default())
        };

        // `memberMerge(["a","b"], ["b","c"])` keeps b in place and appends c
        assert_eq!(
            input.teams(),
            vec![
                ("TEAM-100".to_string(), list(&["a", "b", "c"])),
                ("TEAM-200".to_string(), list(&[])),
                ("TEAM-900".to_string(), list(&["x"])),
            ]
        );
    }

    #[test]
    fn selections_read_the_three_sources_in_order() {
        let mut snapshot = in_game_session(vec![json!({ "puuid": "a", "championId": 10 })]);
        snapshot.gameflow_session = Some(json!({
            "phase": "InProgress",
            "gameData": {
                "queue": { "id": 420, "type": "CLASSIC", "gameMode": "CLASSIC" },
                "gameId": 1,
                "playerChampionSelections": [{ "puuid": "a", "championId": 1 }, { "puuid": EMPTY_PUUID, "championId": 9 }],
                "teamOne": [{ "puuid": "a", "championId": 10 }, { "puuid": "b", "championId": 0 }],
                "teamTwo": [{ "puuid": "c", "championId": 30 }],
            },
        }));

        let input = Input {
            additional: Additional {
                selections: vec![("a".to_string(), 99), ("d".to_string(), 4)],
                ..Default::default()
            },
            snapshot,
            ..Input::new(LcSnapshot::default())
        };

        // b's zero champion is skipped, a is overwritten in place, d is appended
        assert_eq!(
            input.selections(),
            vec![
                ("a".to_string(), 99),
                ("c".to_string(), 30),
                ("d".to_string(), 4),
            ]
        );
    }

    #[test]
    fn positions_carry_the_parsed_role_and_the_raw_position() {
        let snapshot = in_game_session(vec![json!({
            "puuid": "a",
            "championId": 1,
            "selectedPosition": "JUNGLE",
            "selectedRole": "JUNGLE.PRIMARY.JUNGLE.TOP",
        })]);

        assert_eq!(
            Input::new(snapshot).positions(),
            vec![(
                "a".to_string(),
                json!({
                    "position": "JUNGLE",
                    "role": {
                        "current": "JUNGLE", "assignmentReason": "PRIMARY",
                        "primary": "JUNGLE", "secondary": "TOP", "fill": "NONE",
                    },
                })
            )]
        );
    }

    #[test]
    fn a_handoff_snapshot_only_supplies_missing_players() {
        let snapshot = in_game_session(vec![json!({
            "puuid": "real1",
            "championId": 2,
            "selectedPosition": "JUNGLE",
            "selectedRole": "JUNGLE.PRIMARY.JUNGLE.TOP",
        })]);
        let handoff = HandoffSnapshot::from_value(&json!({
            "gameId": 12345,
            "teams": { "TEAM-100": ["handoff1"], "TEAM-200": ["real1"] },
            "players": {
                "handoff1": { "championId": 1, "position": "TOP", "spell1Id": 4, "spell2Id": 14 },
                "real1": { "championId": 99, "position": "BOTTOM", "spell1Id": 7, "spell2Id": 4 },
            },
        }))
        .expect("valid");

        let input = Input {
            snapshot,
            handoff: Some(handoff.clone()),
            ..Input::new(LcSnapshot::default())
        };

        assert_eq!(
            input.teams(),
            vec![("TEAM-100".to_string(), list(&["real1", "handoff1"])), ("TEAM-200".to_string(), list(&[]))]
        );
        assert_eq!(input.selections(), vec![("real1".to_string(), 2), ("handoff1".to_string(), 1)]);
        assert_eq!(
            input.positions(),
            vec![
                (
                    "real1".to_string(),
                    json!({
                        "position": "JUNGLE",
                        "role": {
                            "current": "JUNGLE", "assignmentReason": "PRIMARY",
                            "primary": "JUNGLE", "secondary": "TOP", "fill": "NONE",
                        },
                    })
                ),
                ("handoff1".to_string(), json!({ "position": "TOP", "role": null }))
            ]
        );

        // a snapshot from another game is not usable at all
        let other_game = HandoffSnapshot { game_id: 99999, ..handoff };
        let stale = Input {
            snapshot: in_game_session(vec![json!({
                "puuid": "real1", "championId": 2, "selectedPosition": "JUNGLE", "selectedRole": "JUNGLE.PRIMARY.JUNGLE.TOP",
            })]),
            handoff: Some(other_game),
            ..Input::new(LcSnapshot::default())
        };
        assert_eq!(stale.selections(), vec![("real1".to_string(), 2)]);
        assert_eq!(stale.teams(), vec![("TEAM-100".to_string(), list(&["real1"])), ("TEAM-200".to_string(), list(&[]))]);
    }

    #[test]
    fn a_handoff_empty_position_becomes_none() {
        let snapshot = in_game_session(vec![]);
        let handoff = HandoffSnapshot::from_value(&json!({
            "gameId": 12345,
            "teams": { "TEAM-100": ["h"] },
            "players": { "h": { "championId": 1, "position": "", "spell1Id": 4, "spell2Id": 14 } },
        }))
        .expect("valid");

        let input = Input { snapshot, handoff: Some(handoff), ..Input::new(LcSnapshot::default()) };
        assert_eq!(
            input.positions(),
            vec![("h".to_string(), json!({ "position": "NONE", "role": null }))]
        );
    }

    #[test]
    fn participant_groups_are_keyed_by_the_participant_id_and_deduplicated() {
        let snapshot = LcSnapshot::from_json(&json!({
            "gameflow": {
                "phase": "InProgress",
                "session": {
                    "phase": "InProgress",
                    "gameData": {
                        "teamOne": [
                            { "puuid": "a", "teamParticipantId": 1 },
                            { "puuid": "b", "teamParticipantId": 1 },
                            { "puuid": "c", "teamParticipantId": null },
                        ],
                        "teamTwo": [{ "puuid": "d", "teamParticipantId": 2 }],
                    },
                },
            },
        }));
        let additional = Additional {
            team_participant_groups: vec![("a".to_string(), 1), ("e".to_string(), 2), ("f".to_string(), 3)],
            ..Default::default()
        };

        assert_eq!(
            get_team_participant_groups(&snapshot, &additional, None),
            vec![
                ("1".to_string(), list(&["a", "b"])),
                ("2".to_string(), list(&["d", "e"])),
                ("3".to_string(), list(&["f"])),
            ]
        );

        // without a session even a filled `additional` yields nothing
        assert_eq!(
            get_team_participant_groups(&LcSnapshot::default(), &additional, None),
            Vec::new()
        );
    }

    #[test]
    fn premade_indexes_number_groups_and_skip_cross_team_ones() {
        let teams = vec![
            ("TEAM-100".to_string(), list(&["a", "b", "c"])),
            ("TEAM-200".to_string(), list(&["d", "e"])),
        ];
        let groups = vec![("1".to_string(), list(&["a", "b"])), ("2".to_string(), list(&["d", "e"]))];
        let inferred = vec![list(&["a", "d"]), list(&["c"])];

        let map = get_merged_premade_team_map(&teams, &groups, &inferred);

        // the inferred `a,d` pair spans two teams and `c` is a single member, so
        // only the two real groups get numbered, in order
        assert_eq!(
            map,
            vec![("a".to_string(), 1), ("b".to_string(), 1), ("d".to_string(), 2), ("e".to_string(), 2)]
        );
    }

    #[test]
    fn subsets_are_removed_and_equal_groups_keep_their_first_index() {
        let teams = vec![list(&["a", "b"]), list(&["a", "b", "c"]), list(&["d", "e"]), list(&["d", "e"])];
        assert_eq!(
            remove_subsets(&teams, |team| team.clone()),
            vec![list(&["a", "b", "c"]), list(&["d", "e"])]
        );
    }

    #[test]
    fn every_loaded_puuid_of_every_team_in_team_order() {
        let teams = vec![
            ("TEAM-100".to_string(), list(&["a", "b"])),
            ("TEAM-200".to_string(), list(&["c"])),
        ];
        assert_eq!(team_puuids(&teams), list(&["a", "b", "c"]));
    }

    #[test]
    fn the_end_of_game_window_uses_the_top_level_phase() {
        let mut snapshot = LcSnapshot::default();
        snapshot.gameflow_phase = "EndOfGame".to_string();
        assert!(is_in_eog(&snapshot));

        snapshot.gameflow_phase = "InProgress".to_string();
        assert!(!is_in_eog(&snapshot));

        snapshot.gameflow_phase = "WaitingForStats".to_string();
        assert!(is_in_eog(&snapshot));
    }

    #[test]
    fn in_game_phases_are_the_six_the_original_lists() {
        for phase in [
            "GameStart",
            "InProgress",
            "WaitingForStats",
            "PreEndOfGame",
            "EndOfGame",
            "Reconnect",
        ] {
            let snapshot = LcSnapshot::from_json(&json!({
                "gameflow": { "phase": phase, "session": {
                    "phase": phase,
                    "gameData": { "queue": { "id": 1, "type": "CLASSIC", "gameMode": "CLASSIC" }, "gameId": 2 },
                } },
            }));
            assert!(
                matches!(
                    get_query_stage(&snapshot, &OngoingGameSettings::default(), None),
                    QueryStage::InGame { .. }
                ),
                "{phase} is an in-game phase"
            );
        }

        let snapshot = LcSnapshot::from_json(&json!({
            "gameflow": { "phase": "ChampSelect", "session": {
                "phase": "None",
                "gameData": { "queue": { "id": 1, "type": "CLASSIC", "gameMode": "CLASSIC" }, "gameId": 2 },
            } },
        }));
        assert_eq!(
            get_query_stage(&snapshot, &OngoingGameSettings::default(), None),
            QueryStage::Unavailable
        );
    }
}
