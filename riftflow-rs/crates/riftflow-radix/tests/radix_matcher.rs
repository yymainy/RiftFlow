//! Rust port of `src/shared/utils/radix-matcher.test.ts` — the spec the
//! TypeScript implementation is held to. Kept case-for-case.

use serde_json::{json, Value};

use riftflow_radix::RadixMatcher;

fn data_of(results: &[riftflow_radix::FindResult<Value>]) -> Vec<Value> {
    results.iter().map(|r| r.data.clone()).collect()
}

#[test]
fn accepts_valid_routes() {
    for route in [
        "/lol-gameflow/v1/session",
        "/lol-summoner/v1/summoners/:puuid",
        "/lol-chat/v1/conversations/*/messages",
        "/lol-champ-select/v1/session/**",
        "/**",
    ] {
        assert!(RadixMatcher::<Value>::validate_route(route).is_ok(), "{route}");
    }
}

#[test]
fn rejects_malformed_routes() {
    let cases = [
        ("", "route should not be empty"),
        ("/", "route should not be empty"),
        ("/lol/:", "placeholder should have a name"),
        ("/lol/**/session", "wildcard should be the last part"),
        ("/lol/**session", "wildcard should be **"),
        ("/lol/*session", "placeholder * should be the only part"),
        ("/lol/game:flow", "normal nodes should not have : or *"),
        ("/lol/game*flow", "normal nodes should not have : or *"),
    ];

    for (route, message) in cases {
        let err = RadixMatcher::<Value>::validate_route(route)
            .err()
            .unwrap_or_else(|| panic!("{route} should be rejected"));
        assert_eq!(err.to_string(), message, "{route}");
    }
}

#[test]
fn static_routes_match_exactly_and_come_first() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-gameflow/v1/session", json!("static-session"))
        .unwrap();
    matcher
        .insert("/lol-gameflow/v1/:resource", json!("dynamic-resource"))
        .unwrap();

    let one = matcher.find_one("/lol-gameflow/v1/session").unwrap();
    assert_eq!(one.data, json!("static-session"));
    assert!(one.params.is_none());

    assert_eq!(
        data_of(&matcher.find_all("/lol-gameflow/v1/session")),
        vec![json!("static-session"), json!("dynamic-resource")]
    );
    assert!(matcher.find_one("/lol-gameflow/v1/session/extra").is_none());
}

#[test]
fn static_routes_get_remove_clear() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-gameflow/v1/session", json!("session"))
        .unwrap();

    assert_eq!(
        matcher.get_route_data("/lol-gameflow/v1/session"),
        Some(json!("session"))
    );
    assert!(matcher.remove("/lol-gameflow/v1/session"));
    assert!(matcher.find_all("/lol-gameflow/v1/session").is_empty());
    assert!(!matcher.remove("/lol-gameflow/v1/session"));

    matcher
        .insert("/lol-gameflow/v1/session", json!("session"))
        .unwrap();
    matcher.clear();
    assert!(matcher.find_one("/lol-gameflow/v1/session").is_none());
}

#[test]
fn duplicate_static_route_is_rejected() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-gameflow/v1/session", json!("session"))
        .unwrap();
    let err = matcher
        .insert("/lol-gameflow/v1/session", json!("session-2"))
        .unwrap_err();
    assert_eq!(err.to_string(), "route '/lol-gameflow/v1/session' already exists");
}

#[test]
fn named_placeholder_captures_params() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-summoner/v1/summoners/:puuid", json!("summoner"))
        .unwrap();

    let one = matcher
        .find_one("/lol-summoner/v1/summoners/player-1")
        .expect("match");
    assert_eq!(one.data, json!("summoner"));
    assert_eq!(one.params, Some(json!({ "puuid": "player-1" })));

    assert_eq!(
        matcher.get_route_data("/lol-summoner/v1/summoners/:puuid"),
        Some(json!("summoner"))
    );
    assert!(matcher
        .get_route_data("/lol-summoner/v1/summoners/player-1")
        .is_none());
}

