//! Port of `src/main/shards/setting-factory/` (`SettingFactoryMain`,
//! `SetterSettingService`, `DelayedTaskScheduler`).
//!
//! The row layout is reproduced exactly: there is no namespace column, the key is
//! the flattened string `${namespace}/${key}`, `value` is JSON text, `null` is
//! stored as a DELETE, and every write is debounced by 1000 ms with a pending
//! overlay so reads see the in-flight value. The prefix range query relies on
//! BINARY collation ordering, including the original's quirk of replacing the
//! trailing `/` with `0` (0x30, the byte right after `/`) as the upper bound.

use std::collections::HashMap;
use std::future::Future;
use std::time::{Duration, Instant};
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};

use serde_json::Value;

use crate::{cell_to_json_text, Database};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub const SETTINGS_SAVE_DELAY_MS: u64 = 1000;

/// `SettingConfig.schema` — validates and normalizes; returns the value to store.
pub type Validator = Arc<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;

/// `SettingConfig.transform`
pub type Transform = Arc<dyn Fn(&TransformContext) -> Value + Send + Sync>;

/// `SettingConfig.sideEffect` — awaited before the commit; an error aborts it.
pub type SideEffect = Arc<dyn Fn(&SettingChangeContext) -> BoxFuture<'static, Result<(), String>> + Send + Sync>;

pub struct TransformContext {
    pub value: Value,
    pub old_value: Option<Value>,
    pub default_value: Value,
}

pub struct SettingChangeContext {
    pub namespace: String,
    pub key: String,
    pub old_value: Value,
    pub value: Value,
}

#[derive(Clone)]
pub struct SettingDeclaration {
    pub key: String,
    pub default: Value,
    pub schema: Option<Validator>,
    pub transform: Option<Transform>,
    pub side_effect: Option<SideEffect>,
}

/// Pending debounced storage operations, keyed by full row key.
#[derive(Clone, Debug, PartialEq)]
pub enum PendingOp {
    Save(Value),
    Remove,
}

#[derive(Clone, Debug)]
struct PendingTask {
    op: PendingOp,
    /// `debounce(fn, SETTINGS_SAVE_DELAY_MS)`: a later write to the same key
    /// restarts the window, and only elapsed windows may be written.
    due_at: Instant,
}

#[derive(Default)]
pub struct DelayedTaskScheduler {
    tasks: Mutex<HashMap<String, PendingTask>>,
}

impl DelayedTaskScheduler {
    /// `addTask` — replaces any pending task for the same key.
    pub fn schedule(&self, row_key: String, op: PendingOp) {
        let due_at = Instant::now() + Duration::from_millis(SETTINGS_SAVE_DELAY_MS);
        self.tasks.lock().unwrap().insert(row_key, PendingTask { op, due_at });
    }

    pub fn cancel(&self, row_key: &str) {
        self.tasks.lock().unwrap().remove(row_key);
    }

    pub fn peek(&self, row_key: &str) -> Option<PendingOp> {
        self.tasks
            .lock()
            .unwrap()
            .get(row_key)
            .map(|task| task.op.clone())
    }

    pub fn clear(&self) {
        self.tasks.lock().unwrap().clear();
    }

    /// The periodic half of `debounce`: everything whose window has closed.
    pub fn drain_due(&self, now: Instant) -> Vec<(String, PendingOp)> {
        let mut tasks = self.tasks.lock().unwrap();
        let due: Vec<String> = tasks
            .iter()
            .filter(|(_, task)| task.due_at <= now)
            .map(|(key, _)| key.clone())
            .collect();

        due.into_iter()
            .filter_map(|key| tasks.remove(&key).map(|task| (key, task.op)))
            .collect()
    }

    /// `flush` — every due task lands immediately; used on shutdown.
    pub fn drain(&self) -> Vec<(String, PendingOp)> {
        std::mem::take(&mut *self.tasks.lock().unwrap())
            .into_iter()
            .map(|(key, task)| (key, task.op))
            .collect()
    }

