//! `SetterSettingService.readFromJsonConfigFile` and friends: the cache files a
//! shard keeps next to its settings database.
//!
//! The layout is `settings-json-file-service.ts:111-118` —
//! `<userData>/RiftFlowConfig/<namespace>/<filename>` — with `writeConfigFile`'s
//! two-space pretty JSON and `deleteConfigFile`'s "no error if it is not there".
//! Filenames come from constants, never from the network, but the path is still
//! checked for traversal, because a cache file is the one thing a hostile response
//! could try to name.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::config::{RIFTFLOW_API_MAIN_NAMESPACE, CONFIG_DIR_NAME};

pub struct JsonConfigCache {
    root: PathBuf,
}

impl JsonConfigCache {
    /// `path.join(app.getPath('userData'), 'RiftFlowConfig', 'riftflow-api-main')`
    pub fn under_user_data(user_data_dir: &Path) -> Self {
        Self::with_namespace(user_data_dir, RIFTFLOW_API_MAIN_NAMESPACE)
    }

    pub fn with_namespace(user_data_dir: &Path, namespace: &str) -> Self {
        Self {
            root: user_data_dir.join(CONFIG_DIR_NAME).join(namespace),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The absolute path for a cache filename, or `None` when it tries to leave the
    /// shard's directory.
    pub fn path_for(&self, filename: &str) -> Option<PathBuf> {
        let path = Path::new(filename);
        if path.is_absolute() {
            return None;
        }
        if !path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return None;
        }
        Some(self.root.join(path))
    }

    /// `configFileExists`
    pub fn exists(&self, filename: &str) -> bool {
        self.path_for(filename)
            .is_some_and(|path| path.is_file())
    }

    /// `readFromJsonConfigFile` — a missing or unreadable file is an error the
    /// caller logs and moves on from, exactly like the JS throw.
    pub fn read(&self, filename: &str) -> Result<Value, String> {
        let path = self
            .path_for(filename)
            .ok_or_else(|| format!("invalid config file name {filename}"))?;

        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("config file {filename} does not exist: {error}"))?;
        serde_json::from_str(&content).map_err(|error| format!("{filename} is not json: {error}"))
    }

    /// `writeToJsonConfigFile` — `mkdir -p`, then pretty JSON.
    pub fn write(&self, filename: &str, value: &Value) -> Result<(), String> {
        let path = self
            .path_for(filename)
            .ok_or_else(|| format!("invalid config file name {filename}"))?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(value).unwrap_or_default())
            .map_err(|error| format!("cannot write {}: {error}", path.display()))
    }

    /// `deleteJsonConfigFile` — `fs.rm(..., { force: true })`
    pub fn delete(&self, filename: &str) -> Result<(), String> {
        let Some(path) = self.path_for(filename) else {
            return Err(format!("invalid config file name {filename}"));
        };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("cannot delete {}: {error}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigResource;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_cache() -> (JsonConfigCache, PathBuf) {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "riftflow-api-cache-{}-{id}",
            std::process::id()
        ));
        (JsonConfigCache::under_user_data(&dir), dir)
    }

    #[test]
    fn a_resource_caches_under_its_declared_path() {
        let (cache, dir) = temp_cache();
        assert_eq!(
            cache
                .path_for(ConfigResource::AutoSelectGroups.cache_path())
                .unwrap(),
            dir.join("RiftFlowConfig")
                .join("riftflow-api-main")
                .join("config")
                .join("v1")
                .join("auto-select")
                .join("groups.json")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_written_document_round_trips_and_can_be_deleted() {
        let (cache, dir) = temp_cache();
        let name = ConfigResource::SupportedQueues.cache_path();
        let document = json!({ "updatedAt": "2099-01-01T00:00:00.000Z", "queues": [420, 1700] });

        assert!(!cache.exists(name));
        cache.write(name, &document).unwrap();
        assert!(cache.exists(name));
        assert_eq!(cache.read(name).unwrap(), document);

        // `fs.rm(force: true)` does not complain about a file that is gone
        cache.delete(name).unwrap();
        cache.delete(name).unwrap();
        assert!(!cache.exists(name));
        assert!(cache.read(name).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_cache_name_cannot_climb_out_of_the_shard_directory() {
        let (cache, dir) = temp_cache();
        assert!(cache.path_for("../escape.json").is_none());
        assert!(cache.path_for("config/../../escape.json").is_none());
        assert!(cache.path_for("C:\\windows\\system32\\x.json").is_none());
        assert!(cache.path_for("./config/v1/app/feature-gates.json").is_some());

        assert!(cache.write("..", &json!({})).is_err());
        assert!(!cache.exists(".."));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_truncated_file_reads_as_an_error_not_a_panic() {
        let (cache, dir) = temp_cache();
        let name = "bootstrap.json";
        std::fs::create_dir_all(cache.root()).unwrap();
        std::fs::write(cache.path_for(name).unwrap(), "{ not json").unwrap();

        assert!(cache.exists(name));
        let error = cache.read(name).unwrap_err();
        assert!(error.contains("is not json"), "{error}");

        std::fs::remove_dir_all(&dir).ok();
    }
}
