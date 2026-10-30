//! SGP transport rules, ported from `src/main/shards/sgp/http-client-controller.ts`.
//!
//! The renderer keeps issuing `riftflow://sgp/<path>` requests with two marker
//! headers; `plan()` turns one of those into the real per-region HTTPS call — the
//! same work the axios request interceptor did. Keeping it here means the rules
//! (which base URL, which token, how `{subId}` expands) are unit-testable without
//! a League client or a network.

use std::time::Duration;

use serde_json::Value;

pub const URL_PLACEHOLDER_SUB_ID: &str = "@riftflow:sgpServerSubId@";
pub const RIFTFLOW_HEADER_SGP_SERVER_ID: &str = "x-riftflow-sgp-server-id";
pub const RIFTFLOW_HEADER_TOKEN_TYPE: &str = "x-riftflow-token-type";
pub const RIFTFLOW_HEADER_FORCE_STREAM_COLLECT: &str = "x-riftflow-force-stream-collect";

/// Main-side state the interceptor reads, snapshotted per request so this crate
/// does not depend on the app's state container.
#[derive(Debug, Clone, Default)]
pub struct SgpSnapshot {
    /// `leagueServers.servers`
    pub servers: Value,
    /// `availability.sgpServerId`
    pub default_server_id: String,
    pub entitlements_token: Option<String>,
    pub league_session_token: Option<String>,
}

/// A request after the interceptor rules have been applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub base_url: String,
    pub path: String,
    pub authorization: Option<String>,
    pub forwarded: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct SgpReply {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum SgpError {
    /// the original throws on these (no token, unknown server, no base url)
    Rule(String),
    /// transport failure, which the response interceptor counts separately
    Transport(String),
}

impl SgpError {
    pub fn message(&self) -> &str {
        match self {
            SgpError::Rule(text) | SgpError::Transport(text) => text,
        }
    }
}

/// `_getSubId()`: a Tencent id carries the platform in its second half.
pub fn sub_id(server_id: &str) -> String {
    if server_id.starts_with("TENCENT") {
        return server_id
            .split('_')
            .nth(1)
            .unwrap_or_default()
            .to_string();
    }
    server_id.to_string()
}

fn header_lookup(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn is_riftflow_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        RIFTFLOW_HEADER_SGP_SERVER_ID | RIFTFLOW_HEADER_TOKEN_TYPE | RIFTFLOW_HEADER_FORCE_STREAM_COLLECT
    )
}

/// The three rules the request interceptor applies, as a pure function.
pub fn plan(
    snapshot: &SgpSnapshot,
    uri: &str,
    headers: &[(String, String)],
) -> Result<Planned, SgpError> {
    let server_id = header_lookup(headers, RIFTFLOW_HEADER_SGP_SERVER_ID)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| snapshot.default_server_id.clone());
    let token_type = header_lookup(headers, RIFTFLOW_HEADER_TOKEN_TYPE)
        .filter(|value| !value.is_empty());

    let authorization = match token_type.as_deref() {
        None => None,
        Some("entitlements") => Some(
            snapshot
                .entitlements_token
                .clone()
                .ok_or_else(|| SgpError::Rule("Token not found for type: entitlements".into()))?,
        ),
        Some("league-session") => Some(
            snapshot
                .league_session_token
                .clone()
                .ok_or_else(|| SgpError::Rule("Token not found for type: league-session".into()))?,
        ),
        Some(other) => {
            return Err(SgpError::Rule(format!("Token not found for type: {other}")));
        }
    };

    let server = snapshot
        .servers
        .get(&server_id)
        .filter(|value| !value.is_null())
        .ok_or_else(|| {
            SgpError::Rule(format!("Server config not found for sgp server ID: {server_id}"))
        })?;

    let fallback_sub_id = sub_id(&server_id);
    let path = uri.replace(
        URL_PLACEHOLDER_SUB_ID,
        server
            .get("regionPathParam")
            .and_then(Value::as_str)
            .unwrap_or(fallback_sub_id.as_str()),
    );

    let base_field = if token_type.as_deref() == Some("entitlements") {
        "matchHistory"
    } else {
        "common"
    };

    let base_url = server
        .get(base_field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            SgpError::Rule(format!(
                "Base URL not found for sgp server ID: {server_id}, requiredTokenType: {}",
                token_type.unwrap_or_default()
            ))
        })?
        .to_string();

    let forwarded = headers
        .iter()
        .filter(|(name, _)| !is_riftflow_header(name))
        .cloned()
        .collect();

    Ok(Planned {
        base_url,
        path,
        authorization: authorization.map(|token| format!("Bearer {token}")),
        forwarded,
    })
}

pub struct SgpGateway {
    client: reqwest::Client,
}

impl Default for SgpGateway {
    fn default() -> Self {
        Self::new()
    }
}

