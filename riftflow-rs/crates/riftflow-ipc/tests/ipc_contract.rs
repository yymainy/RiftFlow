//! Wire-contract spec for the ported IPC router. These assertions are what the
//! untouched Vue renderer depends on, so key names and message strings are
//! checked literally against `src/main/shards/ipc`.

use std::sync::{Arc, Mutex};

use riftflow_ipc::{
    handler, RiftFlowIpcMain, CallContext, CallOutcome, IpcEnvelope, IpcError, RendererTransport,
    EVENT_CHANNEL,
};
use serde_json::{json, Value};

#[derive(Clone, Default)]
struct RecordingTransport {
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

fn ipc() -> (Arc<RiftFlowIpcMain>, RecordingTransport) {
    let transport = RecordingTransport::default();
    let ipc = Arc::new(RiftFlowIpcMain::new(Arc::new(transport.clone())));
    (ipc, transport)
}

#[tokio::test]
async fn success_envelope_has_exactly_success_and_data() {
    let envelope = IpcEnvelope::ok(json!({ "autoConnect": true }));
    let value = serde_json::to_value(&envelope).unwrap();

    assert_eq!(value, json!({ "success": true, "data": { "autoConnect": true } }));

    // key order is not part of the contract; the exact key set is
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["data".to_string(), "success".to_string()]);
}

#[tokio::test]
async fn success_envelope_keeps_data_when_null() {
    let value = serde_json::to_value(IpcEnvelope::ok(Value::Null)).unwrap();
    assert!(value.as_object().unwrap().contains_key("data"));
    assert_eq!(value, json!({ "success": true, "data": null }));
}

#[tokio::test]
async fn plain_error_envelope_shape() {
    let value = IpcError::plain("nope").to_envelope().to_json();

    assert_eq!(
        value,
        json!({ "success": false, "error": { "message": "nope", "name": "Error" } })
    );
    assert!(!value.as_object().unwrap().contains_key("isAxiosError"));
}

#[tokio::test]
async fn business_error_carries_a_code_and_riftflow_name() {
    let value = IpcError::business("LCU is not initialized", "LCU_UNINITIALIZED")
        .to_envelope()
        .to_json();

    assert_eq!(
        value,
        json!({
            "success": false,
            "error": {
                "message": "LCU is not initialized",
                "name": "RiftFlowIpcError",
                "code": "LCU_UNINITIALIZED"
            }
        })
    );
}

#[tokio::test]
async fn http_error_is_flagged_as_axios_error() {
    let value = IpcError::Http {
        message: "Request failed with status code 404".to_string(),
        name: "AxiosError".to_string(),
        code: Some("ERR_BAD_REQUEST".to_string()),
        status: Some(404),
        status_text: Some("Not Found".to_string()),
        body: Some(json!({ "error": "not found" })),
    }
    .to_envelope()
    .to_json();

    assert_eq!(value["success"], json!(false));
    assert_eq!(value["isAxiosError"], json!(true));
    assert_eq!(value["error"]["response"]["status"], json!(404));
    assert_eq!(value["error"]["response"]["statusText"], json!("Not Found"));
    assert_eq!(value["error"]["response"]["data"], json!({ "error": "not found" }));
    assert_eq!(value["error"]["code"], json!("ERR_BAD_REQUEST"));
    assert_eq!(value["error"]["name"], json!("AxiosError"));
}

#[tokio::test]
async fn http_error_without_response_serializes_response_as_null() {
    let value = IpcError::http("connect ECONNREFUSED", None, None)
        .to_envelope()
        .to_json();

    assert_eq!(value["isAxiosError"], json!(true));
    assert_eq!(value["error"]["response"], json!(null));
}

#[tokio::test]
async fn call_dispatches_by_namespace_and_method_with_positional_args() {
    let (ipc, _transport) = ipc();

    ipc.on_call(
        "league-client-main",
        "subscribeLcuEndpoint",
        handler(|ctx: CallContext, args: Vec<Value>| async move {
            assert_eq!(ctx.renderer, 7);
            assert_eq!(args, vec![json!("/lol-gameflow/v1/gameflow-phase"), json!(2)]);
            Ok(json!("__2"))
        }),
    )
    .unwrap();

    let outcome = ipc
        .handle_call(
            7,
            "league-client-main",
            "subscribeLcuEndpoint",
            vec![json!("/lol-gameflow/v1/gameflow-phase"), json!(2)],
        )
        .await;

    assert_eq!(
        outcome,
        CallOutcome::Reply(IpcEnvelope::ok(json!("__2")))
    );
}

#[tokio::test]
async fn handler_error_becomes_an_envelope_not_a_rejection() {
    let (ipc, _transport) = ipc();

    ipc.on_call(
        "auto-select-main",
        "lockAction",
        handler(|_ctx, _args| async move {
            Err(IpcError::business("no champ select session", "NOT_IN_CHAMP_SELECT"))
        }),
    )
    .unwrap();

    let outcome = ipc.handle_call(1, "auto-select-main", "lockAction", vec![]).await;

    match outcome {
        CallOutcome::Reply(envelope) => {
            assert!(!envelope.success);
            assert_eq!(envelope.to_json()["error"]["code"], json!("NOT_IN_CHAMP_SELECT"));
        }
        CallOutcome::Rejected(message) => panic!("should not reject, got {message}"),
    }
}

