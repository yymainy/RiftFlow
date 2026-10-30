//! `riftflow-api-main` — the app's remote-configuration client, and the two
//! `riftflow://` domains it registers.
//!
//! Ported from `RiftFlow/src/main/shards/riftflow-api/` (`index.ts`, `state.ts`,
//! `context.ts`, `cached-resources.ts`, `config-loader.ts`, `notice-loader.ts`,
//! `release-loader.ts`, `bootstrap-controller.ts`, `protocol-controller.ts`) and
//! `RiftFlow/src/shared/shards/riftflow-api/` (`types.ts`, `schemas.ts`,
//! `base-urls.ts`, `auto-select.ts`). The Electron main process is gone; what is
//! left is a client that reads the same documents, decides with the same rules, and
//! hands `serde_json::Value`s to the shell's `StateSync`.
//!
//! # The two domains
//!
//! `riftflow://riftflow-api/<path>` and `riftflow://riftflow-static/<path>`; both hosts may be
//! replaced by the bootstrap document (`@riftflow/bootstrap` on npm). The
//! endpoint table and the local replies are in [`url`], and [`url::route`] is the
//! side-effect-free function `riftflow-app/src/protocol.rs` calls.
//!
//! # The refresh policy
//!
//! * bootstrap: cached file, then npm once at boot; a generation only moves forward
//!   ([`loops::run_bootstrap`]).
//! * the four cached config documents: cache file first, then immediately and every
//!   two hours; a document replaces the loaded one only when its `updatedAt` is
//!   strictly newer, and a cache file older than the builtin is deleted
//!   ([`state::RiftFlowApiRuntime::load_cached_configs`],
//!   [`state::RiftFlowApiRuntime::refresh_config`], [`time`]).
//! * notice and contact channels: on locale change and hourly, replaced whenever
//!   they arrive, each surviving the other's failure ([`loops::run_notice_tasks`]).
//! * latest release: on demand, for `self-update` ([`state::RiftFlowApiRuntime::refresh_latest_release`]).
//!
//! ```no_run
//! use riftflow_api::prelude::*;
//! use riftflow_api::loops;
//!
//! # async fn example(user_data: &std::path::Path, version: &str) {
//! let runtime = std::sync::Arc::new(RiftFlowApiRuntime::new(
//!     RiftFlowApiClient::new(version),
//!     JsonConfigCache::under_user_data(user_data),
//!     BuiltinConfigs::from_builtin_asset(&serde_json::json!({})),
//!     None,
//! ));
//!
//! // boot: hosts first, because every request below is built from them
//! loops::run_bootstrap(&runtime).await;
//! let decision = route("riftflow://riftflow-api/config/v1/app/feature-gates", &runtime.base_urls());
//! let response = runtime.client.forward(&decision, "GET", &[], &[]).await;
//! # let _ = response;
//! # }
//! ```

pub mod cache;
pub mod client;
pub mod config;
pub mod loops;
pub mod semver;
pub mod state;
pub mod time;
pub mod url;
pub mod validate;

/// The handful of items the shell needs, by one import.
pub mod prelude {
    pub use crate::cache::JsonConfigCache;
    pub use crate::client::{RiftFlowApiClient, RiftFlowForwardResponse};
    pub use crate::config::{
        RIFTFLOW_API_PROTOCOL_DOMAIN, RIFTFLOW_STATIC_PROTOCOL_DOMAIN, RiftFlowServiceBaseUrls,
        CONFIG_RESOURCES, ConfigResource, DEFAULT_RIFTFLOW_API_BASE_URL,
        DEFAULT_RIFTFLOW_STATIC_BASE_URL,
    };
    pub use crate::loops::{
        resource_by_id, run_bootstrap, run_config_tasks, run_notice_tasks, should_apply_bootstrap,
    };
    pub use crate::state::{
        RiftFlowApiRuntime, BuiltinConfigs, NO_GENERATION, PropUpdate, RefreshOutcome, update_for,
    };
    pub use crate::url::{RiftFlowRoute, RiftFlowUrlDomain, route};
    pub use crate::validate::{
        BootstrapDocument, is_auto_select_group_supported_on_sgp_server, validate_config,
    };
}

pub use prelude::*;
