//! File logs live under the user's RiftFlow data directory, plus a console
//! transport, with the same line format, the same
//! level filter and the same `LogMessageFormatter` argument handling.
//!
//! The renderer mirrors every log line through `logger-factory-main:log`, so
//! without this module the app has no log file at all and `openLogsDir` has
//! nothing to open.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// winston `maxsize` for the file transport.
pub const MAX_LOG_BYTES: u64 = 128 * 1024 * 1024;

/// Keep portable EXE and working directories clean, including read-only locations.
pub fn default_logs_dir() -> PathBuf {
    crate::user_data_dir().join("logs")
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

impl Level {
    /// winston's `levels` ordering: a transport at `info` also writes `warn` and
    /// `error`, and drops `debug`.
    pub fn passes(self, message: Level) -> bool {
        (message as u8) <= (self as u8)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
        }
    }
}

/// Unknown levels fall back to `info`, which is what `initAppLogger` does with
/// its default parameter.
pub fn parse_level(text: &str) -> Level {
    match text {
        "error" => Level::Error,
        "warn" => Level::Warn,
        "debug" => Level::Debug,
        _ => Level::Info,
    }
}

struct Sink {
    file: Mutex<File>,
    path: PathBuf,
    written: AtomicU64,
}

static SINK: OnceLock<Sink> = OnceLock::new();
static LEVEL: Mutex<Level> = Mutex::new(Level::Info);

fn level_now() -> Level {
    *LEVEL.lock().expect("log level lock")
}

/// `initAppLogger(level)`. Returns the log file path so callers can report it.
pub fn init(logs_dir: &Path, level: Level) -> PathBuf {
    ensure_dir(logs_dir);

    let filename = format!("RiftFlow_{}.log", stamp(SystemTime::now(), true));
    let path = logs_dir.join(&filename);

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));

    let already = file.metadata().map(|meta| meta.len()).unwrap_or(0);

    if SINK
        .set(Sink {
            file: Mutex::new(file),
            path: path.clone(),
            written: AtomicU64::new(already),
        })
        .is_err()
    {
        // `init` runs once at boot; a second call means a test leaked global state
        panic!("logger already initialised");
    }

    *LEVEL.lock().expect("log level lock") = level;

    path
}

/// `setLevel` on both transports.
pub fn set_level(level: Level) {
    *LEVEL.lock().expect("log level lock") = level;
}

pub fn level() -> Level {
    level_now()
}

pub fn log_file_path() -> Option<PathBuf> {
    SINK.get().map(|sink| sink.path.clone())
}

/// The winston pipeline: filter by level, then write file and console.
pub fn log(namespace: &str, level: Level, message: &str) {
    if !level_now().passes(level) {
        return;
    }

    let line = format!(
        "[{}] [{}] [{}] {}",
        stamp(SystemTime::now(), false),
        namespace,
        level.as_str(),
        message
    );

    if let Some(sink) = SINK.get() {
        let mut file = sink.file.lock().expect("log file lock");
        // `maxsize` only rotates by creating a new file; here a past-limit log
        // stops growing so a runaway renderer cannot fill the disk
        if sink.written.load(Ordering::Relaxed) < MAX_LOG_BYTES {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
            sink.written.fetch_add(line.len() as u64 + 1, Ordering::Relaxed);
        }
    }

    eprintln!("{}", colored(&line, level));
}

/// `logger.info(namespace, ...args)` used by the main-side shards.
pub fn info(namespace: &str, message: impl AsRef<str>) {
    log(namespace, Level::Info, message.as_ref());
}

pub fn warn(namespace: &str, message: impl AsRef<str>) {
    log(namespace, Level::Warn, message.as_ref());
}

/// `LogMessageFormatter.objectsToString`, applied to whatever survived the IPC
// JSON boundary (functions and `undefined` are already gone).
pub fn format_values(values: &[Value]) -> String {
    values
        .iter()
        .map(format_value)
        .collect::<Vec<String>>()
        .join(" ")
}

fn format_value(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }

    if is_error_like(value) {
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return match value.get("stack").and_then(Value::as_str) {
            Some(stack) => format!("{message}\n{stack}"),
            None => message.to_string(),
        };
    }

    // winston prints objects with two-space JSON; strings keep their quotes here
    serde_json::to_string_pretty(value).unwrap_or_else(|_| format!("{value}"))
}

/// `_isLikelyErrorObject`: a `stack` or `message` string property.
fn is_error_like(value: &Value) -> bool {
    value.as_object().is_some_and(|map| {
        map.get("stack").and_then(Value::as_str).is_some()
            || map.get("message").and_then(Value::as_str).is_some()
    })
}

const RESET: &str = "\x1b[0m";
const WHITE: &str = "\x1b[37m";
const BRIGHT_BLUE_BOLD: &str = "\x1b[94;1m";

fn level_color(level: Level) -> &'static str {
    match level {
        Level::Info => "\x1b[32m",
        Level::Debug => "\x1b[36m",
        Level::Warn => "\x1b[33m",
        Level::Error => "\x1b[31m",
    }
}

