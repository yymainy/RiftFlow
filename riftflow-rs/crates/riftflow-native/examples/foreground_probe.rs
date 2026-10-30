//! What the in-game-send foreground gate actually sees, printed against a live match.
//!
//! `sendInGame` refuses with `Game client is not foreground`, and that single line is
//! the difference between "the game really is not in front", "this client names its game
//! process differently" and "our check is wrong". Run it while the game window is the
//! foreground one:
//!
//!     cargo run -p riftflow-native --example foreground_probe

use std::ffi::c_void;
use std::time::Duration;

#[link(name = "user32")]
extern "system" {
    fn GetForegroundWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(window: *mut c_void, process_id: *mut u32) -> u32;
    fn GetWindowTextW(window: *mut c_void, buffer: *mut u16, max_count: i32) -> i32;
    fn GetClassNameW(window: *mut c_void, buffer: *mut u16, max_count: i32) -> i32;
}

/// `pids_by_name` in reverse: the executable name behind a pid, from the same Toolhelp
/// snapshot the port's own lookup uses.
fn process_name(pid: u32) -> String {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return "<snapshot failed>".to_string();
    };

    let mut entry = PROCESSENTRY32W::default();
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut name = String::from("<pid not found>");

    unsafe {
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                if entry.th32ProcessID == pid {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|unit| *unit == 0)
                        .unwrap_or(entry.szExeFile.len());
                    name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                    break;
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }

    name
}

fn wide_to_string(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

fn main() {
    // Sample repeatedly: the console window this runs in can itself take the
    // foreground for a moment, and the interesting state is the match, a couple of
    // seconds after the user alt-tabs into it. Results also go to a file so a run
    // launched from a hidden console is still readable.
    let samples = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    let gap = Duration::from_millis(
        std::env::args()
            .nth(2)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1500),
    );

    let mut report = String::new();
    report.push_str(&format!("self is_elevated = {}\n", riftflow_native::is_elevated()));

    for index in 0..samples {
        if index > 0 {
            std::thread::sleep(gap);
        }

        report.push_str(&sample());
        print!("{}", report);

        let path = std::env::temp_dir().join("riftflow-foreground-probe.txt");
        let _ = std::fs::write(&path, report.as_bytes());
    }
}

fn sample() -> String {
    let game_pids = riftflow_native::pids_by_name(riftflow_native::GAME_CLIENT_PROCESS_NAME);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or(0);

    let mut out = format!("--- t={stamp}ms ---\n");
    out.push_str(&format!(
        "pids_by_name({:?}) = {game_pids:?}\n",
        riftflow_native::GAME_CLIENT_PROCESS_NAME
    ));
    out.push_str(&format!(
        "our gate is_process_foreground(any) = {}\n",
        game_pids
            .iter()
            .any(|pid| riftflow_native::is_process_foreground(*pid))
    ));

    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        out.push_str("GetForegroundWindow() = null\n");
        return out;
    }

    let mut pid: u32 = 0;
    unsafe {
        GetWindowThreadProcessId(foreground, &mut pid);
    }

    let mut title = [0u16; 256];
    let mut class = [0u16; 256];
    let title_len = unsafe { GetWindowTextW(foreground, title.as_mut_ptr(), 256) };
    let class_len = unsafe { GetClassNameW(foreground, class.as_mut_ptr(), 256) };

    out.push_str(&format!(
        "foreground pid = {pid} ({})\n",
        process_name(pid)
    ));
    out.push_str(&format!(
        "foreground window = {:?} / class {:?}\n",
        wide_to_string(&title[..(title_len.max(0) as usize).min(256)]),
        wide_to_string(&class[..(class_len.max(0) as usize).min(256)])
    ));
    out.push_str(&format!(
        "foreground is a game pid = {}\n",
        game_pids.contains(&pid)
    ));

    out
}
