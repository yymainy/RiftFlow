//! `shared/utils/team-up-calc.ts` — the premade-team graph, its union-find and the
//! two set reductions the inference ends with.

use std::collections::{HashMap, HashSet};

/// `TeamUpGraph` edge: how often two puuids sat in one team, and in which games.
#[derive(Clone, Debug, Default)]
struct Edge {
    weight: i64,
    ids: Vec<String>,
}

#[derive(Clone, Debug)]
struct TogetherTime {
    players: Vec<String>,
    times: i64,
    /// `undefined` in JS whenever the pair was never seen; only reachable with a
    /// threshold of zero or less, which the original would have thrown on
    ids: Option<Vec<String>>,
}

#[derive(Default)]
pub struct TeamUpGraph {
    adjacency: HashMap<String, HashMap<String, Edge>>,
}

impl TeamUpGraph {
    fn add_vertex(&mut self, vertex: &str) {
        self.adjacency
            .entry(vertex.to_string())
            .or_default();
    }

    /// `addEdge` — the graph is undirected, so both directions carry the weight.
    fn add_edge(&mut self, from: &str, to: &str, id: &str) {
        self.add_vertex(from);
        self.add_vertex(to);

        for (a, b) in [(from, to), (to, from)] {
            let edge = self
                .adjacency
                .get_mut(a)
                .expect("vertex")
                .entry(b.to_string())
                .or_insert_with(|| Edge {
                    weight: 0,
                    ids: Vec::new(),
                });
            edge.weight += 1;
            if !edge.ids.iter().any(|existing| existing == id) {
                edge.ids.push(id.to_string());
            }
        }
    }

    /// `getTogetherTimes`: the weakest pair inside a group is how often the whole
    /// group sat together.
    fn get_together_times(&self, groups: &[Vec<String>]) -> Vec<TogetherTime> {
        let mut results = Vec::new();

        for players in groups {
            let mut count = i64::MAX;
            let mut ids: Option<Vec<String>> = None;

            for i in 0..players.len() {
                for j in (i + 1)..players.len() {
                    let Some(adjacency) = self.adjacency.get(&players[i]) else {
                        count = 0;
                        break;
                    };

                    let Some(edge) = adjacency.get(&players[j]) else {
                        count = 0;
                        break;
                    };

                    if edge.weight < count {
                        ids = Some(edge.ids.clone());
                        count = edge.weight;
                    }
                }

                if count == 0 {
                    break;
                }
            }

            results.push(TogetherTime {
                players: players.clone(),
                times: count,
                ids,
            });
        }

        results
    }
}

/// `combinations` — including the empty subset, in the original's DFS order.
pub fn combinations(nums: &[String]) -> Vec<Vec<String>> {
    let mut res: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    backtrack(nums, &mut current, 0, &mut res);
    res
}

fn backtrack(nums: &[String], current: &mut Vec<String>, start: usize, res: &mut Vec<Vec<String>>) {
    res.push(current.clone());

    for i in start..nums.len() {
        current.push(nums[i].clone());
        backtrack(nums, current, i + 1, res);
        current.pop();
    }
}

/// `calculateTogetherTimes` — every group of `players` that appeared together in
/// at least `threshold` of `matches`.
pub fn calculate_together_times(
    matches: &[(Vec<String>, String)],
    players: &[String],
    threshold: i64,
) -> Vec<(Vec<String>, i64, Vec<String>)> {
    let mut graph = TeamUpGraph::default();
    let set: HashSet<&String> = players.iter().collect();

    for (members, id) in matches {
        let filtered: Vec<String> = members
            .iter()
            .filter(|member| set.contains(member))
            .cloned()
            .collect();

        for i in 0..filtered.len().saturating_sub(1) {
            for j in (i + 1)..filtered.len() {
                graph.add_edge(&filtered[i], &filtered[j], id);
            }
        }
    }

    let groups: Vec<Vec<String>> = combinations(players)
        .into_iter()
        .filter(|group| group.len() > 1)
        .collect();

    let result: Vec<TogetherTime> = graph
        .get_together_times(&groups)
        .into_iter()
        .filter(|time| time.times >= threshold)
        .collect();

    let mut map: Vec<(String, TogetherTime)> = Vec::new();

    for time in result {
        // `toSetKey` sorts, so two groups seen in the same games collapse together
        let key = match &time.ids {
            Some(ids) => {
                let mut sorted = ids.clone();
                sorted.sort();
                sorted.join("|")
            }
            None => String::new(),
        };

        match map.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, item)) => {
                if time.players.len() > item.players.len()
                    && is_superset(&time.players, &item.players)
                {
                    item.players = time.players.clone();
                }
            }
            None => map.push((key, time)),
        }
    }

    map.into_iter()
        .map(|(_, time)| (time.players, time.times, time.ids.unwrap_or_default()))
        .collect()
}

