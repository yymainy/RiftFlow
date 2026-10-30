//! The endpoints, hosts, domains and cadence of the remote-config shard.
//!
//! Verbatim from the original: `shared/shards/riftflow-api/types.ts` (hosts,
//! language), `main/shards/riftflow-api/context.ts` (intervals),
//! `main/shards/riftflow-api/cached-resources.ts` (resource id → API path →
//! cache file) and `main/shards/riftflow-api/bootstrap-controller.ts` (npm
//! discovery, timeout).

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// `DEFAULT_RIFTFLOW_API_BASE_URL`
pub const DEFAULT_RIFTFLOW_API_BASE_URL: &str = "https://riftflow-api.yymainy.online";
/// `DEFAULT_RIFTFLOW_STATIC_BASE_URL`
pub const DEFAULT_RIFTFLOW_STATIC_BASE_URL: &str = "https://riftflow-api.yymainy.online/static";

/// `DEFAULT_RIFTFLOW_API_LANGUAGE`
pub const DEFAULT_RIFTFLOW_API_LANGUAGE: &str = "zh-CN";
/// `RiftFlowApiLanguage` — the only two values the API accepts for `?lang=`.
pub const RIFTFLOW_API_LANGUAGES: [&str; 2] = ["zh-CN", "en"];

/// `RIFTFLOW_API_PROTOCOL_DOMAIN`
pub const RIFTFLOW_API_PROTOCOL_DOMAIN: &str = "riftflow-api";
/// `RIFTFLOW_STATIC_PROTOCOL_DOMAIN`
pub const RIFTFLOW_STATIC_PROTOCOL_DOMAIN: &str = "riftflow-static";
/// `RIFTFLOW_PROXY_PROTOCOL`
pub const RIFTFLOW_PROXY_PROTOCOL: &str = "riftflow";
/// `RIFTFLOW_PROXY_REQUEST_ID_HEADER` — stripped before the request leaves.
pub const RIFTFLOW_PROXY_REQUEST_ID_HEADER: &str = "x-riftflow-proxy-request-id";

/// `RiftFlowApiMain.id` — also the config-cache namespace.
pub const RIFTFLOW_API_MAIN_NAMESPACE: &str = "riftflow-api-main";
pub const SGP_MAIN_NAMESPACE: &str = "sgp-main";
pub const AUTO_SELECT_MAIN_NAMESPACE: &str = "auto-select-main";
/// `SetterSettingService.CONFIG_DIR_NAME` — `<userData>/RiftFlowConfig/<namespace>/…`
pub const CONFIG_DIR_NAME: &str = "RiftFlowConfig";

/// `RIFTFLOW_API_REQUEST_TIMEOUT = 10_000`
pub const RIFTFLOW_API_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// `RIFTFLOW_API_FEATURE_GATES_UPDATE_INTERVAL` / `RIFTFLOW_API_CACHED_RESOURCE_UPDATE_INTERVAL`
pub const RIFTFLOW_API_CACHED_RESOURCE_UPDATE_INTERVAL: Duration = Duration::from_secs(2 * 60 * 60);
/// `RIFTFLOW_API_FEATURE_GATES_UPDATE_INTERVAL` — the same two hours.
pub const RIFTFLOW_API_FEATURE_GATES_UPDATE_INTERVAL: Duration = Duration::from_secs(2 * 60 * 60);
/// `RIFTFLOW_API_NOTICE_UPDATE_INTERVAL`
pub const RIFTFLOW_API_NOTICE_UPDATE_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// `RIFTFLOW_API_BOOTSTRAP_CACHE_PATH`
pub const RIFTFLOW_API_BOOTSTRAP_CACHE_PATH: &str = "bootstrap.json";
/// `RIFTFLOW_API_BOOTSTRAP_NPM_LATEST_URL`
pub const RIFTFLOW_API_BOOTSTRAP_NPM_LATEST_URL: &str =
    "";

/// The four `RIFTFLOW_API_CACHED_RESOURCES` entries, in the original's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigResource {
    /// `app/feature-gates`
    FeatureGates,
    /// `sgp/supported-queues`
    SupportedQueues,
    /// `sgp/league-servers`
    LeagueServers,
    /// `auto-select/groups`
    AutoSelectGroups,
}

