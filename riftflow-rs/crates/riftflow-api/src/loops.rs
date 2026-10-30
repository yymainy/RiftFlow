//! The timers and the boot order: `IntervalTask` cadences plus
//! `RiftFlowApiMain.onInit` (`index.ts:70-83`).
//!
//! Plain `async fn`s the shell spawns — no runtime, no tauri, no task registry of
//! our own. The boot order is the original's: cached bootstrap, then npm discovery
//! in the background, then the local config cache, then the two refresh loops.

use std::sync::Arc;

use tokio::sync::watch;

use crate::cache::JsonConfigCache;
use crate::config::{
    RIFTFLOW_API_BOOTSTRAP_CACHE_PATH, RIFTFLOW_API_CACHED_RESOURCE_UPDATE_INTERVAL,
    RIFTFLOW_API_NOTICE_UPDATE_INTERVAL, CONFIG_RESOURCES, ConfigResource, normalize_language,
};
use crate::state::{RiftFlowApiRuntime, NO_GENERATION, RefreshOutcome};
use crate::validate::{BootstrapDocument, validate_bootstrap_document};

/// `_updateFromNpm`: a document applies when nothing is known yet, or when its
/// generation is strictly newer than the current one.
pub fn should_apply_bootstrap(current: i64, next: i64) -> bool {
    current == NO_GENERATION || next > current
}

/// `_loadFromLocal` — the cached generation, `None` when the file is missing or is
/// not a bootstrap document any more (in which case it is deleted, like the JS).
pub fn load_cached_bootstrap(cache: &JsonConfigCache) -> Option<BootstrapDocument> {
    if !cache.exists(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH) {
        return None;
    }

    let cached = match cache.read(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("[riftflow-api] Failed to load bootstrap from local cache: {error}");
            delete_bootstrap(cache);
            return None;
        }
    };

    match validate_bootstrap_document(&cached) {
        Ok(document) => Some(document),
        Err(error) => {
            eprintln!("[riftflow-api] Failed to load bootstrap from local cache: {error}");
            delete_bootstrap(cache);
            None
        }
    }
}

fn delete_bootstrap(cache: &JsonConfigCache) {
    if let Err(error) = cache.delete(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH) {
        eprintln!("[riftflow-api] Failed to delete invalid bootstrap cache: {error}");
    }
}

/// `RiftFlowApiBootstrapController.init()`: the cached hosts first, then npm, whose
/// newer generation replaces both hosts and is persisted for the next boot.
pub async fn run_bootstrap(runtime: &RiftFlowApiRuntime) -> Option<BootstrapDocument> {
    // Keep built-in routing until this application's own discovery is configured.
    if std::env::var("RIFTFLOW_BOOTSTRAP_URL").unwrap_or_default().is_empty() {
        return None;
    }
    if let Some(document) = load_cached_bootstrap(runtime.cache()) {
        eprintln!("[riftflow-api] Loaded bootstrap generation {}", document.generation);
        runtime.apply_bootstrap(&document);
    }

    let document = match runtime.client.get_bootstrap().await {
        Ok(document) => document,
        Err(error) => {
            eprintln!("[riftflow-api] Failed to update bootstrap from npm: {error}");
            return None;
        }
    };

    if !should_apply_bootstrap(runtime.generation(), document.generation) {
        eprintln!(
            "[riftflow-api] Bootstrap generation {} is up to date",
            runtime.generation()
        );
        return None;
    }

    runtime.apply_bootstrap(&document);
    if let Err(error) = runtime
        .cache()
        .write(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH, &document.to_json())
    {
        eprintln!("[riftflow-api] Failed to cache bootstrap: {error}");
    }
    eprintln!(
        "[riftflow-api] Updated bootstrap to generation {}",
        document.generation
    );
    Some(document)
}

/// `RiftFlowApiConfigLoader`: `initFromLocal()` publishes the cached documents, then
/// each resource refreshes immediately and every two hours.
///
/// The original armed one `IntervalTask` per resource; all four share
/// `RIFTFLOW_API_CACHED_RESOURCE_UPDATE_INTERVAL`, so one tick that walks the list is
/// the same schedule, and the `isUpdating` guard already made a re-entry a no-op.
///
/// `on_refresh` sees every outcome. The shell needs it because
/// `sgp-main:state#supportedQueues` is a derived prop: publishing the fetched list
/// is only half of `SgpState.supportedQueues`, which puts the queue of the game in
/// progress in front of it.
pub async fn run_config_tasks(
    runtime: Arc<RiftFlowApiRuntime>,
    on_refresh: impl Fn(&RefreshOutcome) + Send + Sync + 'static,
) {
    let cached = runtime.load_cached_configs();
    runtime.publish(&cached);

    loop {
        for resource in CONFIG_RESOURCES {
            let outcome = runtime.refresh_and_publish(resource).await;
            eprintln!("[riftflow-api] {}", describe(&outcome));
            on_refresh(&outcome);
        }
        tokio::time::sleep(RIFTFLOW_API_CACHED_RESOURCE_UPDATE_INTERVAL).await;
    }
}

/// `RiftFlowApiNoticeLoader`: the locale reaction fires immediately, then hourly, and
/// a locale change restarts the hour — the `IntervalTask` was re-`start()`ed.
pub async fn run_notice_tasks(runtime: Arc<RiftFlowApiRuntime>, mut locale: watch::Receiver<String>) {
    loop {
        let language = normalize_language(locale.borrow().as_str()).to_string();
        let updates = runtime.refresh_notice_and_publish(&language).await;
        if !updates.is_empty() {
            let keys: Vec<String> = updates.iter().map(|update| update.key()).collect();
            eprintln!("[riftflow-api] updated {} for {language}", keys.join(", "));
        }

        // whichever comes first: the hour is up, or the user changed the language
        tokio::select! {
            _ = tokio::time::sleep(RIFTFLOW_API_NOTICE_UPDATE_INTERVAL) => {}
            changed = locale.changed() => {
                if changed.is_err() {
                    // nobody publishes locales any more; keep the hourly refresh
                    tokio::time::sleep(RIFTFLOW_API_NOTICE_UPDATE_INTERVAL).await;
                }
            }
        }
    }
}

