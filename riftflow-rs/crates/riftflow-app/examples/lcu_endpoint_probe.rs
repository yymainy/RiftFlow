//! One LCU call against the live client, printed as `METHOD path -> status body`.
//!
//! The 国服 (Tencent) client answers functions it does not expose with its own 404
//! text (`Invalid function` / `Invalid URI format`), so questions like "does
//! `POST /login/dodge` exist on this client" can only be settled against a running
//! client — and inside the app a proxied call is invisible unless it happens to fail.
//!
//!     cargo run -p riftflow-app --example lcu_endpoint_probe -- GET /lol-gameflow/v1/gameflow-phase
//!     cargo run -p riftflow-app --example lcu_endpoint_probe -- POST /login/dodge

use std::io::Write;

use riftflow_lcu::http::{LcuHttp, RequestConfig};
use riftflow_lcu::parse_command_line;
use serde_json::{Map, Value};

fn credentials() -> Option<(i64, String)> {
    for pid in riftflow_native::pids_by_name(riftflow_native::LEAGUE_CLIENT_UX_PROCESS_NAME) {
        let Ok(line) = riftflow_native::read_command_line(pid) else {
            continue;
        };
        if let Some(parsed) = parse_command_line(&line) {
            return Some((parsed.port, parsed.basic_auth()));
        }
    }

    None
}

/// An elevated run has no console, so the answer also goes on disk.
fn write_out(lines: &[String]) {
    let path = std::env::temp_dir().join("riftflow-lcu-endpoint-probe.txt");
    if let Ok(mut file) = std::fs::File::create(&path) {
        let _ = file.write_all(lines.join("\n").as_bytes());
        let _ = file.flush();
    }
}

#[tokio::main]
async fn main() {
    let mut argv = std::env::args().skip(1);
    let method = argv
        .next()
        .unwrap_or_else(|| "GET".to_string())
        .to_uppercase();
    let path = argv.next().unwrap_or_else(|| "/health".to_string());
    let body: Value = argv
        .next()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(Value::Null);

    let mut lines = Vec::new();
    let mut report = |line: String| {
        println!("{line}");
        lines.push(line);
    };

    report(format!("is_elevated = {}", riftflow_native::is_elevated()));

    let Some((port, auth)) = credentials() else {
        report("no readable LeagueClientUx command line (client not running?)".to_string());
        write_out(&lines);
        return;
    };

    let http = LcuHttp::new(port, auth);
    let config = RequestConfig {
        method: method.clone(),
        url: path.clone(),
        data: if body.is_null() { None } else { Some(body) },
        params: Map::new(),
        headers: Map::new(),
        response_type: None,
    };

    match http.request(config).await {
        Ok(response) => {
            let snippet: String = serde_json::to_string(&response.body)
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect();
            report(format!("{method} {path} -> {} {snippet}", response.status));
        }
        Err(error) => report(format!("{method} {path} -> ERROR {error}")),
    }

    write_out(&lines);
}
