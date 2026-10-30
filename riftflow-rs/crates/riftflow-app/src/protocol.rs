//! Port of `riftflow-protocol-main` (`protocol-router.ts` plus the domain handlers
//! the shards register on it): the `riftflow://` proxy the renderer uses for LCU
//! HTTP, Riot Client HTTP, the RiftFlow API and local files.
//!
//! Two entry points share one core, because WebView2 only routes a custom scheme
//! through Tauri's protocol handler while the document is still loading — after
//! that the request dies in the network stack (measured, not assumed):
//! * [`handle`] is Tauri's `riftflow` uri-scheme protocol, used for navigations and
//!   resource loads (an `<img src="riftflow://…">`);
//! * [`fetch`] is a command the renderer shim calls for `fetch`/XHR traffic, so
//!   the app's LCU data never depends on scheme interception at all.
//!
//! Either way the `domain` / `uri` split is the one `protocol-router.ts:55` makes,
//! and the response shape is the original's: status, string-valued headers, and no
//! body for the codes that must not carry one.

use std::sync::Arc;

use base64::Engine as _;
use serde_json::{Map, Value, json};
use tauri::http::{HeaderName, HeaderValue, Response, StatusCode};
use tauri::{AppHandle, Manager, UriSchemeContext, UriSchemeResponder, Wry};

use riftflow_lcu::http::{LcuHttp, LcuRawResponse, RequestConfig};

/// `RIFTFLOW_PROXY_REQUEST_ID_HEADER` — never forwarded to the LCU.
const REQUEST_ID_HEADER: &str = "x-riftflow-proxy-request-id";

/// `RiftFlowProtocolProxyRequestCancellation._controllers`, in the same shape: the
/// renderer's axios interceptor puts a request id on any `riftflow://` call that has
/// an `AbortSignal`, and calls `cancelProxyRequest(id)` when that signal fires.
/// Dropping the sender ends the request, exactly like aborting the controller.
static CANCELLATIONS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<()>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn take_registration(headers: &[(String, String)]) -> Option<(String, tokio::sync::oneshot::Receiver<()>)> {
    let id = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(REQUEST_ID_HEADER))
        .map(|(_, value)| value.clone())?;

    let (sender, receiver) = tokio::sync::oneshot::channel();
    CANCELLATIONS.lock().unwrap().insert(id.clone(), sender);

    Some((id, receiver))
}

/// `RiftFlowProtocolProxyRequestCancellation.cancel` — `false` for an id that is no
/// longer in flight, which is what the original answers too.
pub fn cancel(request_id: &str) -> bool {
    match CANCELLATIONS.lock().unwrap().remove(request_id) {
        Some(sender) => {
            drop(sender);
            true
        }
        None => false,
    }
}

fn dispose(request_id: &str) {
    CANCELLATIONS.lock().unwrap().remove(request_id);
}

/// A proxied answer, before it becomes an HTTP response or an IPC payload.
pub struct Proxied {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Proxied {
    /// The aborted case. 499 is nginx's "client closed request"; nothing on the
    /// renderer's side pretends this was a real response.
    fn aborted() -> Self {
        Self {
            status: 499,
            headers: cors_headers(),
            body: b"aborted".to_vec(),
        }
    }
}

/// `riftflow-protocol-main/cancelProxyRequest`.
pub fn register_ipc(ipc: &Arc<riftflow_ipc::RiftFlowIpcMain>) -> anyhow::Result<()> {
    ipc.on_call(
        "riftflow-protocol-main",
        "cancelProxyRequest",
        riftflow_ipc::handler(|_ctx, args| async move {
            let request_id = args.first().and_then(Value::as_str).unwrap_or_default();
            Ok(json!(cancel(request_id)))
        }),
    )
}

pub fn handle(
    ctx: UriSchemeContext<'_, Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    if std::env::var("RIFTFLOW_IPC_TRACE").is_ok() {
        eprintln!("[protocol] {} {}", request.method(), request.uri());
    }
    let method = request.method().to_string();
    let uri = request.uri().to_string();
    let headers = request
        .headers()
        .iter()
        .map(|(name, value)| (name.as_str().to_string(), value.to_str().unwrap_or("").to_string()))
        .collect();
    let body = request.body().clone();

    tauri::async_runtime::spawn(async move {
        let proxied = dispatch(&app, &method, &uri, headers, body).await;
        responder.respond(to_http_response(proxied));
    });
}

