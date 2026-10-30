//! The LCU HTTP client: `league-client/index.ts` `_initHttpInstance`,
//! `request`, `requestForRenderer`, plus `peekClient` and `writeItemSetsToDisk`.
//!
//! Wire-compatible details preserved: Basic auth over `https://127.0.0.1:<port>`,
//! self-signed certificate accepted (verified live against the 国服 client),
//! 17500 ms timeout, proxy disabled, two retries, and a separate 8-wide limiter
//! for `lol-game-data/assets` requests.

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, Method};
use serde_json::{Map, Value};
use tokio::sync::Semaphore;

use crate::{ASSET_REQUEST_CONCURRENCY, HTTP_PING_URL, REQUEST_TIMEOUT_MS};

#[derive(Clone, Debug)]
pub struct LcuResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Map<String, Value>,
    pub body: Value,
}

/// What came back off the wire, before any body interpretation.
pub struct LcuRawResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Map<String, Value>,
    pub bytes: Vec<u8>,
}

impl LcuRawResponse {
    /// The two shapes the IPC layer has always exposed: base64 for
    /// `responseType: 'arraybuffer'`, JSON-or-text otherwise.
    pub fn body_to_json(&self, bytes: &[u8], wants_bytes: bool) -> Value {
        if wants_bytes {
            use base64::Engine;
            return Value::String(base64::engine::general_purpose::STANDARD.encode(bytes));
        }

        let text = String::from_utf8_lossy(bytes);
        serde_json::from_str(&text).unwrap_or_else(|_| {
            if text.is_empty() {
                Value::Null
            } else {
                Value::String(text.to_string())
            }
        })
    }
}

/// A request as it arrives over IPC (`AxiosRequestConfig` shaped JSON).
#[derive(Clone, Debug, Default)]
pub struct RequestConfig {
    pub method: String,
    pub url: String,
    pub data: Option<Value>,
    pub params: Map<String, Value>,
    pub headers: Map<String, Value>,
    pub response_type: Option<String>,
}

impl RequestConfig {
    /// Tolerant of the partial config the renderer sends.
    pub fn from_json(value: &Value) -> anyhow::Result<Self> {
        let url = value
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        anyhow::ensure!(!url.is_empty(), "request url is required");

        let as_map = |key: &str| -> Map<String, Value> {
            value
                .get(key)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default()
        };

        Ok(Self {
            method: value
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("get")
                .to_string(),
            url,
            data: value.get("data").cloned(),
            params: as_map("params"),
            headers: as_map("headers"),
            response_type: value.get("responseType").and_then(Value::as_str).map(str::to_string),
        })
    }

    fn method(&self) -> anyhow::Result<Method> {
        Method::from_bytes(self.method.to_uppercase().as_bytes())
            .map_err(|_| anyhow::anyhow!("Unsupported HTTP method: {}", self.method))
    }

    fn is_idempotent(&self) -> bool {
        matches!(
            self.method.to_uppercase().as_str(),
            "GET" | "HEAD" | "OPTIONS" | "PUT" | "DELETE"
        )
    }
}

pub struct LcuHttp {
    client: Client,
    base_url: String,
    asset_limiter: Arc<Semaphore>,
}

