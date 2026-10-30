//! Bridges the `riftflow://sgp/*` proxy domain onto [`riftflow_sgp`]: it reads the
//! snapshot the SGP interceptor needs out of the synced state, runs the request,
//! and keeps the connectivity counters the response interceptor maintained.

use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use riftflow_sgp::{SgpGateway, SgpSnapshot};

use crate::lc_state::{LEAGUE_CLIENT_NAMESPACE, SGP_NAMESPACE};
use crate::protocol::Proxied;

fn snapshot(app: &AppHandle) -> SgpSnapshot {
    let state = app.state::<crate::AppState>();
    let sync = &state.state_sync;

    let servers = sync
        .get(SGP_NAMESPACE, "state", "leagueServers")
        .and_then(|config| config.get("servers").cloned())
        .unwrap_or(Value::Null);

    let default_server_id = sync
        .get(SGP_NAMESPACE, "state", "availability")
        .and_then(|availability| {
            availability
                .get("sgpServerId")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();

    let entitlements_token = sync
        .get(LEAGUE_CLIENT_NAMESPACE, "entitlements", "token")
        .and_then(|token| {
            token
                .get("accessToken")
                .and_then(Value::as_str)
                .map(str::to_string)
        });

    let league_session_token = sync
        .get(LEAGUE_CLIENT_NAMESPACE, "leagueSession", "token")
        .and_then(|token| token.as_str().map(str::to_string));

    SgpSnapshot {
        servers,
        default_server_id,
        entitlements_token,
        league_session_token,
    }
}

/// `connectionSuccessesCounted` / `connectionFailuresCounted` — the renderer shows
/// these in the debug panel, and the reset on an empty `sgpServerId` lives in
/// `lc_state::sync_availability`.
fn count(app: &AppHandle, success: bool) {
    let state = app.state::<crate::AppState>();
    let sync = &state.state_sync;
    let path = if success {
        "connectionSuccessesCounted"
    } else {
        "connectionFailuresCounted"
    };

    let current = sync
        .get(SGP_NAMESPACE, "state", path)
        .and_then(|value| value.as_i64())
        .unwrap_or_default();

    let _ = sync.set(
        SGP_NAMESPACE,
        "state",
        path,
        Value::from(current + 1),
    );
}

pub async fn request(
    app: &AppHandle,
    gateway: &Arc<SgpGateway>,
    method: &str,
    uri: &str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> Proxied {
    match gateway
        .request(&snapshot(app), method, uri, &headers, &body)
        .await
    {
        Ok(reply) => {
            count(app, true);
            Proxied::new(reply.status, reply.headers, reply.body)
        }
        Err(error) => {
            // a rule failure is not a connectivity failure, matching the original's
            // `error.response === undefined` test
            if matches!(error, riftflow_sgp::SgpError::Transport(_)) {
                count(app, false);
            }
            if std::env::var("RIFTFLOW_LCU_TRACE").is_ok() {
                eprintln!("[sgp] {method} {uri} → {}", error.message());
            }
            Proxied::text(500, error.message())
        }
    }
}