/// The command form of the same proxy: `{ status, statusText, headers, body }`,
/// with the body base64 so binary assets survive.
pub async fn fetch(
    app: &AppHandle,
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
) -> Result<Value, String> {
    let bytes = match body {
        Some(encoded) => base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| error.to_string())?,
        None => Vec::new(),
    };

    if std::env::var("RIFTFLOW_IPC_TRACE").is_ok() {
        eprintln!("[protocol-ipc] {method} {url}");
    }

    let proxied = dispatch(app, &method, &url, headers, bytes).await;

    // the shim turns this rejection into the `AbortError` axios expects
    if proxied.status == 499 {
        return Err("aborted".to_string());
    }

    let status_text = StatusCode::from_u16(proxied.status)
        .ok()
        .and_then(|code| code.canonical_reason())
        .unwrap_or_default()
        .to_string();

    Ok(json!({
        "status": proxied.status,
        "statusText": status_text,
        "headers": proxied.headers,
        "body": base64::engine::general_purpose::STANDARD.encode(&proxied.body),
    }))
}

fn to_http_response(proxied: Proxied) -> Response<Vec<u8>> {
    let mut builder = Response::builder().status(proxied.status);
    for (name, value) in &proxied.headers {
        if let (Ok(header), Ok(parsed)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            builder = builder.header(header, parsed);
        }
    }

    builder
        .body(proxied.body)
        .expect("protocol response")
}

