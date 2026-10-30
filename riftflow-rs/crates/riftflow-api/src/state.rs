//! `RiftFlowApiState` plus the two loaders' policies: what a fetched document has to
//! beat before it replaces what is loaded, and which synced prop each result feeds.
//!
//! The state itself is the app's `StateSync`; this type only holds the current
//! values it compares against, so every function here can be called for its return
//! value and the caller decides when to push.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU8, Ordering};

use serde_json::Value;

use riftflow_state::StateSync;

use crate::cache::JsonConfigCache;
use crate::client::RiftFlowApiClient;
use crate::config::{RIFTFLOW_API_MAIN_NAMESPACE, RiftFlowServiceBaseUrls, ConfigResource};
use crate::time::{EPOCH_FLOOR, decide_local_cache_load, decide_remote_replace};
use crate::validate::{
    BootstrapDocument, validate_config, validate_contact_channels, validate_notice,
    validate_release,
};

/// One `propSync` write: the namespace, sub-state, prop path and the value. The app
/// hands these to `StateSync::set` — the props are the original's, so the renderer's
/// stores keep working without knowing anything changed.
#[derive(Clone, Debug, PartialEq)]
pub struct PropUpdate {
    pub namespace: &'static str,
    pub state_id: &'static str,
    pub prop: &'static str,
    pub value: Value,
}

impl PropUpdate {
    pub fn key(&self) -> String {
        format!("{}:{}#{}", self.namespace, self.state_id, self.prop)
    }
}

/// What `refresh_config` decided, in the loader's own terms.
#[derive(Debug, PartialEq)]
pub enum RefreshOutcome {
    /// `Updated <name>` — the value replaced the loaded one and the cache file was
    /// rewritten
    Updated {
        resource: ConfigResource,
        updates: Vec<PropUpdate>,
    },
    /// `<name> is up to date` — same or older `updatedAt`
    UpToDate { resource: ConfigResource },
    /// `Invalid <name> response` — the document arrived and zod rejected it
    Invalid {
        resource: ConfigResource,
        reason: String,
    },
    /// `Update <name> failed` — the request itself did not work
    Failed {
        resource: ConfigResource,
        reason: String,
    },
    /// `_updateAndSave` while the same resource is already in flight
    Overlapping { resource: ConfigResource },
}

impl RefreshOutcome {
    pub fn resource(&self) -> ConfigResource {
        match self {
            RefreshOutcome::Updated { resource, .. }
            | RefreshOutcome::UpToDate { resource }
            | RefreshOutcome::Invalid { resource, .. }
            | RefreshOutcome::Failed { resource, .. }
            | RefreshOutcome::Overlapping { resource } => *resource,
        }
    }

    pub fn updates(&self) -> &[PropUpdate] {
        match self {
            RefreshOutcome::Updated { updates, .. } => updates,
            _ => &[],
        }
    }
}

/// The `BUILTIN_*` seed. The shell owns the asset
/// (`riftflow-app/assets/riftflow-builtin.json`) — these are what a fetched document has
/// to beat on `updatedAt`.
#[derive(Clone, Debug)]
pub struct BuiltinConfigs {
    pub league_servers: Value,
    pub supported_queues: Value,
    pub auto_select_groups: Value,
}

impl BuiltinConfigs {
    /// Reads the three keys `lc_state.rs` already publishes, so both paths start
    /// from the same numbers.
    pub fn from_builtin_asset(asset: &Value) -> Self {
        let pick = |key: &str| {
            asset
                .get(key)
                .cloned()
                .unwrap_or_else(|| missing_builtin(key))
        };
        Self {
            league_servers: pick("BUILTIN_SGP_LEAGUE_SERVERS_CONFIG"),
            supported_queues: pick("BUILTIN_SUPPORTED_QUEUES"),
            auto_select_groups: pick("BUILTIN_AUTO_SELECT_GROUPS"),
        }
    }

    /// Everything currently loaded, as the props the renderer reads.
    pub fn updates(&self) -> Vec<PropUpdate> {
        vec![
            update_for(ConfigResource::LeagueServers, self.league_servers.clone()),
            update_for(ConfigResource::SupportedQueues, self.supported_queues.clone()),
            update_for(ConfigResource::AutoSelectGroups, self.auto_select_groups.clone()),
        ]
    }
}