/// `RIFTFLOW_API_CACHED_RESOURCES`, ordered exactly as the original declares them.
pub const CONFIG_RESOURCES: [ConfigResource; 4] = [
    ConfigResource::FeatureGates,
    ConfigResource::SupportedQueues,
    ConfigResource::LeagueServers,
    ConfigResource::AutoSelectGroups,
];

impl ConfigResource {
    /// The `resource` field — the suffix after `/config/v1/`.
    pub fn resource(self) -> &'static str {
        match self {
            ConfigResource::FeatureGates => "app/feature-gates",
            ConfigResource::SupportedQueues => "sgp/supported-queues",
            ConfigResource::LeagueServers => "sgp/league-servers",
            ConfigResource::AutoSelectGroups => "auto-select/groups",
        }
    }

    /// `api.getConfig(...)` — the path actually requested.
    pub fn api_path(self) -> String {
        format!("/config/v1/{}", self.resource())
    }

    /// The `cachePath` field, relative to the shard's config directory.
    pub fn cache_path(self) -> &'static str {
        match self {
            ConfigResource::FeatureGates => "config/v1/app/feature-gates.json",
            ConfigResource::SupportedQueues => "config/v1/sgp/supported-queues.json",
            ConfigResource::LeagueServers => "config/v1/sgp/league-servers.json",
            ConfigResource::AutoSelectGroups => "config/v1/auto-select/groups.json",
        }
    }

    /// The `name` field, used in log lines.
    pub fn name(self) -> &'static str {
        match self {
            ConfigResource::FeatureGates => "feature gates",
            ConfigResource::SupportedQueues => "supported queues",
            ConfigResource::LeagueServers => "league servers",
            ConfigResource::AutoSelectGroups => "auto select groups",
        }
    }

    /// The `id` field, which is also the `RiftFlowApiState` prop name.
    pub fn id(self) -> &'static str {
        match self {
            ConfigResource::FeatureGates => "featureGates",
            ConfigResource::SupportedQueues => "supportedQueues",
            ConfigResource::LeagueServers => "leagueServers",
            ConfigResource::AutoSelectGroups => "autoSelectGroups",
        }
    }

    /// Every cached resource shares the two-hour cadence.
    pub fn interval(self) -> Duration {
        RIFTFLOW_API_CACHED_RESOURCE_UPDATE_INTERVAL
    }

    /// Match a requested path (with or without the `/config/v1/` prefix) back to
    /// the allowlisted resource — the API domain serves nothing else.
    pub fn from_path(path: &str) -> Option<ConfigResource> {
        let trimmed = path
            .trim_start_matches('/')
            .trim_start_matches("config/v1/")
            .trim_end_matches('/');
        CONFIG_RESOURCES
            .into_iter()
            .find(|resource| resource.resource() == trimmed)
    }

    /// The synced props this resource feeds. `supportedQueues` and
    /// `autoSelectGroups` are *derived* props in the original
    /// (`SgpState.supportedQueues`, `AutoSelectState.groups`), so the value pushed
    /// is the inner array, not the config document.
    pub fn prop_target(self) -> (&'static str, &'static str, &'static str, PropValueShape) {
        match self {
            ConfigResource::FeatureGates => (
                RIFTFLOW_API_MAIN_NAMESPACE,
                "state",
                "featureGates",
                PropValueShape::Document,
            ),
            ConfigResource::SupportedQueues => (
                SGP_MAIN_NAMESPACE,
                "state",
                "supportedQueues",
                PropValueShape::Field("queues"),
            ),
            ConfigResource::LeagueServers => (
                SGP_MAIN_NAMESPACE,
                "state",
                "leagueServers",
                PropValueShape::Document,
            ),
            ConfigResource::AutoSelectGroups => (
                AUTO_SELECT_MAIN_NAMESPACE,
                "state",
                "groups",
                PropValueShape::Field("groups"),
            ),
        }
    }
}