/// A refresh's log line, in the original's words.
pub fn describe(outcome: &RefreshOutcome) -> String {
    let name = outcome.resource().name();
    match outcome {
        RefreshOutcome::Updated { .. } => format!("Updated {name}"),
        RefreshOutcome::UpToDate { .. } => format!("{name} is up to date"),
        RefreshOutcome::Invalid { reason, .. } => format!("Invalid {name} response: {reason}"),
        RefreshOutcome::Failed { reason, .. } => format!("Update {name} failed: {reason}"),
        RefreshOutcome::Overlapping { .. } => format!("{name} refresh already in flight"),
    }
}

/// `refresh_config` for every resource at once — the shell's "check now" hook, if
/// it wants one instead of waiting for the two-hour tick.
pub async fn refresh_all_once(runtime: &RiftFlowApiRuntime) -> Vec<RefreshOutcome> {
    let mut outcomes = Vec::with_capacity(CONFIG_RESOURCES.len());
    for resource in CONFIG_RESOURCES {
        outcomes.push(runtime.refresh_config(resource).await);
    }
    outcomes
}

/// Which resource a `ConfigResource` is, by its `id` — the reverse of
/// `ConfigResource::id`, used when a caller names the resource to refresh.
pub fn resource_by_id(id: &str) -> Option<ConfigResource> {
    CONFIG_RESOURCES
        .into_iter()
        .find(|resource| resource.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_cache() -> (JsonConfigCache, std::path::PathBuf) {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "riftflow-api-loops-{}-{id}",
            std::process::id()
        ));
        (JsonConfigCache::under_user_data(&dir), dir)
    }

    #[test]
    fn only_a_newer_generation_moves_the_hosts() {
        assert!(should_apply_bootstrap(NO_GENERATION, 1));
        assert!(should_apply_bootstrap(1, 2));
        assert!(!should_apply_bootstrap(2, 2));
        assert!(!should_apply_bootstrap(2, 1));
    }

    #[test]
    fn a_cached_bootstrap_survives_and_a_foreign_one_is_deleted() {
        let (cache, dir) = temp_cache();
        let document = BootstrapDocument {
            generation: 3,
            base_urls: crate::config::RiftFlowServiceBaseUrls {
                api: "https://api-3.example.com".to_string(),
                static_assets: "https://static-3.example.com".to_string(),
            },
        };
        cache
            .write(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH, &document.to_json())
            .unwrap();

        assert_eq!(load_cached_bootstrap(&cache), Some(document.clone()));

        // `schemaVersion: 2` is the fixture from `bootstrap-controller.test.ts`
        cache
            .write(
                RIFTFLOW_API_BOOTSTRAP_CACHE_PATH,
                &json!({ "schemaVersion": 2 }),
            )
            .unwrap();
        assert_eq!(load_cached_bootstrap(&cache), None);
        assert!(
            !cache.exists(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH),
            "an invalid cache has to go"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_bootstrap_file_is_written_in_the_shape_it_is_read_in() {
        let (cache, dir) = temp_cache();
        let document = BootstrapDocument {
            generation: 7,
            base_urls: crate::config::RiftFlowServiceBaseUrls {
                api: "https://api-7.example.com".to_string(),
                static_assets: "https://static-7.example.com".to_string(),
            },
        };

        cache
            .write(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH, &document.to_json())
            .unwrap();
        assert_eq!(
            cache.read(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH).unwrap(),
            json!({
                "schemaVersion": 1,
                "generation": 7,
                "baseUrls": {
                    "api": "https://api-7.example.com",
                    "static": "https://static-7.example.com",
                }
            })
        );
        assert_eq!(load_cached_bootstrap(&cache).unwrap(), document);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_log_lines_are_the_originals() {
        let resource = ConfigResource::LeagueServers;
        assert_eq!(
            describe(&RefreshOutcome::UpToDate { resource }),
            "league servers is up to date"
        );
        assert_eq!(
            describe(&RefreshOutcome::Invalid {
                resource,
                reason: "bad".to_string(),
            }),
            "Invalid league servers response: bad"
        );
        assert_eq!(
            describe(&RefreshOutcome::Failed {
                resource,
                reason: "offline".to_string(),
            }),
            "Update league servers failed: offline"
        );
        assert_eq!(
            describe(&RefreshOutcome::Overlapping { resource }),
            "league servers refresh already in flight"
        );
    }

    #[test]
    fn the_notice_cadence_is_an_hour_and_the_configs_two() {
        assert_eq!(RIFTFLOW_API_NOTICE_UPDATE_INTERVAL.as_secs(), 3600);
        assert_eq!(RIFTFLOW_API_CACHED_RESOURCE_UPDATE_INTERVAL.as_secs(), 7200);
    }

    #[test]
    fn a_resource_can_be_named_by_its_state_key() {
        assert_eq!(
            resource_by_id("autoSelectGroups"),
            Some(ConfigResource::AutoSelectGroups)
        );
        assert_eq!(resource_by_id("featureGates"), Some(ConfigResource::FeatureGates));
        assert_eq!(resource_by_id("notice"), None);
    }
}
