//! `member-merge.ts` — how two participant lists of the same team are combined
//! without losing the order either side already established.

pub fn member_merge(origin: &[String], extra: &[String]) -> Vec<String> {
    let (n, m) = (origin.len(), extra.len());
    let origin_set: std::collections::HashSet<&String> = origin.iter().collect();
    let extra_set: std::collections::HashSet<&String> = extra.iter().collect();

    let overlap: std::collections::HashSet<&String> = origin_set
        .iter()
        .filter(|member| extra_set.contains(*member))
        .copied()
        .collect();

    let origin_overlap: Vec<&String> = origin.iter().filter(|x| overlap.contains(x)).collect();
    let extra_overlap: Vec<&String> = extra.iter().filter(|x| overlap.contains(x)).collect();

    let order_is_correct = origin_overlap.len() == extra_overlap.len()
        && origin_overlap
            .iter()
            .zip(extra_overlap.iter())
            .all(|(a, b)| a == b);

    // 错位检查，错位直接尾接
    if !order_is_correct {
        let mut result = origin.to_vec();
        result.extend(
            extra
                .iter()
                .filter(|x| !origin_set.contains(x))
                .cloned()
                .collect::<Vec<_>>(),
        );
        return result;
    }

    let mut dp = vec![vec![0i32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if origin[i] == extra[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if origin[i] == extra[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    // 目前局部的合并顺序是先 origin 后 extra，而不是依次拼接
    // e.g. A,B,T + C,D,T -> A,B,C,D,T
    let mut result: Vec<String> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);

    for (ia, jb) in pairs {
        while i < ia {
            result.push(origin[i].clone());
            i += 1;
        }
        while j < jb {
            result.push(extra[j].clone());
            j += 1;
        }
        result.push(origin[ia].clone());
        i = ia + 1;
        j = jb + 1;
    }

    while i < n {
        result.push(origin[i].clone());
        i += 1;
    }
    while j < m {
        result.push(extra[j].clone());
        j += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn a_displaced_overlap_is_appended_instead_of_interleaved() {
        // no shared member keeps the order question out of it entirely
        assert_eq!(
            member_merge(&list(&["a", "b"]), &list(&["c", "d"])),
            list(&["a", "b", "c", "d"])
        );

        // the overlapping members appear in different orders, so `extra` is
        // appended minus what `origin` already had
        assert_eq!(
            member_merge(&list(&["a", "t"]), &list(&["t", "b"])),
            list(&["a", "t", "b"])
        );
        assert_eq!(
            member_merge(&list(&["a", "b", "t"]), &list(&["t", "c", "d"])),
            list(&["a", "b", "t", "c", "d"])
        );
    }

    #[test]
    fn an_aligned_overlap_lets_the_lcs_insert_the_new_members() {
        assert_eq!(
            member_merge(&list(&["a", "b", "t"]), &list(&["c", "d", "t"])),
            list(&["a", "b", "c", "d", "t"])
        );
        assert_eq!(
            member_merge(&list(&["t1", "x", "t2"]), &list(&["t1", "t2"])),
            list(&["t1", "x", "t2"])
        );
    }

    #[test]
    fn duplicates_are_not_dropped_by_the_append_path() {
        // the misaligned branch only filters `extra` against `origin`, so a member
        // listed twice in `extra` still lands twice
        assert_eq!(
            member_merge(&list(&["b"]), &list(&["a", "a"])),
            list(&["b", "a", "a"])
        );
    }

    #[test]
    fn empty_sides_shortcut() {
        assert_eq!(member_merge(&[], &list(&["a"])), list(&["a"]));
        assert_eq!(member_merge(&list(&["a"]), &[]), list(&["a"]));
        assert_eq!(member_merge(&[], &[]), Vec::<String>::new());
    }
}