fn missing_builtin(key: &str) -> Value {
    eprintln!("[riftflow-api] builtin asset is missing {key}; remote config will replace it");
    serde_json::json!({ "updatedAt": EPOCH_FLOOR })
}

/// The runtime: the HTTP client, the cache directory and the values the version
/// comparison runs against.
pub struct RiftFlowApiRuntime {
    pub client: RiftFlowApiClient,
    cache: JsonConfigCache,
    state_sync: Option<Arc<StateSync>>,
    current: std::sync::RwLock<CurrentValues>,
    /// one bit per `ConfigResource`, the loader's `isUpdating*` flags
    updating: AtomicU8,
    /// `RiftFlowApiBootstrapController._generation`; `NO_GENERATION` until a bootstrap
    /// document has been applied
    generation: AtomicI64,
}

/// `_generation === null` — nothing discovered yet, so any document applies.
pub const NO_GENERATION: i64 = i64::MIN;

#[derive(Clone, Debug)]
struct CurrentValues {
    feature_gates: Option<Value>,
    notice: Option<Value>,
    contact_channels: Option<Value>,
    latest_release: Option<Value>,
    documents: [Value; 4],
}

impl CurrentValues {
    fn new(builtins: &BuiltinConfigs) -> Self {
        Self {
            feature_gates: None,
            notice: None,
            contact_channels: None,
            latest_release: None,
            documents: [
                // feature gates have no builtin document at all
                Value::Null,
                builtins.supported_queues.clone(),
                builtins.league_servers.clone(),
                builtins.auto_select_groups.clone(),
            ],
        }
    }

    fn document(&self, resource: ConfigResource) -> &Value {
        &self.documents[index(resource)]
    }
}

fn index(resource: ConfigResource) -> usize {
    match resource {
        ConfigResource::FeatureGates => 0,
        ConfigResource::SupportedQueues => 1,
        ConfigResource::LeagueServers => 2,
        ConfigResource::AutoSelectGroups => 3,
    }
}

impl RiftFlowApiRuntime {
    /// `state_sync` may be `None` for tests and for the shell's own pre-boot phase:
    /// the values are still tracked, they just are not pushed anywhere.
    pub fn new(
        client: RiftFlowApiClient,
        cache: JsonConfigCache,
        builtins: BuiltinConfigs,
        state_sync: Option<Arc<StateSync>>,
    ) -> Self {
        let current = CurrentValues::new(&builtins);
        Self {
            client,
            cache,
            state_sync,
            current: std::sync::RwLock::new(current),
            updating: AtomicU8::new(0),
            generation: AtomicI64::new(NO_GENERATION),
        }
    }

    /// The bootstrap generation currently in force, if any.
    pub fn generation(&self) -> i64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn set_generation(&self, generation: i64) {
        self.generation.store(generation, Ordering::SeqCst);
    }

    pub fn cache(&self) -> &JsonConfigCache {
        &self.cache
    }

    pub fn base_urls(&self) -> RiftFlowServiceBaseUrls {
        self.client.base_urls()
    }

    /// `RiftFlowApiState.isUpdating<Name>`
    pub fn is_updating(&self, resource: ConfigResource) -> bool {
        self.updating.load(Ordering::SeqCst) & (1 << index(resource)) != 0
    }

