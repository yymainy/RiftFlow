//! The Riot Client's own local HTTP API (`riot-client-main`).
//!
//! Port of `src/main/shards/riot-client/{index.ts,ipc-handlers.ts,context.ts}`.
//! This is *not* the LCU: different port (`--riotclient-app-port`) and a
//! different Basic credential (`riot:` + `--riotclient-auth-token`), both read
//! from the UX command line the same way `LeagueClientMain` reads its own.
//! `LcuHttp` from `riftflow-lcu` is the right client for it — same
//! `https://127.0.0.1:<port>`, same self-signed cert, same 17 500 ms timeout
//! (`RIOT_CLIENT_REQUEST_TIMEOUT_MS`), no proxy.
//!
//! Known deviation: axios retried nothing for the Riot Client (the retry wrapper
//! was only put on the LCU client), while `LcuHttp` retries idempotent 5xx
//! responses and bails after the third attempt. A persistent RC 5xx therefore
//! surfaces as an IPC error instead of an axios-shaped response object.

use std::sync::{Arc, Mutex};

use riftflow_ipc::{handler, RiftFlowIpcMain, IpcError, IpcResult};
use riftflow_lcu::http::{LcuHttp, RequestConfig};
use riftflow_lcu::LeagueClientMain;
use serde_json::Value;

pub const RIOT_CLIENT_MAIN_NAMESPACE: &str = "riot-client-main";

/// The JS `RiotClientRcuUninitializedError`; the name is part of the wire
/// contract because the protocol domain answers 503 with it.
pub const RCU_UNINITIALIZED_CODE: &str = "RiotClientRcuUninitializedError";

/// `_initHttpInstance` runs once per auth change and keeps the axios agent
/// (keep-alive included) alive between requests, so the client is cached and
/// only rebuilt when the port or token changes.
#[derive(Default)]
pub struct RiotClientService {
    cached: Mutex<Option<Cached>>,
}

struct Cached {
    port: i64,
    token: String,
    http: Arc<LcuHttp>,
}

impl RiotClientService {
    pub fn new() -> Self {
        Self::default()
    }

    fn http_for(&self, port: i64, token: &str) -> Arc<LcuHttp> {
        {
            let guard = self.cached.lock().unwrap();
            if let Some(cached) = guard.as_ref() {
                if cached.port == port && cached.token == token {
                    return cached.http.clone();
                }
            }
        }

        // `Authorization: Basic base64("riot:" + riotClientAuthToken)` — note
        // this is the RIOT client token, not the LCU remoting token.
        let http = Arc::new(LcuHttp::new(port, basic_auth_for_riot(token)));
        let mut guard = self.cached.lock().unwrap();
        *guard = Some(Cached {
            port,
            token: token.to_string(),
            http: http.clone(),
        });
        http
    }

    /// The transport `riftflow://riot-client/*` proxies onto, or `None` when the
    /// client is not running (`RiotClientRcuUninitializedError` in the original).
    pub(crate) fn http(&self, league_client: &LeagueClientMain) -> Option<Arc<LcuHttp>> {
        let auth = league_client
            .state
            .auth_public()
            .filter(|auth| auth.riot_client_port > 0 && !auth.riot_client_auth_token.is_empty())?;

        Some(self.http_for(auth.riot_client_port, &auth.riot_client_auth_token))
    }

    /// `requestForRenderer` resolves with the stripped axios response even for
    /// HTTP error statuses (axios' rejection carries `error.response`, which the
    /// catch path returns); only transport failures propagate as throws.
    pub async fn request_for_renderer(
        &self,
        league_client: &LeagueClientMain,
        args: &[Value],
    ) -> IpcResult<Value> {
        let http = self.http(league_client).ok_or_else(|| {
            // the original throws inside `request` / dereferences a null
            // axios instance; both surface as an unresolved rejection
            IpcError::business("RC Uninitialized", RCU_UNINITIALIZED_CODE)
        })?;

        let config =
            RequestConfig::from_json(args.first().unwrap_or(&Value::Null)).map_err(IpcError::plain)?;

        http.request_for_renderer(config)
            .await
            .map_err(|error| {
                let message = error.to_string();
                match parse_status(&message) {
                    Some(status) => IpcError::http(message, Some(status), None),
                    None => IpcError::plain(message),
                }
            })
    }
}

fn basic_auth_for_riot(token: &str) -> String {
    use base64::Engine;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("riot:{token}"))
    )
}

/// Same axios-error reconstruction `riftflow_lcu::shard::classify_request_error`
/// does for the LCU path — the renderer reads `error.response.status`.
fn parse_status(message: &str) -> Option<i64> {
    message
        .split("status ")
        .nth(1)
        .and_then(|tail| tail.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|digits| digits.parse::<i64>().ok())
}

pub fn register(
    ipc: &Arc<RiftFlowIpcMain>,
    league_client: &Arc<LeagueClientMain>,
    service: &Arc<RiotClientService>,
) -> anyhow::Result<()> {
    let client = league_client.clone();
    let service = service.clone();
    ipc.on_call(
        RIOT_CLIENT_MAIN_NAMESPACE,
        "http-request",
        handler(move |_ctx, args| {
            let client = client.clone();
            let service = service.clone();
            async move { service.request_for_renderer(&client, &args).await }
        }),
    )
}
