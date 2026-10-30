//! The `leagueClient.data` sub-states this shard reads, taken as one snapshot per
//! pass.
//!
//! The original held a live `LeagueClientData` observable. Here the same values
//! live in [`riftflow_state::StateSync`] under `league-client-main` (hydrated and
//! pushed by `riftflow-app/src/lc_state.rs`), and every computed in `computed-state.ts`
//! is fed a *copy* of them so it stays a pure function: `LcSnapshot::from_state`
//! for the pipeline, `LcSnapshot::from_json` for tests and fixtures.

use riftflow_state::StateSync;
use serde_json::Value;

use crate::fetch::ApiSource;

pub const LEAGUE_CLIENT_NAMESPACE: &str = "league-client-main";
pub const SGP_NAMESPACE: &str = "sgp-main";
pub const APP_COMMON_NAMESPACE: &str = "app-common-main";

#[derive(Clone, Debug, Default)]
pub struct LcSnapshot {
    /// `data.gameflow.phase`, the top-level prop, not the session's own phase
    pub gameflow_phase: String,
    pub gameflow_session: Option<Value>,
    pub lobby: Option<Value>,
    pub champ_select_session: Option<Value>,
    pub me: Option<Value>,
    /// `leagueClient.state.auth` — `{ region, rsoPlatformId, … }` while connected
    pub auth: Option<Value>,
    /// `appCommon.settings.preferredLolSource`
    pub preferred_lol_source: String,
    /// `sgp.state.availability.serversSupported.matchHistory`
    pub sgp_match_history_supported: bool,
    /// `sgp.state.isTokenReady`
    pub sgp_token_ready: bool,
}

fn present(value: Option<Value>) -> Option<Value> {
    value.filter(|value| !value.is_null())
}

impl LcSnapshot {
    pub fn from_state(state_sync: &StateSync) -> Self {
        let read = |state_id: &str, path: &str| state_sync.get(LEAGUE_CLIENT_NAMESPACE, state_id, path);

        Self {
            gameflow_phase: read("gameflow", "phase")
                .and_then(|phase| phase.as_str().map(str::to_string))
                .unwrap_or_else(|| "None".to_string()),
            gameflow_session: present(read("gameflow", "session")),
            lobby: present(read("lobby", "lobby")),
            champ_select_session: present(read("champSelect", "session")),
            me: present(read("summoner", "me")),
            auth: present(read("state", "auth")),
            preferred_lol_source: state_sync
                .get(APP_COMMON_NAMESPACE, "settings", "preferredLolSource")
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_else(|| "sgp".to_string()),
            sgp_match_history_supported: state_sync
                .get(SGP_NAMESPACE, "state", "availability")
                .and_then(|availability| {
                    availability
                        .pointer("/serversSupported/matchHistory")
                        .and_then(Value::as_bool)
                })
                .unwrap_or(false),
            sgp_token_ready: state_sync
                .get(SGP_NAMESPACE, "state", "isTokenReady")
                .and_then(|ready| ready.as_bool())
                .unwrap_or(false),
        }
    }

    /// The same shape `leagueClient.data` has, for fixtures.
    pub fn from_json(value: &Value) -> Self {
        let data = |state_id: &str, path: &str| {
            present(value.get(state_id).and_then(|sub| sub.get(path)).cloned())
        };

        Self {
            gameflow_phase: value
                .pointer("/gameflow/phase")
                .and_then(Value::as_str)
                .unwrap_or("None")
                .to_string(),
            gameflow_session: data("gameflow", "session"),
            lobby: data("lobby", "lobby"),
            champ_select_session: data("champSelect", "session"),
            me: data("summoner", "me"),
            auth: data("state", "auth"),
            preferred_lol_source: value
                .pointer("/appCommon/preferredLolSource")
                .and_then(Value::as_str)
                .unwrap_or("sgp")
                .to_string(),
            sgp_match_history_supported: value
                .pointer("/sgp/serversSupported/matchHistory")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            sgp_token_ready: value
                .pointer("/sgp/isTokenReady")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }

    /// `state.apiShouldUse` — SGP only when preferred *and* the server supports it.
    pub fn api_should_use(&self) -> ApiSource {
        if self.preferred_lol_source == "sgp" && self.sgp_match_history_supported {
            ApiSource::Sgp
        } else {
            ApiSource::Lcu
        }
    }

    pub fn self_puuid(&self) -> Option<String> {
        self.me
            .as_ref()
            .and_then(|me| me.get("puuid"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// `leagueClient.state.auth` — the two ids `loadSavedInfo` and the end-of-game
    /// recording need. `None` when the client is not connected.
    pub fn auth(&self) -> Option<(String, String)> {
        let auth = self.auth.as_ref()?;
        let region = auth.get("region").and_then(Value::as_str).unwrap_or_default();
        let platform = auth.get("rsoPlatformId").and_then(Value::as_str).unwrap_or_default();

        Some((region.to_string(), platform.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sgp_wins_only_when_the_server_supports_match_history() {
        let preferred = json!({
            "appCommon": { "preferredLolSource": "sgp" },
            "sgp": { "serversSupported": { "matchHistory": true } },
        });
        assert_eq!(LcSnapshot::from_json(&preferred).api_should_use(), ApiSource::Sgp);

        let unsupported = json!({
            "appCommon": { "preferredLolSource": "sgp" },
            "sgp": { "serversSupported": { "matchHistory": false } },
        });
        assert_eq!(LcSnapshot::from_json(&unsupported).api_should_use(), ApiSource::Lcu);

        let lcu = json!({ "appCommon": { "preferredLolSource": "lcu" } });
        assert_eq!(LcSnapshot::from_json(&lcu).api_should_use(), ApiSource::Lcu);
    }

    #[test]
    fn a_null_sub_state_reads_as_absent() {
        let snapshot = LcSnapshot::from_json(&json!({
            "gameflow": { "phase": "None", "session": null },
            "lobby": { "lobby": null },
        }));

        assert!(snapshot.gameflow_session.is_none());
        assert!(snapshot.lobby.is_none());
        assert_eq!(snapshot.gameflow_phase, "None");
    }

    #[test]
    fn the_saved_player_query_needs_both_the_self_puuid_and_the_auth() {
        let snapshot = LcSnapshot::from_json(&json!({
            "summoner": { "me": { "puuid": "self" } },
            "state": { "auth": { "region": "HN1", "rsoPlatformId": "TENCENT" } },
        }));

        assert_eq!(snapshot.self_puuid().as_deref(), Some("self"));
        assert_eq!(
            snapshot.auth(),
            Some(("HN1".to_string(), "TENCENT".to_string()))
        );

        // a disconnected client has no auth prop at all
        assert_eq!(LcSnapshot::default().auth(), None);
        assert_eq!(
            LcSnapshot::from_json(&json!({ "state": { "auth": null } })).auth(),
            None
        );
    }
}
