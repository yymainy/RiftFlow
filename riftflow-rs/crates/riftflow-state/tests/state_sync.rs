//! Spec for the MobxUtilsMain replacement: the same lazy-subscription rule and
//! the exact `riftflow-event` frame the renderer shards expect,
//! `["mobx-utils-main", "update-state-prop/<ns>:<stateId>", path, value, config]`.

use std::sync::{Arc, Mutex};

use riftflow_ipc::{RiftFlowIpcMain, RendererTransport};
use riftflow_state::{Action, StateSync};
use serde_json::{json, Value};

#[derive(Clone, Default)]
struct RecordingTransport {
    /// (renderer, channel, args)
    sent: Arc<Mutex<Vec<(u64, String, Vec<Value>)>>>,
}

impl RendererTransport for RecordingTransport {
    fn send_to(&self, renderer: u64, event: &str, args: Vec<Value>) -> anyhow::Result<()> {
        self.sent
            .lock()
            .unwrap()
            .push((renderer, event.to_string(), args));
        Ok(())
    }
}

struct Harness {
    sync: Arc<StateSync>,
    ipc: Arc<RiftFlowIpcMain>,
    transport: RecordingTransport,
}

impl Harness {
    fn new() -> Self {
        let transport = RecordingTransport::default();
        let ipc = Arc::new(RiftFlowIpcMain::new(Arc::new(transport.clone())));
        let sync = Arc::new(StateSync::new(ipc.clone()));
        Self { sync, ipc, transport }
    }

    fn frames(&self) -> Vec<(u64, String, Vec<Value>)> {
        self.transport.sent.lock().unwrap().clone()
    }

    fn subscribe(&self, renderer: u64, namespace: &str, state_id: &str) -> Value {
        self.ipc.renderer_register(renderer, Some("register"));
        self.sync
            .subscribe_and_get_initial_state(renderer, namespace, state_id)
            .unwrap()
    }
}

#[test]
fn nothing_is_pushed_until_a_renderer_subscribes() {
    let h = Harness::new();
    h.sync
        .prop_sync(
            "league-client-main",
            "state",
            &[("connectionState", json!("disconnected"))],
            true,
        )
        .unwrap();

    h.sync
        .set("league-client-main", "state", "connectionState", json!("connected"))
        .unwrap();

    assert!(h.frames().is_empty(), "JS only pushes to subscribers");
}

#[test]
fn subscribed_renderer_receives_the_exact_frame() {
    let h = Harness::new();
    h.sync
        .prop_sync(
            "league-client-main",
            "state",
            &[("connectionState", json!("disconnected"))],
            true,
        )
        .unwrap();
    h.subscribe(11, "league-client-main", "state");

    h.sync
        .set("league-client-main", "state", "connectionState", json!("connected"))
        .unwrap();

    let frames = h.frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].0, 11);
    assert_eq!(frames[0].1, "riftflow-event");
    assert_eq!(
        frames[0].2,
        vec![
            json!("mobx-utils-main"),
            json!("update-state-prop/league-client-main:state"),
            json!("connectionState"),
            json!("connected"),
            json!({ "action": "update", "raw": true }),
        ]
    );
}

#[test]
fn deep_state_key_uses_the_namespaced_event_name() {
    let h = Harness::new();
    h.sync
        .prop_sync("respawn-timer-main", "state", &[("info", json!(null))], true)
        .unwrap();
    h.subscribe(4, "respawn-timer-main", "state");

    h.sync
        .set("respawn-timer-main", "state", "info", json!({ "remaining": 12 }))
        .unwrap();

    let frames = h.frames();
    assert_eq!(frames[0].2[1], json!("update-state-prop/respawn-timer-main:state"));
    assert_eq!(frames[0].2[2], json!("info"));
    assert_eq!(frames[0].2[3], json!({ "remaining": 12 }));
}

#[test]
fn unchanged_value_does_not_push() {
    let h = Harness::new();
    h.sync
        .prop_sync("auto-gameflow-main", "settings", &[("enabled", json!(true))], true)
        .unwrap();
    h.subscribe(1, "auto-gameflow-main", "settings");

    let changed = h
        .sync
        .set("auto-gameflow-main", "settings", "enabled", json!(true))
        .unwrap();

    assert!(!changed);
    assert!(h.frames().is_empty());
}

