//! The two `riftflow://` domains the shard registers, and the pure routing decision
//! `riftflow-app/src/protocol.rs` makes from a URL.
//!
//! **What each domain serves** (`protocol-controller.ts` registered both against
//! one generic passthrough; this port names the endpoints instead):
//!
//! * `riftflow://riftflow-api/<path>?<query>` — the JSON API on the *api* host
//!   (`https://api.yuru-yuri…`, i.e. `DEFAULT_RIFTFLOW_API_BASE_URL`, replaced by the
//!   bootstrap document's `baseUrls.api` when one is known). Exactly six paths, in
//!   the shape the renderer's `RiftFlowApiHttpApiAxiosHelper` asks for:
//!   `config/v1/app/feature-gates`, `config/v1/auto-select/groups`,
//!   `config/v1/sgp/league-servers`, `config/v1/sgp/supported-queues`,
//!   `notice/v1/latest?lang=`, `releases/v1/latest?lang=`,
//!   `website/v1/contact-channels` and `statistics/v1/records` (POST).
//!   Anything else is a local 404 — the original forwarded an arbitrary path, which
//!   also let `riftflow://riftflow-api/https://evil.example` leave the host entirely.
//! * `riftflow://riftflow-static/<path>` — the file host (`baseUrls.static`), for the
//!   assets and release artifacts the API only names. Any path is forwarded
//!   (percent-encoded on the way out), and an authoritative `https://` path is used
//!   verbatim, matching `resolveRiftFlowStaticUrl`; a non-http(s) scheme is refused.
//!
//! The split itself is `protocol-router.ts:55-58` — strip `riftflow://`, cut at the
//! first `/`, `trim()` both halves — including the `riftflow://localhost/<domain>/…`
//! form `protocol.rs` handles for Tauri.

use crate::config::{
    RiftFlowServiceBaseUrls, ConfigResource, RIFTFLOW_API_PROTOCOL_DOMAIN, RIFTFLOW_PROXY_PROTOCOL,
    RIFTFLOW_STATIC_PROTOCOL_DOMAIN,
};

/// Which of this shard's two domains a URL belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiftFlowUrlDomain {
    RiftFlowApi,
    RiftFlowStatic,
}

impl RiftFlowUrlDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            RiftFlowUrlDomain::RiftFlowApi => RIFTFLOW_API_PROTOCOL_DOMAIN,
            RiftFlowUrlDomain::RiftFlowStatic => RIFTFLOW_STATIC_PROTOCOL_DOMAIN,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            RIFTFLOW_API_PROTOCOL_DOMAIN => Some(RiftFlowUrlDomain::RiftFlowApi),
            RIFTFLOW_STATIC_PROTOCOL_DOMAIN => Some(RiftFlowUrlDomain::RiftFlowStatic),
            _ => None,
        }
    }

    /// `Accept: application/json`, which only the API instance sets.
    pub fn accept_json(self) -> bool {
        matches!(self, RiftFlowUrlDomain::RiftFlowApi)
    }
}

/// The body of a locally decided response. For a forwarded request the body is
/// whatever upstream sends back, so it is not decided here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteBody {
    Empty,
    Text(String),
}

/// What `riftflow-app` should answer with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RiftFlowRoute {
    /// Send this request upstream and mirror status, headers and bytes back.
    Forward {
        domain: RiftFlowUrlDomain,
        /// the absolute URL to fetch, query included
        target_url: String,
        /// whether to add `Accept: application/json`
        accept_json: bool,
        /// the endpoint that matched, for method checks and logging
        endpoint: Option<RiftFlowApiEndpoint>,
    },
    /// Answer without touching the network.
    Reply {
        status: u16,
        content_type: &'static str,
        body: RouteBody,
    },
    /// Not one of these two domains — `league-client`, `riot-client`, `local`,
    /// `sgp` and friends stay with the app's other handlers.
    NotOurs,
}

impl RiftFlowRoute {
    /// The `(status, headers, body)` a locally decided reply carries, in the shape
    /// `protocol.rs` wants for a `Proxied` (it adds its own CORS headers). `None`
    /// when the answer is upstream or belongs to another domain.
    pub fn local_parts(&self) -> Option<(u16, Vec<(String, String)>, Vec<u8>)> {
        let RiftFlowRoute::Reply {
            status,
            content_type,
            body,
        } = self
        else {
            return None;
        };

        Some((
            *status,
            vec![(
                "content-type".to_string(),
                (*content_type).to_string(),
            )],
            match body {
                RouteBody::Empty => Vec::new(),
                RouteBody::Text(text) => text.as_bytes().to_vec(),
            },
        ))
    }
}

