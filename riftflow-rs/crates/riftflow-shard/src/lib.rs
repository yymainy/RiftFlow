//! Port of the shard runtime in `src/shared/riftflow-shard/manager.ts`.
//!
//! TypeScript gets dependency injection from `reflect-metadata` decorators;
//! Rust cannot, so a shard declares its dependency ids at registration time and
//! pulls typed instances out of the [`Instances`] bag inside its builder.
//! Everything else keeps the original semantics and error strings:
//! priority-descending dependency resolution, depth-first construction, cycle
//! detection, a per-shard timing report, and reverse-order disposal.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::Value;

pub const SHARED_GLOBAL_ID: &str = "<riftflow-shared-global>";
pub const INTERNAL_RUNNER_ID: &str = "<riftflow-shard-runner~(∠・ω<)⌒★>";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Win32,
    Darwin,
    Unknown,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Platform::Win32
        } else if cfg!(target_os = "macos") {
            Platform::Darwin
        } else {
            Platform::Unknown
        }
    }
}

/// The Winston logger object on the JS side.
pub trait AppLogger: Send + Sync {
    fn log(&self, level: &str, message: &str, namespace: Option<&str>, extra: Option<Value>);

    fn debug(&self, message: &str) {
        self.log("debug", message, None, None);
    }
    fn info(&self, message: &str) {
        self.log("info", message, None, None);
    }
    fn warn(&self, message: &str) {
        self.log("warn", message, None, None);
    }
    fn error(&self, message: &str) {
        self.log("error", message, None, None);
    }
}

/// App-level event bus, replacing the Node `EventEmitter` in
/// `manager.global.events`.
#[derive(Default)]
pub struct AppEventBus {
    listeners: Mutex<HashMap<String, Vec<(u64, Arc<dyn Fn(Value) + Send + Sync>)>>>,
    next_id: Mutex<u64>,
}

impl AppEventBus {
    pub fn on<F: Fn(Value) + Send + Sync + 'static>(&self, event: &str, listener: F) -> u64 {
        let mut next = self.next_id.lock().unwrap();
        *next += 1;
        let id = *next;
        self.listeners
            .lock()
            .unwrap()
            .entry(event.to_string())
            .or_default()
            .push((id, Arc::new(listener)));
        id
    }

    pub fn off(&self, event: &str, id: u64) {
        let mut listeners = self.listeners.lock().unwrap();
        if let Some(entry) = listeners.get_mut(event) {
            entry.retain(|(existing, _)| *existing != id);
            if entry.is_empty() {
                listeners.remove(event);
            }
        }
    }

    pub fn emit(&self, event: &str, payload: Value) {
        let snapshot: Vec<Arc<dyn Fn(Value) + Send + Sync>> = self
            .listeners
            .lock()
            .unwrap()
            .get(event)
            .map(|entry| entry.iter().map(|(_, f)| f.clone()).collect())
            .unwrap_or_default();

        for listener in snapshot {
            listener(payload.clone());
        }
    }

    pub fn remove_all_listeners(&self) {
        self.listeners.lock().unwrap().clear();
    }

    pub fn listener_count(&self, event: &str) -> usize {
        self.listeners
            .lock()
            .unwrap()
            .get(event)
            .map(|entry| entry.len())
            .unwrap_or(0)
    }
}

/// The `manager.global.*` closures supplied by the app shell
/// (`src/main/bootstrap/index.ts`).
pub trait AppControl: Send + Sync {
    fn quit(&self);
    fn restart(&self);
    fn log_level(&self) -> String;
    fn set_log_level(&self, level: &str);
    fn write_base_config(&self, patch: Value);
}

