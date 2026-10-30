//! Port of `league-client-ux/ux-command-line-reader.ts` plus the process helpers
//! from `src/main/native/`.
//!
//! The original has two credential paths: a native one (`NtQueryInformationProcess`
//! from the `riftflow-tools` addon) used by default, and a `Get-CimInstance` path used
//! when `useWmi` is on and the app is elevated. Both are the same *shape* of call,
//! so this module is written against a [`ProcessProbe`] trait. The CIM probe is
//! implemented here; the native probe lands with the `windows`-crate addon
//! replacement and is a drop-in for the same trait.
//!
//! Verified against a live 国服 client: an unelevated process gets an *empty*
//! command line back for the elevated `LeagueClientUx.exe`, which is precisely the
//! condition `hasClientButNoCommandLine` exists to surface.

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ux_command_line::{parse_command_line, UxCommandLine};

pub trait ProcessProbe: Send + Sync {
    fn pids_by_name(&self, process_name: &str) -> Vec<u32>;
    /// `Err` models "this process's command line is not readable from here".
    fn command_line(&self, pid: u32) -> anyhow::Result<String>;
}

const POWERSHELL_BASE_ARGS: [&str; 4] = [
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
];

/// `process-utils-win32.ts` — the CIM/`shell` query path.
pub struct PowerShellProbe;

impl PowerShellProbe {
    fn run(command: &str) -> anyhow::Result<String> {
        let mut cmd = Command::new("powershell");
        cmd.args(POWERSHELL_BASE_ARGS).arg("-Command").arg(command);
        hide_console(&mut cmd);

        let output = cmd.output()?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }

        anyhow::bail!(
            "command failed with code {}: {}",
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "null".to_string()),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}

#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_cmd: &mut Command) {}