/// The allowlisted API endpoints behind the `riftflow-api` domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiftFlowApiEndpoint {
    Config(ConfigResource),
    LatestNotice,
    LatestRelease,
    ContactChannels,
    StatisticsRecords,
}

impl RiftFlowApiEndpoint {
    /// The methods the shipped renderer uses for this endpoint. `statistics` is
    /// the only writer; everything else is a read.
    pub fn supports_method(self, method: &str) -> bool {
        let method = method.to_ascii_uppercase();
        match self {
            RiftFlowApiEndpoint::StatisticsRecords => method == "POST",
            _ => method == "GET" || method == "HEAD",
        }
    }
}

/// `api.getConfig(resource)` / `getLatestNotice()` / … — the paths behind the
/// `riftflow-api` domain, as one matcher so the router and the client agree.
pub fn riftflow_api_endpoint(path: &str) -> Option<RiftFlowApiEndpoint> {
    let normalized = normalize_path(path);
    match normalized.as_str() {
        "notice/v1/latest" => return Some(RiftFlowApiEndpoint::LatestNotice),
        "releases/v1/latest" => return Some(RiftFlowApiEndpoint::LatestRelease),
        "website/v1/contact-channels" => return Some(RiftFlowApiEndpoint::ContactChannels),
        "statistics/v1/records" => return Some(RiftFlowApiEndpoint::StatisticsRecords),
        _ => {}
    }

    if normalized.starts_with("config/v1/") {
        return ConfigResource::from_path(&normalized).map(RiftFlowApiEndpoint::Config);
    }

    None
}

/// The whole routing decision for one `riftflow://` URL. Side-effect free, so the
/// dispatcher's table can be tested without a network or a webview.
pub fn route(url: &str, base_urls: &RiftFlowServiceBaseUrls) -> RiftFlowRoute {
    let Some((domain, rest)) = split_url(url) else {
        return RiftFlowRoute::NotOurs;
    };
    let Some(domain) = RiftFlowUrlDomain::from_name(domain.trim()) else {
        return RiftFlowRoute::NotOurs;
    };

    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path.to_string(), Some(query.to_string())),
        None => (rest.to_string(), None),
    };
    let query = query.filter(|query| !query.is_empty());

    if path.trim().is_empty() {
        return bad_request(domain, "url has no path");
    }

    match domain {
        RiftFlowUrlDomain::RiftFlowApi => route_api(&path, query.as_deref(), &base_urls.api),
        RiftFlowUrlDomain::RiftFlowStatic => route_static(&path, query.as_deref(), &base_urls.static_assets),
    }
}

fn route_api(path: &str, query: Option<&str>, base: &str) -> RiftFlowRoute {
    // the original let axios treat an absolute-looking path as its own target;
    // pinned to the API host here, so a stray path cannot leave it
    if is_absolute_url(path) {
        return json_error(
            400,
            "InvalidRiftFlowApiUrl",
            &format!("riftflow-api path must be relative, got {path}"),
        );
    }

    let decoded = percent_decode(path);
    if decoded.contains(['\0', '\r', '\n', '\\', '#']) {
        return json_error(
            400,
            "InvalidRiftFlowApiUrl",
            &format!("riftflow-api path has forbidden characters: {path}"),
        );
    }

    let Some(endpoint) = riftflow_api_endpoint(&decoded) else {
        return json_error(
            404,
            "UnknownRiftFlowApiPath",
            &format!("no riftflow-api endpoint for /{}", normalize_path(&decoded)),
        );
    };

    RiftFlowRoute::Forward {
        domain: RiftFlowUrlDomain::RiftFlowApi,
        // re-encoded after decoding, so a `%20` in the request cannot become a
        // literal space in the outbound URI
        target_url: join_query(
            combine_url(base, &percent_encode_path(&normalize_path(&decoded))),
            query,
        ),
        accept_json: RiftFlowUrlDomain::RiftFlowApi.accept_json(),
        endpoint: Some(endpoint),
    }
}

