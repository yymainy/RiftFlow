//! `shared/shards/ongoing-game/reload.ts` — `resolveOngoingGamePlayerReloadScopes`.

use serde_json::Value;

pub const RELOAD_SCOPES: [&str; 5] = ["matchHistory", "summoner", "rankedStats", "savedInfo", "championMastery"];

/// `includes` and `excludes` may not be combined; neither given means everything.
///
/// A `throw` here is the original's behaviour too, and the IPC layer turns it into
/// a rejected call, so this returns `Result`.
pub fn resolve_reload_scopes(options: &serde_json::Value) -> Result<Vec<String>, String> {
    let has_includes = options.get("includes").is_some();
    let has_excludes = options.get("excludes").is_some();

    if has_includes && has_excludes {
        return Err("includes and excludes cannot be used together".to_string());
    }

    if has_includes {
        // `[...new Set(options.includes)]` — a name nobody knows is kept, because
        // the original never validates it; it just matches no scope later
        let mut unique: Vec<String> = Vec::new();
        for scope in options.get("includes").and_then(Value::as_array).into_iter().flatten() {
            let Some(scope) = scope.as_str().map(str::to_string) else { continue };
            if !unique.contains(&scope) {
                unique.push(scope);
            }
        }
        return Ok(unique);
    }

    if has_excludes {
        let excluded: Vec<String> = options
            .get("excludes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|scope| scope.as_str().map(str::to_string))
            .collect();

        return Ok(RELOAD_SCOPES
            .iter()
            .map(|scope| scope.to_string())
            .filter(|scope| !excluded.contains(scope))
            .collect());
    }

    Ok(RELOAD_SCOPES.iter().map(|scope| scope.to_string()).collect())
}

/// `scopes.includes('matchHistory')` and friends.
pub fn includes(scopes: &[String], scope: &str) -> bool {
    scopes.iter().any(|candidate| candidate == scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn no_options_means_every_scope_in_declaration_order() {
        assert_eq!(
            resolve_reload_scopes(&json!({})).unwrap(),
            vec![
                "matchHistory".to_string(),
                "summoner".to_string(),
                "rankedStats".to_string(),
                "savedInfo".to_string(),
                "championMastery".to_string()
            ]
        );
    }

    #[test]
    fn includes_are_deduplicated_but_not_reordered() {
        assert_eq!(
            resolve_reload_scopes(&json!({ "includes": ["savedInfo", "summoner", "savedInfo"] })).unwrap(),
            vec!["savedInfo".to_string(), "summoner".to_string()]
        );
    }

    #[test]
    fn excludes_keep_the_order_of_the_declaration() {
        assert_eq!(
            resolve_reload_scopes(&json!({ "excludes": ["savedInfo", "matchHistory"] })).unwrap(),
            vec![
                "summoner".to_string(),
                "rankedStats".to_string(),
                "championMastery".to_string()
            ]
        );
    }

    #[test]
    fn mixing_the_two_is_an_error() {
        let error = resolve_reload_scopes(&json!({ "includes": ["summoner"], "excludes": ["savedInfo"] })).unwrap_err();
        assert_eq!(error, "includes and excludes cannot be used together");
    }

    #[test]
    fn an_unknown_scope_is_neither_validated_nor_matched() {
        let scopes = resolve_reload_scopes(&json!({ "includes": ["notAScope"] })).unwrap();
        assert_eq!(scopes, vec!["notAScope".to_string()]);
        assert!(!includes(&scopes, "summoner"));
    }

    #[test]
    fn an_empty_includes_loads_nothing() {
        assert!(resolve_reload_scopes(&json!({ "includes": [] })).unwrap().is_empty());
    }
}