    pub fn pending_count(&self) -> usize {
        self.tasks.lock().unwrap().len()
    }
}

/// Primitive validators, the `z.boolean()` / `z.number()` / `z.string()` /
/// `z.enum(...)` equivalents.
pub fn boolean() -> Validator {
    Arc::new(|value: &Value| match value.as_bool() {
        Some(b) => Ok(Value::Bool(b)),
        None => Err("Expected boolean, received type mismatch".to_string()),
    })
}

pub fn number() -> Validator {
    Arc::new(|value: &Value| match value.as_f64() {
        // keep integral numbers as JSON integers, since `z.number()` preserves
        // the incoming value and the renderer compares these round-tripped
        Some(n) if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 => {
            Ok(Value::from(n as i64))
        }
        Some(n) => Ok(Value::from(n)),
        None => Err("Expected number".to_string()),
    })
}

pub fn string() -> Validator {
    Arc::new(|value: &Value| match value.as_str() {
        Some(s) => Ok(Value::String(s.to_string())),
        None => Err("Expected string".to_string()),
    })
}

pub fn nullable_string() -> Validator {
    Arc::new(|value: &Value| match value {
        Value::Null => Ok(Value::Null),
        Value::String(s) => Ok(Value::String(s.clone())),
        _ => Err("Expected string or null".to_string()),
    })
}

pub fn string_enum(allowed: &[&str]) -> Validator {
    let allowed: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
    Arc::new(move |value: &Value| match value.as_str() {
        Some(s) if allowed.iter().any(|a| a == s) => Ok(Value::String(s.to_string())),
        Some(s) => Err(format!("Invalid enum value: {s}")),
        None => Err("Expected string".to_string()),
    })
}

/// `SetterSettingService` for one namespace.
/// Notified after a value is committed, so the shell can push the synced prop and
/// feature shards can react. MobX's `reaction` did this implicitly by mutating the
/// settings object; here the write has one obvious exit point.
pub type ChangeSink = Arc<dyn Fn(&str, &str, &Value) + Send + Sync>;

pub struct SettingsService {
    namespace: String,
    declarations: Vec<SettingDeclaration>,
    values: Arc<RwLock<HashMap<String, Value>>>,
    scheduler: Arc<DelayedTaskScheduler>,
    db: Arc<Mutex<Database>>,
    change_sink: RwLock<Option<ChangeSink>>,
}

fn row_key(namespace: &str, key: &str) -> String {
    format!("{namespace}/{key}")
}

