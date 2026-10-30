//! The one seam between this crate and the network.
//!
//! Every read the pipeline does is a [`FetchRequest`] handed to a [`Fetcher`], so
//! the loaders, the queue and the state machine are all testable with a stub.
//! `transport::LcuSgpFetcher` is the production one: `LcuHttp` for the `lcu`
//! source and the `riftflow-sgp` gateway for `sgp`.
//!
//! The URL and header builders below are the exact `http-api-axios-helper` calls
//! the original loaders made.

use serde_json::Value;

/// `state.apiShouldUse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiSource {
    Lcu,
    Sgp,
}

impl ApiSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApiSource::Lcu => "lcu",
            ApiSource::Sgp => "sgp",
        }
    }
}

/// `MatchHistoryQueryParams`, minus the abort signal and the `__sgpServerId` the
/// gateway reads off its own snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MatchHistoryQueryParams {
    pub start_index: Option<i64>,
    pub count: Option<i64>,
    pub tag: Option<String>,
    pub tags_query_type: Option<String>,
}

impl MatchHistoryQueryParams {
    /// `{ startIndex: 0, count, ...state.matchHistoryTagParams }`
    pub fn with_tag(start_index: i64, count: i64, tag: Option<String>) -> Self {
        Self {
            start_index: Some(start_index),
            count: Some(count),
            tag,
            tags_query_type: None,
        }
    }

    /// the `Pick<MatchHistoryQueryParams, 'tag' | 'tagsQueryType'>` the state holds
    pub fn to_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert(
            "tag".to_string(),
            self.tag.clone().map(Value::String).unwrap_or(Value::Null),
        );
        map.insert(
            "tagsQueryType".to_string(),
            self.tags_query_type.clone().map(Value::String).unwrap_or(Value::Null),
        );
        Value::Object(map)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FetchRequest {
    pub source: ApiSource,
    pub method: String,
    pub url: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
}

impl FetchRequest {
    fn lcu(url: impl Into<String>) -> Self {
        Self { source: ApiSource::Lcu, method: "GET".to_string(), url: url.into(), query: Vec::new(), headers: Vec::new() }
    }

    fn sgp(url: impl Into<String>, token_type: &str, server_id: Option<String>) -> Self {
        let mut headers = vec![(riftflow_sgp::RIFTFLOW_HEADER_TOKEN_TYPE.to_string(), token_type.to_string())];
        if let Some(server_id) = server_id {
            headers.insert(0, (riftflow_sgp::RIFTFLOW_HEADER_SGP_SERVER_ID.to_string(), server_id));
        }

        Self { source: ApiSource::Sgp, method: "GET".to_string(), url: url.into(), query: Vec::new(), headers }
    }

