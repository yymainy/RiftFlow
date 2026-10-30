//! Live SGP probe: resolves the real tokens from a running client and issues the
//! match-history SUMMARY request in a few variants, so a gateway-side failure can
//! be attributed to HTTP version, headers, or the server itself rather than
//! guessed at.
//!
//!     cargo run -p riftflow-sgp --example sgp_probe

use std::time::Duration;

use serde_json::Value;

use riftflow_lcu::http::LcuHttp;
use riftflow_sgp::{SgpGateway, SgpSnapshot, sub_id};

fn servers() -> Value {
    let raw = std::fs::read_to_string("../riftflow-app/assets/riftflow-builtin.json")
        .expect("riftflow-builtin.json next to riftflow-app");
    let parsed: Value = serde_json::from_str(&raw).expect("valid json");
    parsed
        .get("BUILTIN_SGP_LEAGUE_SERVERS_CONFIG")
        .and_then(|config| config.get("servers"))
        .cloned()
        .unwrap_or(Value::Null)
}

async fn fetch_tokens(http: &LcuHttp) -> (Option<String>, Option<String>, String) {
    let entitlements = http
        .get("/entitlements/v1/token")
        .await
        .ok()
        .and_then(|token| {
            token
                .get("accessToken")
                .and_then(Value::as_str)
                .map(str::to_string)
        });

    let session = http
        .get("/lol-league-session/v1/league-session-token")
        .await
        .ok()
        .and_then(|token| {
            token
                .as_str()
                .or_else(|| token.get("token").and_then(Value::as_str))
                .map(str::to_string)
        });

    let puuid = http
        .get("/lol-summoner/v1/current-summoner")
        .await
        .ok()
        .and_then(|summoner| {
            summoner
                .get("puuid")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();

    (entitlements, session, puuid)
}

async fn attempt(
    label: &str,
    client: reqwest::Client,
    base: &str,
    url: &str,
    token: &str,
    headers: &[(&str, &str)],
) {
    let mut builder = client
        .get(url)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }

    match tokio::time::timeout(Duration::from_secs(25), builder.send()).await {
        Ok(Ok(response)) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            println!(
                "[{label}] via {base} → {status} :: {}",
                text.chars().take(180).collect::<String>().replace('\n', " ")
            );
        }
        Ok(Err(error)) => println!("[{label}] transport error: {error}"),
        Err(_) => println!("[{label}] timed out after 25s"),
    }
}

#[tokio::main]
async fn main() {
    let reader = riftflow_lcu::discovery::CommandLineReader::new(
        std::sync::Arc::new(riftflow_native::Win32Probe),
        riftflow_native::is_elevated(),
    );
    let (clients, flag) = reader.read(false);
    let Some(client) = clients.first() else {
        println!("no LeagueClientUx running (flag={flag:?})");
        return;
    };

    println!(
        "client pid={} port={} region={} rso={}",
        client.pid, client.port, client.region, client.rso_platform_id
    );

    let http = LcuHttp::new(
        client.port,
        format!(
            "Basic {}",
            base64_auth(&client.auth_token)
        ),
    );

    let (entitlements, session, puuid) = fetch_tokens(&http).await;
    println!(
        "tokens: entitlements={} leagueSession={} puuid={}",
        entitlements.as_deref().map(|t| t.len()).unwrap_or(0),
        session.as_deref().map(|t| t.len()).unwrap_or(0),
        &puuid[..puuid.len().min(18)]
    );

    let server_id = format!("{}_{}", client.region.to_uppercase(), sub_id(&format!(
        "{}_{}",
        client.region.to_uppercase(),
        client.rso_platform_id.to_uppercase()
    )));
    let config = servers();
    let server = config.get(&server_id).cloned().unwrap_or(Value::Null);
    let base = server
        .get("matchHistory")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    println!("serverId={server_id} base={base}");

    let Some(token) = entitlements.as_deref() else {
        println!("no entitlements token, cannot continue");
        return;
    };

    let url = format!("{base}/match-history-query/v1/products/lol/player/{puuid}/SUMMARY?startIndex=0&count=20");

    let default_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .unwrap();
    let http1_client = reqwest::Client::builder()
        .http1_only()
        .timeout(Duration::from_secs(25))
        .build()
        .unwrap();

    for index in 1..=3 {
        attempt(
            &format!("attempt{index} accept"),
            default_client.clone(),
            &base,
            &url,
            &token,
            &[("accept", "application/json, text/plain, */*")],
        )
        .await;
    }

    attempt(
        "http1-only",
        http1_client,
        &base,
        &url,
        &token,
        &[("accept", "application/json, text/plain, */*")],
    )
    .await;

    attempt("no-accept", default_client.clone(), &base, &url, &token, &[]).await;

    attempt(
        "with-user-agent",
        default_client,
        &base,
        &url,
        &token,
        &[
            ("accept", "application/json, text/plain, */*"),
            (
                "user-agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            ),
        ],
    )
    .await;

    // and the gateway path the app actually uses, for comparison
    let gateway = SgpGateway::new();
    let snapshot = SgpSnapshot {
        servers: config.clone(),
        default_server_id: server_id.clone(),
        entitlements_token: entitlements.clone(),
        league_session_token: session.clone(),
    };
    let headers = vec![
        ("accept".to_string(), "application/json, text/plain, */*".to_string()),
        ("x-riftflow-token-type".to_string(), "entitlements".to_string()),
    ];
    match gateway
        .request(&snapshot, "GET", &url_after_base(&url, &base), &headers, &[])
        .await
    {
        Ok(reply) => println!(
            "[gateway] {} → {} ({} bytes)",
            url_after_base(&url, &base),
            reply.status,
            reply.body.len()
        ),
        Err(error) => println!("[gateway] error: {}", error.message()),
    }
}

fn url_after_base<'a>(full: &'a str, base: &str) -> &'a str {
    full.strip_prefix(base).unwrap_or(full)
}

fn base64_auth(token: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(format!("riot:{token}"))
}