impl ProcessProbe for PowerShellProbe {
    fn pids_by_name(&self, process_name: &str) -> Vec<u32> {
        // `getPidsByName` is an exact name match in the original, so the filter is
        // built from the literal name with single quotes escaped for WQL.
        let escaped = process_name.replace('\'', "\\'");
        let script = format!(
            "Get-CimInstance -ClassName Win32_Process -Filter \"Name='{escaped}'\" | Select-Object -ExpandProperty ProcessId"
        );

        match Self::run(&script) {
            Ok(output) => output
                .lines()
                .filter_map(|line| line.trim().parse::<u32>().ok())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    fn command_line(&self, pid: u32) -> anyhow::Result<String> {
        // Same interpolation as getCommandLinePowershell; pid is numeric so there
        // is no injection surface.
        let script = format!(
            "Get-CimInstance -ClassName Win32_Process -Filter \"ProcessId={pid}\" -Property CommandLine | Select-Object -ExpandProperty CommandLine"
        );
        Self::run(&script)
    }
}

/// `LeagueClientUxCommandLineReader`
pub struct CommandLineReader {
    probe: std::sync::Arc<dyn ProcessProbe>,
    ux_process_name: String,
    /// `isElevated` — gates the WMI branch, which returns nothing without it.
    elevated: bool,
    has_client_but_no_command_line_count: AtomicUsize,
}

impl CommandLineReader {
    pub fn new(probe: std::sync::Arc<dyn ProcessProbe>, elevated: bool) -> Self {
        Self {
            probe,
            ux_process_name: crate::LEAGUE_CLIENT_UX_PROCESS_NAME.to_string(),
            elevated,
            has_client_but_no_command_line_count: AtomicUsize::new(0),
        }
    }

    pub fn set_elevated(&mut self, elevated: bool) {
        self.elevated = elevated;
    }

    /// `read()` — the `useWmi` branch only runs when elevated, and resets the
    /// no-command-line counter; the default branch counts up to 5 before flagging.
    pub fn read(&self, use_wmi: bool) -> (Vec<UxCommandLine>, Option<bool>) {
        let pids = self.probe.pids_by_name(&self.ux_process_name);

        if use_wmi {
            if !self.elevated {
                return (Vec::new(), None);
            }

            let clients = pids
                .iter()
                .filter_map(|pid| self.probe.command_line(*pid).ok())
                .filter_map(|line| parse_command_line(&line))
                .collect();

            self.has_client_but_no_command_line_count.store(0, Ordering::SeqCst);
            return (clients, Some(false));
        }

        let mut clients = Vec::new();
        for pid in &pids {
            if let Ok(line) = self.probe.command_line(*pid) {
                if let Some(parsed) = parse_command_line(&line) {
                    clients.push(parsed);
                }
            }
        }

        if !pids.is_empty() && clients.is_empty() {
            let previous = self.has_client_but_no_command_line_count.fetch_add(1, Ordering::SeqCst);
            if previous + 1 >= 5 {
                return (clients, Some(true));
            }
        }

        (clients, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeProbe {
        pids: Vec<u32>,
        lines: Mutex<std::collections::HashMap<u32, String>>,
    }

    impl ProcessProbe for FakeProbe {
        fn pids_by_name(&self, _name: &str) -> Vec<u32> {
            self.pids.clone()
        }

        fn command_line(&self, pid: u32) -> anyhow::Result<String> {
            let lines = self.lines.lock().unwrap();
            match lines.get(&pid) {
                Some(line) if !line.is_empty() => Ok(line.clone()),
                // an empty command line is what Windows returns cross-integrity
                _ => Ok(String::new()),
            }
        }
    }

    fn fake(pids: Vec<u32>, lines: Vec<(u32, &str)>) -> std::sync::Arc<dyn ProcessProbe> {
        let mut map = std::collections::HashMap::new();
        for (pid, line) in lines {
            map.insert(pid, line.to_string());
        }
        std::sync::Arc::new(FakeProbe {
            pids,
            lines: Mutex::new(map),
        })
    }

    const REAL_CMD: &str = r#""D:\WeGameApps\LOL\LeagueClient\LeagueClientUx.exe" --app-port=52611 --remoting-auth-token=abc-_123 --app-pid=14264 --rso_platform_id=HN1 --region=TENCENT --riotclient-app-port=51100 --riotclient-auth-token=xyz"#;

    #[test]
    fn parses_a_wegame_style_command_line() {
        let parsed = parse_command_line(REAL_CMD).expect("parses");
        assert_eq!(parsed.port, 52611);
        assert_eq!(parsed.pid, 14264);
        assert_eq!(parsed.auth_token, "abc-_123");
        assert_eq!(parsed.region, "TENCENT");
        assert_eq!(parsed.rso_platform_id, "HN1");
        assert_eq!(parsed.riot_client_port, 51100);
        assert_eq!(parsed.riot_client_auth_token, "xyz");
        assert!(parsed.certificate.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn missing_fields_match_the_js_defaults() {
        let parsed = parse_command_line("--app-port=1234 --remoting-auth-token=t --app-pid=9")
            .expect("parses");
        assert_eq!(parsed.region, "");
        assert_eq!(parsed.rso_platform_id, "");
        // `Number('')` is 0 in JS
        assert_eq!(parsed.riot_client_port, 0);
        assert_eq!(parsed.riot_client_auth_token, "");
    }

    #[test]
    fn requires_port_token_and_pid() {
        assert!(parse_command_line("--app-port=1 --app-pid=2").is_none());
        assert!(parse_command_line("").is_none());
        assert!(parse_command_line("--remoting-auth-token=only").is_none());
    }

    #[test]
    fn underscore_and_hyphen_rso_forms_both_parse() {
        for flag in ["--rso_platform_id=HN10", "--rso-platform-id=HN10"] {
            let line = format!("--app-port=1 --remoting-auth-token=t --app-pid=2 {flag}");
            assert_eq!(parse_command_line(&line).unwrap().rso_platform_id, "HN10", "{flag}");
        }
    }

    #[test]
    fn wmi_branch_refuses_without_elevation() {
        let reader = CommandLineReader::new(fake(vec![1], vec![(1, REAL_CMD)]), false);
        let (clients, flag) = reader.read(true);
        assert!(clients.is_empty());
        assert_eq!(flag, None, "no counter update when gated out");
    }

    #[test]
    fn wmi_branch_reads_clients_when_elevated() {
        let reader = CommandLineReader::new(fake(vec![1], vec![(1, REAL_CMD)]), true);
        let (clients, flag) = reader.read(true);
        assert_eq!(clients.len(), 1);
        assert_eq!(flag, Some(false));
    }

    #[test]
    fn flags_missing_command_line_only_after_five_attempts() {
        let reader = CommandLineReader::new(fake(vec![7], vec![]), false);

        for attempt in 1..=4 {
            let (clients, flag) = reader.read(false);
            assert!(clients.is_empty());
            assert_eq!(flag, None, "attempt {attempt} must not flag yet");
        }

        let (_clients, flag) = reader.read(false);
        assert_eq!(flag, Some(true), "the fifth attempt flips the flag");
    }

    #[test]
    fn no_processes_means_no_flag() {
        let reader = CommandLineReader::new(fake(vec![], vec![]), false);
        assert_eq!(reader.read(false), (Vec::new(), None));
    }
}