#[test]
fn anonymous_placeholders_use_stable_numeric_keys() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-chat/v1/conversations/*/messages/*", json!("chat-message"))
        .unwrap();

    let one = matcher
        .find_one("/lol-chat/v1/conversations/conv-1/messages/msg-1")
        .expect("match");
    assert_eq!(one.data, json!("chat-message"));
    assert_eq!(
        one.params,
        Some(json!({ "_0": "conv-1", "_1": "msg-1" }))
    );
}

#[test]
fn falsy_data_is_allowed_on_dynamic_routes() {
    let mut matcher = RadixMatcher::new();
    matcher.insert("/flags/:name", json!(false)).unwrap();
    matcher.insert("/counts/:name", json!(0)).unwrap();
    matcher.insert("/labels/:name", json!("")).unwrap();

    assert_eq!(matcher.find_one("/flags/enabled").unwrap().data, json!(false));
    assert_eq!(matcher.find_one("/counts/zero").unwrap().data, json!(0));
    assert_eq!(matcher.find_one("/labels/empty").unwrap().data, json!(""));
    assert_eq!(
        matcher.get_route_data("/flags/:name"),
        Some(json!(false))
    );
}

#[test]
fn duplicate_dynamic_routes_are_rejected() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-summoner/v1/summoners/:puuid", json!("summoner"))
        .unwrap();
    let err = matcher
        .insert("/lol-summoner/v1/summoners/:puuid", json!("summoner-2"))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "route '/lol-summoner/v1/summoners/:puuid' already exists"
    );

    matcher
        .insert(
            "/lol-chat/v1/conversations/:conversationId/*",
            json!("chat-message"),
        )
        .unwrap();
    let err = matcher
        .insert(
            "/lol-chat/v1/conversations/:conversationId/*",
            json!("chat-2"),
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "route '/lol-chat/v1/conversations/:conversationId/*' already exists"
    );

    assert_eq!(
        data_of(&matcher.find_all("/lol-chat/v1/conversations/conv-1/msg-1")),
        vec![json!("chat-message")]
    );
}

#[test]
fn remove_keeps_sibling_routes_sharing_a_prefix() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-summoner/v1/summoners/:puuid/profile", json!("profile"))
        .unwrap();
    matcher
        .insert(
            "/lol-summoner/v1/summoners/:puuid/ranked-stats",
            json!("ranked"),
        )
        .unwrap();

    assert!(matcher.remove("/lol-summoner/v1/summoners/:puuid/profile"));
    assert!(matcher
        .find_one("/lol-summoner/v1/summoners/player-1/profile")
        .is_none());

    let ranked = matcher
        .find_one("/lol-summoner/v1/summoners/player-1/ranked-stats")
        .expect("sibling survives");
    assert_eq!(ranked.data, json!("ranked"));
    assert_eq!(ranked.params, Some(json!({ "puuid": "player-1" })));
}

#[test]
fn captures_many_placeholders_across_a_long_route() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert(
            "/deep/:namespace/v1/*/summoners/:puuid/matches/*/timeline/:frameId/events/*/details/:detailId",
            json!("deep-dynamic"),
        )
        .unwrap();

    let one = matcher
        .find_one(
            "/deep/lol/v1/region-1/summoners/player-1/matches/match-1/timeline/42/events/event-1/details/detail-1",
        )
        .expect("match");

    assert_eq!(one.data, json!("deep-dynamic"));
    assert_eq!(
        one.params,
        Some(json!({
            "namespace": "lol",
            "_0": "region-1",
            "puuid": "player-1",
            "_1": "match-1",
            "frameId": "42",
            "_2": "event-1",
            "detailId": "detail-1"
        }))
    );
}

