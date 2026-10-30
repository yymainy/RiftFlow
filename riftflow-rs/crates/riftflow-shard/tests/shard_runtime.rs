//! Behavioral spec for the ported shard runtime, matching
//! `src/shared/riftflow-shard/manager.ts` semantics and error strings.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use riftflow_shard::{
    AppControl, AppLogger, Instances, Platform, SharedGlobal, RiftFlowManager, RiftFlowShard,
    INTERNAL_RUNNER_ID, SHARED_GLOBAL_ID,
};
use async_trait::async_trait;
use serde_json::{json, Value};

#[derive(Clone)]
struct Trace(Arc<Mutex<Vec<String>>>);

impl Trace {
    fn record(&self, entry: &str) {
        self.0.lock().unwrap().push(entry.to_string());
    }
    fn events(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct TestLogger;

impl AppLogger for TestLogger {
    fn log(&self, _level: &str, _message: &str, _namespace: Option<&str>, _extra: Option<Value>) {}
}

struct TestControl;

impl AppControl for TestControl {
    fn quit(&self) {}
    fn restart(&self) {}
    fn log_level(&self) -> String {
        "info".to_string()
    }
    fn set_log_level(&self, _level: &str) {}
    fn write_base_config(&self, _patch: Value) {}
}

fn global() -> Arc<SharedGlobal> {
    Arc::new(SharedGlobal {
        logger: Arc::new(TestLogger),
        log_filename: String::new().into(),
        events: Default::default(),
        is_elevated: false,
        platform: Platform::current(),
        version: "1.5.1".to_string(),
        is_windows11_22h2_or_higher: false,
        is_ready_to_quit: false.into(),
        startup_deep_link: None.into(),
        base_config: json!({}).into(),
        control: Arc::new(TestControl),
    })
}

/// A shard that logs its construction and each lifecycle hook.
struct Traced {
    name: &'static str,
    trace: Trace,
    fail_init: bool,
}

#[async_trait]
impl RiftFlowShard for Traced {
    async fn on_init(&self) -> anyhow::Result<()> {
        if self.fail_init {
            anyhow::bail!("boom-{}", self.name);
        }
        self.trace.record(&format!("init:{}", self.name));
        Ok(())
    }

    async fn on_finish(&self) -> anyhow::Result<()> {
        self.trace.record(&format!("finish:{}", self.name));
        Ok(())
    }

    async fn on_dispose(&self) -> anyhow::Result<()> {
        self.trace.record(&format!("dispose:{}", self.name));
        Ok(())
    }
}

fn traced(name: &'static str, trace: Trace) -> Arc<Traced> {
    Arc::new(Traced { name, trace, fail_init: false })
}

#[tokio::test]
async fn dependencies_initialize_before_dependents_and_runner_last() {
    let trace = Trace(Arc::new(Mutex::new(Vec::new())));
    let trace_a = trace.clone();
    let trace_b = trace.clone();

    let mut manager = RiftFlowManager::new(global());
    manager
        .use_shard("b", move |_instances: &Instances, _c: &Value| {
            Ok(traced("b", trace_b.clone()))
        })
        .unwrap();
    manager
        .use_shard_with("a", 0, &["b"], {
            let trace_a = trace_a.clone();
            move |instances: &Instances, _c: &Value| {
                // `b` must already exist for `a` to be constructed
                assert!(instances.contains("b"));
                Ok(traced("a", trace_a.clone()))
            }
        }, Value::Null)
        .unwrap();

    let report = manager.setup().await.unwrap();

    assert_eq!(report.order.last().map(String::as_str), Some(INTERNAL_RUNNER_ID));
    let index_b = report.order.iter().position(|id| id == "b").unwrap();
    let index_a = report.order.iter().position(|id| id == "a").unwrap();
    assert!(index_b < index_a, "deps first: {:?}", report.order);

    assert_eq!(
        trace.events(),
        vec!["init:b", "init:a", "finish:b", "finish:a"]
    );

    // reverse order disposal
    manager.dispose().await.unwrap();
    assert_eq!(trace.events()[4..], ["dispose:a", "dispose:b"]);
}

#[tokio::test]
async fn higher_priority_dependencies_are_constructed_first() {
    let order = Arc::new(Mutex::new(Vec::<String>::new()));

    let mut manager = RiftFlowManager::new(global());
    for (name, priority) in [("slow", 1), ("fast", 10)] {
        let order = order.clone();
        manager
            .use_shard_with(
                name,
                priority,
                &[],
                move |_instances: &Instances, _c: &Value| {
                    order.lock().unwrap().push(name.to_string());
                    Ok(traced(name, Trace(Arc::new(Mutex::new(Vec::new())))))
                },
                Value::Null,
            )
            .unwrap();
    }

    // dependent registered last so both deps resolve from it
    let order_clone = order.clone();
    manager
        .use_shard_with("top", 0, &["slow", "fast"], move |_instances, _c| {
            Ok(traced("top", Trace(order_clone.clone())))
        }, Value::Null)
        .unwrap();

    let report = manager.setup().await.unwrap();
    let index_fast = report.order.iter().position(|id| id == "fast").unwrap();
    let index_slow = report.order.iter().position(|id| id == "slow").unwrap();
    assert!(index_fast < index_slow, "{:?}", report.order);
}

#[tokio::test]
async fn duplicate_shard_id_is_rejected() {
    fn build(_i: &Instances, _c: &Value) -> anyhow::Result<Arc<Traced>> {
        Ok(traced("x", Trace(Arc::new(Mutex::new(Vec::new())))))
    }

    let mut manager = RiftFlowManager::new(global());
    manager.use_shard("x", build).unwrap();

    let err = manager.use_shard("x", build).unwrap_err().to_string();
    assert_eq!(err, "Shard with id \"x\" already exists");
}

#[tokio::test]
async fn unregistered_dependency_is_reported_at_setup() {
    let mut manager = RiftFlowManager::new(global());
    manager
        .use_shard_with("a", 0, &["ghost"], |_i, _c| {
            Ok(traced("a", Trace(Arc::new(Mutex::new(Vec::new())))))
        }, Value::Null)
        .unwrap();

    let err = manager.setup().await.unwrap_err().to_string();
    assert_eq!(err, "Shard not registered: \"ghost\"");
}

#[tokio::test]
async fn circular_dependency_message_matches_js() {
    let mut manager = RiftFlowManager::new(global());
    manager
        .use_shard_with("a", 0, &["b"], |_i, _c| {
            Ok(traced("a", Trace(Arc::new(Mutex::new(Vec::new())))))
        }, Value::Null)
        .unwrap();
    manager
        .use_shard_with("b", 0, &["a"], |_i, _c| {
            Ok(traced("b", Trace(Arc::new(Mutex::new(Vec::new())))))
        }, Value::Null)
        .unwrap();

    let err = manager.setup().await.unwrap_err().to_string();
    assert!(
        err.starts_with("Circular dependency detected: "),
        "{err}"
    );
    assert!(err.contains("a -> b -> a"), "{err}");
}

#[tokio::test]
async fn failing_on_init_stops_setup_and_is_recorded() {
    let mut manager = RiftFlowManager::new(global());
    manager
        .use_shard_with("a", 0, &["b"], |_i, _c| {
            Ok(traced("a", Trace(Arc::new(Mutex::new(Vec::new())))))
        }, Value::Null)
        .unwrap();
    manager
        .use_shard("b", |_i: &Instances, _c: &Value| {
            Ok(Arc::new(Traced {
                name: "b",
                trace: Trace(Arc::new(Mutex::new(Vec::new()))),
                fail_init: true,
            }))
        })
        .unwrap();

    let err = manager.setup().await.unwrap_err().to_string();
    assert_eq!(err, "Failed to initialize shard b: boom-b");
    assert!(!manager.is_setup());
}

#[tokio::test]
async fn second_setup_is_rejected() {
    let mut manager = RiftFlowManager::new(global());
    manager
        .use_shard("a", |_i, _c| Ok(traced("a", Trace(Arc::new(Mutex::new(Vec::new()))))))
        .unwrap();
    manager.setup().await.unwrap();

    assert_eq!(manager.setup().await.unwrap_err().to_string(), "Already setup");
    manager.dispose().await.unwrap();
    assert_eq!(
        manager.dispose().await.unwrap_err().to_string(),
        "Not setup yet"
    );
}

#[tokio::test]
async fn typed_lookup_and_shared_global() {
    struct Counter {
        hits: AtomicU64,
    }

    #[async_trait]
    impl RiftFlowShard for Counter {}

    let mut manager = RiftFlowManager::new(global());
    manager
        .use_shard("counter", |_i: &Instances, _c: &Value| {
            Ok(Arc::new(Counter { hits: AtomicU64::new(0) }))
        })
        .unwrap();
    manager.setup().await.unwrap();

    let counter = manager.get::<Counter>("counter").expect("typed lookup");
    counter.hits.fetch_add(1, Ordering::SeqCst);
    assert_eq!(counter.hits.load(Ordering::SeqCst), 1);

    assert!(manager.get::<SharedGlobal>(SHARED_GLOBAL_ID).is_some());
    assert!(manager.get_shard(SHARED_GLOBAL_ID).is_some());
}

#[tokio::test]
async fn report_has_timings_for_every_initialized_shard_and_a_summary() {
    let mut manager = RiftFlowManager::new(global());
    for name in ["a", "b"] {
        manager
            .use_shard(name, move |_i: &Instances, _c: &Value| {
                Ok(traced(name, Trace(Arc::new(Mutex::new(Vec::new())))))
            })
            .unwrap();
    }

    let report = manager.setup().await.unwrap();
    let summary = report.summary.clone().unwrap();

    assert_eq!(summary.shard_count, report.timings.len());
    assert!(report.timings.iter().all(|t| t.ok));
    assert!(summary.slowest_shard.is_some());
    let formatted = report.format();
    assert!(formatted.starts_with("Shard initialization report: 2 initialized shards"), "{formatted}");
    assert!(formatted.contains("Slowest shard initialization:"));
}

#[tokio::test]
async fn config_is_passed_to_the_builder() {
    let mut manager = RiftFlowManager::new(global());
    manager
        .use_shard_with(
            "configured",
            0,
            &[],
            |_i: &Instances, config: &Value| {
                assert_eq!(config.get("windowType").and_then(Value::as_str), Some("main"));
                Ok(traced("configured", Trace(Arc::new(Mutex::new(Vec::new())))))
            },
            json!({ "windowType": "main" }),
        )
        .unwrap();

    manager.setup().await.unwrap();
}
