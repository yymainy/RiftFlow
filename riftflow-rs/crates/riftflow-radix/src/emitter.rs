//! Port of `RadixEventEmitter` (`src/shared/utils/event-emitter.ts`), the
//! fan-out used for LCU websocket endpoint events.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::matcher::{FindResult, RadixMatcher};

pub type Listener = Arc<dyn Fn(Value, Value) + Send + Sync>;

use serde_json::Value;

#[derive(Default)]
struct RouteCallbacks {
    /// Ordered, like the JS `Set` of listeners.
    entries: Mutex<Vec<(u64, Listener)>>,
}

#[derive(Clone)]
pub struct Subscription {
    uri: String,
    id: u64,
}

pub struct RadixEventEmitter {
    matcher: Mutex<RadixMatcher<Arc<RouteCallbacks>>>,
    next_id: AtomicU64,
}

impl Default for RadixEventEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl RadixEventEmitter {
    pub fn new() -> Self {
        Self {
            matcher: Mutex::new(RadixMatcher::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn emit(&self, uri: &str, data: Value) {
        let matched: Vec<FindResult<Arc<RouteCallbacks>>> =
            self.matcher.lock().unwrap().find_all(uri);

        for route in matched {
            let params = route.params.unwrap_or(Value::Null);
            let listeners: Vec<Listener> = route
                .data
                .entries
                .lock()
                .unwrap()
                .iter()
                .map(|(_, l)| l.clone())
                .collect();

            for listener in listeners {
                listener(data.clone(), params.clone());
            }
        }
    }

    /// Register a listener. Reuses the existing route when one is present,
    /// exactly like `getRouteData(uri)` + `callbacks.add(listener)`.
    /// An invalid route is reported rather than thrown, mirroring the JS throw.
    pub fn on<F>(&self, uri: &str, listener: F) -> anyhow::Result<Subscription>
    where
        F: Fn(Value, Value) + Send + Sync + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let listener: Listener = Arc::new(listener);
        let mut matcher = self.matcher.lock().unwrap();
        let route = uri.to_string();

        if let Some(existing) = matcher.get_route_data(&route) {
            existing.entries.lock().unwrap().push((id, listener));
        } else {
            let callbacks = Arc::new(RouteCallbacks {
                entries: Mutex::new(vec![(id, listener)]),
            });
            matcher.insert(&route, callbacks)?;
        }

        Ok(Subscription { uri: route, id })
    }

    pub fn off(&self, sub: &Subscription) {
        let mut matcher = self.matcher.lock().unwrap();

        if let Some(route) = matcher.get_route_data(&sub.uri) {
            {
                let mut entries = route.entries.lock().unwrap();
                entries.retain(|(id, _)| *id != sub.id);
                if entries.is_empty() {
                    drop(entries);
                    matcher.remove(&sub.uri);
                }
            }
        }
    }

    pub fn listener_count(&self, uri: &str) -> usize {
        let matcher = self.matcher.lock().unwrap();
        let matched = matcher.find_all(uri);
        matched
            .iter()
            .map(|r| r.data.entries.lock().unwrap().len())
            .sum()
    }

    pub fn clear(&self) {
        self.matcher.lock().unwrap().clear();
    }
}
