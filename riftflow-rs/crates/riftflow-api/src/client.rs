//! The HTTP client: `bootstrap-controller.ts`'s three axios instances and
//! `http-api-axios-helper/riftflow/{api,static}.ts`'s request construction.
//!
//! Kept to one reqwest client per axios instance, because the two service domains
//! differ in exactly one default header: the API asks for JSON, the file host does
//! not. Every default carries `User-Agent: RiftFlow/<version>` and
//! `x-riftflow-version`, and every request has the original's 10 second timeout.
//!
//! The read helpers return the parsed JSON document *before* validation: the
//! runtime has to tell "the server said no" apart from "the server said something
//! zod would reject", because only the first one is retried on the next tick.

use std::sync::RwLock;

use serde_json::{Value, json};

use crate::config::{
    RIFTFLOW_API_BOOTSTRAP_NPM_LATEST_URL, RIFTFLOW_API_REQUEST_TIMEOUT, RIFTFLOW_PROXY_REQUEST_ID_HEADER,
    RiftFlowServiceBaseUrls, ConfigResource,
};
use crate::url::{RiftFlowRoute, RiftFlowUrlDomain, combine_url, normalize_path, resolve_static_url};
use crate::validate::{BootstrapDocument, validate_bootstrap_document};

/// What `protocol.rs` needs back: status, the headers worth forwarding, the bytes.
#[derive(Debug)]
pub struct RiftFlowForwardResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub struct RiftFlowApiClient {
    api: reqwest::Client,
    static_assets: reqwest::Client,
    npm: reqwest::Client,
    base_urls: RwLock<RiftFlowServiceBaseUrls>,
    app_version: String,
}

impl RiftFlowApiClient {
    /// The two axios instances plus the npm client, built with the defaults.
    pub fn new(app_version: &str) -> Self {
        Self::with_base_urls(
            app_version,
            RiftFlowServiceBaseUrls::default(),
        )
    }

    pub fn with_base_urls(app_version: &str, base_urls: RiftFlowServiceBaseUrls) -> Self {
        // every axios instance carried `User-Agent` and `x-riftflow-version`; the API
        // instance alone added `Accept: application/json`
        let default_headers = |accept_json: bool| {
            let mut headers = reqwest::header::HeaderMap::new();
            if let Ok(value) = reqwest::header::HeaderValue::from_str(app_version) {
                headers.insert("x-riftflow-version", value);
            }
            if accept_json {
                headers.insert(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("application/json"),
                );
            }
            headers
        };

        let build = |accept_json: bool| {
            reqwest::ClientBuilder::new()
                .timeout(RIFTFLOW_API_REQUEST_TIMEOUT)
                // axios follows up to five redirects; a config host may answer a
                // 302 when a document moved
                .redirect(reqwest::redirect::Policy::limited(5))
                .user_agent(user_agent(app_version))
                .default_headers(default_headers(accept_json))
                .build()
                .expect("riftflow http client")
        };

        Self {
            // `apiHttp`
            api: build(true),
            // `staticHttp`: the same identity headers, no Accept
            static_assets: build(false),
            // `_npmHttp`: version only, and no base url — the URL is absolute
            npm: build(false),
            base_urls: RwLock::new(base_urls),
            app_version: app_version.to_string(),
        }
    }

    pub fn app_version(&self) -> &str {
        &self.app_version
    }

    pub fn base_urls(&self) -> RiftFlowServiceBaseUrls {
        self.base_urls.read().unwrap().clone()
    }

    /// `_applyBootstrap`: the generation the shard discovered replaces both hosts.
    pub fn set_base_urls(&self, base_urls: RiftFlowServiceBaseUrls) {
        *self.base_urls.write().unwrap() = base_urls;
    }

    /// `_fetchLatestBootstrap` — the npm package's `riftflowBootstrap` field.
    pub async fn get_bootstrap(&self) -> anyhow::Result<BootstrapDocument> {
        let response = self
            .npm
            .get(std::env::var("RIFTFLOW_BOOTSTRAP_URL").unwrap_or_else(|_| RIFTFLOW_API_BOOTSTRAP_NPM_LATEST_URL.to_string()))
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("bootstrap request failed: {error}"))?;

        let status = response.status().as_u16();
        let text = response
            .text()
            .await
            .map_err(|error| anyhow::anyhow!("bootstrap body failed: {error}"))?;
        if !(200..300).contains(&status) {
            anyhow::bail!("{}", describe_failure(status, &text));
        }