impl LcuHttp {
    pub fn new(port: i64, basic_auth: String) -> Self {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_millis(REQUEST_TIMEOUT_MS))
            .no_proxy()
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                if let Ok(value) = basic_auth.parse() {
                    headers.insert(reqwest::header::AUTHORIZATION, value);
                }
                headers
            })
            .build()
            .expect("reqwest client");

        Self {
            client,
            base_url: format!("https://127.0.0.1:{port}"),
            asset_limiter: Arc::new(Semaphore::new(ASSET_REQUEST_CONCURRENCY)),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// `axiosRetry(httpClient, { retries: 2 })` — retried here for network errors
    /// and 5xx on idempotent methods. NOTE: axios-retry's default backoff was not
    /// re-derived from the library, so retries are immediate; recorded as a
    /// checklist item rather than guessed at.
    /// axios' `combineURLs`: exactly one slash between base and path, so callers
    /// may pass either `lol-chat/v1/friends` or `/lol-chat/v1/friends`.
    fn full_url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    pub async fn request(&self, config: RequestConfig) -> anyhow::Result<LcuResponse> {
        let wants_bytes = config
            .response_type
            .as_deref()
            .is_some_and(|kind| kind == "arraybuffer");
        let raw = self.request_limited(&config).await?;

        let body = raw.body_to_json(&raw.bytes, wants_bytes);

        Ok(LcuResponse {
            status: raw.status,
            status_text: raw.status_text,
            headers: raw.headers,
            body,
        })
    }

    /// The `riftflow://league-client/*` proxy answers with the bytes the LCU sent,
    /// so champion icons and other assets survive the round trip unchanged.
    pub async fn request_bytes(&self, config: RequestConfig) -> anyhow::Result<LcuRawResponse> {
        self.request_limited(&config).await
    }

    async fn request_limited(&self, config: &RequestConfig) -> anyhow::Result<LcuRawResponse> {
        let is_asset = config
            .url
            .trim_start_matches('/')
            .starts_with("lol-game-data/assets");
        if is_asset {
            let permit = self
                .asset_limiter
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| anyhow::anyhow!("asset limiter closed"))?;
            let response = self.request_inner(config).await;
            drop(permit);
            return response;
        }
        self.request_inner(config).await
    }

    async fn request_inner(&self, config: &RequestConfig) -> anyhow::Result<LcuRawResponse> {
        let mut attempts = 0;

        loop {
            attempts += 1;
            let result = self.dispatch(config).await;

            match result {
                Ok(response) if response.status < 500 || !config.is_idempotent() => {
                    return Ok(response)
                }
                Ok(response) => {
                    if attempts > 2 {
                        anyhow::bail!(
                            "LCU request failed with status {}",
                            response.status
                        );
                    }
                }
                Err(error) => {
                    if attempts > 2 || !config.is_idempotent() {
                        return Err(error);
                    }
                }
            }
        }
    }

    async fn dispatch(&self, config: &RequestConfig) -> anyhow::Result<LcuRawResponse> {
        let mut builder = self
            .client
            .request(config.method()?, self.full_url(&config.url))
            .query(&config.params);

        for (key, value) in &config.headers {
            let Ok(name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) else {
                continue;
            };
            let Some(text) = value.as_str() else {
                continue;
            };
            if let Ok(header_value) = reqwest::header::HeaderValue::from_str(text) {
                builder = builder.header(name, header_value);
            }
        }

        if let Some(data) = &config.data {
            builder = builder.json(data);
        }

        let response = builder.send().await.map_err(|error| {
            // The reconnect loop only keeps retrying on this specific failure,
            // mirroring the JS check on `error.code === 'ECONNREFUSED'`.
            if error.is_connect() {
                anyhow::anyhow!("ECONNREFUSED: {error}")
            } else {
                anyhow::anyhow!(error.to_string())
            }
        })?;

        let status = response.status().as_u16();
        let status_text = response
            .status()
            .canonical_reason()
            .unwrap_or_default()
            .to_string();

        let mut headers = Map::new();
        for (key, value) in response.headers().iter() {
            headers.insert(
                key.as_str().to_string(),
                Value::String(value.to_str().unwrap_or_default().to_string()),
            );
        }

        let bytes = response.bytes().await.map_err(|e| anyhow::anyhow!(e.to_string()))?;

        Ok(LcuRawResponse {
            status,
            status_text,
            headers,
            bytes: bytes.to_vec(),
        })
    }

    /// `requestForRenderer` — strips the non-serializable parts of an axios
    /// response, keeping only `config.data` and `config.url`.

    pub async fn request_for_renderer(&self, config: RequestConfig) -> anyhow::Result<Value> {
        let url = config.url.clone();
        let sent_data = config.data.clone();
        let response = self.request(config).await?;

        Ok(serde_json::json!({
            "status": response.status,
            "statusText": response.status_text,
            "headers": response.headers,
            "data": response.body,
            "config": { "data": sent_data, "url": url },
        }))
    }

    pub async fn get(&self, path: &str) -> anyhow::Result<Value> {
        let response = self
            .request(RequestConfig {
                method: "GET".to_string(),
                url: path.to_string(),
                ..Default::default()
            })
            .await?;
        Ok(response.body)
    }

    pub async fn put(&self, path: &str, body: Value) -> anyhow::Result<Value> {
        let response = self
            .request(RequestConfig {
                method: "PUT".to_string(),
                url: path.to_string(),
                data: Some(body),
                ..Default::default()
            })
            .await?;
        Ok(response.body)
    }

    pub async fn post(&self, path: &str, body: Value) -> anyhow::Result<Value> {
        let response = self
            .request(RequestConfig {
                method: "POST".to_string(),
                url: path.to_string(),
                data: Some(body),
                ..Default::default()
            })
            .await?;
        Ok(response.body)
    }

    pub async fn patch(&self, path: &str, body: Value) -> anyhow::Result<Value> {
        let response = self
            .request(RequestConfig {
                method: "PATCH".to_string(),
                url: path.to_string(),
                data: Some(body),
                ..Default::default()
            })
            .await?;
        Ok(response.body)
    }

    pub async fn delete(&self, path: &str) -> anyhow::Result<Value> {
        let response = self
            .request(RequestConfig {
                method: "DELETE".to_string(),
                url: path.to_string(),
                ..Default::default()
            })
            .await?;
        Ok(response.body)
    }

    /// The connection health probe. On the 国服 client `/version` answers 404
    /// `Invalid function`, so this endpoint — not `/version` — is the probe.
    pub async fn ping(&self) -> anyhow::Result<()> {
        self.get(HTTP_PING_URL).await?;
        Ok(())
    }

    pub fn limiter_permits(&self) -> usize {
        self.asset_limiter.available_permits()
    }
}

