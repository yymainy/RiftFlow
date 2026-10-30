//! The two SemVer behaviours the feature-gate schema needs: `valid()` (is this a
//! version at all) and `gte()` (is the lower bound already at or past the upper
//! one). The `semver` crate is not a workspace dependency, so the grammar is
//! checked here — strict SemVer 2.0 with the optional `v` prefix npm's parser
//! accepts.

use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<PreRelease>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreRelease {
    Numeric(u64),
    Alphanumeric(String),
}

/// `semver.valid(text)` — `None` for anything that is not a plain version
/// (ranges, `>=`, leading zeros, empty identifiers all fail).
pub fn parse(text: &str) -> Option<SemVer> {
    let text = text.trim();
    // npm's parser tolerates exactly one leading `v`
    let text = match text.strip_prefix('v').or_else(|| text.strip_prefix('V')) {
        Some(rest) => rest,
        None => text,
    };
    if text.is_empty() {
        return None;
    }

    // build metadata never affects precedence, but it has to be well formed
    let (core, build) = match text.split_once('+') {
        Some((core, build)) => (core, Some(build)),
        None => (text, None),
    };
    if let Some(build) = build {
        if !build.split('.').all(is_dot_separated_ascii) {
            return None;
        }
    }

    let (numbers, pre) = match core.split_once('-') {
        Some((numbers, pre)) => (numbers, Some(pre)),
        None => (core, None),
    };

    let mut parts = numbers.split('.');
    let major = parse_number(parts.next()?)?;
    let minor = parse_number(parts.next()?)?;
    let patch = parse_number(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }

    let mut pre_parts = Vec::new();
    if let Some(pre) = pre {
        if pre.is_empty() {
            return None;
        }
        for identifier in pre.split('.') {
            pre_parts.push(parse_pre_release(identifier)?);
        }
    }

    Some(SemVer {
        major,
        minor,
        patch,
        pre: pre_parts,
    })
}

pub fn is_valid(text: &str) -> bool {
    parse(text).is_some()
}

fn parse_number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // no leading zeros on a numeric core identifier
    if text.len() > 1 && text.starts_with('0') {
        return None;
    }
    text.parse().ok()
}

fn parse_pre_release(text: &str) -> Option<PreRelease> {
    if text.is_empty() || !is_dot_separated_ascii(text) {
        return None;
    }
    if text.bytes().all(|byte| byte.is_ascii_digit()) {
        // numeric prerelease identifiers may not have leading zeros
        if text.len() > 1 && text.starts_with('0') {
            return None;
        }
        return text.parse().ok().map(PreRelease::Numeric);
    }
    Some(PreRelease::Alphanumeric(text.to_string()))
}

fn is_dot_separated_ascii(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.')
}

impl PreRelease {
    /// Numeric identifiers always have lower precedence than alphanumeric ones.
    fn compare(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(left), Self::Numeric(right)) => left.cmp(right),
            (Self::Numeric(_), Self::Alphanumeric(_)) => Ordering::Less,
            (Self::Alphanumeric(_), Self::Numeric(_)) => Ordering::Greater,
            (Self::Alphanumeric(left), Self::Alphanumeric(right)) => left.cmp(right),
        }
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                // a pre-release has lower precedence than the release
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self
                    .pre
                    .iter()
                    .zip(other.pre.iter())
                    .map(|(left, right)| left.compare(right))
                    .find(|ordering| ordering != &Ordering::Equal)
                    .unwrap_or_else(|| self.pre.len().cmp(&other.pre.len())),
            })
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `semver.gte(min, max)` — `None` when either side is not a version, which the
/// schema reports as its own error before getting here.
pub fn gte(minimum: &str, maximum: &str) -> Option<bool> {
    match (parse(minimum), parse(maximum)) {
        (Some(left), Some(right)) => Some(left >= right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_versions_are_valid() {
        assert!(is_valid("1.5.0"));
        assert!(is_valid("v1.5.0"));
        assert!(is_valid("0.0.0"));
        assert!(is_valid("1.5.0-beta.1"));
        assert!(is_valid("1.5.0+build.7"));
    }

    #[test]
    fn ranges_and_malformed_versions_are_not() {
        for text in [
            "", "1.5", "1.5.0.0", "01.5.0", "1.0.0-01", "1.0.0-", "1.0.0+", ">=1.5.0", "1.x.0",
            "1.0.0-beta..1", "abc",
        ] {
            assert!(!is_valid(text), "{text} should be invalid");
        }
    }

    #[test]
    fn precedence_follows_the_spec() {
        assert!(parse("2.0.0") > parse("1.99.99"));
        assert!(parse("1.0.0") > parse("1.0.0-rc.1"));
        assert!(parse("1.0.0-rc.2") > parse("1.0.0-rc.1"));
        assert!(parse("1.0.0-alpha.1") < parse("1.0.0-alpha.beta"));
        assert_eq!(parse("1.0.0+build"), parse("1.0.0+other"));
    }

    #[test]
    fn gte_catches_an_inverted_bound() {
        assert_eq!(gte("1.6.0", "1.5.0"), Some(true));
        assert_eq!(gte("1.5.0", "1.5.0"), Some(true));
        assert_eq!(gte("1.4.0", "1.5.0"), Some(false));
        assert_eq!(gte("garbage", "1.5.0"), None);
    }
}
