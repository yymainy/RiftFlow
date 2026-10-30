//! The 组队状况 preset, ported from `presets/premade.ts`.

use crate::options::{PremadePresetOptions, PresetTarget};
use crate::snapshot::{
    count_selected_champion_ids, player_display_name, target_teams, GameSnapshot, PresetPlayer,
    PresetTeam,
};
use crate::text::{self, Args, Locale, Punctuation};

/// One premade group inside a team, in the order the players appear in the team.
struct PremadeGroup<'a> {
    group_index: i64,
    players: Vec<&'a PresetPlayer>,
}

/// `buildPremadePresetLines`
pub fn build_premade_lines(
    snapshot: &GameSnapshot,
    target: PresetTarget,
    options: &PremadePresetOptions,
    selected_premade_indices: &[i64],
    locale: Locale,
) -> Vec<String> {
    let all_teams = snapshot.preset_teams(locale);
    let selected = selected_premade_indices.to_vec();

    let selected_team_groups: Vec<(&PresetTeam, Vec<PremadeGroup<'_>>)> = target_teams(
        target,
        &all_teams,
        locale,
    )
    .into_iter()
    .map(|team| {
        let groups = premade_groups_of_team(team)
            .into_iter()
            .filter(|group| selected.contains(&group.group_index))
            .collect();

        (team, groups)
    })
    .collect();

    let players: Vec<&PresetPlayer> = selected_team_groups
        .iter()
        .flat_map(|(_, groups)| groups.iter().flat_map(|group| group.players.iter().copied()))
        .collect();
    let selected_champion_id_counts = count_selected_champion_ids(&players);

    let line_separator = text::punctuation(locale, Punctuation::LineSeparator);
    let group_separator = text::punctuation(locale, Punctuation::GroupSeparator);

    selected_team_groups
        .into_iter()
        .map(|(team, groups)| {
            if groups.is_empty() {
                return premade_no_group_text(team, locale);
            }

            let group_parts: Vec<String> = groups
                .iter()
                .map(|group| {
                    let members: Vec<String> = group
                        .players
                        .iter()
                        .map(|player| {
                            player_display_name(
                                snapshot,
                                player,
                                options.name_display_strategy,
                                &selected_champion_id_counts,
                                locale,
                            )
                        })
                        .collect();

                    format!("[{}]", text::join_members(locale, &members))
                })
                .collect();

            format!(
                "{}{}{}",
                premade_team_title(team, locale),
                line_separator,
                group_parts.join(group_separator)
            )
        })
        .collect()
}

/// `premadeGroupsOfTeam`: groups of two or more, ordered by their index.
fn premade_groups_of_team(team: &PresetTeam) -> Vec<PremadeGroup<'_>> {
    let mut by_group: Vec<(i64, Vec<&PresetPlayer>)> = Vec::new();

    for player in &team.players {
        let Some(group_index) = player.premade_group else {
            continue;
        };

        match by_group.iter_mut().find(|(index, _)| *index == group_index) {
            Some((_, players)) => players.push(player),
            None => by_group.push((group_index, vec![player])),
        }
    }

    by_group.sort_by_key(|(group_index, _)| *group_index);
    by_group.retain(|(_, players)| players.len() >= 2);

    by_group
        .into_iter()
        .map(|(group_index, players)| PremadeGroup { group_index, players })
        .collect()
}

/// `premadeTeamTitle`
fn premade_team_title(team: &PresetTeam, locale: Locale) -> String {
    text::preset(
        locale,
        match team.team_identifier.as_str() {
            "TEAM-100" => "premade.blueTeamTitle",
            "TEAM-200" => "premade.redTeamTitle",
            _ => "premade.teamTitle",
        },
        Args::new(),
    )
}

