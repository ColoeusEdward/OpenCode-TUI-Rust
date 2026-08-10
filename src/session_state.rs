use crate::model::{ModelRef, Session, SessionLocation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Session,
}

pub struct SessionState {
    pub screen: Screen,
    pub sessions: Vec<Session>,
    pub children: Vec<Session>,
    pub show_archived: bool,
    pub selected_session: usize,
    pub current_session: Option<Session>,
    pub opening_session: Option<String>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            screen: Screen::Home,
            sessions: Vec::new(),
            children: Vec::new(),
            show_archived: false,
            selected_session: 0,
            current_session: None,
            opening_session: None,
        }
    }
}

impl SessionState {
    pub fn replace_sessions(&mut self, sessions: Vec<Session>) {
        let previous_id = self
            .current_session
            .as_ref()
            .map(|session| session.id.clone())
            .or_else(|| {
                self.sessions
                    .get(self.selected_session)
                    .map(|session| session.id.clone())
            });
        self.sessions = sessions;
        if let Some(previous_id) = previous_id {
            if self
                .sessions
                .iter()
                .any(|session| session.id == previous_id)
            {
                self.select_session(&previous_id);
            } else if !self.sessions.is_empty() {
                self.selected_session = self.selected_session.min(self.sessions.len() - 1);
            }
        } else if !self.sessions.is_empty() {
            self.selected_session = 0;
        }
    }

    pub fn select_session(&mut self, session_id: &str) {
        if let Some(index) = self
            .sessions
            .iter()
            .position(|session| session.id == session_id)
        {
            self.selected_session = index;
        }
    }

    pub fn upsert_session(&mut self, session: Session) {
        if let Some(existing) = self.sessions.iter_mut().find(|item| item.id == session.id) {
            *existing = session;
        } else {
            self.sessions.push(session);
        }
        self.sessions
            .sort_by_key(|session| std::cmp::Reverse(session.time.updated));
    }

    pub fn is_current_session(&self, session_id: &str) -> bool {
        self.current_session
            .as_ref()
            .is_some_and(|session| session.id == session_id)
    }

    pub fn replace_children(&mut self, session_id: &str, children: Vec<Session>) {
        if self.is_current_session(session_id) {
            self.children = children;
        }
    }

    pub fn session_visible(&self, session: &Session) -> bool {
        self.show_archived == session.time.archived.is_some()
    }

    pub fn update_agent(&mut self, session_id: &str, agent: &str) {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.agent = Some(agent.to_owned());
        }
        if let Some(session) = self
            .current_session
            .as_mut()
            .filter(|session| session.id == session_id)
        {
            session.agent = Some(agent.to_owned());
        }
    }

    pub fn update_model(&mut self, session_id: &str, model: &ModelRef) {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.model = Some(model.clone());
        }
        if let Some(session) = self
            .current_session
            .as_mut()
            .filter(|session| session.id == session_id)
        {
            session.model = Some(model.clone());
        }
    }

    pub fn update_location(
        &mut self,
        session_id: &str,
        directory: &str,
        workspace_id: Option<String>,
        timestamp: i64,
    ) {
        let session_location = SessionLocation {
            directory: Some(directory.to_owned()),
            workspace_id: workspace_id.clone(),
        };
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.directory = Some(directory.to_owned());
            session.workspace_id = workspace_id.clone();
            session.location = Some(session_location.clone());
        }
        if let Some(session) = self
            .current_session
            .as_mut()
            .filter(|session| session.id == session_id)
        {
            session.directory = Some(directory.to_owned());
            session.workspace_id = workspace_id;
            session.location = Some(session_location);
        }
        self.touch(session_id, timestamp);
    }

    pub fn touch(&mut self, session_id: &str, timestamp: i64) {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.time.updated = session.time.updated.max(timestamp);
        }
        if let Some(session) = self
            .current_session
            .as_mut()
            .filter(|session| session.id == session_id)
        {
            session.time.updated = session.time.updated.max(timestamp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Screen, SessionState};
    use crate::model::{Session, SessionTime};

    #[test]
    fn session_state_starts_on_home_without_a_selection() {
        let state = SessionState::default();

        assert_eq!(state.screen, Screen::Home);
        assert!(state.sessions.is_empty());
        assert_eq!(state.selected_session, 0);
        assert!(state.current_session.is_none());
    }

    #[test]
    fn upsert_session_sorts_recent_sessions_and_selects_by_id() {
        let mut state = SessionState::default();
        state.upsert_session(Session {
            id: "older".to_owned(),
            time: SessionTime {
                updated: 1,
                ..SessionTime::default()
            },
            ..Session::default()
        });
        state.upsert_session(Session {
            id: "newer".to_owned(),
            time: SessionTime {
                updated: 2,
                ..SessionTime::default()
            },
            ..Session::default()
        });
        state.select_session("older");

        assert_eq!(state.sessions[0].id, "newer");
        assert_eq!(state.selected_session, 1);
    }
}