    fn query(mut self, key: &str, value: Option<impl ToString>) -> Self {
        if let Some(value) = value {
            self.query.push((key.to_string(), value.to_string()));
        }
        self
    }
}

/// `/lol-match-history/v1/products/lol/<puuid>/matches?begIndex&endIndex`
///
/// The LCU takes a *closed* range, so a `count` of 50 asks for `endIndex` 49 — the
/// original computed `count - 1` at the call site and the off-by-one is on purpose.
pub fn lcu_match_history(puuid: &str, params: &MatchHistoryQueryParams) -> FetchRequest {
    let beg = params.start_index.unwrap_or(0);
    let end = params.count.unwrap_or(20) - 1;

    FetchRequest::lcu(format!("/lol-match-history/v1/products/lol/{puuid}/matches"))
        .query("begIndex", Some(beg))
        .query("endIndex", Some(end))
}

/// `/lol-match-history/v1/games/<gameId>` — the summary the LCU list lacks.
pub fn lcu_game(game_id: i64) -> FetchRequest {
    FetchRequest::lcu(format!("/lol-match-history/v1/games/{game_id}"))
}

/// `/lol-match-history/v1/game-timelines/<gameId>`
pub fn lcu_game_details(game_id: i64) -> FetchRequest {
    FetchRequest::lcu(format!("/lol-match-history/v1/game-timelines/{game_id}"))
}

pub fn lcu_summoner(puuid: &str) -> FetchRequest {
    FetchRequest::lcu(format!("/lol-summoner/v2/summoners/puuid/{puuid}"))
}

pub fn lcu_ranked_stats(puuid: &str) -> FetchRequest {
    FetchRequest::lcu(format!("/lol-ranked/v1/ranked-stats/{puuid}"))
}

pub fn lcu_champion_mastery(puuid: &str) -> FetchRequest {
    FetchRequest::lcu(format!("/lol-champion-mastery/v1/{puuid}/champion-mastery"))
}

/// `/match-history-query/v1/products/lol/player/<puuid>/SUMMARY`
pub fn sgp_match_history(
    puuid: &str,
    params: &MatchHistoryQueryParams,
    server_id: Option<String>,
) -> FetchRequest {
    FetchRequest::sgp(
        format!("/match-history-query/v1/products/lol/player/{puuid}/SUMMARY"),
        "entitlements",
        server_id,
    )
    .query("startIndex", params.start_index)
    .query("count", params.count)
    .query("tag", params.tag.clone())
    .query("tagsQueryType", params.tags_query_type.clone())
}

/// the `{subId}` placeholder is expanded by `riftflow_sgp::plan`
pub fn sgp_game_summary(game_id: i64, server_id: Option<String>) -> FetchRequest {
    FetchRequest::sgp(
        format!(
            "/match-history-query/v1/products/lol/{}_{game_id}/SUMMARY",
            riftflow_sgp::URL_PLACEHOLDER_SUB_ID
        ),
        "entitlements",
        server_id,
    )
}

pub fn sgp_game_details(game_id: i64, server_id: Option<String>) -> FetchRequest {
    FetchRequest::sgp(
        format!(
            "/match-history-query/v1/products/lol/{}_{game_id}/DETAILS",
            riftflow_sgp::URL_PLACEHOLDER_SUB_ID
        ),
        "entitlements",
        server_id,
    )
}

/// `sgp.api.gsm.getByPuuid` — the ledge request that answers with the live game.
/// It is the only read here on the `league-session` token.
pub fn sgp_gsm_by_puuid(puuid: &str, server_id: Option<String>) -> FetchRequest {
    FetchRequest::sgp(
        format!(
            "/gsm/v1/ledge/region/{}/puuid/{puuid}",
            riftflow_sgp::URL_PLACEHOLDER_SUB_ID
        ),
        "league-session",
        server_id,
    )
}

/// `isAxiosError(error) && error.response?.status === 404`, which the original
/// treats as "this game is not in the ledger" rather than as a failure.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchError {
    pub status: Option<u16>,
    pub message: String,
}