/// `premadeNoGroupText`
fn premade_no_group_text(team: &PresetTeam, locale: Locale) -> String {
    text::preset(
        locale,
        match team.team_identifier.as_str() {
            "TEAM-100" => "premade.blueTeamEmpty",
            "TEAM-200" => "premade.redTeamEmpty",
            _ => "premade.empty",
        },
        Args::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::NameDisplayStrategy;
    use serde_json::json;

    fn snapshot() -> GameSnapshot {
        GameSnapshot::from_json(&json!({
            "selfPuuid": "p1",
            "teams": { "TEAM-100": ["p1", "p2", "p3", "p4"] },
            "summoner": {
                "p1": { "gameName": "RIP董事长", "tagLine": "81406" },
                "p2": { "gameName": "Lee Player", "tagLine": "002" },
                "p3": { "gameName": "Naut Player", "tagLine": "003" },
                "p4": { "gameName": "Jinx Player", "tagLine": "004" }
            },
            "championSelections": { "p1": 103, "p2": 64, "p3": 111, "p4": 222 },
            "mergedPremadeTeamMap": { "p1": 1, "p2": 1, "p3": 2, "p4": 2 },
            "gameData": { "champions": {
                "103": { "name": "无极剑圣" },
                "64": { "name": "盲僧" },
                "111": { "name": "深海泰坦" },
                "222": { "name": "暴走萝莉" }
            } }
        }))
    }

    fn indices(values: &[i64]) -> Vec<i64> {
        values.to_vec()
    }

    #[test]
    fn builds_lines_from_the_selected_premade_groups() {
        let lines = build_premade_lines(
            &snapshot(),
            PresetTarget::Friendly,
            &PremadePresetOptions::default(),
            &indices(&[1, 2]),
            Locale::ZhCn,
        );

        assert_eq!(lines, vec!["蓝方开黑：[无极剑圣, 盲僧] [深海泰坦, 暴走萝莉]".to_string()]);
    }

    #[test]
    fn builds_english_lines_with_player_names() {
        let mut snapshot = snapshot();
        snapshot.champion_names.clear();
        for (id, name) in [
            (103, "Master Yi"),
            (64, "Lee Sin"),
            (111, "Nautilus"),
            (222, "Jinx"),
        ] {
            snapshot.champion_names.insert(id, name.to_string());
        }

        let options = PremadePresetOptions {
            name_display_strategy: NameDisplayStrategy::PreferName,
            ..Default::default()
        };

        let lines = build_premade_lines(
            &snapshot,
            PresetTarget::Friendly,
            &options,
            &indices(&[1, 2]),
            Locale::En,
        );

        assert_eq!(
            lines,
            vec!["Blue premades: [RIP董事长#81406, Lee Player#002] [Naut Player#003, Jinx Player#004]".to_string()]
        );
    }

    #[test]
    fn a_duplicated_champion_falls_back_to_the_player_name() {
        let mut snapshot = snapshot();
        snapshot.champion_selections.insert("p2".to_string(), 103);

        let lines = build_premade_lines(
            &snapshot,
            PresetTarget::Friendly,
            &PremadePresetOptions::default(),
            &indices(&[1, 2]),
            Locale::ZhCn,
        );

        assert_eq!(
            lines,
            vec!["蓝方开黑：[RIP董事长#81406, Lee Player#002] [深海泰坦, 暴走萝莉]".to_string()]
        );
    }

    #[test]
    fn every_team_is_reported_even_without_a_selected_group() {
        let mut snapshot = snapshot();
        snapshot.teams = vec![
            ("TEAM-100".to_string(), vec!["p1".to_string(), "p2".to_string()]),
            ("TEAM-200".to_string(), vec!["p3".to_string(), "p4".to_string()]),
        ];
        snapshot.merged_premade_team_map = [("p1", 1), ("p2", 1)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();

        let lines = build_premade_lines(
            &snapshot,
            PresetTarget::All,
            &PremadePresetOptions::default(),
            &indices(&[1, 2]),
            Locale::ZhCn,
        );

        assert_eq!(
            lines,
            vec!["蓝方开黑：[无极剑圣, 盲僧]".to_string(), "红方无开黑".to_string()]
        );
    }

    #[test]
    fn a_solo_player_is_not_a_premade_group() {
        let mut snapshot = snapshot();
        snapshot
            .merged_premade_team_map
            .insert("p4".to_string(), 3);

        let lines = build_premade_lines(
            &snapshot,
            PresetTarget::Friendly,
            &PremadePresetOptions::default(),
            &indices(&[1, 2, 3]),
            Locale::ZhCn,
        );

        assert_eq!(lines, vec!["蓝方开黑：[无极剑圣, 盲僧]".to_string()]);
    }

    #[test]
    fn cherry_teams_get_the_neutral_title() {
        let mut snapshot = snapshot();
        snapshot.self_puuid = None;
        snapshot.teams = vec![(
            "CHERRY-1".to_string(),
            vec!["p1".to_string(), "p2".to_string()],
        )];

        let lines = build_premade_lines(
            &snapshot,
            PresetTarget::All,
            &PremadePresetOptions::default(),
            &indices(&[1]),
            Locale::ZhCn,
        );

        assert_eq!(lines, vec!["开黑：[无极剑圣, 盲僧]".to_string()]);

        snapshot.merged_premade_team_map.clear();
        let lines = build_premade_lines(
            &snapshot,
            PresetTarget::All,
            &PremadePresetOptions::default(),
            &indices(&[1]),
            Locale::En,
        );

        assert_eq!(lines, vec!["No premade groups".to_string()]);
    }
}