async fn dispatch(
    app: &AppHandle,
    method: &str,
    url: &str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> Proxied {
    // `protocol-router.ts` registers the cancellation before the handler runs and
    // disposes it in a `finally`, so an id is only live while its request is
    let registration = take_registration(&headers);

    let proxied = match registration {
        None => dispatch_inner(app, method, url, headers, body).await,
        Some((id, cancelled)) => {
            let outcome = tokio::select! {
                response = dispatch_inner(app, method, url, headers, body) => Some(response),
                _ = cancelled => None,
            };
            dispose(&id);
            outcome.unwrap_or_else(Proxied::aborted)
        }
    };

    if proxied.status >= 400 {
        let snippet: String = String::from_utf8_lossy(&proxied.body).chars().take(200).collect();
        // Unconditional, and through the file logger: an elevated instance exposes no
        // CDP, so without this a proxied call that the client refuses is completely
        // invisible and a button that "does nothing, no error" cannot be diagnosed.
        crate::logger::log(
            "riftflow-protocol-main",
            crate::logger::Level::Warn,
            &format!("{method} {url} -> {} {snippet}", proxied.status),
        );
    }

    proxied
}

async fn dispatch_inner(
    app: &AppHandle,
    method: &str,
    url: &str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> Proxied {
    // `protocol-router.ts`: strip the scheme, then split host from the rest
    let after_scheme = url
        .split_once("://")
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| url.to_string());
    let (domain, rest) = match after_scheme.split_once('/') {
        Some((head, tail)) => (head, tail),
        None => (after_scheme.as_str(), ""),
    };
    // the localhost form Tauri hands back keeps the domain in the path
    let (domain, rest) = if domain.eq_ignore_ascii_case("localhost") {
        match rest.split_once('/') {
            Some((head, tail)) => (head, tail.to_string()),
            None => (rest, String::new()),
        }
    } else {
        (domain, rest.to_string())
    };

    let target = match rest.split_once('?') {
        Some((path, query)) => (path.to_string(), Some(query.to_string())),
        None => (rest, None),
    };

    if method.eq_ignore_ascii_case("OPTIONS") {
        return Proxied {
            status: 204,
            headers: cors_headers(),
            body: Vec::new(),
        };
    }

    match domain {
        "league-client" => {
            // Champion portraits are public static data, usable before the client
            // starts. They are embedded with the renderer in the standalone EXE.
            if method.eq_ignore_ascii_case("GET") {
                if let Some(id) = target.0.strip_prefix("lol-game-data/assets/v1/champion-icons/")
                    .and_then(|name| name.strip_suffix(".png"))
                    .and_then(|id| id.parse::<i64>().ok())
                {
                    if let Some(asset) = app.asset_resolver().get(format!("game-data/champion-icons/{id}.png")) {
                        let mut headers = cors_headers();
                        let mime = if asset.bytes.starts_with(b"\xff\xd8") {
                            "image/jpeg"
                        } else if asset.bytes.starts_with(b"RIFF") {
                            "image/webp"
                        } else {
                            "image/png"
                        };
                        headers.push(("content-type".into(), mime.into()));
                        headers.push(("cache-control".into(), "public, max-age=86400".into()));
                        return Proxied { status: 200, headers, body: asset.bytes };
                    }
                }
            }
            let http = app.state::<crate::AppState>().league_client.http();
            match http {
                None => Proxied::json(
                    503,
                    json!({ "error": "LeagueClientLcuUninitializedError" }),
                ),
                Some(http) => proxy_lcu(&http, method, &target, headers, body).await,
            }
        }
        "local" => local_file(&target.0),
        // the SGP gateway: per-region base url + bearer token, resolved by rules in
        // `riftflow_sgp`, with the connectivity counters kept in `sgp_service`
        "sgp" => {
            let gateway = app.state::<crate::AppState>().sgp_gateway.clone();
            let path = match &target.1 {
                Some(query) => format!("/{}?{query}", target.0),
                None => format!("/{}", target.0),
            };
            crate::sgp_service::request(app, &gateway, method, &path, headers, body).await
        }
        // `app-common/renderer-link-protocol.ts`: every hit is reported to the
        // renderer, `/evaluate` runs code in a target window, and the answer is
        // always an empty 204
        "renderer-link" => {
            let state = app.state::<crate::AppState>();
            state
                .ipc
                .send_event("app-common-main", "renderer-link", &[json!(url)]);

            if target.0 == "evaluate" {
                let params = query_params(target.1.as_deref().unwrap_or_default());
                match (params.get("target"), params.get("code")) {
                    (Some(t), Some(code)) => evaluate_in(&app, t, code),
                    _ => {}
                }
            }

            Proxied {
                status: 204,
                headers: cors_headers(),
                body: Vec::new(),
            }
        }
        // `riot-client/protocol-controller.ts`: same proxy as the LCU arm, but on
        // the Riot Client's own port with the `riot:` basic auth, and a 503 naming
        // `RiotClientRcuUninitializedError` while the client is not running
        "riot-client" => {
            let state = app.state::<crate::AppState>();
            match state.riot_client.http(&state.league_client) {
                None => Proxied::json(
                    503,
                    json!({ "error": crate::riot_client::RCU_UNINITIALIZED_CODE }),
                ),
                Some(http) => proxy_lcu(&http, method, &target, headers, body).await,
            }
        }
        // `riftflow-api/protocol-controller.ts`: the endpoint table decides whether a
        // hit is answered locally or forwarded to the (possibly bootstrapped) hosts
        "riftflow-api" | "riftflow-static" => {
            let state = app.state::<crate::AppState>();
            crate::riftflow_api_main::proxy(&state.riftflow_api, method, url, headers, body).await
        }
        // every domain the renderer uses has an arm above; anything else is a
        // made-up host, so say so rather than answering with an empty 200
        _ => Proxied::text(
            404,
            &format!("No handler for {url}"),
        ),
    }
}

/// The `?a=b&c=d` part of a renderer-link url, decoded once.
fn query_params(query: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(
            percent_decode(key),
            percent_decode(value),
        );
    }

    out
}

/// `webContents.executeJavaScript(code)` on the addressed window. The original
/// also allows `target=main`, which runs arbitrary JS *in the main process* and
/// only in a dev build — there is no Rust equivalent, and pretending otherwise
/// would be worse than saying so.
fn evaluate_in(app: &AppHandle, target: &str, code: &str) {
    if target == "main" {
        eprintln!("[protocol] renderer-link target=main 不支持：Rust 侧没有主进程执行任意 JS 的等价能力");
        return;
    }

    match app.get_webview_window(target) {
        Some(window) => {
            if let Err(error) = window.eval(code) {
                eprintln!("[protocol] renderer-link evaluate {target} 失败: {error}");
            }
        }
        None => eprintln!("[protocol] renderer-link evaluate: 没有 {target} 窗口"),
    }
}