#[test]
fn subscribe_returns_declared_props_with_value_and_config() {
    let h = Harness::new();
    h.sync
        .prop_sync(
            "league-client-ux-main",
            "state",
            &[
                ("launchedClients", json!([{ "pid": 1, "port": 8232 }])),
                ("hasClientButNoCommandLine", json!(false)),
            ],
            true,
        )
        .unwrap();
    h.sync
        .prop_sync(
            "league-client-ux-main",
            "settings",
            &[("useWmi", json!(false))],
            true,
        )
        .unwrap();

    let state = h.subscribe(2, "league-client-ux-main", "state");
    assert_eq!(
        state,
        json!({
            "launchedClients": {
                "value": [{ "pid": 1, "port": 8232 }],
                "config": { "raw": true }
            },
            "hasClientButNoCommandLine": {
                "value": false,
                "config": { "raw": true }
            }
        })
    );

    let settings = h.subscribe(2, "league-client-ux-main", "settings");
    assert_eq!(
        settings,
        json!({ "useWmi": { "value": false, "config": { "raw": true } } })
    );
}

#[test]
fn duplicate_prop_path_and_unknown_state_error_strings() {
    let h = Harness::new();
    h.sync
        .prop_sync("sgp-main", "state", &[("tokens", json!(null))], true)
        .unwrap();

    let err = h
        .sync
        .prop_sync("sgp-main", "state", &[("tokens", json!(null))], true)
        .unwrap_err()
        .to_string();
    assert_eq!(err, "Prop path tokens already registered for state");

    let err = h
        .sync
        .subscribe_and_get_initial_state(1, "sgp-main", "missing")
        .unwrap_err()
        .to_string();
    assert_eq!(err, "State sgp-main:missing not found");

    let err = h
        .sync
        .set("sgp-main", "state", "undeclared", json!(1))
        .unwrap_err()
        .to_string();
    assert_eq!(err, "Prop path undeclared not registered for state");
}

#[test]
fn unsubscribe_stops_the_flow() {
    let h = Harness::new();
    h.sync
        .prop_sync("statistics-main", "state", &[("summary", json!(0))], true)
        .unwrap();
    h.subscribe(8, "statistics-main", "state");

    h.sync
        .set("statistics-main", "state", "summary", json!(1))
        .unwrap();
    assert_eq!(h.frames().len(), 1);

    h.sync.unsubscribe(8);
    h.sync
        .set("statistics-main", "state", "summary", json!(2))
        .unwrap();
    assert_eq!(h.frames().len(), 1);
}

#[test]
fn notify_prop_change_pushes_even_when_the_value_is_equal() {
    let h = Harness::new();
    h.sync
        .prop_sync("ongoing-game-main", "state", &[("games", json!([]))], true)
        .unwrap();
    h.subscribe(6, "ongoing-game-main", "state");

    h.sync
        .notify_state_prop_change("ongoing-game-main", "state", "games", json!([]), Action::Update, true);

    let frames = h.frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(
        frames[0].2,
        vec![
            json!("mobx-utils-main"),
            json!("update-state-prop/ongoing-game-main:state"),
            json!("games"),
            json!([]),
            json!({ "action": "update", "raw": true }),
        ]
    );
}

#[test]
fn delete_action_is_carried_in_the_config_argument() {
    let h = Harness::new();
    h.sync
        .prop_sync("saved-player-main", "state", &[("players", json!([]))], true)
        .unwrap();
    h.subscribe(6, "saved-player-main", "state");

    h.sync.notify_state_prop_change(
        "saved-player-main",
        "state",
        "players",
        json!([]),
        Action::Delete,
        true,
    );

    assert_eq!(h.frames()[0].2[4], json!({ "action": "delete", "raw": true }));
}

#[test]
fn raw_false_config_is_preserved() {
    let h = Harness::new();
    h.sync
        .prop_sync("game-client-main", "state", &[("info", json!(null))], false)
        .unwrap();

    let initial = h.subscribe(6, "game-client-main", "state");
    assert_eq!(initial["info"]["config"], json!({ "raw": false }));

    h.sync
        .set("game-client-main", "state", "info", json!({ "pid": 1 }))
        .unwrap();

    assert_eq!(
        h.frames()[0].2[4],
        json!({ "action": "update", "raw": false })
    );
}

#[test]
fn every_subscriber_of_a_state_gets_its_own_copy() {
    let h = Harness::new();
    h.sync
        .prop_sync("tray-main", "state", &[("visible", json!(false))], true)
        .unwrap();
    h.subscribe(20, "tray-main", "state");
    h.subscribe(21, "tray-main", "state");

    h.sync
        .set("tray-main", "state", "visible", json!(true))
        .unwrap();

    let frames = h.frames();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].0, 20);
    assert_eq!(frames[1].0, 21);
}