    /// The loader's `getCurrentTimestamp`: the loaded document's `updatedAt`, the
    /// epoch floor for feature gates (which have no builtin), or `""` — which never
    /// parses, and so never compares as older.
    pub fn updated_at(&self, resource: ConfigResource) -> String {
        let current = self.current.read().unwrap();
        match resource {
            ConfigResource::FeatureGates => current
                .feature_gates
                .as_ref()
                .and_then(|value| value.get("updatedAt"))
                .and_then(Value::as_str)
                .unwrap_or(EPOCH_FLOOR)
                .to_string(),
            other => current
                .document(other)
                .get("updatedAt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }
    }

    pub fn current_document(&self, resource: ConfigResource) -> Value {
        self.current.read().unwrap().document(resource).clone()
    }

    pub fn notice(&self) -> Option<Value> {
        self.current.read().unwrap().notice.clone()
    }

    pub fn contact_channels(&self) -> Option<Value> {
        self.current.read().unwrap().contact_channels.clone()
    }

    pub fn feature_gates(&self) -> Option<Value> {
        self.current.read().unwrap().feature_gates.clone()
    }

    pub fn latest_release(&self) -> Option<Value> {
        self.current.read().unwrap().latest_release.clone()
    }

    /// Every prop the runtime currently holds — what `publish_builtin` writes once,
    /// with the fetched values replacing the builtin ones.
    pub fn prop_updates(&self) -> Vec<PropUpdate> {
        let current = self.current.read().unwrap();
        let mut updates = Vec::with_capacity(6);

        for resource in crate::config::CONFIG_RESOURCES {
            let document = if matches!(resource, ConfigResource::FeatureGates) {
                // `featureGates: null` is the pre-load value the renderer expects
                current.feature_gates.clone()
            } else {
                Some(current.document(resource).clone())
            };

            if let Some(document) = document.filter(|value| !value.is_null()) {
                updates.push(update_for(resource, document));
            }
        }

        for (prop, value) in [
            ("notice", current.notice.clone()),
            ("contactChannels", current.contact_channels.clone()),
        ] {
            if let Some(value) = value {
                updates.push(PropUpdate {
                    namespace: RIFTFLOW_API_MAIN_NAMESPACE,
                    state_id: "state",
                    prop,
                    value,
                });
            }
        }

        updates
    }

    /// `_initResourceFromLocal` for all four resources, in declaration order.
    /// Returns the props a newer cache file brought in.
    pub fn load_cached_configs(&self) -> Vec<PropUpdate> {
        let mut updates = Vec::new();

        for resource in crate::config::CONFIG_RESOURCES {
            let Some(document) = self.read_cached_document(resource) else {
                continue;
            };
            if let Some(update) = self.apply_cached_document(resource, document) {
                updates.push(update);
            }
        }

        updates
    }

    /// One cache file, already schema-checked — `None` when it is absent, invalid,
    /// or unreadable (the last two delete it, like the JS).
    fn read_cached_document(&self, resource: ConfigResource) -> Option<Value> {
        if !self.cache.exists(resource.cache_path()) {
            return None;
        }

        let cached = match self.cache.read(resource.cache_path()) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("[riftflow-api] Invalid cached {}: {error}", resource.name());
                self.delete_cached(resource);
                return None;
            }
        };

        match validate_config(resource, &cached) {
            Ok(document) => Some(document),
            Err(error) => {
                // this is the branch that deletes a legacy feature-gate document
                eprintln!("[riftflow-api] Invalid cached {}: {error}", resource.name());
                self.delete_cached(resource);
                None
            }
        }
    }

    fn apply_cached_document(&self, resource: ConfigResource, document: Value) -> Option<PropUpdate> {
        let cached_at = timestamp_of(&document);
        let current_at = self.updated_at(resource);
        let decision = decide_local_cache_load(&cached_at, &current_at);

        match decision {
            crate::time::LocalCacheDecision::DiscardAsStale => {
                eprintln!("[riftflow-api] Removed stale cached {}", resource.name());
                self.delete_cached(resource);
                None
            }
            crate::time::LocalCacheDecision::Apply => Some(self.store_document(resource, document)),
            // equal or unparseable: nothing to do, and nothing to delete
            _ => None,
        }
    }

    fn delete_cached(&self, resource: ConfigResource) {
        if let Err(error) = self.cache.delete(resource.cache_path()) {
            eprintln!(
                "[riftflow-api] Failed to delete cached {}: {error}",
                resource.name()
            );
        }
    }

