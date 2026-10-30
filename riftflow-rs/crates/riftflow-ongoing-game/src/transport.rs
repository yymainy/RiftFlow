//! The production [`Fetcher`]: `LcuHttp` for `lcu`, the `riftflow-sgp` gateway for
//! `sgp`, with the snapshot `sgp/http-client-controller.ts` used to read off
//! `leagueClient` and `SgpMain.state`.
//!
//! The original axios instances rejected on any non-2xx status, so both transports
//! report the status code back through [`FetchError::status`] — that is how
//! `additional-info-controller.ts` turns a 404 into "no such game".

use std::sync::Arc;

use riftflow_lcu::http::RequestConfig;
use riftflow_lcu::LeagueClientMain;
use riftflow_sgp::{SgpGateway, SgpSnapshot};
use riftflow_state::StateSync;
use serde_json::Value;

use crate::fetch::{ApiSource, FetchError, FetchRequest, Fetcher};
use crate::snapshot::{APP_COMMON_NAMESPACE, LEAGUE_CLIENT_NAMESPACE, SGP_NAMESPACE};

pub struct LcuSgpFetcher {
    league_client: Arc<LeagueClientMain>,
    sgp: Arc<SgpGateway>,
    state_sync: Arc<StateSync>,
}

impl LcuSgpFetcher {
    pub fn new(
        league_client: Arc<LeagueClientMain>,
        sgp: Arc<SgpGateway>,
        state_sync: Arc<StateSync>,
    ) -> Arc<Self> {
        Arc::new(Self { league_client, sgp, state_sync })
    }

    /// `preferredLolSource`, the app-common setting the state's `apiShouldUse` reads.
    pub fn preferred_lol_source(&self) -> String {
        self.state_sync
            .get(APP_COMMON_NAMESPACE, "settings", "preferredLolSource")
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "sgp".to_string())
    }

    fn sgp_snapshot(&self) -> SgpSnapshot {
        let servers = self
            .state_sync
            .get(SGP_NAMESPACE, "state", "leagueServers")
            .and_then(|config| config.get("servers").cloned())
            .unwrap_or(Value::Null);

        let default_server_id = self
            .state_sync
            .get(SGP_NAMESPACE, "state", "availability")
            .and_then(|availability| {
                availability.get("sgpServerId").and_then(Value::as_str).map(str::to_string)
            })
            .unwrap_or_default();

        let token = |state_id: &str, object_field: Option<&str>| {
            self.state_sync
                .get(LEAGUE_CLIENT_NAMESPACE, state_id, "token")
                .and_then(|token| match object_field {
                    Some(field) => token
                        .get(field)
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    None => token.as_str().map(str::to_string),
                })
        };

        SgpSnapshot {
            servers,
            default_server_id,
            entitlements_token: token("entitlements", Some("accessToken")),
            league_session_token: token("leagueSession", None),
        }
    }
}

#[async_trait::async_trait]
impl Fetcher for LcuSgpFetcher {
    async fn fetch(&self, request: FetchRequest) -> Result<Value, FetchError> {
        match request.source {
            ApiSource::Lcu => self.fetch_lcu(request).await,
            ApiSource::Sgp => self.fetch_sgp(request).await,
        }
    }
}

impl LcuSgpFetcher {
    async fn fetch_lcu(&self, request: FetchRequest) -> Result<Value, FetchError> {
        let Some(http) = self.league_client.http() else {
            return Err(FetchError::new("LeagueClient is not connected"));
        };

        let mut params = serde_json::Map::new();
        for (key, value) in &request.query {
            params.insert(key.clone(), Value::String(value.clone()));
        }

        let mut headers = serde_json::Map::new();
        for (key, value) in &request.headers {
            headers.insert(key.clone(), Value::String(value.clone()));
        }

        let config = RequestConfig {
            method: request.method.clone(),
            url: request.url.clone(),
            params,
            headers,
            ..Default::default()
        };

        match http.request(config).await {
            Ok(response) if response.status < 400 => Ok(response.body),
            Ok(response) => Err(FetchError::with_status(response.status)),
            Err(error) => Err(FetchError::new(error.to_string())),
        }
    }

    async fn fetch_sgp(&self, request: FetchRequest) -> Result<Value, FetchError> {
        let snapshot = self.sgp_snapshot();

        // the query string is part of the URL for the gateway, which has no `params`
        let url = if request.query.is_empty() {
            request.url.clone()
        } else {
            let query = request
                .query
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("&");
            format!("{}?{query}", request.url)
        };

        match self
            .sgp
            .request(&snapshot, &request.method, &url, &request.headers, &[])
            .await
        {
            Ok(reply) if reply.status < 400 => Ok(serde_json::from_slice(&reply.body).unwrap_or(Value::Null)),
            Ok(reply) => Err(FetchError::with_status(reply.status)),
            Err(error) => Err(FetchError::new(error.message().to_string())),
        }
    }
}
