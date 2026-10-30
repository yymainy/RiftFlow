//! Credential-path probe. Run it elevated once to prove the same reading strategy
//! the shipped `riftflow-tools` addon used works for auto-connect:
//!
//!     Start-Process -Verb RunAs -FilePath target\debug\examples\credential_probe.exe
//!
//! Results are written to `%TEMP%\riftflow-native-probe.txt` because an elevated
//! process has no console attached here.

use std::io::Write;

fn main() {
    let mut lines = Vec::new();
    let mut record = |line: String| {
        println!("{line}");
        lines.push(line);
    };

    record(format!("is_elevated = {}", riftflow_native::is_elevated()));

    let ux_pids = riftflow_native::pids_by_name(riftflow_native::LEAGUE_CLIENT_UX_PROCESS_NAME);
    let lc_pids = riftflow_native::pids_by_name(riftflow_native::LEAGUE_CLIENT_PROCESS_NAME);
    let game_pids = riftflow_native::pids_by_name(riftflow_native::GAME_CLIENT_PROCESS_NAME);
    record(format!(
        "pids ux={ux_pids:?} leagueclient={lc_pids:?} gameclient={game_pids:?}"
    ));

    for pid in &ux_pids {
        match riftflow_native::read_command_line(*pid) {
            Ok(line) => match riftflow_lcu::parse_command_line(&line) {
                Some(parsed) => record(format!(
                    "pid={pid} PARSED port={} region={} rso={} tokenLen={} riotPort={}",
                    parsed.port,
                    parsed.region,
                    parsed.rso_platform_id,
                    parsed.auth_token.len(),
                    parsed.riot_client_port
                )),
                None => record(format!(
                    "pid={pid} READ_OK len={} but PARSE_FAILED (no port/token/pid)",
                    line.len()
                )),
            },
            Err(error) => record(format!("pid={pid} READ_FAILED {error}")),
        }
    }

    // the reader the shell actually uses, including the 5-attempt flag rule
    let reader = riftflow_lcu::discovery::CommandLineReader::new(
        std::sync::Arc::new(riftflow_native::Win32Probe),
        riftflow_native::is_elevated(),
    );
    let (clients, flag) = reader.read(false);
    record(format!(
        "CommandLineReader native: clients={} hasClientButNoCommandLine={flag:?}",
        clients.len()
    ));

    if let Some(first) = clients.first() {
        record(format!(
            "first client pid={} port={} region={}",
            first.pid, first.port, first.region
        ));
    }

    let Some(path) = std::env::var("TEMP")
        .ok()
        .map(|temp| std::path::PathBuf::from(temp).join("riftflow-native-probe.txt"))
    else {
        return;
    };
    if let Ok(mut file) = std::fs::File::create(&path) {
        for line in &lines {
            let _ = writeln!(file, "{line}");
        }
    }
}