#[tokio::test]
async fn unknown_method_rejects_at_the_transport_level() {
    let (ipc, _transport) = ipc();

    let outcome = ipc.handle_call(1, "tray-main", "notRegistered", vec![]).await;

    assert_eq!(
        outcome,
        CallOutcome::Rejected("No function \"notRegistered\" in namespace \"tray-main\"".to_string())
    );
}

#[tokio::test]
async fn duplicate_registration_message_matches_js() {
    let (ipc, _transport) = ipc();

    let noop = handler(|_ctx, _args| async move { Ok(Value::Null) });
    ipc.on_call("app-common-main", "getVersion", noop.clone()).unwrap();

    let err = ipc.on_call("app-common-main", "getVersion", noop).unwrap_err().to_string();
    assert_eq!(
        err,
        "Function \"getVersion\" in namespace \"app-common-main\" already exists"
    );
}

#[tokio::test]
async fn same_method_name_in_two_namespaces_is_allowed() {
    let (ipc, _transport) = ipc();
    ipc.on_call("sgp-main", "get", handler(|_c, _a| async move { Ok(json!("sgp")) }))
        .unwrap();
    ipc.on_call("saved-player-main", "get", handler(|_c, _a| async move { Ok(json!("saved")) }))
        .unwrap();

    assert_eq!(ipc.call_count(), 2);
    let a = ipc.handle_call(1, "sgp-main", "get", vec![]).await;
    let b = ipc.handle_call(1, "saved-player-main", "get", vec![]).await;
    assert_eq!(a, CallOutcome::Reply(IpcEnvelope::ok(json!("sgp"))));
    assert_eq!(b, CallOutcome::Reply(IpcEnvelope::ok(json!("saved"))));
}

#[tokio::test]
async fn register_reply_has_no_data_key_and_repeat_register_fails() {
    let (ipc, _transport) = ipc();

    let first = serde_json::to_value(ipc.renderer_register(3, Some("register"))).unwrap();
    assert_eq!(first, json!({ "success": true }));

    // the JS original falls through to the error branch on a duplicate
    let second = serde_json::to_value(ipc.renderer_register(3, Some("register"))).unwrap();
    assert_eq!(
        second,
        json!({ "success": false, "error": { "message": "invalid action \"register\"" } })
    );

    let unknown = serde_json::to_value(ipc.renderer_register(3, Some("toggle"))).unwrap();
    assert_eq!(
        unknown,
        json!({ "success": false, "error": { "message": "invalid action \"toggle\"" } })
    );

    assert_eq!(ipc.registered_renderer_ids(), vec![3]);
    let unregistered = serde_json::to_value(ipc.renderer_register(3, Some("unregister"))).unwrap();
    assert_eq!(unregistered, json!({ "success": true }));
    assert!(ipc.registered_renderer_ids().is_empty());

    // unregistering an unknown renderer also lands in the error branch
    let again = serde_json::to_value(ipc.renderer_register(9, Some("unregister"))).unwrap();
    assert_eq!(
        again,
        json!({ "success": false, "error": { "message": "invalid action \"unregister\"" } })
    );
}

#[tokio::test]
async fn register_action_defaults_to_register() {
    let (ipc, _transport) = ipc();
    let value = serde_json::to_value(ipc.renderer_register(5, None)).unwrap();
    assert_eq!(value, json!({ "success": true }));
    assert_eq!(ipc.registered_renderer_ids(), vec![5]);
}

#[tokio::test]
async fn send_event_payload_is_namespace_event_then_args() {
    let (ipc, transport) = ipc();
    ipc.renderer_register(1, Some("register"));
    ipc.renderer_register(2, Some("register"));

    ipc.send_event(
        "league-client-main",
        "extra-lcu-event",
        &[json!("__1"), json!({ "uri": "/x", "data": 5 }), json!({ "puuid": "p" })],
    );

    let sent = transport.sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    for (renderer, event, args) in sent.iter() {
        assert_eq!(event, EVENT_CHANNEL);
        assert_eq!(
            args,
            &vec![
                json!("league-client-main"),
                json!("extra-lcu-event"),
                json!("__1"),
                json!({ "uri": "/x", "data": 5 }),
                json!({ "puuid": "p" }),
            ],
            "renderer {renderer}"
        );
    }
    assert_eq!(sent[0].0, 1);
    assert_eq!(sent[1].0, 2);
}

#[tokio::test]
async fn send_event_to_one_renderer_only() {
    let (ipc, transport) = ipc();
    ipc.renderer_register(1, Some("register"));
    ipc.renderer_register(2, Some("register"));

    ipc.send_event_to(2, "respawn-timer-main", "state-changed", &[json!(true)]);

    let sent = transport.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, 2);
    assert_eq!(
        sent[0].2,
        vec![json!("respawn-timer-main"), json!("state-changed"), json!(true)]
    );
}

#[tokio::test]
async fn destroyed_renderer_stops_receiving_events() {
    let (ipc, transport) = ipc();
    ipc.renderer_register(1, Some("register"));
    ipc.forget_renderer(1);

    ipc.send_event("tray-main", "clicked", &[]);
    assert!(transport.sent.lock().unwrap().is_empty());
}