/// `writeItemSetsToDisk` path selection. 国服 writes to
/// `<install>/../Game/Config/Global/Recommended`, Riot regions to
/// `<install>/Config/Global/Recommended`. Node's `join(dir, '..', …)` drops the
/// final segment, which is what `parent()` does here.
pub fn item_set_target_dir(install_dir: &str, region: &str) -> std::path::PathBuf {
    use std::path::Path;

    let base = Path::new(install_dir);
    if region == "TENCENT" {
        match base.parent() {
            Some(parent) => parent
                .join("Game")
                .join("Config")
                .join("Global")
                .join("Recommended"),
            None => base
                .join("Game")
                .join("Config")
                .join("Global")
                .join("Recommended"),
        }
    } else {
        base.join("Config").join("Global").join("Recommended")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tencent_region_walks_up_to_the_game_folder() {
        let path = item_set_target_dir(r"D:\WeGameApps\LOL\LeagueClient", "TENCENT");
        assert_eq!(
            path,
            std::path::PathBuf::from(r"D:\WeGameApps\LOL")
                .join("Game")
                .join("Config")
                .join("Global")
                .join("Recommended")
        );
    }

    #[test]
    fn riot_region_keeps_the_install_dir() {
        let path = item_set_target_dir("/install/League of Legends", "NA1");
        assert_eq!(
            path,
            std::path::Path::new("/install/League of Legends")
                .join("Config")
                .join("Global")
                .join("Recommended")
        );
    }

    #[test]
    fn request_config_defaults_match_axios() {
        let config =
            RequestConfig::from_json(&serde_json::json!({ "url": "/lol-gameflow/v1/act" })).unwrap();
        assert_eq!(config.method, "get");
        assert!(config.data.is_none());

        let err = RequestConfig::from_json(&serde_json::json!({})).unwrap_err();
        assert_eq!(err.to_string(), "request url is required");
    }

    /// The `riftflow://` router hands over the path without its leading slash, while
    /// the ported helpers pass it with one; axios accepts both.
    #[test]
    fn both_path_forms_join_the_base_url_once() {
        let http = LcuHttp::new(52611, "Basic xyz".to_string());
        assert_eq!(
            http.full_url("/lol-chat/v1/friends"),
            "https://127.0.0.1:52611/lol-chat/v1/friends"
        );
        assert_eq!(
            http.full_url("lol-chat/v1/friends"),
            "https://127.0.0.1:52611/lol-chat/v1/friends"
        );
    }
}