/// `RiftFlowSharedGlobal`, typed rather than a cast-everything object bag.
pub struct SharedGlobal {
    pub logger: Arc<dyn AppLogger>,
    pub log_filename: RwLock<String>,
    pub events: AppEventBus,
    pub is_elevated: bool,
    pub platform: Platform,
    pub version: String,
    pub is_windows11_22h2_or_higher: bool,
    pub is_ready_to_quit: RwLock<bool>,
    pub startup_deep_link: RwLock<Option<String>>,
    pub base_config: RwLock<Value>,
    pub control: Arc<dyn AppControl>,
}

impl SharedGlobal {
    pub fn set_ready_to_quit(&self, value: bool) {
        *self.is_ready_to_quit.write().unwrap() = value;
    }

    pub fn ready_to_quit(&self) -> bool {
        *self.is_ready_to_quit.read().unwrap()
    }

    /// `baseConfig.write(partial)` — merge locally, then persist.
    pub fn patch_base_config(&self, patch: Value) {
        {
            let mut current = self.base_config.write().unwrap();
            match (current.as_object_mut(), patch.as_object()) {
                (Some(target), Some(incoming)) => {
                    for (key, value) in incoming {
                        target.insert(key.clone(), value.clone());
                    }
                }
                _ => *current = patch.clone(),
            }
        }
        self.control.write_base_config(patch);
    }
}

/// Typed instance bag. It is filled in resolution order, so a shard's builder
/// only ever sees dependencies that are already constructed.
#[derive(Clone, Default)]
pub struct Instances {
    map: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>,
}

impl Instances {
    pub fn insert<T: Send + Sync + 'static>(&self, id: &str, instance: Arc<T>) {
        self.map
            .write()
            .unwrap()
            .insert(id.to_string(), instance as Arc<dyn Any + Send + Sync>);
    }

    /// Stores an already-erased instance (whose `TypeId` is the concrete `T`).
    pub fn insert_dyn(&self, id: &str, instance: Arc<dyn Any + Send + Sync>) {
        self.map.write().unwrap().insert(id.to_string(), instance);
    }

    /// Recovers the concrete `Arc<T>`. Note that unsizing `Arc<T>` into
    /// `Arc<dyn Any + Send + Sync>` records `T`, not `Arc<T>`, so the only way
    /// back is `Arc::downcast`.
    pub fn get<T: Send + Sync + 'static>(&self, id: &str) -> Option<Arc<T>> {
        let guard = self.map.read().unwrap();
        let found = guard.get(id)?;
        found.clone().downcast::<T>().ok()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.map.read().unwrap().contains_key(id)
    }

    pub fn clear(&self) {
        self.map.write().unwrap().clear();
    }
}