fn is_superset(set: &[String], subset: &[String]) -> bool {
    if subset.len() > set.len() {
        return false;
    }

    let unique: HashSet<&String> = set.iter().collect();
    subset.iter().all(|element| unique.contains(element))
}

/// `removeSubsets` — drop anything contained in another entry; an exact duplicate
/// is dropped in favour of the earlier index.
pub fn remove_subsets<T, F: Fn(&T) -> Vec<String>>(objects: &[T], getter: F) -> Vec<T>
where
    T: Clone,
{
    let sets: Vec<HashSet<String>> = objects.iter().map(|object| getter(object).into_iter().collect()).collect();

    objects
        .iter()
        .cloned()
        .enumerate()
        .filter(|(index, _object)| {
            let current = &sets[*index];

            !sets.iter().enumerate().any(|(other_index, other)| {
                if other_index == *index || other.len() < current.len() {
                    return false;
                }

                if other.len() == current.len() {
                    return other_index < *index && current.iter().all(|element| other.contains(element));
                }

                current.iter().all(|element| other.contains(element))
            })
        })
        .map(|(_, object)| object)
        .collect()
}

/// `mergeOverlappingSets` — groups sharing any member become one group.
pub fn merge_overlapping_sets(keys: &[Vec<String>]) -> Vec<Vec<String>> {
    let mut elements: Vec<String> = Vec::new();
    for key in keys {
        for element in key {
            if !elements.iter().any(|existing| existing == element) {
                elements.push(element.clone());
            }
        }
    }

    let mut parent: HashMap<String, String> = elements
        .iter()
        .map(|element| (element.clone(), element.clone()))
        .collect();
    let mut rank: HashMap<String, usize> = elements.iter().map(|element| (element.clone(), 0)).collect();

    for key in keys {
        for i in 0..key.len().saturating_sub(1) {
            for j in (i + 1)..key.len() {
                union(&mut parent, &mut rank, &key[i], &key[j]);
            }
        }
    }

    // keyed by root, in the order the sets first appeared
    let mut union_map: Vec<(String, Vec<String>)> = Vec::new();
    for key in keys {
        let root = find(&parent, key.first().map(String::as_str).unwrap_or_default());
        match union_map.iter_mut().find(|(existing, _)| *existing == root) {
            Some((_, members)) => {
                for element in key {
                    if !members.iter().any(|existing| existing == element) {
                        members.push(element.clone());
                    }
                }
            }
            None => {
                let mut members: Vec<String> = Vec::new();
                for element in key {
                    if !members.iter().any(|existing| existing == element) {
                        members.push(element.clone());
                    }
                }
                union_map.push((root, members));
            }
        }
    }

    union_map.into_iter().map(|(_, members)| members).collect()
}

fn find(parent: &HashMap<String, String>, element: &str) -> String {
    let mut current = parent.get(element).cloned().unwrap_or_else(|| element.to_string());
    if current == element {
        return current;
    }

    // path compression, but `find` only reads, so walk it without rewriting
    loop {
        let next = parent.get(&current).cloned().unwrap_or_else(|| current.clone());
        if next == current {
            return current;
        }
        current = next;
    }
}