fn route_static(path: &str, query: Option<&str>, base: &str) -> RiftFlowRoute {
    let decoded = percent_decode(path);

    // `resolveRiftFlowStaticUrl`: an authoritative absolute URL wins, but only over
    // http(s) — a `file://` or `javascript:` target is not this domain's job
    if is_absolute_url(&decoded) {
        let scheme = decoded.split_once("://").map(|(scheme, _)| scheme.to_ascii_lowercase());
        return match scheme.as_deref() {
            Some("https") | Some("http") => RiftFlowRoute::Forward {
                domain: RiftFlowUrlDomain::RiftFlowStatic,
                target_url: join_query(decoded.clone(), query),
                accept_json: false,
                endpoint: None,
            },
            _ => json_error(
                400,
                "InvalidRiftFlowStaticUrl",
                &format!("unsupported scheme for {path}"),
            ),
        };
    }

    if decoded.contains(['\0', '\r', '\n', '\\', '#']) {
        return json_error(
            400,
            "InvalidRiftFlowStaticUrl",
            &format!("riftflow-static path has forbidden characters: {path}"),
        );
    }

    let normalized = normalize_path(&decoded);
    if normalized.is_empty() {
        return bad_request(RiftFlowUrlDomain::RiftFlowStatic, "url has no file name");
    }

    RiftFlowRoute::Forward {
        domain: RiftFlowUrlDomain::RiftFlowStatic,
        target_url: join_query(combine_url(base, &percent_encode_path(&normalized)), query),
        accept_json: false,
        endpoint: None,
    }
}

/// `protocol-router.ts:55` — the domain / uri split, including the
/// `riftflow://localhost/<domain>/…` form Tauri hands the shell.
pub fn split_url(url: &str) -> Option<(&str, &str)> {
    // only this scheme: an `https://riftflow-api…` URL is not the proxy's business
    let after_scheme = url
        .strip_prefix(&format!("{RIFTFLOW_PROXY_PROTOCOL}://"))
        .or_else(|| url.strip_prefix(&format!("{RIFTFLOW_PROXY_PROTOCOL}:/")))?;
    let (head, tail) = match after_scheme.split_once('/') {
        Some((head, tail)) => (head, tail),
        None => (after_scheme, ""),
    };

    Some(if head.eq_ignore_ascii_case("localhost") {
        match tail.split_once('/') {
            Some((head, tail)) => (head, tail),
            None => (tail, ""),
        }
    } else {
        (head, tail)
    })
}

/// axios `combineURLs`: the base loses its trailing slashes, the path its leading
/// ones, and exactly one `/` joins them.
pub fn combine_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// `resolveRiftFlowStaticUrl` — `new URL(path, `${base}/`)`: an absolute path is the
/// answer, a relative one joins the base and gets encoded.
pub fn resolve_static_url(base: &str, path: &str) -> String {
    if is_absolute_url(path) {
        return path.to_string();
    }
    join_query(
        combine_url(base, &percent_encode_path(&normalize_path(path))),
        None,
    )
}

/// `new URL(path, base)` is not a plain join, so the query is glued back on after
/// the path has been rebuilt.
fn join_query(url: String, query: Option<&str>) -> String {
    match query {
        Some(query) => format!("{url}?{query}"),
        None => url,
    }
}

/// RFC 3986 `remove_dot_segments`, with the leading slashes dropped: `./a/../b`
/// and `%2e%2e` tricks both end up as what the upstream host would see.
pub fn normalize_path(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();

    for segment in path.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }

    segments.join("/")
}

/// axios `isAbsoluteURL`: `^([a-z][a-z\d+\-.]*:)?//` — note the scheme is optional,
/// so `//host/path` can change host as well.
pub fn is_absolute_url(text: &str) -> bool {
    if text.starts_with("//") {
        return true;
    }
    match text.split_once(':') {
        Some((scheme, rest)) => rest.starts_with("//") && is_scheme(scheme),
        None => false,
    }
}

fn is_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|char| char.is_ascii_alphanumeric() || matches!(char, '+' | '-' | '.'))
}

/// The characters a URL path may not carry literally. Everything else, including a
/// valid `%` escape, is left alone, so encoding twice cannot corrupt a name.
fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        if is_path_literal(byte) {
            out.push(byte as char);
            index += 1;
            continue;
        }
        // a real UTF-8 sequence is encoded byte by byte; ASCII goes straight out
        if byte.is_ascii() {
            out.push_str(&format!("%{byte:02X}"));
            index += 1;
            continue;
        }
        let character = path[index..].chars().next().unwrap_or('\u{fffd}');
        for raw in character.encode_utf8(&mut [0u8; 4]).as_bytes() {
            out.push_str(&format!("%{raw:02X}"));
        }
        index += character.len_utf8();
    }

    out
}

