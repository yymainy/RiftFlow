//! `champ-select-members.ts` and `champ-select-handoff.ts`.
//!
//! The champ-select session hides opponent puuids behind `obfuscatedPuuid` unless
//! the `ongoing-game.deobfuscation` gate is on; `magic()` was the native addon that
//! turned one into the other, so it arrives here as [`Deobfuscate`] and a member it
//! cannot resolve is skipped rather than guessed at.

use serde_json::{Map, Value, json};

use crate::record::Pairs;

/// `@main/native` `magic`.
pub trait Deobfuscate: Send + Sync {
    fn magic(&self, obfuscated_puuid: &str) -> Option<String>;
}

/// The addon is not loaded (or the gate is off), so hidden members stay hidden.
pub struct NoDeobfuscation;

impl Deobfuscate for NoDeobfuscation {
    fn magic(&self, _obfuscated_puuid: &str) -> Option<String> {
        None
    }
}

pub const EMPTY_PUUID: &str = "00000000-0000-0000-0000-000000000000";

/// `VisibleChampSelectMember`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChampSelectMember {
    pub puuid: String,
    pub team_identifier: String,
    pub champion_id: i64,
    pub position: String,
    pub spell1_id: i64,
    pub spell2_id: i64,
}

/// `getVisibleChampSelectPuuid`.
pub fn visible_puuid(member: &Value, deobfuscation_enabled: bool, magic: &dyn Deobfuscate) -> Option<String> {
    let hidden = member.get("nameVisibilityType").and_then(Value::as_str) == Some("HIDDEN");
    let obfuscated = member
        .get("obfuscatedPuuid")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if hidden && !obfuscated.is_empty() && deobfuscation_enabled {
        return magic.magic(obfuscated);
    }

    let puuid = member.get("puuid").and_then(Value::as_str).unwrap_or_default();
    if puuid.is_empty() || puuid == EMPTY_PUUID {
        return None;
    }

    Some(puuid.to_string())
}

/// `collectVisibleChampSelectMembers`: `myTeam` first, then `theirTeam`.
pub fn collect_visible_members(
    session: &Value,
    deobfuscation_enabled: bool,
    magic: &dyn Deobfuscate,
) -> Vec<ChampSelectMember> {
    let mut members = Vec::new();

    for side in ["myTeam", "theirTeam"] {
        let list: Vec<Value> = session
            .get(side)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for member in list {
            let Some(puuid) = visible_puuid(&member, deobfuscation_enabled, magic) else {
                continue;
            };

            let team = member.get("team").and_then(Value::as_i64).unwrap_or_default();
            members.push(ChampSelectMember {
                champion_id: first_number(&member, &["championId", "championPickIntent"]),
                puuid,
                team_identifier: if team == 100 || team == 1 { "TEAM-100" } else { "TEAM-200" }.to_string(),
                position: member
                    .get("assignedPosition")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_uppercase(),
                spell1_id: member.get("spell1Id").and_then(Value::as_i64).unwrap_or_default(),
                spell2_id: member.get("spell2Id").and_then(Value::as_i64).unwrap_or_default(),
            });
        }
    }

    members
}

fn first_number(value: &Value, keys: &[&str]) -> i64 {
    keys.iter()
        .filter_map(|key| value.get(*key).and_then(Value::as_i64))
        .find(|number| *number != 0)
        .unwrap_or_default()
}

/// `ChampSelectHandoffSnapshot`: what champ-select knew, kept for the first
/// seconds of the game when the gameflow session has not filled in yet.
#[derive(Clone, Debug, PartialEq)]
pub struct HandoffSnapshot {
    pub game_id: i64,
    pub teams: Pairs<Vec<String>>,
    pub players: Pairs<HandoffPlayer>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HandoffPlayer {
    pub champion_id: i64,
    pub position: String,
    pub spell1_id: i64,
    pub spell2_id: i64,
}

impl HandoffSnapshot {
    /// `buildChampSelectHandoffSnapshot`.
    pub fn build(
        session: &Value,
        deobfuscation_enabled: bool,
        magic: &dyn Deobfuscate,
    ) -> Option<Self> {
        let game_id = session.get("gameId").and_then(Value::as_i64).unwrap_or_default();
        if !deobfuscation_enabled || game_id == 0 {
            return None;
        }

        let mut teams: Pairs<Vec<String>> = Vec::new();
        let mut players: Pairs<HandoffPlayer> = Vec::new();

        for member in collect_visible_members(session, deobfuscation_enabled, magic) {
            match teams.iter_mut().find(|(identifier, _)| *identifier == member.team_identifier) {
                Some((_, members)) => members.push(member.puuid.clone()),
                None => teams.push((member.team_identifier.clone(), vec![member.puuid.clone()])),
            }

            match players.iter_mut().find(|(puuid, _)| *puuid == member.puuid) {
                // the original overwrites per puuid; members are unique anyway
                Some((_, player)) => *player = handoff_player(&member),
                None => players.push((member.puuid.clone(), handoff_player(&member))),
            }
        }

        if players.is_empty() {
            return None;
        }

        Some(Self {
            game_id,
            teams,
            players,
        })
    }