        let metadata: Value = serde_json::from_str(&text)
            .map_err(|error| anyhow::anyhow!("bootstrap is not json: {error}"))?;
        let document = metadata
            .get("riftflowBootstrap")
            .ok_or_else(|| anyhow::anyhow!("riftflowBootstrap is missing from the npm metadata"))?;
        validate_bootstrap_document(document)
            .map_err(|error| anyhow::anyhow!(error))
    }

    /// `api.getConfig(resource)` — `/config/v1/<resource>`, unparsed.
    pub async fn get_config_document(&self, resource: ConfigResource) -> anyhow::Result<Value> {
        self.get_json(
            RiftFlowUrlDomain::RiftFlowApi,
            &resource.api_path(),
            &[],
        )
        .await
    }

    /// `api.getLatestNotice(lang)`
    pub async fn get_notice_document(&self, language: &str) -> anyhow::Result<Value> {
        self.get_json(
            RiftFlowUrlDomain::RiftFlowApi,
            "/notice/v1/latest",
            &[("lang", language)],
        )
        .await
    }

    /// `api.getContactChannels()`
    pub async fn get_contact_channels_document(&self) -> anyhow::Result<Value> {
        self.get_json(RiftFlowUrlDomain::RiftFlowApi, "/website/v1/contact-channels", &[])
            .await
    }

    /// `api.getLatestRelease(lang)`
    pub async fn get_latest_release_document(&self, language: &str) -> anyhow::Result<Value> {
        self.get_json(
            RiftFlowUrlDomain::RiftFlowApi,
            "/releases/v1/latest",
            &[("lang", language)],
        )
        .await
    }

    /// `api.postStatisticsRecord(version)`
    pub async fn post_statistics_record(&self, version: &str) -> anyhow::Result<Value> {
        let url = self.url_for(RiftFlowUrlDomain::RiftFlowApi, "/statistics/v1/records", &[]);
        let response = self
            .api
            .post(&url)
            .json(&json!({ "version": version }))
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("statistics request failed: {error}"))?;
        read_json(response).await
    }

    /// `staticAssets.getFile(path)` — one file off the static host.
    pub async fn get_static_file(&self, path: &str) -> anyhow::Result<RiftFlowForwardResponse> {
        let url = self.static_url(path);
        self.fetch_bytes(&self.static_assets, &url).await
    }

    /// `staticAssets.resolveUrl(path)`
    pub fn static_url(&self, path: &str) -> String {
        resolve_static_url(&self.base_urls().static_assets, path)
    }

    /// The full URL for one API path, query pairs included. Exposed because the
    /// router and the client must never disagree about a path.
    pub fn url_for(&self, domain: RiftFlowUrlDomain, path: &str, query: &[(&str, &str)]) -> String {
        let base = self.base_urls();
        with_query(
            combine_url(base.for_domain(domain), &normalize_path(path)),
            query,
        )
    }

    async fn get_json(
        &self,
        domain: RiftFlowUrlDomain,
        path: &str,
        query: &[(&str, &str)],
    ) -> anyhow::Result<Value> {
        let url = self.url_for(domain, path, query);
        let response = self
            .api
            .get(&url)
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("{path} request failed: {error}"))?;
        read_json(response).await
    }

    /// The `riftflow://riftflow-api/…` and `riftflow://riftflow-static/…` proxy: the renderer's
    /// headers go along (minus the cancellation id), the answer comes back raw, and
    /// any status is a success — `validateStatus: () => true`.
    pub async fn forward(
        &self,
        route: &RiftFlowRoute,
        method: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> anyhow::Result<RiftFlowForwardResponse> {
        let RiftFlowRoute::Forward {
            domain, target_url, ..
        } = route
        else {
            anyhow::bail!("only a forwarded riftflow:// route can be proxied");
        };

        anyhow::ensure!(
            method_supported(route, method),
            "{method} is not allowed on this riftflow-api endpoint"
        );

        let client = match domain {
            RiftFlowUrlDomain::RiftFlowApi => &self.api,
            RiftFlowUrlDomain::RiftFlowStatic => &self.static_assets,
        };
        let method = reqwest::Method::from_bytes(method.to_ascii_uppercase().as_bytes())
            .map_err(|_| anyhow::anyhow!("unsupported method {method}"))?;

        let mut request = client.request(method, target_url);
        for (name, value) in forwarded_request_headers(headers) {
            request = request.header(name, value);
        }
        if !body.is_empty() {
            request = request.body(body.to_vec());
        }

        let response = request
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("riftflow proxy request failed: {error}"))?;

        let status = response.status().as_u16();
        let mut out = Vec::new();
        for (name, value) in response.headers() {
            let name = name.as_str().to_string();
            if is_hop_by_hop_response_header(&name) {
                continue;
            }
            out.push((name, value.to_str().unwrap_or_default().to_string()));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| anyhow::anyhow!("riftflow proxy body failed: {error}"))?;

        Ok(RiftFlowForwardResponse {
            status,
            headers: out,
            body: bytes.to_vec(),
        })
    }

    async fn fetch_bytes(
        &self,
        client: &reqwest::Client,
        url: &str,
    ) -> anyhow::Result<RiftFlowForwardResponse> {
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("static request failed: {error}"))?;

        let status = response.status().as_u16();
        let mut headers = Vec::new();
        for (name, value) in response.headers() {
            let name = name.as_str().to_string();
            if is_hop_by_hop_response_header(&name) {
                continue;
            }
            headers.push((name, value.to_str().unwrap_or_default().to_string()));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| anyhow::anyhow!("static body failed: {error}"))?;

        Ok(RiftFlowForwardResponse {
            status,
            headers,
            body: bytes.to_vec(),
        })
    }
}

