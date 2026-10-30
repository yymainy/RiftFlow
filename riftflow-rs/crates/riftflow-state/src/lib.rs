//! Port of `src/main/shards/mobx-utils/index.ts` (`MobxUtilsMain`).
//!
//! MobX observes class fields through proxies; Rust has no such reflection, so
//! a shard's synced state lives *inside* the [`StateSync`] store as one JSON
//! value per declared prop path. Writes go through `set`, which diffs and pushes
//! the identical `update-state-prop/<namespace>:<stateId>` event, including the
//! lazy-subscription rule: only renderers that called
//! `subscribeAndGetInitialState` ever receive pushes.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use riftflow_ipc::{RiftFlowIpcMain, RendererId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MOBX_UTILS_MAIN_NAMESPACE: &str = "mobx-utils-main";

/// Key the compat shim revives into a real JS `Set` after delivery. Electron's
/// structured clone transferred `Set` values as-is; a JSON wire cannot, and the
/// renderer calls `.has()` on the champion-id props, so an array is not enough.
pub const SET_MARKER: &str = "__riftflowSet__";

/// `PropConfig` — whether the renderer should `markRaw` the value.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropConfig {
    pub raw: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Update,
    Delete,
}

impl Action {
    fn as_str(&self) -> &'static str {
        match self {
            Action::Update => "update",
            Action::Delete => "delete",
        }
    }
}

struct RegisteredState {
    /// Declared order, matching the JS `Map` that `subscribeAndGetInitialState`
    /// turns into the returned object.
    props: Vec<(String, PropConfig)>,
    values: HashMap<String, Value>,
    /// Props whose renderer-side home is a `Set`, encoded with [`SET_MARKER`].
    set_paths: Vec<String>,
    subscribers: Vec<RendererId>,
}

pub struct StateSync {
    ipc: Arc<RiftFlowIpcMain>,
    states: RwLock<HashMap<String, RegisteredState>>,
}

fn state_key(namespace: &str, state_id: &str) -> String {
    format!("{namespace}:{state_id}")
}

impl StateSync {
    pub fn new(ipc: Arc<RiftFlowIpcMain>) -> Self {
        Self {
            ipc,
            states: RwLock::new(HashMap::new()),
        }
    }

    /// `propSync(namespace, stateId, obj, propPath, { raw })`.
    /// `initial_values` supplies what MobX would have read off the object.
    pub fn prop_sync(
        &self,
        namespace: &str,
        state_id: &str,
        props: &[(&str, Value)],
        raw: bool,
    ) -> anyhow::Result<()> {
        let key = state_key(namespace, state_id);
        let config = PropConfig { raw };
        let mut states = self.states.write().unwrap();

        let entry = states.entry(key.clone()).or_insert_with(|| RegisteredState {
            props: Vec::new(),
            values: HashMap::new(),
            set_paths: Vec::new(),
            subscribers: Vec::new(),
        });

        for (path, value) in props {
            if entry.props.iter().any(|(existing, _)| existing == path) {
                anyhow::bail!(
                    "Prop path {} already registered for {}",
                    path,
                    state_id
                );
            }

            entry.props.push((path.to_string(), config));
            entry.values.insert(path.to_string(), value.clone());
        }

        Ok(())
    }

    pub fn get(&self, namespace: &str, state_id: &str, path: &str) -> Option<Value> {
        let key = state_key(namespace, state_id);
        self.states
            .read()
            .unwrap()
            .get(key.as_str())?
            .values
            .get(path)
            .cloned()
    }

    /// Declare that a prop is a `Set` on the renderer side, so pushes carry the
    /// marker the shim revives.
    pub fn mark_as_set(&self, namespace: &str, state_id: &str, path: &str) -> anyhow::Result<()> {
        let key = state_key(namespace, state_id);
        let mut states = self.states.write().unwrap();
        let entry = states
            .get_mut(key.as_str())
            .ok_or_else(|| anyhow::anyhow!("State {} not found", key))?;

        if !entry.props.iter().any(|(existing, _)| existing == path) {
            anyhow::bail!("Prop path {} not registered for {}", path, state_id);
        }

        if !entry.set_paths.iter().any(|existing| existing == path) {
            entry.set_paths.push(path.to_string());
        }

        Ok(())
    }

