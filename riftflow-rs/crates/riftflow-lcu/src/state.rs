//! Port of `league-client/state.ts` and `league-client-ux/state.ts`.
//!
//! The originals are MobX classes whose fields are mirrored to the renderer by
//! `propSync`. Rust has no observable fields, so the setters here are the only way
//! to mutate state and every mutation publishes through [`riftflow_state::StateSync`]
//! in the same shape the MobX reaction produced:
//! `update-state-prop/league-client-main:state` with `["auth", value, {action,raw}]`.

use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::ux_command_line::{UxCommandLine, UxCommandLinePublic};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

/// `LeagueClientState`
#[derive(Default)]
pub struct LeagueClientState {
    inner: RwLock<LeagueClientStateInner>,
}

#[derive(Default)]
struct LeagueClientStateInner {
    auth: Option<UxCommandLine>,
    connection_state: ConnectionState,
    connecting_client: Option<UxCommandLine>,
}

impl LeagueClientState {
    pub fn connection_state(&self) -> ConnectionState {
        self.inner.read().unwrap().connection_state
    }

    pub fn auth_public(&self) -> Option<UxCommandLinePublic> {
        self.inner
            .read()
            .unwrap()
            .auth
            .as_ref()
            .map(|auth| auth.without_certificate())
    }

    pub fn region(&self) -> String {
        self.inner
            .read()
            .unwrap()
            .auth
            .as_ref()
            .map(|auth| auth.region.clone())
            .unwrap_or_default()
    }

    pub fn connecting_client(&self) -> Option<UxCommandLine> {
        self.inner.read().unwrap().connecting_client.clone()
    }

    pub fn set_connecting(&self) -> (ConnectionState, Option<UxCommandLinePublic>) {
        let mut inner = self.inner.write().unwrap();
        inner.connection_state = ConnectionState::Connecting;
        (inner.connection_state, None)
    }

    pub fn set_connected(&self, auth: &UxCommandLine) -> (ConnectionState, Option<UxCommandLinePublic>) {
        let mut inner = self.inner.write().unwrap();
        inner.connection_state = ConnectionState::Connected;
        inner.auth = Some(auth.clone());
        inner.connecting_client = None;
        (
            inner.connection_state,
            Some(auth.without_certificate()),
        )
    }

    /// `setDisconnected` also clears the credential bag, as the original does.
    pub fn set_disconnected(&self) -> (ConnectionState, Option<UxCommandLinePublic>) {
        let mut inner = self.inner.write().unwrap();
        inner.connection_state = ConnectionState::Disconnected;
        inner.auth = None;
        (inner.connection_state, None)
    }

    /// `setConnectingClient` — `None` cancels an in-flight attempt.
    pub fn set_connecting_client(&self, client: Option<UxCommandLine>) -> Option<UxCommandLinePublic> {
        self.inner.write().unwrap().connecting_client = client.clone();
        client.map(|client| client.without_certificate())
    }
}

/// `LeagueClientSettings`
pub struct LeagueClientSettings {
    pub auto_connect: RwLock<bool>,
}

impl Default for LeagueClientSettings {
    fn default() -> Self {
        // `LeagueClientSettings.autoConnect` defaults to true
        Self {
            auto_connect: RwLock::new(true),
        }
    }
}

impl LeagueClientSettings {
    pub fn auto_connect(&self) -> bool {
        *self.auto_connect.read().unwrap()
    }

    pub fn set_auto_connect(&self, value: bool) {
        *self.auto_connect.write().unwrap() = value;
    }
}

/// `LeagueClientUxState`
#[derive(Default)]
pub struct LeagueClientUxState {
    inner: RwLock<LeagueClientUxStateInner>,
}

#[derive(Default)]
struct LeagueClientUxStateInner {
    launched_clients: Vec<UxCommandLinePublic>,
    has_client_but_no_command_line: bool,
}

impl LeagueClientUxState {
    pub fn launched_clients(&self) -> Vec<UxCommandLinePublic> {
        self.inner.read().unwrap().launched_clients.clone()
    }

    pub fn has_client_but_no_command_line(&self) -> bool {
        self.inner.read().unwrap().has_client_but_no_command_line
    }

    pub fn set_launched_clients(&self, clients: Vec<UxCommandLinePublic>) -> Vec<UxCommandLinePublic> {
        let mut inner = self.inner.write().unwrap();
        inner.launched_clients = clients.clone();
        clients
    }

    pub fn set_has_client_but_no_command_line(&self, value: bool) -> bool {
        self.inner.write().unwrap().has_client_but_no_command_line = value;
        value
    }
}

/// `LeagueClientUxSettings`
pub struct LeagueClientUxSettings {
    pub use_wmi: RwLock<bool>,
}

impl Default for LeagueClientUxSettings {
    fn default() -> Self {
        Self {
            use_wmi: RwLock::new(false),
        }
    }
}

impl LeagueClientUxSettings {
    pub fn use_wmi(&self) -> bool {
        *self.use_wmi.read().unwrap()
    }
}
