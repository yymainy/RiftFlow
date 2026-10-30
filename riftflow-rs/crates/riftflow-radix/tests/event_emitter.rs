//! Port of `src/shared/utils/event-emitter.test.ts` semantics: shared route
//! callback sets, unsubscribe pruning, and params delivery.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use serde_json::{json, Value};

use riftflow_radix::RadixEventEmitter;

#[test]
fn emits_to_matching_route_with_params() {
    let emitter = RadixEventEmitter::new();
    let seen = Arc::new(Mutex::new(Vec::<(Value, Value)>::new()));

    let sink = seen.clone();
    emitter
        .on("/lol-summoner/v1/summoners/:puuid", move |data, params| {
            sink.lock().unwrap().push((data, params));
        })
        .unwrap();

    emitter.emit("/lol-summoner/v1/summoners/player-1", json!({ "phase": "x" }));

    let got = seen.lock().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, json!({ "phase": "x" }));
    assert_eq!(got[0].1, json!({ "puuid": "player-1" }));
}

#[test]
fn two_listeners_on_the_same_uri_share_one_route() {
    let emitter = RadixEventEmitter::new();
    let hits = Arc::new(AtomicUsize::new(0));

    for _ in 0..2 {
        let hits = hits.clone();
        emitter
            .on("/lol-gameflow/v1/session", move |_data, _params| {
                hits.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
    }

    assert_eq!(emitter.listener_count("/lol-gameflow/v1/session"), 2);
    emitter.emit("/lol-gameflow/v1/session", json!(1));
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn unsubscribing_the_last_listener_prunes_the_route() {
    let emitter = RadixEventEmitter::new();
    let hits = Arc::new(AtomicUsize::new(0));

    let hits_a = hits.clone();
    let sub_a = emitter
        .on("/lol-chat/v1/me", move |_d, _p| {
            hits_a.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();

    let hits_b = hits.clone();
    let _sub_b = emitter
        .on("/lol-chat/v1/me", move |_d, _p| {
            hits_b.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();

    emitter.off(&sub_a);
    assert_eq!(emitter.listener_count("/lol-chat/v1/me"), 1);

    emitter.emit("/lol-chat/v1/me", json!(null));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[test]
fn static_route_receives_null_params() {
    let emitter = RadixEventEmitter::new();
    let params = Arc::new(Mutex::new(None::<Value>));

    let sink = params.clone();
    emitter
        .on("/lol-gameflow/v1/gameflow-phase", move |_d, p| {
            *sink.lock().unwrap() = Some(p);
        })
        .unwrap();

    emitter.emit("/lol-gameflow/v1/gameflow-phase", json!("InProgress"));

    // JS passes `undefined` for static routes; `null` is its serializable stand-in
    assert_eq!(*params.lock().unwrap(), Some(Value::Null));
}

#[test]
fn wildcard_and_placeholder_fan_out_in_insertion_order() {
    let emitter = RadixEventEmitter::new();
    let order = Arc::new(Mutex::new(Vec::<String>::new()));

    for (route, tag) in [
        ("/lol-chat/**", "all-chat"),
        ("/lol-chat/v1/conversations/:id/**", "conversation"),
    ] {
        let order = order.clone();
        let tag = tag.to_string();
        emitter
            .on(route, move |_d, _p| {
                order.lock().unwrap().push(tag.clone());
            })
            .unwrap();
    }

    emitter.emit("/lol-chat/v1/conversations/c-1/messages", json!(0));

    // broad wildcard fires from the ancestor node before the deeper route matches
    assert_eq!(
        *order.lock().unwrap(),
        vec!["all-chat".to_string(), "conversation".to_string()]
    );
}

#[test]
fn invalid_route_is_rejected_instead_of_thrown() {
    let emitter = RadixEventEmitter::new();
    assert!(emitter.on("", |_d, _p| {}).is_err());
    assert!(emitter.on("/", |_d, _p| {}).is_err());
    assert!(emitter.on("/lol/:", |_d, _p| {}).is_err());
    assert!(emitter.on("/lol/**/session", |_d, _p| {}).is_err());

    // `/lol/` collapses to `["lol"]` after the JS `filter(Boolean)`, so it is a
    // legal static route rather than a malformed one.
    assert!(emitter.on("/lol/", |_d, _p| {}).is_ok());
}

#[test]
fn clear_drops_every_route() {
    let emitter = RadixEventEmitter::new();
    emitter
        .on("/a/:b", |_d, _p| {})
        .unwrap();
    emitter.on("/c/**", |_d, _p| {}).unwrap();
    emitter.clear();

    assert_eq!(emitter.listener_count("/a/1"), 0);
    assert_eq!(emitter.listener_count("/c/d/e"), 0);
}