/// `RiftFlow/1.5.1` — every request's identity, exactly as axios set it.
pub fn user_agent(app_version: &str) -> String {
    format!("RiftFlow/{app_version}")
}

/// Whether a proxied method may reach this route. A local reply never needs it.
pub fn method_supported(route: &RiftFlowRoute, method: &str) -> bool {
    match route {
        RiftFlowRoute::Forward {
            endpoint: Some(endpoint),
            ..
        } => endpoint.supports_method(method),
        RiftFlowRoute::Forward { .. } => true,
        RiftFlowRoute::Reply { .. } | RiftFlowRoute::NotOurs => false,
    }
}

/// The request headers the original passes on: everything the renderer sent minus
/// `RIFTFLOW_PROXY_REQUEST_ID_HEADER`, plus the instance defaults for any key the
/// renderer did not set.
pub fn forwarded_request_headers(
    headers: &[(String, String)],
) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| {
            !name.eq_ignore_ascii_case(RIFTFLOW_PROXY_REQUEST_ID_HEADER) && !is_hop_by_hop_request_header(name)
        })
        .cloned()
        .collect()
}

fn is_hop_by_hop_request_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host" | "content-length" | "connection" | "transfer-encoding" | "accept-encoding"
            | "upgrade"
    )
}

/// `content-encoding` is dropped because reqwest already inflated the body, and
/// `content-length` because the byte count we hand back is the decoded one.
fn is_hop_by_hop_response_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-encoding"
            | "content-length"
    )
}

fn with_query(url: String, query: &[(&str, &str)]) -> String {
    if query.is_empty() {
        return url;
    }
    let encoded: Vec<String> = query
        .iter()
        .map(|(key, value)| format!("{key}={}", encode_query_value(value)))
        .collect();
    format!("{url}?{}", encoded.join("&"))
}

fn encode_query_value(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'.' | b'_' | b'~' | b':')
            {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

async fn read_json(response: reqwest::Response) -> anyhow::Result<Value> {
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|error| anyhow::anyhow!("body failed: {error}"))?;

    if !(200..300).contains(&status) {
        return Err(anyhow::anyhow!(describe_failure(status, &text)));
    }

    serde_json::from_str(&text).map_err(|error| anyhow::anyhow!("response is not json: {error}"))
}

