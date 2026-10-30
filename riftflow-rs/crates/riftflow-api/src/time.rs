//! `Date.parse` over the ISO-8601 subset the config documents use, plus the two
//! `updatedAt` comparisons that decide whether a value replaces what is loaded.
//!
//! `chrono` is not a workspace dependency, so the parser is here instead: RFC 3339
//! with a `Z` or `±hh:mm` offset and optional fractional seconds — exactly what
//! `z.iso.datetime({ offset: true })` accepts, so a document that passes validation
//! always has a timestamp this function can read.

use std::cmp::Ordering;

/// `state.featureGates?.updatedAt ?? '1970-01-01T00:00:00.000Z'` — feature gates
/// have no builtin, so the empty snapshot starts at the epoch.
pub const EPOCH_FLOOR: &str = "1970-01-01T00:00:00.000Z";

/// `Date.parse(text)` as epoch milliseconds, or `None` where JS gives `NaN`.
pub fn iso_datetime_to_millis(text: &str) -> Option<i64> {
    // YYYY-MM-DD THH:MM:SS [.fff] (Z | ±hh:mm)
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || !is_t(bytes[10]) {
        return None;
    }
    let (date, rest) = text.split_at(10);
    let time = &rest[1..];

    let mut parts = date.split('-');
    let year: i64 = parse_fixed_i64(parts.next()?, 4)?;
    let month: u32 = parse_fixed(parts.next()?, 2)?;
    let day: u32 = parse_fixed(parts.next()?, 2)?;
    if parts.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }

    // seconds may carry a fraction; the offset may follow it
    let (clock, offset) = split_offset(time)?;
    let (hms, fraction) = match clock.split_once('.') {
        Some((head, tail)) => (head, Some(tail)),
        None => (clock, None),
    };

    let mut clock_parts = hms.split(':');
    let hour: u32 = parse_fixed(clock_parts.next()?, 2)?;
    let minute: u32 = parse_fixed(clock_parts.next()?, 2)?;
    let second: u32 = parse_fixed(clock_parts.next()?, 2)?;
    if clock_parts.next().is_some() {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let millis = match fraction {
        None => 0,
        Some("") => return None,
        Some(digits) => {
            if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            // JS keeps the first three digits of the fraction
            let padded = format!("{digits:0<3}");
            padded[..3].parse::<i64>().ok()?
        }
    };

    if day == 0 || day > days_in_month(year, month) {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let mut value = ((days * 24 + hour as i64) * 60 + minute as i64) * 60 + second as i64;
    value = value * 1000 + millis;

    Some(value - offset * 60_000)
}

/// The signed offset in minutes (`Z` and a bare `±hh:mm` both included).
fn split_offset(time: &str) -> Option<(&str, i64)> {
    let bytes = time.as_bytes();
    let index = bytes
        .iter()
        .position(|byte| *byte == b'Z' || *byte == b'z' || *byte == b'+' || *byte == b'-')?;

    let (clock, marker) = time.split_at(index);
    let sign = match marker.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        // `Z`/`z` is zero offset and nothing after it
        _ => {
            return if marker.len() == 1 {
                Some((clock, 0))
            } else {
                None
            };
        }
    };

    let offset = &marker[1..];
    let mut parts = offset.split(':');
    let hours: i64 = parse_fixed_i64(parts.next()?, 2)?;
    let minutes: i64 = parts
        .next()
        .and_then(|rest| parse_fixed_i64(rest, 2))
        .unwrap_or(0);
    if parts.next().is_some() || hours > 23 || minutes > 59 {
        return None;
    }

    Some((clock, sign * (hours * 60 + minutes)))
}

fn is_t(byte: u8) -> bool {
    byte == b'T' || byte == b't'
}

fn parse_fixed(text: &str, len: usize) -> Option<u32> {
    if text.len() != len || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn parse_fixed_i64(text: &str, len: usize) -> Option<i64> {
    parse_fixed(text, len).map(|value| value as i64)
}

/// `days_from_civil` (Howard Hinnant), so no `chrono`/`time` crate is needed.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 } as i64;
    let day_of_year = (153 * month_index + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap_year(year) => 29,
        _ => 28,
    }
}

/// `Date.parse(a) <=> Date.parse(b)`, `None` when either side is not a date —
/// the JS `NaN` case, where every comparison is false.
pub fn compare_datetimes(a: &str, b: &str) -> Option<Ordering> {
    match (iso_datetime_to_millis(a), iso_datetime_to_millis(b)) {
        (Some(left), Some(right)) => Some(left.cmp(&right)),
        _ => None,
    }
}