impl SgpGateway {
    /// `axios.create()` has no timeout, so neither does this — but a request that
    /// never resolves would hang the renderer's queue forever, so 30 s caps it.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .gzip(true)
                .build()
                .expect("reqwest client"),
        }
    }

    pub async fn request(
        &self,
        snapshot: &SgpSnapshot,
        method: &str,
        uri: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<SgpReply, SgpError> {
        let planned = plan(snapshot, uri, headers)?;
        let url = format!("{}{}", planned.base_url.trim_end_matches('/'), planned.path);

        let method = reqwest::Method::from_bytes(method.to_uppercase().as_bytes())
            .map_err(|_| SgpError::Rule(format!("Unsupported HTTP method: {method}")))?;

        let mut builder = self.client.request(method, &url);

        for (name, value) in &planned.forwarded {
            if is_hop_by_hop(name) {
                continue;
            }
            if let (Ok(header), Ok(parsed)) = (
                reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                builder = builder.header(header, parsed);
            }
        }

        if let Some(authorization) = &planned.authorization {
            builder = builder.header(reqwest::header::AUTHORIZATION, authorization);
        }

        if !body.is_empty() {
            builder = builder.body(body.to_vec());
        }

        let response = builder
            .send()
            .await
            .map_err(|error| SgpError::Transport(error.to_string()))?;

        let status = response.status().as_u16();
        let status_text = response
            .status()
            .canonical_reason()
            .unwrap_or_default()
            .to_string();

        let response_headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().to_string(), value.to_str().unwrap_or_default().to_string()))
            .collect();

        let body = response
            .bytes()
            .await
            .map_err(|error| SgpError::Transport(error.to_string()))?;

        Ok(SgpReply {
            status,
            status_text,
            headers: response_headers,
            body: body.to_vec(),
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot() -> SgpSnapshot {
        SgpSnapshot {
            servers: json!({
                "TENCENT_HN1": {
                    "matchHistory": "https://hn1-k8s-sgp.lol.qq.com:21019",
                    "common": "https://hn1-k8s-sgp.lol.qq.com:21019",
                    "isTencent": true
                },
                "EUW": {
                    "matchHistory": "https://euc1-red.pp.sgp.pvp.net",
                    "common": "https://euw-red.lol.sgp.pvp.net",
                    "isTencent": false,
                    "regionPathParam": "EUW1"
                }
            }),
            default_server_id: "TENCENT_HN1".to_string(),
            entitlements_token: Some("ENT".to_string()),
            league_session_token: Some("SESSION".to_string()),
        }
    }

    #[test]
    fn tencent_sub_id_is_the_platform_half() {
        assert_eq!(sub_id("TENCENT_HN1"), "HN1");
        assert_eq!(sub_id("NA1"), "NA1");
    }

    #[test]
    fn entitlements_token_selects_the_match_history_base() {
        let planned = plan(
            &snapshot(),
            "/match-history-query/v1/products/lol/player/p123/SUMMARY",
            &[
                (RIFTFLOW_HEADER_TOKEN_TYPE.to_string(), "entitlements".to_string()),
                ("accept".to_string(), "application/json".to_string()),
            ],
        )
        .expect("planned");

        assert_eq!(planned.base_url, "https://hn1-k8s-sgp.lol.qq.com:21019");
        assert_eq!(planned.authorization.as_deref(), Some("Bearer ENT"));
        // the marker headers never reach the server, the real ones do
        assert_eq!(planned.forwarded, vec![("accept".to_string(), "application/json".to_string())]);
    }

    #[test]
    fn league_session_token_uses_the_common_base() {
        let planned = plan(
            &snapshot(),
            "/gsm/v1/product/lol/summoner-ledge",
            &[
                (RIFTFLOW_HEADER_TOKEN_TYPE.to_string(), "league-session".to_string()),
                (RIFTFLOW_HEADER_SGP_SERVER_ID.to_string(), "EUW".to_string()),
            ],
        )
        .expect("planned");

        assert_eq!(planned.base_url, "https://euw-red.lol.sgp.pvp.net");
        assert_eq!(planned.authorization.as_deref(), Some("Bearer SESSION"));
    }

    #[test]
    fn the_sub_id_placeholder_prefers_region_param_then_the_platform() {
        let euw = plan(
            &snapshot(),
            &format!("/match-history-query/v1/products/lol/{URL_PLACEHOLDER_SUB_ID}_42/SUMMARY"),
            &[
                (RIFTFLOW_HEADER_TOKEN_TYPE.to_string(), "entitlements".to_string()),
                (RIFTFLOW_HEADER_SGP_SERVER_ID.to_string(), "EUW".to_string()),
            ],
        )
        .expect("planned");
        assert_eq!(euw.path, "/match-history-query/v1/products/lol/EUW1_42/SUMMARY");

        let tencent = plan(
            &snapshot(),
            &format!("/match-history-query/v1/products/lol/{URL_PLACEHOLDER_SUB_ID}_42/SUMMARY"),
            &[(RIFTFLOW_HEADER_TOKEN_TYPE.to_string(), "entitlements".to_string())],
        )
        .expect("planned");
        assert_eq!(tencent.path, "/match-history-query/v1/products/lol/HN1_42/SUMMARY");
    }

    #[test]
    fn missing_token_and_unknown_server_say_which_rule_failed() {
        let no_token = SgpSnapshot {
            entitlements_token: None,
            ..snapshot()
        };
        let error = plan(
            &no_token,
            "/x",
            &[(RIFTFLOW_HEADER_TOKEN_TYPE.to_string(), "entitlements".to_string())],
        )
        .unwrap_err();
        assert_eq!(error.message(), "Token not found for type: entitlements");

        let error = plan(
            &snapshot(),
            "/x",
            &[(RIFTFLOW_HEADER_SGP_SERVER_ID.to_string(), "NOPE".to_string())],
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Server config not found for sgp server ID: NOPE"
        );
    }
}
