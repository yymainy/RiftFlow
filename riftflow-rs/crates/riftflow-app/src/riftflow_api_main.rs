//! The shell side of `riftflow-api-main`: build the runtime, run the refresh loops,
//! and answer the `riftflow://riftflow-api/…` and `riftflow://riftflow-static/…` domains.
//!
//! All the deciding lives in `riftflow_api` (bootstrap generation, the
//! `updatedAt` comparison, the endpoint table); this file only owns the LCU-free
//! plumbing — where the cache directory is, which locale the notice documents are
//! fetched for, and how a route becomes a `Proxied`.

use std::path::Path;
use std::sync::Arc;

use riftflow_api::prelude::*;
use riftflow_state::StateSync;
use riftflow_store::settings::SettingFactory;
use serde_json::Value;
use tokio::sync::watch;

use crate::protocol::Proxied;
use crate::settings_registry::SettingsHub;

/// The seed documents. Read from the same asset `lc_state.rs` publishes, so the
/// builtins and the remote config cannot disagree about what they are competing
/// against.
fn builtin_asset() -> Value {
    serde_json::from_str(include_str!("../assets/riftflow-builtin.json"))
        .expect("assets/riftflow-builtin.json should be valid JSON")
}

pub fn build(
    user_data_dir: &Path,
    version: &str,
    state_sync: &Arc<StateSync>,
) -> Arc<RiftFlowApiRuntime> {
    let builtins = BuiltinConfigs::from_builtin_asset(&builtin_asset());

    let runtime = Arc::new(RiftFlowApiRuntime::new(
        RiftFlowApiClient::new(version),
        JsonConfigCache::under_user_data(user_data_dir),
        builtins.clone(),
        Some(state_sync.clone()),
    ));

    runtime.publish(&builtins.updates());
    runtime
}

/// `RiftFlowApiMain.onInit()` plus the two `watch()` loops. Bootstrap runs first
/// because every later request is built from the hosts it may have replaced.
pub fn start(
    runtime: Arc<RiftFlowApiRuntime>,
    settings: Arc<SettingFactory>,
    hub: Arc<SettingsHub>,
    state_sync: Arc<StateSync>,
) {
    let (locale_tx, locale_rx) = watch::channel(current_locale(&settings));

    tauri::async_runtime::spawn(async move {
        let mut changes = hub.subscribe();
        tauri::async_runtime::spawn(async move {
            while let Ok((namespace, key, value)) = changes.recv().await {
                if namespace == "app-common-main" && key == "locale" {
                    if let Some(text) = value.as_str() {
                        let _ = locale_tx.send(text.to_string());
                    }
                }
            }
        });

        run_bootstrap(&runtime).await;

        let cached = runtime.load_cached_configs();
        runtime.publish(&cached);

        // the loader publishes the fetched list into `sgp-main:state#supportedQueues`
        // verbatim; that prop is derived in the original, so hand the document to
        // `lc_state` and let it recompute after every refresh
        let hook_runtime = runtime.clone();
        let hook_sync = state_sync.clone();
        let on_refresh = move |outcome: &RefreshOutcome| {
            if matches!(outcome.resource(), ConfigResource::SupportedQueues) {
                let document = hook_runtime.current_document(ConfigResource::SupportedQueues);
                if let Some(queues) = document.get("queues") {
                    crate::lc_state::set_fetched_supported_queues(queues.clone());
                    crate::lc_state::sync_supported_queues(&hook_sync);
                }
            }
        };

        tokio::join!(
            run_config_tasks(runtime.clone(), on_refresh),
            run_notice_tasks(runtime, locale_rx)
        );
    });
}

fn current_locale(settings: &SettingFactory) -> String {
    settings
        .get_setting("app-common-main", "locale")
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| riftflow_api::config::DEFAULT_RIFTFLOW_API_LANGUAGE.to_string())
}

/// The two protocol domains. Anything the endpoint table does not recognise as
/// ours is answered here rather than by falling through to another domain, because
/// `riftflow-api`/`riftflow-static` are only ever routed by this table.
pub async fn proxy(
    runtime: &RiftFlowApiRuntime,
    method: &str,
    url: &str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> Proxied {
    let route = route(url, &runtime.base_urls());

    if let RiftFlowRoute::NotOurs = route {
        return Proxied::text(404, &format!("No handler for {url}"));
    }

    if let Some((status, headers, bytes)) = route.local_parts() {
        return Proxied::new(status, headers, bytes);
    }

    match runtime.client.forward(&route, method, &headers, &body).await {
        Ok(response) => Proxied::new(response.status, response.headers, response.body),
        Err(error) => {
            eprintln!("[riftflow-api] {method} {url} → {}", error);
            Proxied::text(500, &error.to_string())
        }
    }
}