/// The `@Shard`-decorated class contract.
#[async_trait]
pub trait RiftFlowShard: Send + Sync + 'static {
    async fn on_init(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_finish(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_dispose(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A builder returns the concrete `Arc<S>` (for typed lookup) together with the
/// trait object used to drive lifecycle hooks. `Arc<dyn Trait>` is not
/// downcastable, so both handles must come out of the same construction.
pub struct BuiltShard {
    pub shard: Arc<dyn RiftFlowShard>,
    pub typed: Arc<dyn Any + Send + Sync>,
}

type BoxedBuilder = Arc<dyn Fn(&Instances, &Value) -> anyhow::Result<BuiltShard> + Send + Sync>;

struct Registration {
    id: String,
    priority: i32,
    deps: Vec<String>,
    builder: BoxedBuilder,
    config: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShardInitializationTiming {
    pub id: String,
    pub started_at: u128,
    pub ended_at: u128,
    pub duration_ms: f64,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShardInitializationSummary {
    pub started_at: u128,
    pub ended_at: u128,
    pub total_duration_ms: f64,
    pub shard_count: usize,
    pub slowest_shard: Option<ShardInitializationTiming>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShardInitializationReport {
    pub order: Vec<String>,
    pub timings: Vec<ShardInitializationTiming>,
    pub summary: Option<ShardInitializationSummary>,
}

/// Rendered like `formatShardInitializationReport` in bootstrap/index.ts.
impl ShardInitializationReport {
    pub fn format(&self) -> String {
        let timing_of = |id: &str| -> String {
            match self.timings.iter().find(|t| t.id == id) {
                Some(t) if t.ok => format!("{}({:.2}ms)", id, t.duration_ms),
                Some(t) => format!("{}({:.2}ms failed)", id, t.duration_ms),
                None => format!("{}(-)", id),
            }
        };

        let order_text = self
            .order
            .iter()
            .map(|id| timing_of(id))
            .collect::<Vec<_>>()
            .join(" -> ");

        let slowest_text = self
            .summary
            .as_ref()
            .and_then(|s| s.slowest_shard.clone())
            .map(|t| format!("{}({:.2}ms)", t.id, t.duration_ms))
            .unwrap_or_else(|| "none".to_string());

        let summary = self.summary.clone().unwrap_or(ShardInitializationSummary {
            started_at: 0,
            ended_at: 0,
            total_duration_ms: 0.0,
            shard_count: 0,
            slowest_shard: None,
        });

        format!(
            "Shard initialization report: {} initialized shards in {:.2}ms\nInitialization order: {}\nSlowest shard initialization: {}",
            summary.shard_count, summary.total_duration_ms, order_text, slowest_text
        )
    }
}

struct RunnerShard;

#[async_trait]
impl RiftFlowShard for RunnerShard {}

struct GlobalShard;

#[async_trait]
impl RiftFlowShard for GlobalShard {}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn empty_report() -> ShardInitializationReport {
    ShardInitializationReport {
        order: Vec::new(),
        timings: Vec::new(),
        summary: Some(ShardInitializationSummary {
            started_at: 0,
            ended_at: 0,
            total_duration_ms: 0.0,
            shard_count: 0,
            slowest_shard: None,
        }),
    }
}

fn timing(id: &str, started_at: u128, ended_at: u128, ok: bool, error: Option<String>) -> ShardInitializationTiming {
    ShardInitializationTiming {
        id: id.to_string(),
        started_at,
        ended_at,
        duration_ms: ended_at.saturating_sub(started_at) as f64,
        ok,
        error,
    }
}

pub struct RiftFlowManager {
    registrations: HashMap<String, Registration>,
    /// `manager.use(...)` call order, so stable sorts match JS.
    insertion_order: Vec<String>,
    instances: Instances,
    shards: HashMap<String, Arc<dyn RiftFlowShard>>,
    report: ShardInitializationReport,
    is_setup: bool,
    pub global: Arc<SharedGlobal>,
}

impl RiftFlowManager {
    pub fn new(global: Arc<SharedGlobal>) -> Self {
        Self {
            registrations: HashMap::new(),
            insertion_order: Vec::new(),
            instances: Instances::default(),
            shards: HashMap::new(),
            report: empty_report(),
            is_setup: false,
            global,
        }
    }

    /// `manager.use(ShardClass)` — priority 0, no declared dependencies.
    pub fn use_shard<S, F>(&mut self, id: &str, builder: F) -> anyhow::Result<()>
    where
        S: RiftFlowShard,
        F: Fn(&Instances, &Value) -> anyhow::Result<Arc<S>> + Send + Sync + 'static,
    {
        self.use_shard_with(id, 0, &[], builder, Value::Null)
    }

    /// `@Shard(id, priority)` plus the constructor parameter list and config.
    pub fn use_shard_with<S, F>(
        &mut self,
        id: &str,
        priority: i32,
        deps: &[&str],
        builder: F,
        config: Value,
    ) -> anyhow::Result<()>
    where
        S: RiftFlowShard,
        F: Fn(&Instances, &Value) -> anyhow::Result<Arc<S>> + Send + Sync + 'static,
    {
        if self.registrations.contains_key(id) {
            anyhow::bail!("Shard with id \"{}\" already exists", id);
        }

        let boxed: BoxedBuilder = Arc::new(move |instances, _config| {
            let concrete = builder(instances, _config)?;
            Ok(BuiltShard {
                shard: concrete.clone() as Arc<dyn RiftFlowShard>,
                typed: concrete as Arc<dyn Any + Send + Sync>,
            })
        });

        self.insertion_order.push(id.to_string());
        self.registrations.insert(
            id.to_string(),
            Registration {
                id: id.to_string(),
                priority,
                deps: deps.iter().map(|d| d.to_string()).collect(),
                builder: boxed,
                config,
            },
        );

        Ok(())
    }

    pub fn registered_ids(&self) -> &[String] {
        &self.insertion_order
    }

    pub fn is_setup(&self) -> bool {
        self.is_setup
    }

    /// `manager.getInstance(id)`
    pub fn get<T: Send + Sync + 'static>(&self, id: &str) -> Option<Arc<T>> {
        self.instances.get::<T>(id)
    }

    pub fn get_shard(&self, id: &str) -> Option<Arc<dyn RiftFlowShard>> {
        self.shards.get(id).cloned()
    }

    pub fn instances(&self) -> &Instances {
        &self.instances
    }

    /// `_getInitializationReport()`
    pub fn initialization_report(&self) -> ShardInitializationReport {
        self.report.clone()
    }

    /// `manager.setup()`
    pub async fn setup(&mut self) -> anyhow::Result<ShardInitializationReport> {
        if self.is_setup {
            anyhow::bail!("Already setup");
        }

        // shared global is a pre-inserted singleton, like JS
        let global = self.global.clone();
        self.instances.insert::<SharedGlobal>(SHARED_GLOBAL_ID, global.clone());
        self.shards
            .insert(SHARED_GLOBAL_ID.to_string(), Arc::new(GlobalShard));

        let all_ids: Vec<String> = self
            .insertion_order
            .iter()
            .filter(|id| id.as_str() != INTERNAL_RUNNER_ID)
            .cloned()
            .collect();

        self.insert_registration(Registration {
            id: INTERNAL_RUNNER_ID.to_string(),
            priority: i32::MIN,
            deps: all_ids,
            builder: Arc::new(|_instances, _config| {
                let shard = Arc::new(RunnerShard);
                Ok(BuiltShard {
                    typed: shard.clone() as Arc<dyn Any + Send + Sync>,
                    shard: shard as Arc<dyn RiftFlowShard>,
                })
            }),
            config: Value::Null,
        });

        self.report = empty_report();

        let mut visited: Vec<String> = Vec::new();
        let mut order: Vec<String> = Vec::new();
        self.initialize_shard(INTERNAL_RUNNER_ID, &mut visited, &mut order)?;
        self.report.order = order;

        let setup_started_at = now_millis();

        for id in self.report.order.clone() {
            // JS records/invokes only for shards that declare the hook, and the
            // synthetic runner declares none.
            if id == INTERNAL_RUNNER_ID {
                continue;
            }

            let Some(shard) = self.shards.get(&id).cloned() else {
                continue;
            };

            let started_at = now_millis();
            match shard.on_init().await {
                Ok(()) => {
                    let ended_at = now_millis();
                    self.report.timings.push(timing(&id, started_at, ended_at, true, None));
                }
                Err(error) => {
                    let ended_at = now_millis();
                    let message = error.to_string();
                    self.report.timings.push(timing(
                        &id,
                        started_at,
                        ended_at,
                        false,
                        Some(message.clone()),
                    ));
                    let summary = self.build_summary(setup_started_at, ended_at);
                    self.report.summary = Some(summary);
                    anyhow::bail!("Failed to initialize shard {}: {}", id, message);
                }
            }
        }

        let setup_ended_at = now_millis();
        let summary = self.build_summary(setup_started_at, setup_ended_at);
        self.report.summary = Some(summary);

        for id in self.report.order.clone() {
            // JS records/invokes only for shards that declare the hook, and the
            // synthetic runner declares none.
            if id == INTERNAL_RUNNER_ID {
                continue;
            }

            let Some(shard) = self.shards.get(&id).cloned() else {
                continue;
            };
            if let Err(error) = shard.on_finish().await {
                anyhow::bail!("Failed to trigger onFinish hook of shard {}: {}", id, error);
            }
        }

        self.is_setup = true;
        Ok(self.report.clone())
    }

    /// `manager.dispose()` — hooks run in reverse initialization order.
    pub async fn dispose(&mut self) -> anyhow::Result<()> {
        if !self.is_setup {
            anyhow::bail!("Not setup yet");
        }

        let reversed: Vec<String> = self.report.order.iter().rev().cloned().collect();
        for id in reversed {
            let Some(shard) = self.shards.get(&id).cloned() else {
                continue;
            };
            if let Err(error) = shard.on_dispose().await {
                anyhow::bail!(
                    "Failed to trigger onDispose hook of shard {}: {}",
                    id,
                    error
                );
            }
        }

        self.shards.clear();
        self.instances.clear();
        self.report = empty_report();
        self.is_setup = false;
        Ok(())
    }

    fn insert_registration(&mut self, registration: Registration) {
        self.insertion_order.retain(|id| id != &registration.id);
        self.insertion_order.push(registration.id.clone());
        self.registrations
            .insert(registration.id.clone(), registration);
    }

    /// Mirrors `_initializeShard`.
    fn initialize_shard(
        &mut self,
        id: &str,
        visited: &mut Vec<String>,
        order: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        let (deps, builder, config) = match self.registrations.get(id) {
            Some(registration) => (
                registration.deps.clone(),
                registration.builder.clone(),
                registration.config.clone(),
            ),
            None => anyhow::bail!("Shard not registered: \"{}\"", id),
        };

        for dep in &deps {
            if !self.registrations.contains_key(dep) {
                anyhow::bail!("Shard not registered: \"{}\"", dep);
            }
        }

        let mut dep_priorities: HashMap<String, i32> = HashMap::new();
        for (key, registration) in self.registrations.iter() {
            dep_priorities.insert(key.clone(), registration.priority);
        }

        // JS `toSorted((a, b) => bM.priority - aM.priority)` is a stable sort,
        // so equal priorities keep their dependency-declaration order.
        let mut sorted_deps = deps.clone();
        sorted_deps.sort_by(|a, b| {
            let a_priority = dep_priorities.get(a).copied().unwrap_or(i32::MIN);
            let b_priority = dep_priorities.get(b).copied().unwrap_or(i32::MIN);
            b_priority.cmp(&a_priority)
        });

        for dep in sorted_deps {
            if visited.contains(&dep) {
                let mut path = visited.clone();
                path.push(dep.clone());
                anyhow::bail!("Circular dependency detected: {}", path.join(" -> "));
            }

            if self.instances.contains(&dep) {
                continue;
            }

            visited.push(dep.clone());
            self.initialize_shard(&dep, visited, order)?;
            visited.pop();
        }

        order.push(id.to_string());

        let built = builder(&self.instances, &config)?;

        self.instances.insert_dyn(id, built.typed);
        self.shards.insert(id.to_string(), built.shard);

        Ok(())
    }

    fn build_summary(&self, started_at: u128, ended_at: u128) -> ShardInitializationSummary {
        let slowest_shard = self
            .report
            .timings
            .iter()
            .filter(|t| t.ok)
            .fold(None, |acc: Option<&ShardInitializationTiming>, t| {
                match acc {
                    None => Some(t),
                    Some(best) if t.duration_ms > best.duration_ms => Some(t),
                    Some(best) => Some(best),
                }
            })
            .cloned();

        ShardInitializationSummary {
            started_at,
            ended_at,
            total_duration_ms: ended_at.saturating_sub(started_at) as f64,
            shard_count: self.report.timings.len(),
            slowest_shard,
        }
    }
}