/// Endpoints the 国服 (Tencent) client does not expose under the global path the
/// renderer asks for, as `(method, as-called, as-exposed)`.
///
/// The shipped UI bundle is byte-identical to the original, so a client-side difference
/// like this has to be absorbed here: 秒退 calls `POST /login/dodge`, which this client
/// answers `404 Invalid URI format` — the action exists, under
/// `/lol-gameflow/v1/session/dodge` (measured: it answers GET with
/// `405 WRONG_METHOD`, while paths it does not know answer `404`).
const LCU_URI_FALLBACKS: &[(&str, &str, &str)] = &[
    (
        "POST",
        "login/dodge",
        "lol-gameflow/v1/session/dodge",
    ),
];

/// The client's own "this URI is not in my router" answer, as opposed to a 404 from a
/// real handler (`No active delegate`) — only the former means "try the alternate".
fn unknown_uri(status: u16, body: &[u8]) -> bool {
    status == 404 && String::from_utf8_lossy(body).contains("Invalid URI format")
}

fn uri_fallback(method: &str, path: &str) -> Option<&'static str> {
    LCU_URI_FALLBACKS
        .iter()
        .find(|(m, p, _)| m.eq_ignore_ascii_case(method) && *p == path.trim_start_matches('/'))
        .map(|(_, _, alternate)| *alternate)
}

async fn proxy_lcu(
    http: &LcuHttp,
    method: &str,
    target: &(String, Option<String>),
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> Proxied {
    let url = match &target.1 {
        Some(query) => format!("/{}?{query}", target.0),
        None => format!("/{}", target.0),
    };

    let config = RequestConfig {
        method: method.to_string(),
        url,
        data: json_body(&body),
        params: Map::new(),
        headers: forward_headers(headers),
        // the body is handed through untouched, so responseType never applies here
        response_type: None,
    };

    match http.request_bytes(config.clone()).await {
        Ok(raw) => {
            let alternate = unknown_uri(raw.status, &raw.bytes)
                .then(|| uri_fallback(method, &target.0))
                .flatten();

            let raw = match alternate {
                Some(path) => {
                    crate::logger::log(
                        "riftflow-protocol-main",
                        crate::logger::Level::Warn,
                        &format!(
                            "{method} /{} 本机客户端不识别，改用 /{path} 重试",
                            target.0
                        ),
                    );

                    match http
                        .request_bytes(RequestConfig {
                            url: format!("/{path}"),
                            ..config
                        })
                        .await
                    {
                        Ok(retry) => retry,
                        Err(error) => {
                            crate::logger::log(
                                "riftflow-protocol-main",
                                crate::logger::Level::Warn,
                                &format!("{method} /{path} 重试失败: {error}"),
                            );
                            raw
                        }
                    }
                }
                None => raw,
            };

            if std::env::var("RIFTFLOW_LCU_TRACE").is_ok() {
                eprintln!(
                    "[protocol] {method} riftflow://league-client/… → {} ({} bytes)",
                    raw.status,
                    raw.bytes.len()
                );
            }
            Proxied::from_raw(raw)
        }
        Err(error) => {
            eprintln!("[protocol] league-client request failed: {error}");
            Proxied::text(500, &error.to_string())
        }
    }
}

impl Proxied {
    pub(crate) fn new(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        let mut headers = headers;
        headers.extend(cors_headers());

        // `shouldNotHaveBody` keeps the status and drops the body, like the original
        Self {
            status,
            headers,
            body: if should_not_have_body(status) {
                Vec::new()
            } else {
                body
            },
        }
    }

    fn from_raw(raw: LcuRawResponse) -> Self {
        let mut headers = Vec::new();
        for (key, value) in &raw.headers {
            // only string-valued headers are copied, like the original's filter
            if let Some(text) = value.as_str() {
                if !is_hop_by_hop(key) {
                    headers.push((key.clone(), text.to_string()));
                }
            }
        }
        headers.extend(cors_headers());

        let body = if should_not_have_body(raw.status) {
            Vec::new()
        } else {
            raw.bytes
        };

        // reqwest already decoded gzip, so the two headers describing the *wire*
        // form of the body would now be a lie: `content-encoding` would make the
        // caller decode again, and the original length would truncate it
        headers.retain(|(name, _)| !matches!(name.to_ascii_lowercase().as_str(), "content-encoding" | "content-length"));
        if !body.is_empty() {
            headers.push(("content-length".to_string(), body.len().to_string()));
        }

        Self {
            status: raw.status,
            headers,
            body,
        }
    }

    pub(crate) fn text(status: u16, text: &str) -> Self {
        Self {
            status,
            headers: vec![
                ("content-type".to_string(), "text/plain".to_string()),
                (
                    "access-control-allow-origin".to_string(),
                    "*".to_string(),
                ),
            ],
            body: text.as_bytes().to_vec(),
        }
    }

    fn json(status: u16, value: Value) -> Self {
        Self {
            status,
            headers: vec![
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "access-control-allow-origin".to_string(),
                    "*".to_string(),
                ),
            ],
            body: value.to_string().into_bytes(),
        }
    }
}