fn is_path_literal(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+'
                | b',' | b';' | b'=' | b':' | b'@' | b'/' | b'%'
        )
}

/// `decodeURIComponent` — the same tolerant version `protocol.rs` uses for the
/// `local` domain; an invalid escape is copied through. Bytes are collected first,
/// so a percent-encoded UTF-8 sequence decodes to one character and re-encodes to
/// exactly what came in.
pub fn percent_decode(input: &str) -> String {
    let raw = input.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut index = 0;

    while index < raw.len() {
        if raw[index] == b'%' && index + 2 < raw.len() {
            if let Ok(byte) = u8::from_str_radix(&input[index + 1..index + 3], 16) {
                bytes.push(byte);
                index += 3;
                continue;
            }
        }
        bytes.push(raw[index]);
        index += 1;
    }

    String::from_utf8_lossy(&bytes).into_owned()
}

fn bad_request(domain: RiftFlowUrlDomain, reason: &str) -> RiftFlowRoute {
    json_error(
        400,
        "InvalidRiftFlowUrl",
        &format!("{} proxy: {reason}", domain.as_str()),
    )
}

/// `RiftFlowApiErrorBody` — the shape the API host itself answers with, so the
/// renderer's error handling does not have to know this came from the shell.
fn json_error(status: u16, code: &str, message: &str) -> RiftFlowRoute {
    RiftFlowRoute::Reply {
        status,
        content_type: "application/json",
        body: RouteBody::Text(
            serde_json::json!({ "error": { "code": code, "message": message } }).to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_urls() -> RiftFlowServiceBaseUrls {
        RiftFlowServiceBaseUrls::default()
    }

    fn forwarded(route: &RiftFlowRoute) -> (&str, bool) {
        match route {
            RiftFlowRoute::Forward {
                target_url,
                accept_json,
                ..
            } => (target_url.as_str(), *accept_json),
            other => panic!("expected a forward, got {other:?}"),
        }
    }

    #[test]
    fn every_allowlisted_api_path_forwards_to_the_api_host() {
        for (url, expected) in [
            (
                "riftflow://riftflow-api/config/v1/app/feature-gates",
                "https://riftflow-api.yymainy.online/config/v1/app/feature-gates",
            ),
            (
                "riftflow://riftflow-api/config/v1/auto-select/groups",
                "https://riftflow-api.yymainy.online/config/v1/auto-select/groups",
            ),
            (
                "riftflow://riftflow-api/config/v1/sgp/league-servers",
                "https://riftflow-api.yymainy.online/config/v1/sgp/league-servers",
            ),
            (
                "riftflow://riftflow-api/notice/v1/latest?lang=en",
                "https://riftflow-api.yymainy.online/notice/v1/latest?lang=en",
            ),
            (
                "riftflow://riftflow-api/releases/v1/latest?lang=zh-CN",
                "https://riftflow-api.yymainy.online/releases/v1/latest?lang=zh-CN",
            ),
            (
                "riftflow://riftflow-api/website/v1/contact-channels",
                "https://riftflow-api.yymainy.online/website/v1/contact-channels",
            ),
            (
                "riftflow://riftflow-api/statistics/v1/records",
                "https://riftflow-api.yymainy.online/statistics/v1/records",
            ),
        ] {
            let route = route(url, &base_urls());
            assert_eq!(forwarded(&route).0, expected, "{url}");
            assert!(forwarded(&route).1, "{url} should ask for json");
        }
    }

    #[test]
    fn the_api_endpoint_matcher_names_the_allowlisted_paths() {
        assert_eq!(
            riftflow_api_endpoint("config/v1/app/feature-gates"),
            Some(RiftFlowApiEndpoint::Config(ConfigResource::FeatureGates))
        );
        assert_eq!(
            riftflow_api_endpoint("/config/v1/auto-select/groups"),
            Some(RiftFlowApiEndpoint::Config(ConfigResource::AutoSelectGroups))
        );
        assert_eq!(
            riftflow_api_endpoint("notice/v1/latest"),
            Some(RiftFlowApiEndpoint::LatestNotice)
        );
        assert_eq!(riftflow_api_endpoint("config/v1/unknown/thing"), None);
        assert_eq!(riftflow_api_endpoint(""), None);
    }

    #[test]
    fn only_statistics_is_a_writer() {
        assert!(RiftFlowApiEndpoint::StatisticsRecords.supports_method("post"));
        assert!(!RiftFlowApiEndpoint::StatisticsRecords.supports_method("GET"));
        assert!(RiftFlowApiEndpoint::LatestNotice.supports_method("GET"));
        assert!(!RiftFlowApiEndpoint::LatestNotice.supports_method("DELETE"));
    }

    #[test]
    fn an_encoded_slash_still_matches_the_allowlist() {
        let decision = route(
            "riftflow://riftflow-api/notice%2fv1/latest?lang=en",
            &base_urls(),
        );
        assert_eq!(
            forwarded(&decision).0,
            "https://riftflow-api.yymainy.online/notice/v1/latest?lang=en"
        );
    }

    #[test]
    fn an_unknown_api_path_is_a_local_json_404() {
        let route = route("riftflow://riftflow-api/admin/v1/secrets", &base_urls());
        let RiftFlowRoute::Reply {
            status,
            content_type,
            body,
        } = &route
        else {
            panic!("expected a local reply, got {route:?}");
        };

        assert_eq!(*status, 404);
        assert_eq!(*content_type, "application/json");
        let RouteBody::Text(body) = body else {
            panic!("a 404 carries a body");
        };
        let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(parsed["error"]["code"], json!("UnknownRiftFlowApiPath"));
        assert_eq!(
            parsed["error"]["message"],
            json!("no riftflow-api endpoint for /admin/v1/secrets")
        );
    }

    #[test]
    fn an_absolute_api_path_cannot_leave_the_host() {
        // axios would have followed this to evil.example; the port answers 400
        let route = route("riftflow://riftflow-api/https://evil.example/config", &base_urls());
        let RiftFlowRoute::Reply { status, body, .. } = &route else {
            panic!("expected a local reply, got {route:?}");
        };
        assert_eq!(*status, 400);
        let RouteBody::Text(body) = body else {
            unreachable!()
        };
        assert!(body.contains("InvalidRiftFlowApiUrl"), "{body}");
    }

    #[test]
    fn dot_segments_are_resolved_before_the_allowlist() {
        let escaped = route(
            "riftflow://riftflow-api/admin/../../config/v1/sgp/league-servers",
            &base_urls(),
        );
        assert_eq!(
            forwarded(&escaped).0,
            "https://riftflow-api.yymainy.online/config/v1/sgp/league-servers"
        );

        // escaping above the root cannot reach a different endpoint
        let clamped = route("riftflow://riftflow-api/../../notice/v1/latest", &base_urls());
        assert_eq!(
            forwarded(&clamped).0,
            "https://riftflow-api.yymainy.online/notice/v1/latest"
        );
    }

    #[test]
    fn a_path_with_control_characters_is_refused() {
        for url in [
            "riftflow://riftflow-api/config%00/v1",
            "riftflow://riftflow-api/\\notice/v1/latest",
            "riftflow://riftflow-api/notice/v1/latest#fragment",
        ] {
            let route = route(url, &base_urls());
            assert!(
                matches!(route, RiftFlowRoute::Reply { status: 400, .. }),
                "{url} → {route:?}"
            );
        }
    }

    #[test]
    fn the_static_domain_forwards_any_file() {
        let artifact = route(
            "riftflow://riftflow-static/RiftFlow%20Desktop-1.5.0-win.7z",
            &base_urls(),
        );
        assert_eq!(
            forwarded(&artifact).0,
            "https://riftflow-api.yymainy.online/static/RiftFlow%20Desktop-1.5.0-win.7z"
        );
        assert!(!forwarded(&artifact).1, "the file host asks for no particular type");

        let icon = route("riftflow://riftflow-static/icons/champion/103.png", &base_urls());
        assert_eq!(
            forwarded(&icon).0,
            "https://riftflow-api.yymainy.online/static/icons/champion/103.png"
        );
    }

    #[test]
    fn a_static_absolute_url_is_allowed_but_only_over_http() {
        let cdn = route(
            "riftflow://riftflow-static/https://cdn.example.com/file.7z",
            &base_urls(),
        );
        assert_eq!(forwarded(&cdn).0, "https://cdn.example.com/file.7z");

        let file = route("riftflow://riftflow-static/file:///C:/secrets", &base_urls());
        assert!(matches!(file, RiftFlowRoute::Reply { status: 400, .. }));
    }

    #[test]
    fn an_empty_path_neither_domain_can_serve() {
        for url in ["riftflow://riftflow-api", "riftflow://riftflow-api/", "riftflow://riftflow-static/"] {
            let route = route(url, &base_urls());
            assert!(
                matches!(route, RiftFlowRoute::Reply { status: 400, .. }),
                "{url} → {route:?}"
            );
        }
    }

    #[test]
    fn other_domains_are_left_to_the_app() {
        let urls = [
            "riftflow://league-client/lol-chat/v1/me",
            "riftflow://riot-client/rcl-auth-v1/user",
            "riftflow://local/C:/x/y.png",
            "riftflow://sgp/1/TENCENT_HN1/match-history",
            "riftflow:///riftflow-api/config/v1/app/feature-gates",
            "https://riftflow-api.yymainy.online/config/v1/app/feature-gates",
        ];
        for url in urls {
            assert_eq!(route(url, &base_urls()), RiftFlowRoute::NotOurs, "{url}");
        }
    }

    #[test]
    fn the_localhost_form_tauri_hands_back_routes_the_same() {
        let plain = route(
            "riftflow://riftflow-api/config/v1/app/feature-gates",
            &base_urls(),
        );
        let tauri = route(
            "riftflow://localhost/riftflow-api/config/v1/app/feature-gates",
            &base_urls(),
        );
        assert_eq!(plain, tauri);

        // and the spaces the original trims
        assert_eq!(
            forwarded(&route(
                "riftflow:// riftflow-api /config/v1/sgp/supported-queues",
                &base_urls()
            ))
            .0,
            "https://riftflow-api.yymainy.online/config/v1/sgp/supported-queues"
        );
    }

    #[test]
    fn a_bootstrap_generation_moves_both_domains() {
        let base_urls = RiftFlowServiceBaseUrls {
            api: "https://api-2.example.com/".to_string(),
            static_assets: "https://static-2.example.com".to_string(),
        };

        assert_eq!(
            forwarded(&route(
                "riftflow://riftflow-api/config/v1/app/feature-gates",
                &base_urls
            ))
            .0,
            "https://api-2.example.com/config/v1/app/feature-gates"
        );
        assert_eq!(
            forwarded(&route("riftflow://riftflow-static/a.png", &base_urls)).0,
            "https://static-2.example.com/a.png"
        );
    }

    #[test]
    fn the_url_split_matches_the_router() {
        assert_eq!(
            split_url("riftflow://riftflow-api/notice/v1/latest?lang=en"),
            Some(("riftflow-api", "notice/v1/latest?lang=en"))
        );
        assert_eq!(split_url("riftflow://riftflow-api"), Some(("riftflow-api", "")));
        assert_eq!(
            split_url("riftflow://localhost/sgp/1/EUW/x"),
            Some(("sgp", "1/EUW/x"))
        );
        assert_eq!(split_url("not-a-url"), None);
    }

    #[test]
    fn static_paths_are_encoded_the_way_new_url_does() {
        assert_eq!(
            resolve_static_url("https://riftflow-api.yymainy.online/static", "RiftFlow Desktop-1.5.0-win.7z"),
            "https://riftflow-api.yymainy.online/static/RiftFlow%20Desktop-1.5.0-win.7z"
        );
        assert_eq!(
            resolve_static_url(
                "https://riftflow-api.yymainy.online/static",
                "https://cdn.example.com/file.7z"
            ),
            "https://cdn.example.com/file.7z"
        );
        // an already encoded name is not encoded twice
        assert_eq!(
            resolve_static_url("https://h", "RiftFlow%20Desktop.7z"),
            "https://h/RiftFlow%20Desktop.7z"
        );
        assert_eq!(
            resolve_static_url("https://h", "公告/说明.md"),
            "https://h/%E5%85%AC%E5%91%8A/%E8%AF%B4%E6%98%8E.md"
        );
        assert_eq!(
            resolve_static_url("https://h/", "./deep/../a b.png"),
            "https://h/a%20b.png"
        );
    }

    #[test]
    fn url_helpers_cover_the_joining_rules() {
        assert_eq!(
            combine_url("https://h/x/", "/a/b"),
            "https://h/x/a/b",
            "axios strips the leading slash"
        );
        assert!(is_absolute_url("https://a/b"));
        assert!(is_absolute_url("//a/b"), "a scheme-relative url is absolute too");
        assert!(!is_absolute_url("/a/b"));
        assert!(!is_absolute_url("1https://a/b"));
        assert_eq!(normalize_path("/a/./b/../c/"), "a/c");
        assert_eq!(normalize_path("../.."), "");
        assert_eq!(percent_decode("%2e%2E/a%20b"), "../a b");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
