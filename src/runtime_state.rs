use crate::model::SessionStatus;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting {
        attempt: u32,
        retry_in_secs: u64,
    },
}

impl ConnectionState {
    pub fn label(&self) -> String {
        match self {
            Self::Disconnected => "disconnected".to_owned(),
            Self::Connecting => "connecting".to_owned(),
            Self::Connected => "connected".to_owned(),
            Self::Reconnecting {
                attempt,
                retry_in_secs,
            } => format!("reconnecting (attempt {attempt}, retry in {retry_in_secs}s)"),
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

#[derive(Debug)]
pub struct RuntimeState {
    pub connection: ConnectionState,
    pub working: bool,
    pub server_health: String,
    pub sidebar_visible: bool,
    pub session_statuses: HashMap<String, SessionStatus>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            connection: ConnectionState::default(),
            working: false,
            server_health: String::new(),
            sidebar_visible: true,
            session_statuses: HashMap::new(),
        }
    }
}

impl RuntimeState {
    pub fn mark_connecting(&mut self) {
        self.connection = ConnectionState::Connecting;
    }

    pub fn mark_connected(&mut self) {
        self.connection = ConnectionState::Connected;
    }

    pub fn mark_disconnected(&mut self) {
        self.connection = ConnectionState::Disconnected;
    }

    pub fn mark_reconnecting(&mut self, attempt: u32, retry_in_secs: u64) {
        self.connection = ConnectionState::Reconnecting {
            attempt,
            retry_in_secs,
        };
    }

    pub fn is_connected(&self) -> bool {
        self.connection.is_connected()
    }

    pub fn connection_label(&self) -> String {
        self.connection.label()
    }

    pub fn set_health(&mut self, health: impl Into<String>) {
        self.server_health = health.into();
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
    }

    pub fn replace_session_statuses(&mut self, statuses: HashMap<String, SessionStatus>) {
        self.session_statuses = statuses;
    }

    pub fn set_session_status(&mut self, session_id: impl Into<String>, status: SessionStatus) {
        self.session_statuses.insert(session_id.into(), status);
        self.working = self
            .session_statuses
            .values()
            .any(SessionStatus::is_working);
    }

    pub fn clear_session_status(&mut self, session_id: &str) {
        self.session_statuses.remove(session_id);
        self.working = self
            .session_statuses
            .values()
            .any(SessionStatus::is_working);
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionState, RuntimeState};
    use crate::model::SessionStatus;

    #[test]
    fn runtime_defaults_are_disconnected_and_show_the_sidebar() {
        let state = RuntimeState::default();

        assert_eq!(state.connection, ConnectionState::Disconnected);
        assert!(!state.working);
        assert!(state.server_health.is_empty());
        assert!(state.sidebar_visible);
        assert!(state.session_statuses.is_empty());
    }

    #[test]
    fn sidebar_visibility_and_health_are_owned_by_runtime_state() {
        let mut state = RuntimeState::default();
        state.toggle_sidebar();
        state.set_health("healthy");

        assert!(!state.sidebar_visible);
        assert_eq!(state.server_health, "healthy");
    }

    #[test]
    fn connection_transitions_preserve_retry_context_until_connected() {
        let mut state = RuntimeState::default();

        state.mark_connecting();
        assert_eq!(state.connection, ConnectionState::Connecting);

        state.mark_reconnecting(3, 4);
        assert_eq!(
            state.connection,
            ConnectionState::Reconnecting {
                attempt: 3,
                retry_in_secs: 4,
            }
        );
        assert_eq!(
            state.connection_label(),
            "reconnecting (attempt 3, retry in 4s)"
        );
        assert!(!state.is_connected());

        state.mark_connected();
        assert_eq!(state.connection, ConnectionState::Connected);
        assert!(state.is_connected());
    }

    #[test]
    fn session_statuses_derive_the_runtime_working_flag() {
        let mut state = RuntimeState::default();
        state.set_session_status("ses_1", SessionStatus::Busy);
        assert!(state.working);
        state.set_session_status("ses_1", SessionStatus::Idle);
        assert!(!state.working);
        state.set_session_status("ses_2", SessionStatus::Busy);
        state.clear_session_status("ses_2");
        assert!(!state.working);
    }
}