impl FetchError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { status: None, message: message.into() }
    }

    pub fn with_status(status: u16) -> Self {
        Self { status: Some(status), message: format!("HTTP {status}") }
    }

    pub fn is_status(&self, code: u16) -> bool {
        self.status == Some(code)
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for FetchError {}

#[async_trait::async_trait]
pub trait Fetcher: Send + Sync {
    /// The body of a response with a 2xx status; anything else is a [`FetchError`],
    /// because axios rejected on it.
    async fn fetch(&self, request: FetchRequest) -> Result<Value, FetchError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lcu_history_range_is_closed() {
        let params = MatchHistoryQueryParams::with_tag(0, 50, None);
        let request = lcu_match_history("abc", &params);

        assert_eq!(request.source, ApiSource::Lcu);
        assert_eq!(request.url, "/lol-match-history/v1/products/lol/abc/matches");
        assert_eq!(
            request.query,
            vec![("begIndex".to_string(), "0".to_string()), ("endIndex".to_string(), "49".to_string())]
        );
    }

    #[test]
    fn an_omitted_count_reads_the_axios_default() {
        let params = MatchHistoryQueryParams { start_index: None, count: None, ..Default::default() };
        assert_eq!(lcu_match_history("abc", &params).query[1], ("endIndex".to_string(), "19".to_string()));
    }

    #[test]
    fn sgp_params_are_dropped_when_absent() {
        let params = MatchHistoryQueryParams::with_tag(0, 20, Some("q_420".to_string()));
        let request = sgp_match_history("abc", &params, Some("EUW".to_string()));

        assert_eq!(
            request.query,
            vec![
                ("startIndex".to_string(), "0".to_string()),
                ("count".to_string(), "20".to_string()),
                ("tag".to_string(), "q_420".to_string()),
            ]
        );
        assert_eq!(
            request.headers,
            vec![
                (riftflow_sgp::RIFTFLOW_HEADER_SGP_SERVER_ID.to_string(), "EUW".to_string()),
                (riftflow_sgp::RIFTFLOW_HEADER_TOKEN_TYPE.to_string(), "entitlements".to_string()),
            ]
        );
    }

    #[test]
    fn per_game_sgp_paths_carry_the_placeholder() {
        let summary = sgp_game_summary(123, None);
        assert_eq!(
            summary.url,
            format!(
                "/match-history-query/v1/products/lol/{}_123/SUMMARY",
                riftflow_sgp::URL_PLACEHOLDER_SUB_ID
            )
        );

        let details = sgp_game_details(123, None);
        assert_eq!(
            details.url,
            format!(
                "/match-history-query/v1/products/lol/{}_123/DETAILS",
                riftflow_sgp::URL_PLACEHOLDER_SUB_ID
            )
        );
    }

    #[test]
    fn only_the_ledge_read_uses_the_league_session_token() {
        let gsm = sgp_gsm_by_puuid("abc", Some("TENCENT_HN1".to_string()));

        assert_eq!(
            gsm.url,
            format!("/gsm/v1/ledge/region/{}/puuid/abc", riftflow_sgp::URL_PLACEHOLDER_SUB_ID)
        );
        assert_eq!(
            gsm.headers,
            vec![
                (riftflow_sgp::RIFTFLOW_HEADER_SGP_SERVER_ID.to_string(), "TENCENT_HN1".to_string()),
                (riftflow_sgp::RIFTFLOW_HEADER_TOKEN_TYPE.to_string(), "league-session".to_string()),
            ]
        );

        assert_eq!(
            sgp_match_history("abc", &MatchHistoryQueryParams::default(), None).headers[0],
            (riftflow_sgp::RIFTFLOW_HEADER_TOKEN_TYPE.to_string(), "entitlements".to_string())
        );
    }

    #[test]
    fn the_lcu_reads_use_the_original_paths() {
        assert_eq!(lcu_game(7).url, "/lol-match-history/v1/games/7");
        assert_eq!(lcu_game_details(7).url, "/lol-match-history/v1/game-timelines/7");
        assert_eq!(lcu_summoner("p").url, "/lol-summoner/v2/summoners/puuid/p");
        assert_eq!(lcu_ranked_stats("p").url, "/lol-ranked/v1/ranked-stats/p");
        assert_eq!(lcu_champion_mastery("p").url, "/lol-champion-mastery/v1/p/champion-mastery");
    }

    #[test]
    fn tag_params_serialize_as_the_state_prop() {
        let params = MatchHistoryQueryParams::with_tag(0, 20, Some("q_420".to_string()));
        assert_eq!(params.to_json(), serde_json::json!({ "tag": "q_420", "tagsQueryType": null }));

        let cleared = MatchHistoryQueryParams { tag: None, ..Default::default() };
        assert_eq!(cleared.to_json(), serde_json::json!({ "tag": null, "tagsQueryType": null }));
    }

    #[test]
    fn a_404_is_a_status_and_not_a_transport_failure() {
        assert!(FetchError::with_status(404).is_status(404));
        assert!(!FetchError::new("socket").is_status(404));
    }
}