impl SettingsService {
    /// `register(namespace, schema, obj)` — throws on a duplicate namespace and
    /// validates every default up front, like `_validateDefaults`.
    pub fn new(
        namespace: &str,
        declarations: Vec<SettingDeclaration>,
        db: Arc<Mutex<Database>>,
    ) -> anyhow::Result<Self> {
        let mut validated = Vec::with_capacity(declarations.len());

        for declaration in declarations {
            let default = match &declaration.schema {
                Some(validator) => validator(&declaration.default).map_err(|issues| {
                    anyhow::anyhow!(
                        "Invalid default value for setting {}/{}: {}",
                        namespace,
                        declaration.key,
                        issues
                    )
                })?,
                None => declaration.default.clone(),
            };

            validated.push(SettingDeclaration {
                default,
                ..declaration
            });
        }

        Ok(Self {
            namespace: namespace.to_string(),
            declarations: validated,
            values: Arc::new(RwLock::new(HashMap::new())),
            scheduler: Arc::new(DelayedTaskScheduler::default()),
            db,
            change_sink: RwLock::new(None),
        })
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn declaration(&self, key: &str) -> Option<&SettingDeclaration> {
        self.declarations.iter().find(|d| d.key == key)
    }

    /// Every declared key with its current value — what `propSync` needs when the
    /// settings object itself is replaced by this service.
    pub fn entries(&self) -> Vec<(String, Value)> {
        self.declarations
            .iter()
            .map(|declaration| (declaration.key.clone(), self.get(&declaration.key)))
            .collect()
    }

    /// `applyToState` / `_getAllFromStorage`
    pub fn apply_to_state(&self) -> anyhow::Result<()> {
        for declaration in &self.declarations {
            let stored = raw_get(&self.db, &row_key(&self.namespace, &declaration.key))?;
            let value = match stored {
                None => declaration.default.clone(),
                Some(stored) => match &declaration.schema {
                    Some(validator) => match validator(&stored) {
                        Ok(normalized) => {
                            if normalized != stored {
                                // zod normalization differs -> rewrite the row
                                raw_save(&self.db, &row_key(&self.namespace, &declaration.key), &normalized)?;
                            }
                            normalized
                        }
                        Err(issues) => {
                            eprintln!(
                                "Invalid setting value, falling back to default {}\\{}: {}",
                                self.namespace, declaration.key, issues
                            );
                            raw_remove(&self.db, &row_key(&self.namespace, &declaration.key))?;
                            declaration.default.clone()
                        }
                    },
                    None => stored,
                },
            };

            self.values
                .write()
                .unwrap()
                .insert(declaration.key.clone(), value);
        }

        Ok(())
    }

    /// `get(key)` — a pending remove reads back the default, a pending save
    /// reads back the in-flight value.
    pub fn get(&self, key: &str) -> Value {
        let pending = self.scheduler.peek(&row_key(&self.namespace, key));
        if let Some(op) = pending {
            return match op {
                PendingOp::Remove => self.default_of(key),
                PendingOp::Save(value) => value,
            };
        }

        self.values
            .read()
            .unwrap()
            .get(key)
            .cloned()
            .unwrap_or_else(|| self.default_of(key))
    }

    /// `set(key, newValue)` — transform, validate, side effect, then commit.
    /// Async because `sideEffect` is awaited in JS before the commit.
    pub async fn set(&self, key: &str, new_value: Value) -> anyhow::Result<()> {
        let declaration = self
            .declaration(key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Setting {}/{} is not registered", self.namespace, key))?;

        let old_value = self.get(key);
        let applied = self.apply_declaration(&declaration, new_value, &old_value)?;

        // a failing sideEffect aborts the commit, exactly like the awaited JS call
        if let Some(side_effect) = &declaration.side_effect {
            let context = SettingChangeContext {
                namespace: self.namespace.clone(),
                key: key.to_string(),
                old_value: old_value.clone(),
                value: applied.value.clone(),
            };
            side_effect(&context)
                .await
                .map_err(|message| anyhow::anyhow!(message))?;
        }

        self.commit(&declaration, applied.value);
        Ok(())
    }

    fn apply_declaration(
        &self,
        declaration: &SettingDeclaration,
        new_value: Value,
        old_value: &Value,
    ) -> anyhow::Result<Applied> {
        let mut value = new_value;

        if let Some(transform) = &declaration.transform {
            value = transform(&TransformContext {
                value: value.clone(),
                old_value: Some(old_value.clone()),
                default_value: declaration.default.clone(),
            });
        }

        if let Some(validator) = &declaration.schema {
            match validator(&value) {
                Ok(parsed) => value = parsed,
                Err(issues) => {
                    eprintln!(
                        "Invalid setting value, falling back to default {}/{}: {}",
                        self.namespace, declaration.key, issues
                    );
                    return Ok(Applied {
                        value: declaration.default.clone(),
                    });
                }
            }
        }

        Ok(Applied { value })
    }

    pub fn set_change_sink(&self, sink: ChangeSink) {
        *self.change_sink.write().unwrap() = Some(sink);
    }

    fn commit(&self, declaration: &SettingDeclaration, value: Value) {
        self.values
            .write()
            .unwrap()
            .insert(declaration.key.clone(), value.clone());

        if let Some(sink) = self.change_sink.read().unwrap().as_ref() {
            sink(&self.namespace, &declaration.key, &value);
        }

        let key = row_key(&self.namespace, &declaration.key);
        if value.is_null() {
            self.scheduler.schedule(key, PendingOp::Remove);
        } else {
            self.scheduler.schedule(key, PendingOp::Save(value));
        }
    }

    fn default_of(&self, key: &str) -> Value {
        self.declaration(key)
            .map(|d| d.default.clone())
            .unwrap_or(Value::Null)
    }

    /// Call after `SETTINGS_SAVE_DELAY_MS`, or from `onDispose` — the original
    /// flushes every debounced task before the DataSource is destroyed.
    pub fn flush(&self) -> anyhow::Result<usize> {
        self.write_out(self.scheduler.drain())
    }

    /// The periodic flush: only keys whose 1000 ms window has closed.
    pub fn flush_due(&self, now: Instant) -> anyhow::Result<usize> {
        self.write_out(self.scheduler.drain_due(now))
    }

    fn write_out(&self, due: Vec<(String, PendingOp)>) -> anyhow::Result<usize> {
        for (key, op) in &due {
            match op {
                PendingOp::Save(value) => raw_save(&self.db, key, value)?,
                PendingOp::Remove => raw_remove(&self.db, key)?,
            }
        }
        Ok(due.len())
    }

    pub fn pending_count(&self) -> usize {
        self.scheduler.pending_count()
    }

    pub fn in_memory(&self, key: &str) -> Option<Value> {
        self.values.read().unwrap().get(key).cloned()
    }
}

struct Applied {
    value: Value,
}

/// Raw row access, also reachable for namespaces with no registered service
/// (`SettingFactoryMain.setSetting` / `getSetting` fallback path).
fn raw_get(db: &Arc<Mutex<Database>>, key: &str) -> anyhow::Result<Option<Value>> {
    let db = db.lock().unwrap();
    let raw: Option<String> = db
        .connection()
        .query_row("SELECT value FROM \"Settings\" WHERE key = ?1", [key], |r| {
            Ok(cell_to_json_text(r.get::<_, rusqlite::types::Value>(0)?))
        })
        .ok();
    Ok(raw.and_then(|text| serde_json::from_str(&text).ok()))
}

fn raw_save(db: &Arc<Mutex<Database>>, key: &str, value: &Value) -> anyhow::Result<()> {
    let db = db.lock().unwrap();
    db.connection().execute(
        "INSERT OR REPLACE INTO \"Settings\" (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value.to_string()],
    )?;
    Ok(())
}

fn raw_remove(db: &Arc<Mutex<Database>>, key: &str) -> anyhow::Result<()> {
    let db = db.lock().unwrap();
    db.connection()
        .execute("DELETE FROM \"Settings\" WHERE key = ?1", [key])?;
    Ok(())
}

/// The `[lowerBound, upperBound)` range the original builds for prefix scans.
pub fn prefix_bounds(namespace: &str, key_prefix: &str) -> (String, String) {
    let trimmed = key_prefix.trim_end_matches('/');
    let lower = if trimmed.is_empty() {
        format!("{namespace}/")
    } else {
        format!("{namespace}/{trimmed}/")
    };
    // slice(0, -1) + '0'
    let mut chars: Vec<char> = lower.chars().collect();
    chars.pop();
    chars.push('0');
    let upper: String = chars.into_iter().collect();
    (lower, upper)
}

/// `SettingFactoryMain` — namespace registry plus the raw passthrough helpers.
pub struct SettingFactory {
    db: Arc<Mutex<Database>>,
    services: RwLock<HashMap<String, Arc<SettingsService>>>,
}

impl SettingFactory {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self {
            db,
            services: RwLock::new(HashMap::new()),
        }
    }

    /// `register(namespace, schema, obj)`
    pub fn register(
        &self,
        namespace: &str,
        declarations: Vec<SettingDeclaration>,
    ) -> anyhow::Result<Arc<SettingsService>> {
        {
            let services = self.services.read().unwrap();
            if services.contains_key(namespace) {
                anyhow::bail!("namespace {} already created", namespace);
            }
        }

        let service = Arc::new(SettingsService::new(namespace, declarations, self.db.clone())?);
        self.services
            .write()
            .unwrap()
            .insert(namespace.to_string(), service.clone());
        Ok(service)
    }

    pub fn get_service(&self, namespace: &str) -> Option<Arc<SettingsService>> {
        self.services.read().unwrap().get(namespace).cloned()
    }

    /// `setSetting` — registered namespaces go through their service (so
    /// validation, side effects and debouncing apply); others write directly.
    pub async fn set_setting(&self, namespace: &str, key: &str, value: Value) -> anyhow::Result<()> {
        if let Some(service) = self.get_service(namespace) {
            if service.declaration(key).is_some() {
                return service.set(key, value).await;
            }
        }
        raw_save(&self.db, &row_key(namespace, key), &value)
    }

    pub fn get_setting(&self, namespace: &str, key: &str) -> anyhow::Result<Value> {
        if let Some(service) = self.get_service(namespace) {
            if service.declaration(key).is_some() {
                return Ok(service.get(key));
            }
        }
        Ok(raw_get(&self.db, &row_key(namespace, key))?.unwrap_or(Value::Null))
    }

    pub fn get_by_prefix(&self, namespace: &str, key_prefix: &str) -> anyhow::Result<Vec<(String, Value)>> {
        let (lower, upper) = prefix_bounds(namespace, key_prefix);
        let db = self.db.lock().unwrap();
        let mut stmt = db.connection().prepare(
            "SELECT key, value FROM \"Settings\" WHERE key >= ?1 AND key < ?2 ORDER BY key",
        )?;
        let rows = stmt.query_map(rusqlite::params![lower, upper], |r| {
            let key: String = r.get(0)?;
            let text = cell_to_json_text(r.get::<_, rusqlite::types::Value>(1)?);
            Ok((key, serde_json::from_str::<Value>(&text).unwrap_or(Value::Null)))
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// `removeSettingsByPrefix` — returns the affected row count.
    pub fn remove_by_prefix(&self, namespace: &str, key_prefix: &str) -> anyhow::Result<usize> {
        let (lower, upper) = prefix_bounds(namespace, key_prefix);
        let db = self.db.lock().unwrap();
        let affected = db.connection().execute(
            "DELETE FROM \"Settings\" WHERE key >= ?1 AND key < ?2",
            rusqlite::params![lower, upper],
        )?;
        Ok(affected)
    }

    /// `onDispose` — flushes every service before the database is destroyed.
    pub fn flush_all(&self) -> anyhow::Result<usize> {
        self.for_each_service(SettingsService::flush)
    }

    pub fn flush_all_due(&self, now: Instant) -> anyhow::Result<usize> {
        self.for_each_service(|service| service.flush_due(now))
    }

    fn for_each_service(&self, op: impl Fn(&SettingsService) -> anyhow::Result<usize>) -> anyhow::Result<usize> {
        let services: Vec<Arc<SettingsService>> =
            self.services.read().unwrap().values().cloned().collect();
        let mut total = 0;
        for service in services {
            total += op(&service)?;
        }
        Ok(total)
    }

    pub fn db(&self) -> &Arc<Mutex<Database>> {
        &self.db
    }
}

/// Ad-hoc persistence that is not a registered setting, e.g.
/// `league-client-main/lastConnectedClient` and the `config-migrate` markers.
impl SettingFactory {
    pub fn save_raw(&self, namespace: &str, key: &str, value: &Value) -> anyhow::Result<()> {
        raw_save(&self.db, &row_key(namespace, key), value)
    }

    pub fn get_raw(&self, namespace: &str, key: &str) -> anyhow::Result<Option<Value>> {
        raw_get(&self.db, &row_key(namespace, key))
    }

    pub fn remove_raw(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
        raw_remove(&self.db, &row_key(namespace, key))
    }

    /// `hasMigration` / `markMigration` — the marker row stores key === value.
    pub fn has_migration(&self, marker: &str) -> anyhow::Result<bool> {
        Ok(raw_get(&self.db, marker)?.is_some())
    }

    pub fn mark_migration(&self, marker: &str) -> anyhow::Result<()> {
        raw_save(&self.db, marker, &Value::String(marker.to_string()))
    }
}