    /// `_updateAndSave`: fetch, validate, compare, then store and cache.
    pub async fn refresh_config(&self, resource: ConfigResource) -> RefreshOutcome {
        let Some(_guard) = UpdatingGuard::begin(self, resource) else {
            return RefreshOutcome::Overlapping { resource };
        };

        let document = match self.client.get_config_document(resource).await {
            Ok(document) => document,
            Err(error) => {
                return RefreshOutcome::Failed {
                    resource,
                    reason: error.to_string(),
                };
            }
        };

        let validated = match validate_config(resource, &document) {
            Ok(validated) => validated,
            Err(error) => {
                return RefreshOutcome::Invalid {
                    resource,
                    reason: error,
                };
            }
        };

        let next_at = timestamp_of(&validated);
        let current_at = self.updated_at(resource);

        if !decide_remote_replace(&next_at, &current_at) {
            return RefreshOutcome::UpToDate { resource };
        }

        let update = self.store_document(resource, validated.clone());
        if let Err(error) = self.cache.write(resource.cache_path(), &validated) {
            eprintln!("[riftflow-api] Failed to cache {}: {error}", resource.name());
        }

        RefreshOutcome::Updated {
            resource,
            updates: vec![update],
        }
    }

    /// `RiftFlowApiNoticeLoader._update` — one refresh, both endpoints, each on its own
    /// failure path, with no version comparison: a notice is replaced whenever it
    /// arrives.
    pub async fn refresh_notice_and_contact_channels(
        &self,
        language: &str,
    ) -> Vec<PropUpdate> {
        let notice = self.client.get_notice_document(language).await;
        let channels = self.client.get_contact_channels_document().await;

        let mut updates = Vec::new();

        match notice {
            Ok(document) => match validate_notice(&document) {
                Ok(validated) => {
                    self.current.write().unwrap().notice = Some(validated.clone());
                    updates.push(PropUpdate {
                        namespace: RIFTFLOW_API_MAIN_NAMESPACE,
                        state_id: "state",
                        prop: "notice",
                        value: validated,
                    });
                }
                Err(error) => eprintln!("[riftflow-api] Update notice failed: {error}"),
            },
            Err(error) => eprintln!("[riftflow-api] Update notice failed: {error}"),
        }

        match channels {
            Ok(document) => match validate_contact_channels(&document) {
                Ok(validated) => {
                    self.current.write().unwrap().contact_channels = Some(validated.clone());
                    updates.push(PropUpdate {
                        namespace: RIFTFLOW_API_MAIN_NAMESPACE,
                        state_id: "state",
                        prop: "contactChannels",
                        value: validated,
                    });
                }
                Err(error) => eprintln!("[riftflow-api] Update contact channels failed: {error}"),
            },
            Err(error) => eprintln!("[riftflow-api] Update contact channels failed: {error}"),
        }

        updates
    }

    /// `RiftFlowApiReleaseLoader._fetchLatestRelease` — the value `self-update` reads.
    pub async fn refresh_latest_release(&self, language: &str) -> anyhow::Result<Value> {
        let document = self.client.get_latest_release_document(language).await?;
        let validated = validate_release(&document)
            .map_err(|error| anyhow::anyhow!("Invalid release response: {error}"))?;
        self.current.write().unwrap().latest_release = Some(validated.clone());
        Ok(validated)
    }

    /// `_applyBootstrap` — the generation's two hosts become the client's base urls.
    pub fn apply_bootstrap(&self, bootstrap: &BootstrapDocument) {
        self.client.set_base_urls(RiftFlowServiceBaseUrls {
            api: bootstrap.base_urls.api.clone(),
            static_assets: bootstrap.base_urls.static_assets.clone(),
        });
        self.set_generation(bootstrap.generation);
    }

    /// Store one document and hand back the prop it feeds.
    fn store_document(&self, resource: ConfigResource, document: Value) -> PropUpdate {
        let update = update_for(resource, document.clone());

        let mut current = self.current.write().unwrap();
        match resource {
            ConfigResource::FeatureGates => current.feature_gates = Some(document.clone()),
            other => current.documents[index(other)] = document,
        }
        drop(current);

        update
    }

    /// Push updates into `StateSync` (when the runtime was given one). A prop that
    /// was never declared is a wiring bug, so it says so on stderr.
    pub fn publish(&self, updates: &[PropUpdate]) {
        let Some(state_sync) = &self.state_sync else {
            return;
        };

        for update in updates {
            if let Err(error) =
                state_sync.set(update.namespace, update.state_id, update.prop, update.value.clone())
            {
                eprintln!("[riftflow-api] cannot publish {}: {error}", update.key());
            }
        }
    }