fn cors_headers() -> Vec<(String, String)> {
    vec![
        (
            "access-control-allow-origin".to_string(),
            "*".to_string(),
        ),
        (
            "access-control-allow-headers".to_string(),
            "*".to_string(),
        ),
        (
            "access-control-allow-methods".to_string(),
            "GET, POST, PUT, PATCH, DELETE, OPTIONS, HEAD".to_string(),
        ),
    ]
}

fn json_body(body: &[u8]) -> Option<Value> {
    if body.is_empty() {
        return None;
    }
    Some(serde_json::from_slice(body).unwrap_or_else(|_| {
        Value::String(String::from_utf8_lossy(body).to_string())
    }))
}

fn forward_headers(headers: Vec<(String, String)>) -> Map<String, Value> {
    let mut out = Map::new();
    for (name, value) in headers {
        if name.eq_ignore_ascii_case(REQUEST_ID_HEADER) || is_hop_by_hop(&name) {
            continue;
        }
        out.insert(name, Value::String(value));
    }
    out
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "content-length"
            | "connection"
            | "transfer-encoding"
            | "accept-encoding"
            | "authorization"
    )
}

fn should_not_have_body(status: u16) -> bool {
    (100..200).contains(&status) || status == 204 || status == 205 || status == 304
}

/// `createLocalFileDomainHandler()`: read the file, answer with its mime type, and
/// map the two error codes the original distinguishes.
fn local_file(path: &str) -> Proxied {
    let decoded = percent_decode(path);

    match std::fs::read(&decoded) {
        Ok(bytes) => Proxied {
            status: 200,
            headers: vec![
                ("content-type".to_string(), mime_for(&decoded).to_string()),
                (
                    "access-control-allow-origin".to_string(),
                    "*".to_string(),
                ),
            ],
            body: bytes,
        },
        Err(error) => {
            let status = match error.kind() {
                std::io::ErrorKind::NotFound => 404,
                std::io::ErrorKind::PermissionDenied => 403,
                _ => 500,
            };
            Proxied::json(
                status,
                json!({ "error": error.to_string(), "filepath": decoded }),
            )
        }
    }
}

/// `mime-types.lookup`, narrowed to what actually ships through this domain.
fn mime_for(path: &str) -> &'static str {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "json" => "application/json",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "html" => "text/html",
        "txt" => "text/plain",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// `decodeURIComponent` for the absolute paths the app puts in this domain.
pub(crate) fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &input[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte as char);
                index += 3;
                continue;
            }
        }
        // multi-byte UTF-8 is copied through, character by character
        let ch = input[index..]
            .chars()
            .next()
            .unwrap_or('\u{fffd}');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{unknown_uri, uri_fallback};

    #[test]
    fn the_cn_dodge_path_is_substituted() {
        assert_eq!(
            uri_fallback("POST", "login/dodge"),
            Some("lol-gameflow/v1/session/dodge")
        );
        // the renderer's axios `url` arrives with a leading slash, and the method is
        // whatever case the caller used
        assert_eq!(
            uri_fallback("post", "/login/dodge"),
            Some("lol-gameflow/v1/session/dodge")
        );
        assert_eq!(uri_fallback("GET", "login/dodge"), None);
        assert_eq!(uri_fallback("POST", "lol-login/v1/session"), None);
    }

    #[test]
    fn only_the_router_s_own_unknown_uri_retries() {
        let unknown =
            br#"{"errorCode":"RESOURCE_NOT_FOUND","httpStatus":404,"message":"Invalid URI format"}"#;
        let no_delegate =
            br#"{"errorCode":"RPC_ERROR","httpStatus":404,"message":"No active delegate"}"#;

        assert!(unknown_uri(404, unknown));
        // a real handler that is simply not available right now must not be re-pointed
        assert!(!unknown_uri(404, no_delegate));
        assert!(!unknown_uri(405, unknown));
    }
}
