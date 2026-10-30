//! Port of `src/main/shards/league-client-ux/ux-command-line-parser.ts` and the
//! `UxCommandLine` shape from `src/shared/shards/league-client-ux/index.ts`.

use serde::{Deserialize, Serialize};

/// Riot's own certificate, shipped verbatim by the original parser.
pub const RIOT_CERTIFICATE: &str = include_str!("../assets/riot-certificate.pem");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UxCommandLine {
    pub port: i64,
    pub pid: i64,
    pub auth_token: String,
    pub rso_platform_id: String,
    pub region: String,
    pub certificate: String,
    pub riot_client_port: i64,
    pub riot_client_auth_token: String,
}

/// The credentials a connection attempt needs, with the certificate elided so it
/// is never accidentally logged or shipped over IPC. `lastConnectedClient`
/// storage keeps the full struct, as the original does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UxCommandLinePublic {
    pub port: i64,
    pub pid: i64,
    pub auth_token: String,
    pub rso_platform_id: String,
    pub region: String,
    pub riot_client_port: i64,
    pub riot_client_auth_token: String,
}

impl UxCommandLine {
    pub fn without_certificate(&self) -> UxCommandLinePublic {
        UxCommandLinePublic {
            port: self.port,
            pid: self.pid,
            auth_token: self.auth_token.clone(),
            rso_platform_id: self.rso_platform_id.clone(),
            region: self.region.clone(),
            riot_client_port: self.riot_client_port,
            riot_client_auth_token: self.riot_client_auth_token.clone(),
        }
    }

    pub fn basic_auth(&self) -> String {
        use base64::Engine;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("riot:{}", self.auth_token))
        )
    }
}

fn re(pattern: &str) -> Result<regex::Regex, regex::Error> {
    regex::Regex::new(pattern)
}

/// `parseCommandLine` — returns `None` unless port, token and pid are all present.
pub fn parse_command_line(line: &str) -> Option<UxCommandLine> {
    let port = re(r"--app-port=([0-9]+)").ok()?;
    let auth = re(r"--remoting-auth-token=([\w\-_]+)").ok()?;
    let pid = re(r"--app-pid=([0-9]+)").ok()?;
    // Some clients use `--rso_platform_id`, others `--rso-platform-id`.
    let rso = re(r"(?i)--rso[_-]platform[_-]id=([\w\-_]+)").ok()?;
    let region = re(r"--region=([\w\-_]+)").ok()?;
    let riot_port = re(r"--riotclient-app-port=([0-9]+)").ok()?;
    let riot_auth = re(r"--riotclient-auth-token=([\w\-_]+)").ok()?;

    let capture = |regex: &regex::Regex, default: &str| -> String {
        regex
            .captures(line)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| default.to_string())
    };

    let port_text = capture(&port, "");
    let auth_token = capture(&auth, "");
    let pid_text = capture(&pid, "");

    if port_text.is_empty() || auth_token.is_empty() || pid_text.is_empty() {
        return None;
    }

    // `Number('')` is 0 in JS, which is what the original produces for a missing
    // riotclient port, so an absent match must become 0 rather than None.
    let riot_client_port: i64 = match riot_port.captures(line).and_then(|c| c.get(1)) {
        Some(m) => m.as_str().parse().unwrap_or(0),
        None => 0,
    };

    Some(UxCommandLine {
        port: port_text.parse().ok()?,
        pid: pid_text.parse().ok()?,
        auth_token,
        rso_platform_id: capture(&rso, ""),
        region: capture(&region, ""),
        certificate: RIOT_CERTIFICATE.to_string(),
        riot_client_port,
        riot_client_auth_token: capture(&riot_auth, ""),
    })
}

/// The endpoint subscriptions opened right after the socket connects.
///
/// `lol-champ-select` events cannot be subscribed at the right moment, so the
/// original keeps a single broad `OnJsonApiEvent` subscription and filters
/// locally — see `src/shared/constants/subscribed-lcu-endpoints.ts`.
pub const SUBSCRIBED_LCU_ENDPOINTS: &[&str] = &["OnJsonApiEvent"];