    /// Fetch one resource and publish whatever it replaced — the shape the app can
    /// spawn directly.
    pub async fn refresh_and_publish(&self, resource: ConfigResource) -> RefreshOutcome {
        let outcome = self.refresh_config(resource).await;
        self.publish(outcome.updates());
        outcome
    }

    pub async fn refresh_notice_and_publish(&self, language: &str) -> Vec<PropUpdate> {
        let updates = self.refresh_notice_and_contact_channels(language).await;
        self.publish(&updates);
        updates
    }
}

/// The `isUpdating*` guard: set on entry, cleared on drop, so a panic in the middle
/// cannot leave a resource permanently skipped.
struct UpdatingGuard<'a> {
    runtime: &'a RiftFlowApiRuntime,
    resource: ConfigResource,
}

impl<'a> UpdatingGuard<'a> {
    fn begin(runtime: &'a RiftFlowApiRuntime, resource: ConfigResource) -> Option<Self> {
        let bit = 1u8 << index(resource);
        let previous = runtime.updating.fetch_or(bit, Ordering::SeqCst);
        if previous & bit != 0 {
            return None;
        }
        Some(Self { runtime, resource })
    }
}

impl Drop for UpdatingGuard<'_> {
    fn drop(&mut self) {
        self.runtime
            .updating
            .fetch_and(!(1u8 << index(self.resource)), Ordering::SeqCst);
    }
}

/// The prop a config document feeds, in the shape the renderer's store declares.
pub fn update_for(resource: ConfigResource, document: Value) -> PropUpdate {
    let (namespace, state_id, prop, shape) = resource.prop_target();
    PropUpdate {
        namespace,
        state_id,
        prop,
        value: shape.project(&document),
    }
}