/// `formatError` over an `RiftFlowApiErrorBody` — the code is what the logs key on.
fn describe_failure(status: u16, text: &str) -> String {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => {
            let code = value
                .pointer("/error/code")
                .and_then(Value::as_str)
                .unwrap_or("HttpError");
            let message = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or(text);
            format!("status {status} {code}: {message}")
        }
        Err(_) => format!("status {status}: {}", text.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DEFAULT_RIFTFLOW_API_BASE_URL, DEFAULT_RIFTFLOW_STATIC_BASE_URL};
    use crate::url::{is_absolute_url, percent_decode, route};

    #[test]
    fn the_client_starts_at_the_default_service_origins() {
        let client = RiftFlowApiClient::new("1.5.1");
        let urls = client.base_urls();
        assert_eq!(urls.api, DEFAULT_RIFTFLOW_API_BASE_URL);
        assert_eq!(urls.static_assets, DEFAULT_RIFTFLOW_STATIC_BASE_URL);
        assert_eq!(user_agent("1.5.1"), "RiftFlow/1.5.1");
    }

    #[test]
    fn a_bootstrap_generation_moves_the_client_with_the_router() {
        let client = RiftFlowApiClient::with_base_urls(
            "1.5.1",
            RiftFlowServiceBaseUrls {
                api: "https://api-2.example.com".to_string(),
                static_assets: "https://static-2.example.com".to_string(),
            },
        );

        let decision = route(
            "riftflow://riftflow-api/config/v1/app/feature-gates",
            &client.base_urls(),
        );
        match &decision {
            RiftFlowRoute::Forward { target_url, .. } => {
                assert_eq!(
                    target_url,
                    "https://api-2.example.com/config/v1/app/feature-gates"
                );
                // the client builds the same url for the same path
                assert_eq!(
                    target_url,
                    &client.url_for(
                        RiftFlowUrlDomain::RiftFlowApi,
                        "/config/v1/app/feature-gates",
                        &[]
                    )
                );
            }
            other => panic!("expected a forward, got {other:?}"),
        }
    }

    #[test]
    fn query_pairs_are_encoded_like_axios_params() {
        let client = RiftFlowApiClient::new("1.5.1");
        assert_eq!(
            client.url_for(RiftFlowUrlDomain::RiftFlowApi, "/notice/v1/latest", &[("lang", "zh-CN")]),
            "https://riftflow-api.yymainy.online/notice/v1/latest?lang=zh-CN"
        );
        assert_eq!(
            client.url_for(RiftFlowUrlDomain::RiftFlowApi, "/notice/v1/latest", &[]),
            "https://riftflow-api.yymainy.online/notice/v1/latest"
        );
    }

    #[test]
    fn the_static_file_url_is_the_resolved_object_name() {
        let client = RiftFlowApiClient::new("1.5.1");
        assert_eq!(
            client.static_url("RiftFlow Desktop-1.5.1-win.7z"),
            "https://riftflow-api.yymainy.online/static/RiftFlow%20Desktop-1.5.1-win.7z"
        );
        assert_eq!(
            client.static_url("https://cdn.example.com/file.7z"),
            "https://cdn.example.com/file.7z"
        );
    }

    #[test]
    fn the_proxy_drops_the_cancellation_header_and_the_hops() {
        let headers = vec![
            ("x-riftflow-proxy-request-id".to_string(), "id".to_string()),
            ("x-test".to_string(), "test".to_string()),
            ("host".to_string(), "evil.example".to_string()),
            ("content-length".to_string(), "12".to_string()),
        ];
        assert_eq!(
            forwarded_request_headers(&headers),
            vec![("x-test".to_string(), "test".to_string())]
        );
    }

    #[test]
    fn only_the_allowlisted_api_methods_are_forwarded() {
        let client = RiftFlowApiClient::new("1.5.1");
        let notice = route("riftflow://riftflow-api/notice/v1/latest?lang=en", &client.base_urls());
        assert!(method_supported(&notice, "GET"));
        assert!(!method_supported(&notice, "POST"));

        let records = route("riftflow://riftflow-api/statistics/v1/records", &client.base_urls());
        assert!(method_supported(&records, "POST"));
        assert!(!method_supported(&records, "GET"));

        // the file host answers anything a webview asks for
        let file = route("riftflow://riftflow-static/a.png", &client.base_urls());
        assert!(method_supported(&file, "GET"));
        assert!(method_supported(&file, "HEAD"));

        assert!(!method_supported(&RiftFlowRoute::NotOurs, "GET"));
    }

    #[test]
    fn a_local_reply_exposes_the_wire_parts_for_the_protocol_layer() {
        let decision = route("riftflow://riftflow-api/admin/v1/secrets", &RiftFlowServiceBaseUrls::default());
        let (status, headers, body) = decision.local_parts().expect("a local reply");
        assert_eq!(status, 404);
        assert_eq!(
            headers,
            vec![("content-type".to_string(), "application/json".to_string())]
        );
        assert!(String::from_utf8_lossy(&body).contains("UnknownRiftFlowApiPath"));
    }

    #[test]
    fn a_failure_reads_as_the_api_error_it_came_back_as() {
        assert_eq!(
            describe_failure(404, r#"{"error":{"code":"NotFound","message":"no such config"}}"#),
            "status 404 NotFound: no such config"
        );
        assert_eq!(describe_failure(500, "boom"), "status 500: boom");
    }

    #[tokio::test]
    async fn a_local_reply_is_never_sent_to_the_network() {
        let client = RiftFlowApiClient::new("1.5.1");
        let decision = route("riftflow://riftflow-api/admin/v1/secrets", &client.base_urls());

        let error = client
            .forward(&decision, "GET", &[], &[])
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("only a forwarded"), "{error}");
        assert!(decision.local_parts().is_some(), "the app answers it locally");
    }

    #[tokio::test]
    async fn a_post_to_a_read_endpoint_is_refused_before_the_request() {
        let client = RiftFlowApiClient::new("1.5.1");
        let decision = route("riftflow://riftflow-api/notice/v1/latest?lang=en", &client.base_urls());
        assert!(
            client.forward(&decision, "POST", &[], &[]).await.is_err(),
            "notice is a read; the proxy must not turn it into a write"
        );
    }

    #[test]
    fn static_paths_survive_a_decode_encode_round_trip() {
        assert!(!is_absolute_url("config/v1/app/feature-gates"));
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(
            normalize_path(&percent_decode("/icons/../icons/103.png")),
            "icons/103.png"
        );
        assert_eq!(
            combine_url("https://h", &normalize_path(&percent_decode("/a/./b/..%2fc"))),
            "https://h/a/c"
        );
    }
}