    fn encode(value: Value, as_set: bool) -> Value {
        if as_set {
            return json!({ SET_MARKER: value });
        }

        value
    }

    /// Assign a prop; pushes to subscribers only when the value changed,
    /// which is what a MobX `reaction` does.
    pub fn set(&self, namespace: &str, state_id: &str, path: &str, value: Value) -> anyhow::Result<bool> {
        let key = state_key(namespace, state_id);
        let (config, as_set, subscribers) = {
            let mut states = self.states.write().unwrap();
            let Some(entry) = states.get_mut(key.as_str()) else {
                anyhow::bail!("State {} not found", key);
            };

            let Some((_, config)) = entry.props.iter().find(|(existing, _)| existing == path) else {
                anyhow::bail!("Prop path {} not registered for {}", path, state_id);
            };
            let config = *config;
            let as_set = entry.set_paths.iter().any(|existing| existing == path);

            if entry.values.get(path) == Some(&value) {
                return Ok(false);
            }

            entry.values.insert(path.to_string(), value.clone());
            (config, as_set, entry.subscribers.clone())
        };

        self.push(&subscribers, &key, path, value, Action::Update, config, as_set);

        Ok(true)
    }

    /// `notifyStatePropChange` — pushes unconditionally, for deep props.
    pub fn notify_state_prop_change(
        &self,
        namespace: &str,
        state_id: &str,
        path: &str,
        value: Value,
        action: Action,
        raw: bool,
    ) {
        let key = state_key(namespace, state_id);
        let states = self.states.read().unwrap();

        // JS returns early when nothing ever subscribed to this state
        let Some(entry) = states.get(key.as_str()) else {
            return;
        };

        let config = entry
            .props
            .iter()
            .find(|(existing, _)| existing == path)
            .map(|(_, config)| *config)
            .unwrap_or(PropConfig { raw });

        let as_set = entry.set_paths.iter().any(|existing| existing == path);
        let subscribers = entry.subscribers.clone();
        drop(states);

        self.push(&subscribers, &key, path, value, action, config, as_set);
    }

    /// `subscribeAndGetInitialState(event.sender, namespace, stateId)`
    pub fn subscribe_and_get_initial_state(
        &self,
        renderer: RendererId,
        namespace: &str,
        state_id: &str,
    ) -> anyhow::Result<Value> {
        let key = state_key(namespace, state_id);
        let mut states = self.states.write().unwrap();

        let Some(entry) = states.get_mut(key.as_str()) else {
            anyhow::bail!("State {} not found", key);
        };

        if !entry.subscribers.contains(&renderer) {
            entry.subscribers.push(renderer);
        }

        let mut initial = serde_json::Map::new();
        for (path, config) in entry.props.clone() {
            let value = entry
                .values
                .get(&path)
                .cloned()
                .unwrap_or(Value::Null);
            let as_set = entry.set_paths.iter().any(|existing| existing == &path);
            initial.insert(
                path,
                json!({ "value": Self::encode(value, as_set), "config": config }),
            );
        }

        Ok(Value::Object(initial))
    }

    pub fn unsubscribe(&self, renderer: RendererId) {
        for entry in self.states.write().unwrap().values_mut() {
            entry.subscribers.retain(|existing| *existing != renderer);
        }
    }

    pub fn clear(&self) {
        self.states.write().unwrap().clear();
    }

    /// The exact event a JS `reaction` emits:
    /// `update-state-prop/<key>`, `[path, value, { action, raw }]` under the
    /// `mobx-utils-main` namespace.
    fn push(
        &self,
        subscribers: &[RendererId],
        key: &str,
        path: &str,
        value: Value,
        action: Action,
        config: PropConfig,
        as_set: bool,
    ) {
        for renderer in subscribers {
            self.ipc.send_event_to(
                *renderer,
                MOBX_UTILS_MAIN_NAMESPACE,
                &format!("update-state-prop/{key}"),
                &[
                    json!(path),
                    Self::encode(value.clone(), as_set),
                    json!({ "action": action.as_str(), "raw": config.raw }),
                ],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_props_are_marked_for_the_shim() {
        assert_eq!(StateSync::encode(json!([1, 2]), false), json!([1, 2]));

        let marked = StateSync::encode(json!([1, 2]), true);
        assert_eq!(marked[SET_MARKER], json!([1, 2]));
    }
}