fn timestamp_of(document: &Value) -> String {
    document
        .get("updatedAt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CONFIG_RESOURCES;
    use serde_json::json;

    fn builtins() -> BuiltinConfigs {
        BuiltinConfigs {
            league_servers: json!({ "updatedAt": "2026-07-18T04:00:00.000Z", "servers": {}, "serverNames": {} }),
            supported_queues: json!({
                "updatedAt": "2026-07-18T04:00:00.000Z",
                "queues": [420, 1700]
            }),
            auto_select_groups: json!({
                "updatedAt": "2026-07-18T04:00:00.000Z",
                "groups": [{
                    "groupId": "ranked",
                    "name": { "zh-CN": "排位模式", "en": "Ranked" },
                    "iconPath": "/lol-game-data/assets/a.png",
                    "isCustom": false,
                    "supportedSgpServers": ["*"],
                    "targetGameModes": [],
                    "positions": [],
                    "additionalPicks": [],
                    "additionalBans": [],
                    "excludedPicks": [],
                    "excludedBans": []
                }]
            }),
        }
    }

    /// A temp directory that removes itself, so the cache tests leave nothing behind.
    struct TempDir(std::path::PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn runtime() -> (RiftFlowApiRuntime, TempDir) {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "riftflow-api-runtime-{}-{id}",
            std::process::id()
        ));
        let cache = JsonConfigCache::under_user_data(&dir);
        let runtime = RiftFlowApiRuntime::new(
            RiftFlowApiClient::new("1.5.1"),
            cache,
            builtins(),
            None,
        );
        (runtime, TempDir(dir))
    }

    #[test]
    fn the_builtin_seed_publishes_the_three_derived_props() {
        let updates = builtins().updates();
        let keys: Vec<String> = updates.iter().map(PropUpdate::key).collect();

        assert_eq!(
            keys,
            vec![
                "sgp-main:state#leagueServers",
                "sgp-main:state#supportedQueues",
                "auto-select-main:state#groups",
            ]
        );
        // `SgpState.supportedQueues` and `AutoSelectState.groups` are arrays
        assert_eq!(updates[1].value, json!([420, 1700]));
        assert_eq!(updates[2].value.as_array().unwrap().len(), 1);
    }

    #[test]
    fn feature_gates_start_at_the_epoch_and_the_rest_at_their_builtin_date() {
        let (runtime, _dir) = runtime();
        assert_eq!(runtime.updated_at(ConfigResource::FeatureGates), EPOCH_FLOOR);
        assert_eq!(
            runtime.updated_at(ConfigResource::AutoSelectGroups),
            "2026-07-18T04:00:00.000Z"
        );
        assert!(runtime.feature_gates().is_none());
    }

    #[test]
    fn a_newer_cache_file_replaces_the_builtin_groups() {
        let (runtime, _dir) = runtime();
        let cached = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "groups": [{
                "groupId": "ranked",
                "name": { "zh-CN": "排位模式", "en": "Ranked" },
                "iconPath": "/lol-game-data/assets/content/src/LeagueClient/GameModeAssets/Classic_SRU/img/game-select-icon-hover.png",
                "isCustom": false,
                "supportedSgpServers": ["*"],
                "targetGameModes": [
                    { "gameMode": "CLASSIC", "queueTypes": ["RANKED_SOLO_5x5"] }
                ],
                "positions": ["top"],
                "additionalPicks": [],
                "additionalBans": [],
                "excludedPicks": [-1],
                "excludedBans": []
            }]
        });
        runtime
            .cache()
            .write(ConfigResource::AutoSelectGroups.cache_path(), &cached)
            .unwrap();

        let updates = runtime.load_cached_configs();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].key(), "auto-select-main:state#groups");
        assert_eq!(updates[0].value.as_array().unwrap()[0]["groupId"], json!("ranked"));
        assert_eq!(
            runtime.updated_at(ConfigResource::AutoSelectGroups),
            "2099-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn an_older_cache_file_is_deleted_instead_of_loaded() {
        let (runtime, _dir) = runtime();
        let stale = json!({
            "updatedAt": "2020-01-01T00:00:00.000Z",
            "queues": [420]
        });
        let path = ConfigResource::SupportedQueues.cache_path();
        runtime.cache().write(path, &stale).unwrap();
        assert!(runtime.cache().exists(path));

        assert!(runtime.load_cached_configs().is_empty());
        assert!(!runtime.cache().exists(path), "stale cache file must go");
        assert_eq!(
            runtime.current_document(ConfigResource::SupportedQueues)["queues"],
            json!([420, 1700]),
            "the builtin stays"
        );
    }

    #[test]
    fn an_invalid_cache_file_is_deleted_and_the_others_still_load() {
        let (runtime, _dir) = runtime();
        // `queues: ["invalid"]` is the fixture from `config-loader.test.ts`
        runtime
            .cache()
            .write(
                ConfigResource::SupportedQueues.cache_path(),
                &json!({ "updatedAt": "2099-01-01T00:00:00.000Z", "queues": ["invalid"] }),
            )
            .unwrap();
        runtime
            .cache()
            .write(
                ConfigResource::FeatureGates.cache_path(),
                &json!({
                    "updatedAt": "2099-01-02T00:00:00.000Z",
                    "gates": { "ongoing-game.deobfuscation": { "platforms": ["win32"] } }
                }),
            )
            .unwrap();

        let updates = runtime.load_cached_configs();
        assert!(!runtime.cache().exists(ConfigResource::SupportedQueues.cache_path()));
        assert_eq!(updates.len(), 1, "only the feature gates survived");
        assert_eq!(updates[0].key(), "riftflow-api-main:state#featureGates");
    }

    #[test]
    fn an_equal_cache_file_changes_nothing() {
        let (runtime, _dir) = runtime();
        let same = runtime.current_document(ConfigResource::SupportedQueues).clone();
        let path = ConfigResource::SupportedQueues.cache_path();
        runtime.cache().write(path, &same).unwrap();

        assert!(runtime.load_cached_configs().is_empty());
        // `cached == current` is neither older nor newer, so the file stays
        assert!(runtime.cache().exists(path));
    }

    #[test]
    fn only_the_declared_resources_are_read_from_the_cache() {
        let (runtime, _dir) = runtime();
        assert_eq!(CONFIG_RESOURCES.len(), 4);
        assert!(runtime.load_cached_configs().is_empty());
        let keys: Vec<String> = runtime
            .prop_updates()
            .iter()
            .map(PropUpdate::key)
            .collect();
        assert_eq!(
            keys,
            vec![
                "sgp-main:state#supportedQueues",
                "sgp-main:state#leagueServers",
                "auto-select-main:state#groups",
            ]
            .into_iter()
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_fetched_document_would_only_replace_a_newer_updated_at() {
        // the comparison `_updateAndSave` runs, without the network
        assert!(decide_remote_replace(
            "2099-01-01T00:00:00.000Z",
            "2026-07-18T04:00:00.000Z"
        ));
        assert!(!decide_remote_replace(
            "2020-01-01T00:00:00.000Z",
            "2026-07-18T04:00:00.000Z"
        ));
        assert!(!decide_remote_replace(
            "2026-07-18T04:00:00.000Z",
            "2026-07-18T04:00:00.000Z"
        ));
    }

    #[test]
    fn the_updating_flag_is_exclusive_and_self_clearing() {
        let (runtime, _dir) = runtime();
        let guard = UpdatingGuard::begin(&runtime, ConfigResource::LeagueServers);
        assert!(guard.is_some());
        assert!(runtime.is_updating(ConfigResource::LeagueServers));
        assert!(!runtime.is_updating(ConfigResource::AutoSelectGroups));

        assert!(
            UpdatingGuard::begin(&runtime, ConfigResource::LeagueServers)
                .is_none(),
            "a second refresh of the same resource is skipped, like the JS guard"
        );

        drop(guard);
        assert!(!runtime.is_updating(ConfigResource::LeagueServers));
    }

    #[test]
    fn storing_a_document_moves_the_comparison_baseline() {
        let (runtime, _dir) = runtime();
        let document = json!({
            "updatedAt": "2099-01-01T00:00:00.000Z",
            "gates": { "a.b": {} }
        });

        let update = runtime.store_document(ConfigResource::FeatureGates, document.clone());
        assert_eq!(update.key(), "riftflow-api-main:state#featureGates");
        assert_eq!(update.value, document);
        assert_eq!(
            runtime.updated_at(ConfigResource::FeatureGates),
            "2099-01-01T00:00:00.000Z"
        );
        assert_eq!(runtime.feature_gates(), Some(document));
    }

    /// A service that cannot be reached: port 1 on loopback refuses, so no test
    /// ever needs the network.
    fn offline_runtime() -> (RiftFlowApiRuntime, TempDir) {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "riftflow-api-offline-{}-{id}",
            std::process::id()
        ));
        let client = RiftFlowApiClient::with_base_urls(
            "1.5.1",
            RiftFlowServiceBaseUrls {
                api: "http://127.0.0.1:1".to_string(),
                static_assets: "http://127.0.0.1:1".to_string(),
            },
        );
        let runtime = RiftFlowApiRuntime::new(
            client,
            JsonConfigCache::under_user_data(&dir),
            builtins(),
            None,
        );
        (runtime, TempDir(dir))
    }

    #[tokio::test]
    async fn an_unreachable_service_reports_a_failure_and_clears_its_guard() {
        let (runtime, _dir) = offline_runtime();
        let outcome = runtime
            .refresh_config(ConfigResource::SupportedQueues)
            .await;

        assert!(
            matches!(outcome, RefreshOutcome::Failed { .. }),
            "expected a failure, got {outcome:?}"
        );
        assert_eq!(outcome.resource(), ConfigResource::SupportedQueues);
        assert!(outcome.updates().is_empty(), "nothing was replaced");
        assert!(
            !runtime.is_updating(ConfigResource::SupportedQueues),
            "the isUpdating flag has to fall back"
        );
        // the builtin list still stands, and the cache file was never written
        assert_eq!(
            runtime.current_document(ConfigResource::SupportedQueues)["queues"],
            json!([420, 1700])
        );
        assert!(!runtime
            .cache()
            .exists(ConfigResource::SupportedQueues.cache_path()));
    }

    #[tokio::test]
    async fn a_notice_refresh_survives_both_requests_failing() {
        let (runtime, _dir) = offline_runtime();
        assert!(
            runtime
                .refresh_notice_and_contact_channels("zh-CN")
                .await
                .is_empty()
        );
        assert!(runtime.notice().is_none());
        assert!(runtime.contact_channels().is_none());
    }
}
