//! `ongoing-game-main` — 进行中游戏 window data pipeline.
//!
//! The window's data path, out of `RiftFlow/src/main/shards/ongoing-game`: which
//! participants to load (`computed-state.ts`), the queue they are loaded through
//! (`queue-keeper.ts`), the four reads per participant (`player-data-loader.ts`), the
//! summaries and timelines (`match-history-loader.ts`), the hidden-participant backfill
//! (`additional-info-controller.ts`), the draft handoff (`champ-select-handoff.ts`) and
//! the premade-team inference (`analysis-controller.ts`).
//!
//! State lives in two places and the split is deliberate:
//! * the seven big `Record` maps (`matchHistory`, `summoner`, `rankedStats`,
//!   `savedInfo`, `championMastery`, `gameDetails`, `additionalGame`) were **not**
//!   propSync'd in the original either — they are read once with `getAll` and pushed
//!   item-by-item as `<namespace>/<event>`. They live in [`store::Store`].
//! * the derived props (`queryStage`, `teams`, `championSelections`, …) and the five
//!   loading-state records go through [`riftflow_state::StateSync`] under
//!   `ongoing-game-main`, exactly as `propSync` did.
//!
//! Reads of `leagueClient.data.*` are taken as an [`snapshot::LcSnapshot`] of the
//! `league-client-main` props (hydrated by `riftflow-app/src/lc_state.rs`), so every
//! computed in this crate stays a pure function over JSON: `LcSnapshot::from_state`
//! in the pipeline, `LcSnapshot::from_json` in fixtures. Network reads go through the
//! [`fetch::Fetcher`] seam for the same reason.

pub mod additional_info;
pub mod analysis;
pub mod champ_select;
pub mod computed;
pub mod ctx;
pub mod feature_gates;
pub mod fetch;
pub mod lru;
pub mod match_history;
pub mod member_merge;
pub mod pipeline;
pub mod player_data;
pub mod queue;
pub mod record;
pub mod reload;
pub mod settings;
pub mod snapshot;
pub mod store;
pub mod summary;
pub mod team_up;
pub mod transport;

pub const ONGOING_GAME_MAIN_NAMESPACE: &str = "ongoing-game-main";

/// `ONGOING_GAME_LOADING_PRIORITY`
pub mod priority {
    pub const ADDITIONAL_INFO: i64 = 50;
    pub const ADDITIONAL_SUMMONER: i64 = -1;
    pub const SUMMONER: i64 = 30;
    pub const MATCH_HISTORY: i64 = 25;
    pub const SAVED_INFO: i64 = 20;
    pub const RANKED_STATS: i64 = 15;
    pub const CHAMPION_MASTERY: i64 = 10;
    pub const GAME_DETAILS_JUNGLE: i64 = 10;
    pub const GAME_DETAILS: i64 = 5;
}

pub use analysis::Analyzer;
pub use champ_select::{Deobfuscate, HandoffSnapshot, NoDeobfuscation};
pub use computed::{Additional, QueryStage};
pub use ctx::{Ctx, SavedPlayer};
pub use feature_gates::FeatureGates;
pub use fetch::{ApiSource, FetchError, Fetcher, FetchRequest, MatchHistoryQueryParams};
pub use pipeline::{HandoffStorage, OngoingGame, OngoingGameDeps};
pub use queue::{QueueKeeper, TagMode, TaskFailure, TaskOptions, PRIORITY_NOW};
pub use settings::OngoingGameSettings;
pub use snapshot::LcSnapshot;
pub use store::{Store, tag_params_from_value};
pub use transport::LcuSgpFetcher;
