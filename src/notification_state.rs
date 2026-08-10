const MAX_HISTORY: usize = 64;
const NOTICE_TTL_TICKS: u8 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationLevel {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl NotificationLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Success => "OK",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRecord {
    pub sequence: u64,
    pub level: NotificationLevel,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct NotificationState {
    notice: Option<String>,
    notice_level: NotificationLevel,
    history: Vec<NotificationRecord>,
    next_sequence: u64,
    notice_ticks: Option<u8>,
}

impl NotificationState {
    pub fn set(&mut self, message: impl Into<String>) {
        self.set_level(NotificationLevel::Info, message);
    }

    pub fn success(&mut self, message: impl Into<String>) {
        self.set_level(NotificationLevel::Success, message);
    }

    pub fn warning(&mut self, message: impl Into<String>) {
        self.set_level(NotificationLevel::Warning, message);
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.set_level(NotificationLevel::Error, message);
    }

    pub fn set_level(&mut self, level: NotificationLevel, message: impl Into<String>) {
        let message = message.into();
        self.notice = Some(message.clone());
        self.notice_level = level;
        self.notice_ticks = Some(NOTICE_TTL_TICKS);
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.history.push(NotificationRecord {
            sequence: self.next_sequence,
            level,
            message,
        });
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
    }

    pub fn active(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn active_level(&self) -> NotificationLevel {
        self.notice_level
    }

    pub fn history(&self) -> &[NotificationRecord] {
        &self.history
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub fn clear(&mut self) {
        self.notice = None;
        self.notice_level = NotificationLevel::Info;
        self.notice_ticks = None;
    }

    pub fn tick(&mut self) {
        let Some(ticks) = self.notice_ticks.as_mut() else {
            return;
        };
        if *ticks <= 1 {
            self.clear();
        } else {
            *ticks -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_HISTORY, NOTICE_TTL_TICKS, NotificationLevel, NotificationState};

    #[test]
    fn notifications_start_empty() {
        assert!(NotificationState::default().active().is_none());
    }

    #[test]
    fn notifications_can_be_set_and_cleared() {
        let mut state = NotificationState::default();
        state.set("working");
        assert_eq!(state.active(), Some("working"));
        assert_eq!(state.active_level(), NotificationLevel::Info);
        assert_eq!(state.history().len(), 1);

        state.clear();
        assert!(state.active().is_none());
        assert_eq!(state.history().len(), 1);
    }

    #[test]
    fn notifications_keep_bounded_levelled_history() {
        let mut state = NotificationState::default();
        state.success("connected");
        state.warning("retrying");
        state.error("failed");

        assert_eq!(state.history()[0].sequence, 1);
        assert_eq!(state.history()[1].level, NotificationLevel::Warning);
        assert_eq!(state.active_level(), NotificationLevel::Error);

        for index in 0..(MAX_HISTORY + 5) {
            state.set(format!("notice {index}"));
        }
        assert_eq!(state.history().len(), MAX_HISTORY);
        assert_eq!(state.history()[0].sequence, 9);

        state.clear_history();
        assert!(state.history().is_empty());
    }

    #[test]
    fn active_notice_expires_without_removing_history() {
        let mut state = NotificationState::default();
        state.set("temporary");

        for _ in 0..(NOTICE_TTL_TICKS - 1) {
            state.tick();
            assert_eq!(state.active(), Some("temporary"));
        }
        state.tick();

        assert!(state.active().is_none());
        assert_eq!(state.history().len(), 1);
    }

    #[test]
    fn setting_a_notice_refreshes_its_expiry() {
        let mut state = NotificationState::default();
        state.set("first");
        for _ in 0..(NOTICE_TTL_TICKS - 1) {
            state.tick();
        }
        state.set("second");
        state.tick();

        assert_eq!(state.active(), Some("second"));
        assert_eq!(state.history().len(), 2);
    }
}
