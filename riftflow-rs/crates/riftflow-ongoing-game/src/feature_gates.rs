//! `shared/shards/feature-gating/index.ts` — `FeatureGateEvaluator` and
//! `isFeatureGateEnabled`, against the `riftflow-api-main` `featureGates` prop.
//!
//! The two gates this shard reads default to `true`, so an absent remote config
//! keeps deobfuscation and the GSM query on, exactly like the original.

use std::collections::HashSet;

use riftflow_state::StateSync;
use serde_json::Value;

pub const ONGOING_GAME_DEOBFUSCATION_FEATURE_GATE: &str = "ongoing-game.deobfuscation";
pub const ONGOING_GAME_GSM_BY_PUUID_FEATURE_GATE: &str = "ongoing-game.gsm-by-puuid";

pub const RIFTFLOW_API_NAMESPACE: &str = "riftflow-api-main";

/// `valid()` + `lt()` + `gte()`: a version that does not parse as
/// `major.minor.patch` disables every versioned gate, like the original's `continue`.
fn parse_version(text: &str) -> Option<(i64, i64, i64)> {
    let head = text
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .collect::<Vec<_>>();

    if head.len() != 3 {
        return None;
    }

    Some((
        head[0].parse().ok()?,
        head[1].parse().ok()?,
        head[2].parse().ok()?,
    ))
}

/// `FeatureGateEvaluator.evaluate`.
pub fn evaluate(
    config: Option<&Value>,
    platform: &str,
    version: &str,
    sgp_server_id: &str,
) -> Option<HashSet<String>> {
    let config = config.filter(|config| !config.is_null())?;
    let mut enabled = HashSet::new();
    let version = parse_version(version);

    if let Some(gates) = config.get("gates").and_then(Value::as_object) {
        for (key, gate) in gates {
            if let Some(platforms) = gate.get("platforms").and_then(Value::as_array) {
                if !platforms.iter().any(|listed| listed.as_str() == Some(platform)) {
                    continue;
                }
            }

            let min = gate
                .get("minVersionInclusive")
                .and_then(Value::as_str)
                .and_then(parse_version);
            let max = gate
                .get("maxVersionExclusive")
                .and_then(Value::as_str)
                .and_then(parse_version);

            if min.is_some() || max.is_some() {
                let Some(current) = version else {
                    continue;
                };
                if matches!(min, Some(min) if current < min) {
                    continue;
                }
                if matches!(max, Some(max) if current >= max) {
                    continue;
                }
            }

            if let Some(servers) = gate.get("sgpServers").and_then(Value::as_array) {
                if sgp_server_id.is_empty()
                    || !servers
                        .iter()
                        .any(|listed| listed.as_str() == Some(sgp_server_id))
                {
                    continue;
                }
            }

            enabled.insert(key.clone());
        }
    }

    Some(enabled)
}

/// `isFeatureGateEnabled` — no evaluation means no remote config yet.
pub fn is_enabled(key: &str, default: bool, evaluation: Option<&HashSet<String>>) -> bool {
    match evaluation {
        Some(evaluation) => evaluation.contains(key),
        None => default,
    }
}

/// The gates as this shard sees them, read once per pass.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureGates {
    pub deobfuscation: bool,
    pub gsm_by_puuid: bool,
}

impl Default for FeatureGates {
    fn default() -> Self {
        Self {
            deobfuscation: true,
            gsm_by_puuid: true,
        }
    }
}

impl FeatureGates {
    pub fn new(
        config: Option<&Value>,
        platform: &str,
        version: &str,
        sgp_server_id: &str,
    ) -> Self {
        let evaluation = evaluate(config, platform, version, sgp_server_id);
        Self {
            deobfuscation: is_enabled(
                ONGOING_GAME_DEOBFUSCATION_FEATURE_GATE,
                true,
                evaluation.as_ref(),
            ),
            gsm_by_puuid: is_enabled(ONGOING_GAME_GSM_BY_PUUID_FEATURE_GATE, true, evaluation.as_ref()),
        }
    }

    pub fn from_state(state_sync: &StateSync, platform: &str, version: &str, sgp_server_id: &str) -> Self {
        let config = state_sync.get(RIFTFLOW_API_NAMESPACE, "state", "featureGates");
        Self::new(config.as_ref(), platform, version, sgp_server_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn no_remote_config_keeps_both_gates_on() {
        let gates = FeatureGates::new(None, "win32", "1.5.1", "");
        assert!(gates.deobfuscation);
        assert!(gates.gsm_by_puuid);

        let gates = FeatureGates::new(Some(&Value::Null), "win32", "1.5.1", "");
        assert!(gates.deobfuscation);
    }

    #[test]
    fn a_config_without_the_key_turns_the_default_off() {
        let config = json!({ "gates": { "other.gate": {} } });
        let gates = FeatureGates::new(Some(&config), "win32", "1.5.1", "");

        assert!(!gates.deobfuscation);
        assert!(!gates.gsm_by_puuid);
    }

    #[test]
    fn an_empty_gate_enables_it_for_every_platform() {
        let config = json!({ "gates": { "ongoing-game.deobfuscation": {} } });
        let gates = FeatureGates::new(Some(&config), "darwin", "1.5.1", "");

        assert!(gates.deobfuscation);
        assert!(!gates.gsm_by_puuid);
    }

    #[test]
    fn platform_and_server_filters_have_to_match() {
        let config = json!({
            "gates": {
                "ongoing-game.deobfuscation": { "platforms": ["win32"] },
                "ongoing-game.gsm-by-puuid": { "sgpServers": ["TENCENT_HN1"] },
            }
        });

        assert!(
            FeatureGates::new(Some(&config), "win32", "1.5.1", "TENCENT_HN1").deobfuscation,
            "win32 is listed"
        );
        assert!(
            !FeatureGates::new(Some(&config), "darwin", "1.5.1", "TENCENT_HN1").deobfuscation,
            "darwin is not"
        );
        assert!(
            FeatureGates::new(Some(&config), "win32", "1.5.1", "TENCENT_HN1").gsm_by_puuid,
            "the server is listed"
        );
        assert!(
            !FeatureGates::new(Some(&config), "win32", "1.5.1", "EUW").gsm_by_puuid,
            "EUW is not"
        );
        assert!(
            !FeatureGates::new(Some(&config), "win32", "1.5.1", "").gsm_by_puuid,
            "an empty server id never matches"
        );
    }

    #[test]
    fn version_bounds_are_inclusive_below_and_exclusive_above() {
        let config = json!({
            "gates": { "ongoing-game.deobfuscation": { "minVersionInclusive": "1.5.0", "maxVersionExclusive": "1.6.0" } }
        });

        assert!(FeatureGates::new(Some(&config), "win32", "1.5.0", "").deobfuscation);
        assert!(FeatureGates::new(Some(&config), "win32", "1.5.9", "").deobfuscation);
        assert!(!FeatureGates::new(Some(&config), "win32", "1.6.0", "").deobfuscation);
        assert!(!FeatureGates::new(Some(&config), "win32", "1.4.9", "").deobfuscation);
    }

    #[test]
    fn an_unparseable_app_version_disables_versioned_gates_only() {
        let config = json!({
            "gates": {
                "ongoing-game.deobfuscation": { "minVersionInclusive": "1.5.0" },
                "ongoing-game.gsm-by-puuid": {},
            }
        });
        let gates = FeatureGates::new(Some(&config), "win32", "not-a-version", "");

        assert!(!gates.deobfuscation);
        assert!(gates.gsm_by_puuid);
    }
}