/// The console transport repaints the three bracketed fields.
fn colored(line: &str, level: Level) -> String {
    let mut parts = line.splitn(4, "] ");
    let Some(raw_timestamp) = parts.next() else {
        return line.to_string();
    };
    let Some(raw_namespace) = parts.next() else {
        return line.to_string();
    };
    let Some(raw_level) = parts.next() else {
        return line.to_string();
    };
    let message = parts.next().unwrap_or_default();

    let timestamp = raw_timestamp.trim_start_matches('[');
    let namespace = raw_namespace.trim_start_matches('[');
    let level_text = raw_level.trim_start_matches('[').trim_end_matches(']');

    format!(
        "{WHITE}[{timestamp}]{RESET} {BRIGHT_BLUE_BOLD}[{namespace}]{RESET} {color}[{level_text}]{RESET} {message}",
        color = level_color(level)
    )
}

fn stamp(now: SystemTime, compact: bool) -> String {
    stamp_with(now, local_offset_millis(), compact)
}

/// `dayjs(...).format(...)` on a wall-clock time, so the log filename and the
/// log lines match what the Electron build wrote.
fn stamp_with(now: SystemTime, offset_minutes: i64, compact: bool) -> String {
    let elapsed = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO);
    let local = elapsed.as_millis() as i64 + offset_minutes * 60_000;
    let mut days = local.div_euclid(86_400_000);
    let time_of_day = local.rem_euclid(86_400_000);
    let millis = time_of_day % 1000;
    let seconds = (time_of_day / 1000) % 60;
    let minutes = (time_of_day / 60_000) % 60;
    let hours = time_of_day / 3_600_000;

    let (year, month, day) = civil_from_days(&mut days);

    if compact {
        return format!("{year:04}{month:02}{day:02}_{hours:02}{minutes:02}{seconds:02}{millis:03}");
    }

    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}:{millis:03}")
}

/// Howard Hinnant's `civil_from_days`, extended with the year rollover needed
/// because `div_euclid` on a negative epoch (pre-1970 clocks) is not the POSIX
/// day number.
fn civil_from_days(days: &mut i64) -> (i64, u32, u32) {
    let z = *days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };

    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[cfg(windows)]
fn local_offset_millis() -> i64 {
    use std::mem::zeroed;

    #[repr(C)]
    struct TimezoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: [u8; 16],
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: [u8; 16],
        daylight_bias: i32,
    }

    const DAYLIGHT_TIME: u32 = 2;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetTimeZoneInformation(buffer: *mut TimezoneInformation) -> u32;
    }

    unsafe {
        let mut info: TimezoneInformation = zeroed();
        let kind = GetTimeZoneInformation(&mut info as *mut _ as *mut _);
        let is_daylight = kind == DAYLIGHT_TIME;
        let extra = if is_daylight { info.daylight_bias } else { info.standard_bias };
        // the API reports "minutes to add to local to get UTC"
        -((info.bias + extra) as i64)
    }
}

#[cfg(not(windows))]
fn local_offset_millis() -> i64 {
    0
}

/// `initAppLogger` recreates the directory when the path exists as a file.
fn ensure_dir(path: &Path) {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            std::fs::remove_file(path).ok();
            std::fs::create_dir_all(path).ok();
        }
        Err(_) => {
            std::fs::create_dir_all(path).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_info_threshold_keeps_warn_and_drops_debug() {
        assert!(Level::Info.passes(Level::Warn));
        assert!(Level::Info.passes(Level::Error));
        assert!(Level::Info.passes(Level::Info));
        assert!(!Level::Info.passes(Level::Debug));
        assert!(Level::Debug.passes(Level::Debug));
    }

    #[test]
    fn an_unknown_level_falls_back_to_info() {
        assert_eq!(parse_level("debug"), Level::Debug);
        assert_eq!(parse_level("verbose"), Level::Info);
        assert_eq!(parse_level(""), Level::Info);
    }

    #[test]
    fn error_objects_are_printed_like_the_formatter_does() {
        let error = json!({ "message": "boom", "stack": "at thing" });
        assert_eq!(format_values(&[error]), "boom\nat thing");

        let text = json!("plain");
        assert_eq!(format_values(&[text, json!(42)]), "plain 42");

        let object = json!({ "a": 1 });
        assert_eq!(format_values(&[object]), "{\n  \"a\": 1\n}");
    }

    #[test]
    fn the_timestamp_uses_the_log_files_shape() {
        // 2026-10-26 00:00:00.000 UTC
        let moment = SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(1_792_972_800_000);
        assert_eq!(stamp_at(moment, 0, false), "2026-10-26 00:00:00:000");
        assert_eq!(stamp_at(moment, 480, true), "20261026_080000000");
    }

    fn stamp_at(now: SystemTime, offset_minutes: i64, compact: bool) -> String {
        stamp_with(now, offset_minutes, compact)
    }
}