#[test]
fn wildcard_matches_at_prefix_and_deeper() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-champ-select/v1/session/**", json!("champ-select"))
        .unwrap();

    let at_prefix = matcher
        .find_one("/lol-champ-select/v1/session")
        .expect("match");
    assert_eq!(at_prefix.data, json!("champ-select"));
    assert_eq!(at_prefix.params, Some(json!({})));

    let deeper = matcher
        .find_one("/lol-champ-select/v1/session/actions/1")
        .expect("match");
    assert_eq!(deeper.params, Some(json!({ "__": "actions/1" })));
}

#[test]
fn wildcard_combines_with_placeholder_params() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert(
            "/lol-chat/v1/conversations/:conversationId/**",
            json!("conversation"),
        )
        .unwrap();

    let one = matcher
        .find_one("/lol-chat/v1/conversations/conv-1/messages/msg-1")
        .expect("match");
    assert_eq!(
        one.params,
        Some(json!({ "__": "messages/msg-1", "conversationId": "conv-1" }))
    );
}

#[test]
fn root_wildcard_route() {
    let mut matcher = RadixMatcher::new();
    matcher.insert("/**", json!("all")).unwrap();

    let root = matcher.find_one("/").expect("match");
    assert_eq!(root.data, json!("all"));
    assert_eq!(root.params, Some(json!({})));

    let deeper = matcher
        .find_one("/lol-gameflow/v1/session")
        .expect("match");
    assert_eq!(
        deeper.params,
        Some(json!({ "__": "lol-gameflow/v1/session" }))
    );
}

#[test]
fn wildcard_stays_separate_from_exact_prefix() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-gameflow/v1/session/**", json!("session-tree"))
        .unwrap();
    matcher
        .insert("/lol-gameflow/v1/session", json!("session-root"))
        .unwrap();

    assert_eq!(
        data_of(&matcher.find_all("/lol-gameflow/v1/session")),
        vec![json!("session-root"), json!("session-tree")]
    );
    assert_eq!(
        matcher.get_route_data("/lol-gameflow/v1/session"),
        Some(json!("session-root"))
    );
    assert_eq!(
        matcher.get_route_data("/lol-gameflow/v1/session/**"),
        Some(json!("session-tree"))
    );
}

#[test]
fn remove_wildcard_leaves_no_stale_prefix_match() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-gameflow/v1/session/**", json!("session-tree"))
        .unwrap();
    matcher
        .insert("/lol-gameflow/v1/session", json!("session-root"))
        .unwrap();

    assert!(matcher.remove("/lol-gameflow/v1/session/**"));
    assert!(matcher.find_all("/lol-gameflow/v1/session/actions").is_empty());
    assert_eq!(
        data_of(&matcher.find_all("/lol-gameflow/v1/session")),
        vec![json!("session-root")]
    );
    assert!(!matcher.remove("/lol-gameflow/v1/session/**"));
}

#[test]
fn find_one_prefers_specific_route_over_broad_wildcard() {
    let mut matcher = RadixMatcher::new();
    matcher
        .insert("/lol-chat/**", json!("all-chat"))
        .unwrap();
    matcher
        .insert(
            "/lol-chat/v1/conversations/:conversationId/messages/:messageId",
            json!("message"),
        )
        .unwrap();

    let one = matcher
        .find_one("/lol-chat/v1/conversations/conv-1/messages/msg-1")
        .expect("match");
    assert_eq!(one.data, json!("message"));
    assert_eq!(
        one.params,
        Some(json!({ "conversationId": "conv-1", "messageId": "msg-1" }))
    );
}

#[test]
fn many_wildcards_sharing_a_prefix() {
    let mut matcher = RadixMatcher::new();
    matcher.insert("/lol-chat/**", json!("all-chat")).unwrap();

    for i in 0..100 {
        matcher
            .insert(
                &format!("/lol-chat/v1/conversations/{i}/**"),
                json!(format!("conversation-{i}")),
            )
            .unwrap();
    }

    let path = "/lol-chat/v1/conversations/42/messages/1";
    let one = matcher.find_one(path).expect("match");
    assert_eq!(one.data, json!("conversation-42"));
    assert_eq!(one.params, Some(json!({ "__": "messages/1" })));

    assert_eq!(
        data_of(&matcher.find_all(path)),
        vec![json!("all-chat"), json!("conversation-42")]
    );

    assert!(matcher.remove("/lol-chat/v1/conversations/42/**"));

    let after = matcher.find_one(path).expect("falls back");
    assert_eq!(after.data, json!("all-chat"));
    assert_eq!(
        after.params,
        Some(json!({ "__": "v1/conversations/42/messages/1" }))
    );

    let sibling = matcher
        .find_one("/lol-chat/v1/conversations/43/messages/1")
        .expect("match");
    assert_eq!(sibling.data, json!("conversation-43"));
    assert_eq!(sibling.params, Some(json!({ "__": "messages/1" })));
}

#[test]
fn long_static_route_matches_only_itself() {
    let mut matcher = RadixMatcher::new();
    let long_route = format!(
        "/{}",
        (0..40)
            .map(|i| format!("segment-{i}"))
            .collect::<Vec<_>>()
            .join("/")
    );
    matcher.insert(&long_route, json!("long-static")).unwrap();

    assert_eq!(
        matcher.find_one(&long_route).unwrap().data,
        json!("long-static")
    );

    let shorter = long_route.rsplit_once('/').unwrap().0;
    assert!(matcher.find_one(shorter).is_none());
    assert!(matcher.find_one(&format!("{long_route}/extra")).is_none());
}
