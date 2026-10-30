//! `analysis-controller.ts` — the two derived props the panel reads.
//!
//! The premade-team inference is ported in full (it is pure arithmetic over the
//! loaded summaries). The *statistical* half is not: `analyzeGames` and
//! `analyzePlayers` live in `shared/data-adapter/analysis/**`, roughly two thousand
//! lines of per-game metric maths that belong to their own port. This module builds
//! exactly the inputs they took and hands them to an [`Analyzer`]; with none wired
//! up the `analysis` prop stays `null`, which is what it was before this crate.

use serde_json::{Value, json};

use crate::record::Pairs;
use crate::summary::to_identities;
use crate::team_up::{calculate_together_times, merge_overlapping_sets, remove_subsets};

/// the reaction's dependency list: `summaryKeys`, `teamKeys`, `detailsKeys`. Each is
/// the `.toSorted()` of a `map` over `Object.entries`, so a new game in the middle of
/// one player's history changes exactly one string.
pub fn analysis_keys(
    records: &[(String, Vec<Value>)],
    teams: &[(String, Vec<String>)],
    details: &[(String, Value)],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut summary_keys: Vec<String> = records
        .iter()
        .map(|(puuid, games)| {
            let games = games
                .iter()
                .map(|game| {
                    format!(
                        "{}:{}",
                        game.get("source").and_then(Value::as_str).unwrap_or_default(),
                        game.get("gameId").and_then(Value::as_i64).unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{puuid}:{games}")
        })
        .collect();
    summary_keys.sort();

    let mut team_keys: Vec<String> = teams
        .iter()
        .map(|(identifier, puuids)| format!("{identifier}:{}", puuids.join(",")))
        .collect();
    team_keys.sort();

    let mut details_keys: Vec<String> = details
        .iter()
        .map(|(_, detail)| {
            format!(
                "{}:{}",
                detail.get("source").and_then(Value::as_str).unwrap_or_default(),
                detail.get("gameId").and_then(Value::as_i64).unwrap_or_default()
            )
        })
        .collect();
    details_keys.sort();

    (summary_keys, team_keys, details_keys)
}

/// the second reaction's dependency: the *distinct* game ids of every history, sorted.
pub fn distinct_game_ids(records: &[(String, Vec<Value>)]) -> Vec<i64> {
    // `new Set(...)` keeps first-seen order, then `.toArray().toSorted()` sorts
    let mut ids: Vec<i64> = Vec::new();
    for (_, games) in records {
        for game in games {
            let Some(game_id) = game.get("gameId").and_then(Value::as_i64) else { continue };
            if !ids.contains(&game_id) {
                ids.push(game_id);
            }
        }
    }
    ids.sort();
    ids
}

/// `matchesByTeams`: one entry per team that had more than one participant in a game.
pub fn premade_game_groups(games: &[Value]) -> Vec<(Vec<String>, String)> {
    let mut groups: Vec<(Vec<String>, String)> = Vec::new();

    for game in games {
        let game_id = game.get("gameId").and_then(Value::as_i64).unwrap_or_default().to_string();
        let mut by_team: Pairs<Vec<String>> = Vec::new();

        for (puuid, team_id) in to_identities(game) {
            match by_team.iter_mut().find(|(existing, _)| *existing == team_id.to_string()) {
                Some((_, members)) => members.push(puuid),
                None => by_team.push((team_id.to_string(), vec![puuid])),
            }
        }

        // `Object.values` order, which is the insertion order of the reduce
        groups.extend(
            by_team
                .into_iter()
                .filter(|(_, members)| members.len() > 1)
                .map(|(_, members)| (members, game_id.clone())),
        );
    }

    groups
}

/// `deduplicatedMap`: the last entry for a game id wins, at the first entry's position.
pub fn dedupe_by_game_id(groups: Vec<(Vec<String>, String)>) -> Vec<(Vec<String>, String)> {
    let mut deduped: Pairs<Vec<String>> = Vec::new();

    for (players, id) in groups {
        match deduped.iter_mut().find(|(existing, _)| *existing == id) {
            Some((_, existing_players)) => *existing_players = players,
            None => deduped.push((id, players)),
        }
    }

    deduped.into_iter().map(|(id, players)| (players, id)).collect()
}

/// `_inferPremadeTeams` without the MobX layer: the groups of `watched` puuids that
/// appeared together in at least `threshold` games.
pub fn infer_premade_teams(
    records: &[(String, Vec<Value>)],
    watched: &[String],
    threshold: i64,
) -> Vec<Vec<String>> {
    if records.is_empty() {
        return Vec::new();
    }

    let matches = records
        .iter()
        .flat_map(|(_, games)| premade_game_groups(games))
        .collect::<Vec<_>>();

    let deduplicated = dedupe_by_game_id(matches);
    let calculated = calculate_together_times(&deduplicated, watched, threshold);

    // the getter of `removeSubsets(calculated, t => t.players)`
    let simplified = remove_subsets(&calculated, |entry| entry.0.clone());
    let players: Vec<Vec<String>> = simplified.iter().map(|entry| entry.0.clone()).collect();

    merge_overlapping_sets(&players)
}

/// `state.analysis`'s per-player input: `{ gameId, summary, details }` per game of
/// that player's history, in history order, with `undefined` details left out.
pub fn analysis_pairs(records: &[(String, Value)], details: &[(String, Value)]) -> Pairs<Vec<Value>> {
    records
        .iter()
        .filter(|(_, record)| record.is_object())
        .map(|(puuid, record)| {
            let pairs = record
                .get("data")
                .and_then(Value::as_array)
                .map(|games| {
                    games
                        .iter()
                        .map(|game| {
                            let game_id = game.get("gameId").and_then(Value::as_i64).unwrap_or_default();
                            let detail = details
                                .iter()
                                .find(|(key, _)| *key == game_id.to_string())
                                .map(|(_, detail)| detail.clone());

                            json!({
                                "gameId": game_id,
                                "summary": game,
                                "details": detail.unwrap_or(Value::Null),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            (puuid.clone(), pairs)
        })
        .collect()
}

/// the seam `analyzeGames` / `analyzePlayers` would be implemented behind.
pub trait Analyzer: Send + Sync {
    /// `analyzeGames(pairs, puuid, { previous })`
    fn analyze_player(&self, puuid: &str, pairs: &[Value], previous: Option<&Value>) -> Option<Value>;

    /// `analyzePlayers(teamPlayerAnalyses)`
    fn analyze_team(&self, player_analyses: &[Value]) -> Option<Value>;
}

/// `_computeAnalysis`: `state.analysis` off the loaded records, or `None`.
///
/// `!state.teams` is truthy-checking in JS only for `null`/`undefined`, and the
/// getter always returns an object, so the original never bails there — an empty
/// team list yields an analysis with no teams but every player still computed.
pub fn compute_analysis(
    records: &[(String, Value)],
    details: &[(String, Value)],
    teams: &[(String, Vec<String>)],
    previous: Option<&Value>,
    analyzer: Option<&dyn Analyzer>,
) -> Option<Value> {
    let analyzer = analyzer?;
    let mut players: Pairs<Value> = Vec::new();

    for (puuid, pairs) in analysis_pairs(records, details) {
        let prior = previous
            .and_then(|analysis| analysis.pointer(&format!("/players/{puuid}")))
            .cloned();

        if let Some(analysis) = analyzer.analyze_player(&puuid, &pairs, prior.as_ref()) {
            players.push((puuid, analysis));
        }
    }

    let mut team_analyses: Pairs<Value> = Vec::new();
    for (identifier, puuids) in teams {
        let member_analyses: Vec<Value> = puuids
            .iter()
            .filter_map(|puuid| {
                players
                    .iter()
                    .find(|(existing, _)| existing == puuid)
                    .map(|(_, analysis)| analysis.clone())
            })
            .collect();

        if let Some(analysis) = analyzer.analyze_team(&member_analyses) {
            team_analyses.push((identifier.clone(), analysis));
        }
    }

    Some(json!({
        "players": crate::record::object(&players),
        "teams": crate::record::object(&team_analyses),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::ApiSource;
    use crate::store::wrap_game;
    use serde_json::json;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn lcu_game(game_id: i64, roster: &[(&str, i64)]) -> Value {
        wrap_game(
            ApiSource::Lcu,
            game_id,
            json!({
                "gameId": game_id,
                "participants": roster
                    .iter()
                    .enumerate()
                    .map(|(index, (_, team))| json!({ "participantId": index + 1, "teamId": team }))
                    .collect::<Vec<_>>(),
                "participantIdentities": roster
                    .iter()
                    .enumerate()
                    .map(|(index, (puuid, _))| json!({ "participantId": index + 1, "player": { "puuid": puuid } }))
                    .collect::<Vec<_>>(),
            }),
        )
    }

    #[test]
    fn a_group_is_only_built_from_teams_with_more_than_one_member() {
        let game = lcu_game(1, &[("a", 100), ("b", 100), ("c", 200)]);
        assert_eq!(
            premade_game_groups(&[game]),
            vec![(list(&["a", "b"]), "1".to_string())]
        );
    }

    #[test]
    fn the_last_history_of_a_game_id_wins_but_keeps_its_slot() {
        let groups = vec![
            (list(&["a", "b"]), "1".to_string()),
            (list(&["c", "d"]), "2".to_string()),
            (list(&["a", "b", "e"]), "1".to_string()),
        ];

        assert_eq!(
            dedupe_by_game_id(groups),
            vec![(list(&["a", "b", "e"]), "1".to_string()), (list(&["c", "d"]), "2".to_string())]
        );
    }

    #[test]
    fn three_games_together_cross_the_default_threshold() {
        let games = (1..=3)
            .map(|index| lcu_game(index, &[("a", 100), ("b", 100), ("c", 200), ("d", 200)]))
            .collect::<Vec<_>>();

        let records = vec![("a".to_string(), games.clone()), ("b".to_string(), games.clone())];
        let watched = list(&["a", "b", "c", "d"]);

        // `new Map(matchesByTeams.map((m) => [m.id, m]))` keys by game id alone, so
        // of the two premade groups in one game only the last one read survives
        let inferred = infer_premade_teams(&records, &watched, 3);
        assert_eq!(inferred, vec![list(&["c", "d"])]);
    }

    #[test]
    fn a_game_with_one_premade_group_infers_it_for_every_player() {
        let games = (1..=3)
            .map(|index| lcu_game(index, &[("a", 100), ("b", 100), ("c", 100), ("d", 200)]))
            .collect::<Vec<_>>();

        let records = vec![("d".to_string(), games)];
        let inferred = infer_premade_teams(&records, &list(&["a", "b", "c", "d"]), 3);
        assert_eq!(inferred, vec![list(&["a", "b", "c"])]);
    }

    #[test]
    fn a_history_shorter_than_the_threshold_infers_nothing() {
        let games = vec![lcu_game(1, &[("a", 100), ("b", 100)])];
        let records = vec![("a".to_string(), games)];

        assert!(infer_premade_teams(&records, &list(&["a", "b"]), 2).is_empty());
        // and with no history at all the answer is empty rather than "everyone"
        assert!(infer_premade_teams(&[], &list(&["a", "b"]), 1).is_empty());
    }

    #[test]
    fn the_same_game_from_two_histories_is_counted_once() {
        // both players' histories name game 1, so the pair only ever played once
        let games = (1..=3)
            .map(|index| lcu_game(index, &[("a", 100), ("b", 100)]))
            .collect::<Vec<_>>();
        let records = vec![("a".to_string(), games.clone()), ("b".to_string(), games)];

        let inferred = infer_premade_teams(&records, &list(&["a", "b"]), 3);
        assert_eq!(inferred, vec![list(&["a", "b"])]);

        let records = vec![("a".to_string(), vec![lcu_game(1, &[("a", 100), ("b", 100)])])];
        assert!(infer_premade_teams(&records, &list(&["a", "b"]), 3).is_empty());
    }

    #[test]
    fn groups_sharing_a_player_merge_into_one_premade() {
        let first = lcu_game(1, &[("a", 100), ("b", 100)]);
        let second = lcu_game(2, &[("b", 100), ("c", 100)]);
        let records = vec![("a".to_string(), vec![first.clone()]), ("c".to_string(), vec![second.clone()])];

        let groups = dedupe_by_game_id(premade_game_groups(&[first, second]));
        let calculated = calculate_together_times(&groups, &list(&["a", "b", "c"]), 1);
        assert_eq!(merge_overlapping_sets(&calculated.iter().map(|t| t.0.clone()).collect::<Vec<_>>()),
                   vec![list(&["a", "b", "c"])]);

        let inferred = infer_premade_teams(&records, &list(&["a", "b", "c"]), 1);
        // the two groups share `b`, so `mergeOverlappingSets` leaves one of them
        assert_eq!(inferred, vec![list(&["a", "b", "c"])]);
    }

    #[test]
    fn the_keys_describe_the_data_and_not_its_order() {
        let records = vec![
            ("a".to_string(), vec![wrap_game(ApiSource::Lcu, 1, json!({}))]),
            ("b".to_string(), vec![wrap_game(ApiSource::Sgp, 2, json!({}))]),
        ];
        let teams = vec![("TEAM-100".to_string(), list(&["a", "b"]))];
        let details = vec![("1".to_string(), wrap_game(ApiSource::Lcu, 1, json!({})))];

        let (summary_keys, team_keys, details_keys) = analysis_keys(&records, &teams, &details);
        assert_eq!(summary_keys, vec!["a:lcu:1".to_string(), "b:sgp:2".to_string()]);
        assert_eq!(team_keys, vec!["TEAM-100:a,b".to_string()]);
        assert_eq!(details_keys, vec!["lcu:1".to_string()]);

        assert_eq!(distinct_game_ids(&records), vec![1, 2]);
    }

    #[test]
    fn pairs_pair_each_summary_with_its_timeline() {
        let records = vec![(
            "a".to_string(),
            json!({ "data": [wrap_game(ApiSource::Lcu, 1, json!({})), wrap_game(ApiSource::Lcu, 2, json!({}))] }),
        )];
        let details = vec![("2".to_string(), wrap_game(ApiSource::Lcu, 2, json!({ "timeline": true })) )];

        let pairs = analysis_pairs(&records, &details);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].1[0]["details"], Value::Null, "game 1 has no timeline yet");
        assert_eq!(pairs[0].1[1]["details"]["data"], json!({ "timeline": true }));

        // a record that is not an object is skipped, like the `!matchHistory` guard
        assert!(analysis_pairs(&[("ghost".to_string(), Value::Null)], &details).is_empty());
    }

    #[test]
    fn without_an_analyzer_the_prop_stays_empty() {
        let records = vec![(
            "a".to_string(),
            json!({ "source": "lcu", "params": {}, "data": [] }),
        )];

        assert_eq!(compute_analysis(&records, &[], &[], None, None), None);
    }

    #[test]
    fn an_analyzer_sees_every_player_and_each_team() {
        struct Echo;
        impl Analyzer for Echo {
            fn analyze_player(&self, puuid: &str, pairs: &[Value], previous: Option<&Value>) -> Option<Value> {
                Some(json!({ "puuid": puuid, "games": pairs.len(), "previous": previous }))
            }

            fn analyze_team(&self, player_analyses: &[Value]) -> Option<Value> {
                Some(json!({ "members": player_analyses.len() }))
            }
        }

        let records = vec![(
            "a".to_string(),
            json!({ "source": "lcu", "params": {}, "data": [wrap_game(ApiSource::Lcu, 1, json!({}))] }),
        )];
        let teams = vec![("TEAM-100".to_string(), list(&["a"]))];
        let previous = json!({ "players": { "a": { "games": 0 } } });

        let analysis = compute_analysis(&records, &[], &teams, Some(&previous), Some(&Echo)).expect("analysis");

        assert_eq!(analysis["players"]["a"]["games"], json!(1));
        assert_eq!(analysis["players"]["a"]["previous"], json!({ "games": 0 }));
        assert_eq!(analysis["teams"]["TEAM-100"], json!({ "members": 1 }));
    }
}
