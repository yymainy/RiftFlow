//! LCU connection layer, ported from `src/main/shards/league-client/` and
//! `src/main/shards/league-client-ux/`.
//!
//! The process here is deliberately the same as the original: poll the UX
//! process command line for credentials, drive a reconnect loop that only keeps
//! retrying on connection-refused, hold one WebSocket subscription
//! (`OnJsonApiEvent`) and fan its frames out through the radix emitter.

pub mod discovery;
pub mod http;
pub mod shard;
pub mod state;
pub mod ux_command_line;
pub mod ws;

pub use shard::{
    CredentialStore, LeagueClientMain, LeagueClientUxMain, LcuDialer, NoopCredentialStore,
    ProductionDialer,
};

pub use state::{ConnectionState, LeagueClientSettings, LeagueClientState, LeagueClientUxSettings, LeagueClientUxState};
pub use ux_command_line::{
    parse_command_line, UxCommandLine, UxCommandLinePublic, RIOT_CERTIFICATE,
    SUBSCRIBED_LCU_ENDPOINTS,
};


pub const LEAGUE_CLIENT_MAIN_NAMESPACE: &str = "league-client-main";
pub const LEAGUE_CLIENT_UX_MAIN_NAMESPACE: &str = "league-client-ux-main";

/// `LeagueClientMain.CONNECT_TO_LC_RETRY_INTERVAL`
pub const CONNECT_TO_LC_RETRY_INTERVAL_MS: u64 = 2000;
/// `LeagueClientMain.HTTP_PING_URL`
pub const HTTP_PING_URL: &str = "/riotclient/auth-token";
/// `LeagueClientMain.REQUEST_TIMEOUT_MS`
pub const REQUEST_TIMEOUT_MS: u64 = 17_500;
/// `LeagueClientMain.FIXED_ITEM_SET_PREFIX`
pub const FIXED_ITEM_SET_PREFIX: &str = "riftflow1";
/// `LeagueClientMain.PROCESS_NAME`
pub const LEAGUE_CLIENT_PROCESS_NAME: &str = "LeagueClient.exe";
/// `LEAGUE_CLIENT_UX_PROCESS_NAME` on win32
pub const LEAGUE_CLIENT_UX_PROCESS_NAME: &str = "LeagueClientUx.exe";
/// `CLIENT_CMD_DEFAULT_POLL_INTERVAL`
pub const CLIENT_CMD_DEFAULT_POLL_INTERVAL_MS: u64 = 1000;
/// `CLIENT_CMD_LONG_POLL_INTERVAL`
pub const CLIENT_CMD_LONG_POLL_INTERVAL_MS: u64 = 5000;

/// The asset limiter concurrency, `new PQueue({ concurrency: 8 })`.
pub const ASSET_REQUEST_CONCURRENCY: usize = 8;

/// Thrown when an LCU call is made before a connection exists. The IPC envelope
/// carries the same `name` the renderer matches on.
#[derive(Debug)]
pub struct LeagueClientLcuUninitializedError;

impl std::fmt::Display for LeagueClientLcuUninitializedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LeagueClient is not connected")
    }
}
