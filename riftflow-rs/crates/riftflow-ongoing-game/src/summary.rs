//! `data-adapter/match-history/identities.ts` plus the two accessors the loaders
//! need on a wrapped game summary.
//!
//! A record of `state.matchHistory[puuid].data` is always the wrapper
//! `{ source, gameId, data }`, never a bare game, so both LCU and SGP payloads are
//! read through the same two functions.

use serde_json::Value;

pub fn wrapper_game_id(wrapper: &Value) -> i64 {
    wrapper.get("gameId").and_then(Value::as_i64).unwrap_or_default()
}

pub fn wrapper_source(wrapper: &Value) -> &str {
    wrapper.get("source").and_then(Value::as_str).unwrap_or_default()
}

/// `toIdentities` — `(puuid, teamId)` of every participant.
///
/// The LCU payload splits identity and statistics in two arrays, so the team id
/// comes from `participants` keyed by `participantId`, and a participant with no
/// statistics entry reads as team `0`.
pub fn to_identities(wrapper: &Value) -> Vec<(String, i64)> {
    let data = wrapper.get("data").unwrap_or(&Value::Null);

    if wrapper_source(wrapper) == "sgp" {
        return data
            .pointer("/json/participants")
            .and_then(Value::as_array)
            .map(|participants| {
                participants
                    .iter()
                    .map(|participant| {
                        (
                            participant
                                .get("puuid")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            participant.get("teamId").and_then(Value::as_i64).unwrap_or_default(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
    }

    let mut team_of: Vec<(i64, i64)> = Vec::new();
    for participant in data.get("participants").and_then(Value::as_array).into_iter().flatten() {
        let participant_id = participant.get("participantId").and_then(Value::as_i64).unwrap_or_default();
        match team_of.iter_mut().find(|(existing, _)| *existing == participant_id) {
            Some((_, team)) => *team = participant.get("teamId").and_then(Value::as_i64).unwrap_or_default(),
            None => team_of.push((
                participant_id,
                participant.get("teamId").and_then(Value::as_i64).unwrap_or_default(),
            )),
        }
    }

    data.get("participantIdentities")
        .and_then(Value::as_array)
        .map(|identities| {
            identities
                .iter()
                .map(|identity| {
                    let participant_id = identity.get("participantId").and_then(Value::as_i64).unwrap_or_default();
                    (
                        identity
                            .pointer("/player/puuid")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        team_of
                            .iter()
                            .find(|(existing, _)| *existing == participant_id)
                            .map(|(_, team)| *team)
                            .unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `state.matchHistory` keyed by puuid: the game ids of each player's first
/// `count` games, in record order — the game-details whitelist.
pub fn whitelisted_game_ids(records: &[(String, Value)], count: i64) -> Vec<i64> {
    records
        .iter()
        .flat_map(|(_, record)| {
            record
                .get("data")
                .and_then(Value::as_array)
                .map(|games| {
                    games
                        .iter()
                        .map(wrapper_game_id)
                        .take(count.max(0) as usize)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sgp_identities_are_read_off_the_json_payload() {
        let wrapper = json!({
            "source": "sgp",
            "gameId": 1,
            "data": { "json": { "participants": [
                { "puuid": "a", "teamId": 100 },
                { "puuid": "b", "teamId": 200 },
            ] } },
        });

        assert_eq!(to_identities(&wrapper), vec![("a".to_string(), 100), ("b".to_string(), 200)]);
    }

    #[test]
    fn lcu_identities_join_the_two_arrays_by_participant_id() {
        let wrapper = json!({
            "source": "lcu",
            "gameId": 2,
            "data": {
                "participants": [
                    { "participantId": 1, "teamId": 100 },
                    { "participantId": 3, "teamId": 200 },
                ],
                "participantIdentities": [
                    { "participantId": 1, "player": { "puuid": "a" } },
                    { "participantId": 3, "player": { "puuid": "c" } },
                    { "participantId": 9, "player": { "puuid": "ghost" } },
                ],
            },
        });

        // an identity with no statistics row is team 0, not missing
        assert_eq!(
            to_identities(&wrapper),
            vec![("a".to_string(), 100), ("c".to_string(), 200), ("ghost".to_string(), 0)]
        );
    }

    #[test]
    fn the_whitelist_slices_each_player_history_at_the_details_count() {
        let record = |ids: &[i64]| json!({
            "source": "lcu",
            "params": {},
            "data": ids
                .iter()
                .map(|id| json!({ "source": "lcu", "gameId": id, "data": {} }))
                .collect::<Vec<_>>(),
        });

        let records = vec![
            ("a".to_string(), record(&[1, 2, 3])),
            ("b".to_string(), record(&[3, 4])),
        ];

        assert_eq!(whitelisted_game_ids(&records, 2), vec![1, 2, 3, 4]);
        assert_eq!(whitelisted_game_ids(&records, 0), Vec::<i64>::new());
        assert_eq!(whitelisted_game_ids(&records, -1), Vec::<i64>::new());
    }

    #[test]
    fn a_missing_participants_array_is_empty_rather_than_a_panic() {
        assert_eq!(to_identities(&json!({ "source": "lcu", "data": {} })), Vec::new());
        assert_eq!(to_identities(&json!({ "source": "sgp" })), Vec::new());
    }
}
