//! What every loader needs: the shared store, the queues, the network seam and the
//! local saved-player lookup.

use std::sync::Arc;

use riftflow_ipc::RiftFlowIpcMain;
use riftflow_state::StateSync;
use serde_json::Value;

use crate::fetch::Fetcher;
use crate::lru::Lru;
use crate::queue::QueueKeeper;
use crate::store::Store;

/// `saved-player-main`'s one read this pipeline needs. The app implements it over
/// `riftflow_store::players::query_saved_player_with_games`; `None` when there is no
/// database, in which case the saved-info scope loads nothing.
#[async_trait::async_trait]
pub trait SavedPlayer: Send + Sync {
    /// `querySavedPlayerWithGames` — `null` when the player was never met.
    async fn query_saved_player_with_games(&self, query: Value) -> anyhow::Result<Option<Value>>;
}

/// `LRUMap({ maxSize: 256 })`, twice — the game-summary and game-detail caches of
/// `match-history-loader.ts`.
pub struct Caches {
    game_summary: std::sync::Mutex<Lru>,
    game_details: std::sync::Mutex<Lru>,
    summary_flights: std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
}

impl Caches {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            game_summary: std::sync::Mutex::new(Lru::new(256)),
            game_details: std::sync::Mutex::new(Lru::new(256)),
            summary_flights: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    pub fn summary(&self, key: &str) -> Option<Value> {
        self.game_summary.lock().unwrap().get(key)
    }

    pub fn summary_gate(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut flights = self.summary_flights.lock().unwrap();
        flights.retain(|_, value| value.strong_count() > 0);
        if let Some(gate) = flights.get(key).and_then(std::sync::Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        flights.insert(key.to_string(), Arc::downgrade(&gate));
        gate
    }

    pub fn put_summary(&self, key: &str, value: &Value) {
        self.game_summary.lock().unwrap().set(key, value.clone());
    }

    pub fn details(&self, key: &str) -> Option<Value> {
        self.game_details.lock().unwrap().get(key)
    }

    pub fn put_details(&self, key: &str, value: &Value) {
        self.game_details.lock().unwrap().set(key, value.clone());
    }
}

/// `OngoingGameMainContext`, minus the MobX-observable parts.
pub struct Ctx {
    pub store: Arc<Store>,
    pub queues: Arc<QueueKeeper>,
    pub fetcher: Arc<dyn Fetcher>,
    pub saved_player: Option<Arc<dyn SavedPlayer>>,
    pub state_sync: Arc<StateSync>,
    pub ipc: Arc<RiftFlowIpcMain>,
    pub cache: Arc<Caches>,
}

impl Ctx {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Arc<Store>,
        queues: Arc<QueueKeeper>,
        fetcher: Arc<dyn Fetcher>,
        saved_player: Option<Arc<dyn SavedPlayer>>,
        state_sync: Arc<StateSync>,
        ipc: Arc<RiftFlowIpcMain>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            queues,
            fetcher,
            saved_player,
            state_sync,
            ipc,
            cache: Caches::new(),
        })
    }
}

/// `[ongoing-game]` tracing, on the same env-var pattern the shell uses.
pub fn trace(message: &str) {
    if std::env::var("RIFTFLOW_ONGOING_GAME_TRACE").is_ok() {
        eprintln!("[ongoing-game] {message}");
    }
}

/// the `logger.warn` of a loader: rare, worth printing even without tracing on
pub fn warn(context: &str, error: &str) {
    eprintln!("[ongoing-game] {context}: {error}");
}