/// What `_initResourceFromLocal` does with a cache file, by timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalCacheDecision {
    /// newer than what is loaded — `resource.apply(state, data)`
    Apply,
    /// older than the builtin — delete the file, keep the current value
    DiscardAsStale,
    /// the same instant — nothing to do
    KeepCurrent,
    /// an unparseable `updatedAt` on either side: JS compares false against `NaN`,
    /// so the file is neither applied nor deleted
    Indeterminate,
}

/// `_initResourceFromLocal`: `cached < current` deletes, `cached > current` applies.
pub fn decide_local_cache_load(cached: &str, current: &str) -> LocalCacheDecision {
    match compare_datetimes(cached, current) {
        Some(Ordering::Greater) => LocalCacheDecision::Apply,
        Some(Ordering::Less) => LocalCacheDecision::DiscardAsStale,
        Some(Ordering::Equal) => LocalCacheDecision::KeepCurrent,
        None => LocalCacheDecision::Indeterminate,
    }
}

/// `_updateAndSave`: a fetched document replaces the current value only when its
/// `updatedAt` is strictly newer.
pub fn decide_remote_replace(next: &str, current: &str) -> bool {
    matches!(
        compare_datetimes(next, current),
        Some(Ordering::Greater)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_iso_timestamp_becomes_epoch_millis() {
        assert_eq!(iso_datetime_to_millis("1970-01-01T00:00:00.000Z"), Some(0));
        // 2026-01-01T00:00:00Z is 1767225600000; +198 days +4h is this instant
        assert_eq!(
            iso_datetime_to_millis("2026-07-18T04:00:00.000Z"),
            Some(1_784_347_200_000)
        );
        // the offset is subtracted, so 04:00+02:00 is 02:00 UTC
        assert_eq!(
            iso_datetime_to_millis("2026-07-18T04:00:00+02:00"),
            Some(1_784_340_000_000)
        );
        assert_eq!(
            iso_datetime_to_millis("2026-07-18T04:00:00-01:30"),
            Some(1_784_352_600_000)
        );
        // a longer fraction keeps its first three digits, like JS
        assert_eq!(
            iso_datetime_to_millis("2026-07-18T04:00:00.123456Z"),
            Some(1_784_347_200_123)
        );
        // the builtin documents all carry this timestamp
        assert_eq!(
            iso_datetime_to_millis("2026-07-18T04:00:00Z"),
            Some(1_784_347_200_000)
        );
    }

    #[test]
    fn junk_is_not_a_date() {
        for text in [
            "",
            "2026-07-18",
            "2026-13-01T00:00:00Z",
            "2026-02-29T00:00:00Z",
            "2024-02-30T00:00:00Z",
            "2026-07-18T24:00:00Z",
            "2026-07-18T04:60:00Z",
            "2026-07-18 04:00:00Z",
            "2026-07-18T04:00:00",
            "2026-07-18T04:00:00.Z",
            "not a date",
        ] {
            assert_eq!(
                iso_datetime_to_millis(text),
                None,
                "{text} should not parse"
            );
        }

        // a leap day that exists does parse
        assert!(iso_datetime_to_millis("2024-02-29T00:00:00Z").is_some());
    }

    #[test]
    fn a_newer_cache_wins_and_an_older_one_is_dropped() {
        let builtin = "2026-07-18T04:00:00.000Z";
        assert_eq!(
            decide_local_cache_load("2099-01-01T00:00:00.000Z", builtin),
            LocalCacheDecision::Apply
        );
        assert_eq!(
            decide_local_cache_load("2020-01-01T00:00:00.000Z", builtin),
            LocalCacheDecision::DiscardAsStale
        );
        assert_eq!(
            decide_local_cache_load(builtin, builtin),
            LocalCacheDecision::KeepCurrent
        );
        assert_eq!(
            decide_local_cache_load("nonsense", builtin),
            LocalCacheDecision::Indeterminate
        );
    }

    #[test]
    fn a_remote_document_replaces_only_when_newer() {
        let current = "2026-07-18T04:00:00.000Z";
        assert!(decide_remote_replace("2099-01-01T00:00:00.000Z", current));
        assert!(!decide_remote_replace("2020-01-01T00:00:00.000Z", current));
        assert!(!decide_remote_replace(current, current));
        assert!(!decide_remote_replace("garbage", current));
    }

    #[test]
    fn the_epoch_floor_is_the_empty_feature_gate_baseline() {
        assert_eq!(iso_datetime_to_millis(EPOCH_FLOOR), Some(0));
        assert!(decide_remote_replace(
            "2026-07-20T04:00:00.000Z",
            EPOCH_FLOOR
        ));
    }
}
