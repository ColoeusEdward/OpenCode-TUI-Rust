use crate::model::{SessionStatus, TokenUsage};
use std::collections::HashMap;
use std::time::{Duration, Instant};

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

#[derive(Debug, Clone, Default)]
pub struct ResponseState {
    started_at: Option<Instant>,
    elapsed: Duration,
    tokens: TokenUsage,
    input_characters: usize,
    output_characters: usize,
    input_exact: bool,
    output_exact: bool,
    message_id: Option<String>,
}

impl ResponseState {
    pub fn elapsed(&self) -> Duration {
        self.elapsed_at(Instant::now())
    }

    #[allow(dead_code)]
    pub fn input_tokens(&self) -> u64 {
        if self.input_exact {
            self.tokens.input
        } else {
            estimate_tokens(self.input_characters)
        }
    }

    #[allow(dead_code)]
    pub fn output_tokens(&self) -> u64 {
        if self.output_exact {
            self.tokens.output
        } else {
            estimate_tokens(self.output_characters)
        }
    }

    #[allow(dead_code)]
    pub fn input_is_exact(&self) -> bool {
        self.input_exact
    }

    #[allow(dead_code)]
    pub fn output_is_exact(&self) -> bool {
        self.output_exact
    }

    pub fn has_data(&self) -> bool {
        self.started_at.is_some()
            || !self.elapsed.is_zero()
            || self.input_characters > 0
            || self.output_characters > 0
            || self.input_exact
            || self.output_exact
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn start_at(&mut self, now: Instant) {
        self.started_at = Some(now);
    }

    fn stop_at(&mut self, now: Instant) {
        if let Some(started_at) = self.started_at.take() {
            self.elapsed = self
                .elapsed
                .saturating_add(now.saturating_duration_since(started_at));
        }
    }

    fn elapsed_at(&self, now: Instant) -> Duration {
        self.started_at
            .map(|started_at| {
                self.elapsed
                    .saturating_add(now.saturating_duration_since(started_at))
            })
            .unwrap_or(self.elapsed)
    }

    fn set_input_text(&mut self, text: &str) {
        if self.input_characters == 0 {
            self.input_characters = text.chars().count();
        }
    }

    fn add_output_text(&mut self, text: &str) {
        self.output_characters = self.output_characters.saturating_add(text.chars().count());
    }

    fn set_message_id(&mut self, message_id: &str) {
        self.message_id = Some(message_id.to_owned());
    }

    fn set_tokens(&mut self, message_id: &str, tokens: TokenUsage, final_usage: bool) {
        if self.message_id.as_deref() != Some(message_id) {
            return;
        }
        self.input_exact |= final_usage || tokens.input > 0 || tokens.cache.read > 0;
        self.output_exact |= final_usage || tokens.output > 0 || tokens.reasoning > 0;
        self.tokens = tokens;
    }
}

#[allow(dead_code)]
fn estimate_tokens(characters: usize) -> u64 {
    characters.div_ceil(4) as u64
}

#[derive(Debug)]
pub struct RuntimeState {
    pub connection: ConnectionState,
    pub working: bool,
    pub response: ResponseState,
    pub server_health: String,
    pub sidebar_visible: bool,
    pub session_statuses: HashMap<String, SessionStatus>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            connection: ConnectionState::default(),
            working: false,
            response: ResponseState::default(),
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

    pub fn set_working(&mut self, working: bool) {
        self.set_working_at(working, Instant::now());
    }

    pub fn begin_response(&mut self, input: &str) {
        let now = Instant::now();
        self.response.reset();
        self.response.set_input_text(input);
        self.response.start_at(now);
        self.working = true;
    }

    pub fn reset_response(&mut self) {
        self.response.reset();
    }

    pub fn set_response_input(&mut self, input: &str) {
        self.response.set_input_text(input);
    }

    pub fn set_response_message(&mut self, message_id: &str) {
        self.response.set_message_id(message_id);
    }

    pub fn add_response_output(&mut self, text: &str) {
        self.response.add_output_text(text);
    }

    pub fn update_response_tokens(&mut self, message_id: &str, tokens: TokenUsage) {
        self.response.set_tokens(message_id, tokens, false);
    }

    pub fn finish_response_tokens(&mut self, message_id: &str, tokens: TokenUsage) {
        self.response.set_tokens(message_id, tokens, true);
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
        let working = self
            .session_statuses
            .values()
            .any(SessionStatus::is_working);
        self.set_working(working);
    }

    pub fn clear_session_status(&mut self, session_id: &str) {
        self.session_statuses.remove(session_id);
        let working = self
            .session_statuses
            .values()
            .any(SessionStatus::is_working);
        self.set_working(working);
    }

    fn set_working_at(&mut self, working: bool, now: Instant) {
        if working {
            if !self.working {
                self.response.reset();
                self.response.start_at(now);
            }
        } else {
            self.response.stop_at(now);
        }
        self.working = working;
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionState, RuntimeState};
    use crate::model::{SessionStatus, TokenUsage};
    use std::time::{Duration, Instant};

    #[test]
    fn runtime_defaults_are_disconnected_and_show_the_sidebar() {
        let state = RuntimeState::default();

        assert_eq!(state.connection, ConnectionState::Disconnected);
        assert!(!state.working);
        assert!(!state.response.has_data());
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

    #[test]
    fn response_state_records_elapsed_time_and_streaming_estimates() {
        let mut state = RuntimeState::default();
        let started_at = Instant::now();
        state.set_working_at(true, started_at);
        state.set_response_input("hello world");
        state.set_response_message("msg_1");
        state.add_response_output("hello");

        assert_eq!(state.response.input_tokens(), 3);
        assert_eq!(state.response.output_tokens(), 2);

        state.set_working_at(false, started_at + Duration::from_secs(2));
        assert_eq!(state.response.elapsed(), Duration::from_secs(2));

        state.finish_response_tokens(
            "msg_1",
            TokenUsage {
                input: 11,
                output: 7,
                ..TokenUsage::default()
            },
        );
        assert_eq!(state.response.input_tokens(), 11);
        assert_eq!(state.response.output_tokens(), 7);
    }
}