fn union(
    parent: &mut HashMap<String, String>,
    rank: &mut HashMap<String, usize>,
    x: &str,
    y: &str,
) {
    let root_x = find(parent, x);
    let root_y = find(parent, y);

    if root_x == root_y {
        return;
    }

    let rank_x = *rank.get(&root_x).unwrap_or(&0);
    let rank_y = *rank.get(&root_y).unwrap_or(&0);

    if rank_x > rank_y {
        parent.insert(root_y, root_x.clone());
    } else if rank_x < rank_y {
        parent.insert(root_x, root_y.clone());
    } else {
        parent.insert(root_y, root_x.clone());
        rank.insert(root_x, rank_x + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn combinations_lists_every_subset_in_the_depth_first_order() {
        let all = combinations(&list(&["a", "b", "c"]));

        // the empty subset is part of the result, and `[a, c]` comes before `[b]`
        assert_eq!(
            all,
            vec![
                vec![],
                list(&["a"]),
                list(&["a", "b"]),
                list(&["a", "b", "c"]),
                list(&["a", "c"]),
                list(&["b"]),
                list(&["b", "c"]),
                list(&["c"]),
            ]
        );
    }

    #[test]
    fn a_group_together_times_its_weakest_pair() {
        let matches = vec![
            (list(&["a", "b"]), "1".to_string()),
            (list(&["a", "b"]), "2".to_string()),
            (list(&["a", "c"]), "3".to_string()),
        ];
        let players = list(&["a", "b", "c"]);

        let times = calculate_together_times(&matches, &players, 1);
        let pairs: Vec<(Vec<String>, i64)> = times
            .iter()
            .map(|(group, times, _)| (group.clone(), *times))
            .collect();

        // `[a, b, c]` is only together as often as a-c is, i.e. once, and it is
        // dropped again because its id set is the same as `[a, c]`'s
        assert_eq!(
            pairs,
            vec![(list(&["a", "b"]), 2), (list(&["a", "c"]), 1)]
        );
    }

    #[test]
    fn players_outside_the_watched_list_never_enter_the_graph() {
        let matches = vec![(list(&["a", "b", "stranger"]), "1".to_string())];
        let times = calculate_together_times(&matches, &list(&["a", "b"]), 1);

        assert_eq!(times[0].0, list(&["a", "b"]));
        assert_eq!(times[0].2, vec!["1".to_string()]);
    }

    #[test]
    fn a_player_that_never_played_a_grouped_game_produces_no_group() {
        let matches = vec![(list(&["a", "b"]), "1".to_string())];
        let players = list(&["a", "b", "lonely"]);

        // `lonely` has no edge at all, so every group containing it reads as zero
        let times = calculate_together_times(&matches, &players, 1);
        assert_eq!(
            times.iter().map(|t| t.0.clone()).collect::<Vec<_>>(),
            vec![list(&["a", "b"])]
        );
    }

    #[test]
    fn the_threshold_drops_the_infrequent_groups() {
        let matches = vec![
            (list(&["a", "b"]), "1".to_string()),
            (list(&["a", "b"]), "2".to_string()),
            (list(&["a", "b"]), "3".to_string()),
            (list(&["c", "d"]), "4".to_string()),
        ];
        let times = calculate_together_times(&matches, &list(&["a", "b", "c", "d"]), 3);

        assert_eq!(times.len(), 1);
        assert_eq!(times[0].0, list(&["a", "b"]));
    }

    #[test]
    fn a_three_man_group_survives_when_all_three_pairs_agree() {
        let matches = vec![
            (list(&["a", "b", "c"]), "1".to_string()),
            (list(&["a", "b", "c"]), "2".to_string()),
        ];
        let times = calculate_together_times(&matches, &list(&["a", "b", "c"]), 2);

        let groups: Vec<Vec<String>> = times.iter().map(|t| t.0.clone()).collect();
        assert_eq!(groups, vec![list(&["a", "b", "c"])]);
    }

    #[test]
    fn subsets_are_removed_and_duplicates_yield_to_the_first() {
        let teams = vec![
            list(&["a", "b"]),
            list(&["a", "b", "c"]),
            list(&["d", "e"]),
            list(&["d", "e"]),
            list(&["c"]),
        ];

        assert_eq!(
            remove_subsets(&teams, |team| team.clone()),
            vec![list(&["a", "b", "c"]), list(&["d", "e"])]
        );
    }

    #[test]
    fn overlapping_sets_merge_into_one_and_disjoint_sets_stay_apart() {
        let merged = merge_overlapping_sets(&[
            list(&["a", "b"]),
            list(&["b", "c"]),
            list(&["d", "e"]),
            list(&["e"]),
        ]);

        // `[e]` shares its root with `[d, e]`, so it folds in instead of repeating
        assert_eq!(merged, vec![list(&["a", "b", "c"]), list(&["d", "e"])]);
    }

    #[test]
    fn an_empty_input_produces_an_empty_result() {
        assert!(remove_subsets(&Vec::<Vec<String>>::new(), |team| team.clone()).is_empty());
        assert!(merge_overlapping_sets(&[]).is_empty());
        assert!(calculate_together_times(&[], &list(&["a", "b"]), 1).is_empty());
    }
}