    /// `isChampSelectHandoffSnapshot` — a stored snapshot the renderer or a newer
    /// build wrote has to keep its shape or it gets dropped.
    pub fn from_value(value: &Value) -> Option<Self> {
        let game_id = value.get("gameId").and_then(Value::as_i64).filter(|id| *id > 0)?;

        let teams = value.get("teams")?.as_object()?;
        let mut team_pairs = Pairs::new();
        for (identifier, members) in teams {
            team_pairs.push((
                identifier.clone(),
                members
                    .as_array()?
                    .iter()
                    .map(|member| member.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()?,
            ));
        }

        let players = value.get("players")?.as_object()?;
        if players.is_empty() {
            return None;
        }

        let mut player_pairs = Pairs::new();
        for (puuid, player) in players {
            player_pairs.push((puuid.clone(), HandoffPlayer {
                champion_id: player.get("championId").and_then(Value::as_i64)?,
                position: player.get("position").and_then(Value::as_str).map(str::to_string)?,
                spell1_id: player.get("spell1Id").and_then(Value::as_i64)?,
                spell2_id: player.get("spell2Id").and_then(Value::as_i64)?,
            }));
        }

        Some(Self {
            game_id,
            teams: team_pairs,
            players: player_pairs,
        })
    }

    pub fn to_value(&self) -> Value {
        let mut teams = Map::new();
        for (identifier, members) in &self.teams {
            teams.insert(identifier.clone(), json!(members));
        }

        let mut players = Map::new();
        for (puuid, player) in &self.players {
            players.insert(
                puuid.clone(),
                json!({
                    "championId": player.champion_id,
                    "position": player.position,
                    "spell1Id": player.spell1_id,
                    "spell2Id": player.spell2_id,
                }),
            );
        }

        json!({ "gameId": self.game_id, "teams": teams, "players": players })
    }
}

fn handoff_player(member: &ChampSelectMember) -> HandoffPlayer {
    HandoffPlayer {
        champion_id: member.champion_id,
        position: member.position.clone(),
        spell1_id: member.spell1_id,
        spell2_id: member.spell2_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct MagicMap(Vec<(&'static str, &'static str)>);

    impl Deobfuscate for MagicMap {
        fn magic(&self, obfuscated: &str) -> Option<String> {
            self.0
                .iter()
                .find(|(key, _)| *key == obfuscated)
                .map(|(_, value)| (*value).to_string())
        }
    }

    fn session(my_team: Vec<Value>, their_team: Vec<Value>, game_id: i64) -> Value {
        json!({ "gameId": game_id, "myTeam": my_team, "theirTeam": their_team })
    }

    fn member(puuid: &str, team: i64) -> Value {
        json!({
            "puuid": puuid,
            "team": team,
            "championId": 0,
            "championPickIntent": 0,
            "assignedPosition": "jungle",
            "spell1Id": 0,
            "spell2Id": 0,
            "nameVisibilityType": "VISIBLE",
            "obfuscatedPuuid": "",
        })
    }

    #[test]
    fn my_team_is_collected_before_their_team() {
        let members = collect_visible_members(
            &session(vec![member("me", 100)], vec![member("them", 200)], 7),
            true,
            &NoDeobfuscation,
        );

        assert_eq!(
            members.iter().map(|m| m.puuid.as_str()).collect::<Vec<_>>(),
            vec!["me", "them"]
        );
        assert_eq!(members[0].team_identifier, "TEAM-100");
        assert_eq!(members[1].team_identifier, "TEAM-200");
        assert_eq!(members[0].position, "JUNGLE");
    }

    #[test]
    fn team_one_maps_to_the_100_identifier() {
        let members = collect_visible_members(&session(vec![member("me", 1)], vec![], 7), true, &NoDeobfuscation);
        assert_eq!(members[0].team_identifier, "TEAM-100");

        let members = collect_visible_members(&session(vec![member("me", 2)], vec![], 7), true, &NoDeobfuscation);
        assert_eq!(members[0].team_identifier, "TEAM-200");
    }

    #[test]
    fn empty_and_placeholder_puuids_are_never_visible() {
        let session = session(
            vec![member("", 100), member(EMPTY_PUUID, 100)],
            vec![member("ok", 200)],
            7,
        );
        let members = collect_visible_members(&session, true, &NoDeobfuscation);

        assert_eq!(members.len(), 1);
        assert_eq!(members[0].puuid, "ok");
    }

    #[test]
    fn a_hidden_member_needs_both_the_gate_and_the_addon() {
        let hidden = json!({
            "puuid": "",
            "team": 100,
            "championId": 42,
            "assignedPosition": "top",
            "spell1Id": 4,
            "spell2Id": 14,
            "nameVisibilityType": "HIDDEN",
            "obfuscatedPuuid": "obf-1",
        });
        let magic = MagicMap(vec![("obf-1", "real-1")]);

        let enabled = collect_visible_members(&session(vec![hidden.clone()], vec![], 7), true, &magic);
        assert_eq!(enabled[0].puuid, "real-1");

        let disabled = collect_visible_members(&session(vec![hidden.clone()], vec![], 7), false, &magic);
        assert!(disabled.is_empty(), "the gate is off, so nothing is resolved");

        let no_addon = collect_visible_members(&session(vec![hidden], vec![], 7), true, &NoDeobfuscation);
        assert!(no_addon.is_empty(), "magic returned nothing");
    }

    #[test]
    fn a_hidden_member_the_addon_cannot_read_is_not_fallen_back_on() {
        // the original returns `magic(x) || null`, so an unresolvable obfuscated id
        // drops the member instead of surfacing the obfuscated string
        let hidden = json!({
            "puuid": "plain-1",
            "team": 100,
            "assignedPosition": "mid",
            "nameVisibilityType": "HIDDEN",
            "obfuscatedPuuid": "unknown",
        });

        assert!(collect_visible_members(&session(vec![hidden], vec![], 7), true, &NoDeobfuscation).is_empty());
    }

    #[test]
    fn the_champion_falls_back_to_the_pick_intent() {
        let picked = json!({
            "puuid": "me", "team": 100, "championId": 0, "championPickIntent": 55,
            "assignedPosition": "", "spell1Id": 0, "spell2Id": 0,
            "nameVisibilityType": "VISIBLE", "obfuscatedPuuid": "",
        });
        let members = collect_visible_members(&session(vec![picked], vec![], 7), true, &NoDeobfuscation);

        assert_eq!(members[0].champion_id, 55);
        assert_eq!(members[0].position, "");
    }

    #[test]
    fn a_snapshot_needs_a_game_id_and_at_least_one_player() {
        assert!(HandoffSnapshot::build(&session(vec![member("me", 100)], vec![], 0), true, &NoDeobfuscation).is_none());
        assert!(HandoffSnapshot::build(&session(vec![member("me", 100)], vec![], 7), false, &NoDeobfuscation).is_none());
        assert!(HandoffSnapshot::build(&session(vec![], vec![], 7), true, &NoDeobfuscation).is_none());

        let snapshot = HandoffSnapshot::build(
            &session(vec![member("me", 100)], vec![member("foe", 200)], 7),
            true,
            &NoDeobfuscation,
        )
        .expect("a two-team snapshot");

        assert_eq!(snapshot.game_id, 7);
        assert_eq!(
            snapshot.to_value(),
            json!({
                "gameId": 7,
                "teams": { "TEAM-100": ["me"], "TEAM-200": ["foe"] },
                "players": {
                    "me": { "championId": 0, "position": "JUNGLE", "spell1Id": 0, "spell2Id": 0 },
                    "foe": { "championId": 0, "position": "JUNGLE", "spell1Id": 0, "spell2Id": 0 },
                },
            })
        );
    }

    #[test]
    fn a_stored_snapshot_is_only_accepted_whole() {
        let good = json!({
            "gameId": 12,
            "teams": { "TEAM-100": ["me"] },
            "players": { "me": { "championId": 1, "position": "TOP", "spell1Id": 4, "spell2Id": 14 } },
        });

        assert_eq!(HandoffSnapshot::from_value(&good).unwrap().game_id, 12);
        assert_eq!(
            HandoffSnapshot::from_value(&good).unwrap().to_value(),
            good,
            "the round trip is byte-identical"
        );

        assert!(HandoffSnapshot::from_value(&json!({ "gameId": 0, "teams": {}, "players": {} })).is_none());
        assert!(HandoffSnapshot::from_value(&json!({
            "gameId": -3, "teams": { "TEAM-100": ["me"] },
            "players": { "me": { "championId": 1, "position": "TOP", "spell1Id": 4, "spell2Id": 14 } },
        }))
        .is_none());
        assert!(HandoffSnapshot::from_value(&json!({
            "gameId": 12, "teams": {},
            "players": { "me": { "championId": 1, "position": "TOP" } },
        }))
        .is_none(), "a player missing spell2Id is not a snapshot");
        assert!(HandoffSnapshot::from_value(&json!({ "gameId": 12 })).is_none());
    }
}