/// Whether a prop holds the whole config document or one field of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropValueShape {
    Document,
    Field(&'static str),
}

impl PropValueShape {
    pub fn project(self, document: &serde_json::Value) -> serde_json::Value {
        match self {
            PropValueShape::Document => document.clone(),
            PropValueShape::Field(field) => {
                document.get(field).cloned().unwrap_or_else(|| match field {
                    // `SgpState.supportedQueues` / `AutoSelectState.groups` read an
                    // array either way, so a malformed document yields the empty one
                    "queues" | "groups" => serde_json::json!([]),
                    _ => serde_json::Value::Null,
                })
            }
        }
    }
}

/// `RiftFlowServiceBaseUrls`. The wire field is named `static`, which Rust cannot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiftFlowServiceBaseUrls {
    pub api: String,
    #[serde(rename = "static")]
    pub static_assets: String,
}

impl Default for RiftFlowServiceBaseUrls {
    fn default() -> Self {
        Self {
            api: DEFAULT_RIFTFLOW_API_BASE_URL.to_string(),
            static_assets: DEFAULT_RIFTFLOW_STATIC_BASE_URL.to_string(),
        }
    }
}

impl RiftFlowServiceBaseUrls {
    /// Which host answers one of the two `riftflow://` domains.
    pub fn for_domain(&self, domain: crate::url::RiftFlowUrlDomain) -> &str {
        match domain {
            crate::url::RiftFlowUrlDomain::RiftFlowApi => &self.api,
            crate::url::RiftFlowUrlDomain::RiftFlowStatic => &self.static_assets,
        }
    }
}

/// `RiftFlowApiLanguage` narrowing: anything the app reports that the API does not
/// speak falls back to the default, which is what the helper's default argument
/// did in JS.
pub fn normalize_language(locale: &str) -> &'static str {
    RIFTFLOW_API_LANGUAGES
        .iter()
        .find(|language| **language == locale)
        .copied()
        .unwrap_or(DEFAULT_RIFTFLOW_API_LANGUAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_origins_are_the_riftflow_hosts() {
        let urls = RiftFlowServiceBaseUrls::default();
        assert_eq!(urls.api, "https://riftflow-api.yymainy.online");
        assert_eq!(urls.static_assets, "https://riftflow-api.yymainy.online/static");
    }

    #[test]
    fn each_resource_maps_to_its_api_path_and_cache_file() {
        assert_eq!(
            ConfigResource::FeatureGates.api_path(),
            "/config/v1/app/feature-gates"
        );
        assert_eq!(
            ConfigResource::LeagueServers.api_path(),
            "/config/v1/sgp/league-servers"
        );
        assert_eq!(
            ConfigResource::AutoSelectGroups.cache_path(),
            "config/v1/auto-select/groups.json"
        );
        assert_eq!(
            ConfigResource::SupportedQueues.prop_target().0,
            "sgp-main"
        );
    }

    #[test]
    fn the_refresh_cadence_matches_the_original() {
        assert_eq!(RIFTFLOW_API_NOTICE_UPDATE_INTERVAL, Duration::from_secs(3600));
        assert_eq!(
            RIFTFLOW_API_CACHED_RESOURCE_UPDATE_INTERVAL,
            Duration::from_secs(7200)
        );
        assert_eq!(RIFTFLOW_API_FEATURE_GATES_UPDATE_INTERVAL, Duration::from_secs(7200));
        assert_eq!(RIFTFLOW_API_REQUEST_TIMEOUT, Duration::from_secs(10));
    }

    #[test]
    fn only_the_two_api_languages_pass_through() {
        assert_eq!(normalize_language("en"), "en");
        assert_eq!(normalize_language("zh-CN"), "zh-CN");
        assert_eq!(normalize_language("zh-TW"), DEFAULT_RIFTFLOW_API_LANGUAGE);
    }

    #[test]
    fn the_base_urls_wire_field_stays_static() {
        let value = serde_json::to_value(RiftFlowServiceBaseUrls::default()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "api": "https://riftflow-api.yymainy.online",
                "static": "https://riftflow-api.yymainy.online/static",
            })
        );
    }
}
