use crate::model::{PermissionRequest, QuestionInfo, QuestionRequest};

#[derive(Default)]
pub struct PendingState {
    pub permissions: Vec<PermissionRequest>,
    pub questions: Vec<QuestionRequest>,
    pub responding_request: Option<String>,
    pub question_index: usize,
    pub question_selected: usize,
    pub question_answers: Vec<Vec<String>>,
    question_draft_id: Option<String>,
}

impl PendingState {
    pub fn current_permission(&self, session_id: Option<&str>) -> Option<&PermissionRequest> {
        let session_id = session_id?;
        self.permissions
            .iter()
            .find(|request| request.session_id == session_id)
    }

    pub fn current_question(&self, session_id: Option<&str>) -> Option<&QuestionRequest> {
        let session_id = session_id?;
        self.questions
            .iter()
            .find(|request| request.session_id == session_id)
    }

    pub fn current_question_info(&self, session_id: Option<&str>) -> Option<&QuestionInfo> {
        self.current_question(session_id)
            .and_then(|request| request.questions.get(self.question_index))
    }

    pub fn is_responding(&self, request_id: &str) -> bool {
        self.responding_request.as_deref() == Some(request_id)
    }

    pub fn start_responding(&mut self, request_id: impl Into<String>) {
        self.responding_request = Some(request_id.into());
    }

    pub fn clear_responding(&mut self) {
        self.responding_request = None;
    }

    pub fn set_permissions(&mut self, permissions: Vec<PermissionRequest>) {
        self.permissions = permissions;
        self.responding_request = self.responding_request.take().filter(|request_id| {
            self.permissions
                .iter()
                .any(|request| &request.id == request_id)
        });
    }

    pub fn set_questions(&mut self, questions: Vec<QuestionRequest>, session_id: Option<&str>) {
        self.questions = questions;
        let request_id = session_id
            .and_then(|session_id| {
                self.questions
                    .iter()
                    .find(|request| request.session_id == session_id)
            })
            .or_else(|| self.questions.first())
            .map(|request| request.id.clone());
        if let Some(request_id) = request_id {
            self.prepare_question_draft(&request_id);
        } else {
            self.reset_question_draft();
        }
    }

    pub fn remove_session(&mut self, session_id: &str) {
        self.permissions
            .retain(|request| request.session_id != session_id);
        self.questions
            .retain(|request| request.session_id != session_id);
        if self.responding_request.as_ref().is_some_and(|request_id| {
            !self
                .permissions
                .iter()
                .any(|request| &request.id == request_id)
                && !self
                    .questions
                    .iter()
                    .any(|request| &request.id == request_id)
        }) {
            self.responding_request = None;
        }
    }

    pub fn upsert_permission(&mut self, request: PermissionRequest) {
        if let Some(existing) = self
            .permissions
            .iter_mut()
            .find(|existing| existing.id == request.id)
        {
            *existing = request;
        } else {
            self.permissions.push(request);
        }
    }

    pub fn remove_permission(&mut self, request_id: &str) {
        self.permissions.retain(|request| request.id != request_id);
        if self.responding_request.as_deref() == Some(request_id) {
            self.responding_request = None;
        }
    }

    pub fn upsert_question(&mut self, request: QuestionRequest, session_id: Option<&str>) {
        let request_id = request.id.clone();
        let is_current = session_id == Some(request.session_id.as_str());
        if let Some(existing) = self
            .questions
            .iter_mut()
            .find(|existing| existing.id == request.id)
        {
            *existing = request;
        } else {
            self.questions.push(request);
        }
        if is_current {
            self.prepare_question_draft(&request_id);
        }
    }

    pub fn remove_question(&mut self, request_id: &str) {
        self.questions.retain(|request| request.id != request_id);
        if self.responding_request.as_deref() == Some(request_id) {
            self.responding_request = None;
        }
        if self.question_draft_id.as_deref() == Some(request_id) {
            self.reset_question_draft();
        }
    }

    pub fn prepare_current_question_draft(&mut self, session_id: Option<&str>) {
        let request_id = self
            .current_question(session_id)
            .map(|request| request.id.clone());
        if let Some(request_id) = request_id {
            self.prepare_question_draft(&request_id);
        } else {
            self.reset_question_draft();
        }
    }

    fn prepare_question_draft(&mut self, request_id: &str) {
        if self.question_draft_id.as_deref() != Some(request_id) {
            self.question_draft_id = Some(request_id.to_owned());
            self.question_index = 0;
            self.question_selected = 0;
            let question_count = self
                .questions
                .iter()
                .find(|request| request.id == request_id)
                .map(|request| request.questions.len())
                .unwrap_or_default();
            self.question_answers = vec![Vec::new(); question_count];
        } else if let Some(request) = self
            .questions
            .iter()
            .find(|request| request.id == request_id)
        {
            self.question_index = self
                .question_index
                .min(request.questions.len().saturating_sub(1));
            if let Some(question) = request.questions.get(self.question_index) {
                self.question_selected = self
                    .question_selected
                    .min(question.options.len().saturating_sub(1));
            }
            self.question_answers
                .resize(request.questions.len(), Vec::new());
        }
    }

    fn reset_question_draft(&mut self) {
        self.question_draft_id = None;
        self.question_index = 0;
        self.question_selected = 0;
        self.question_answers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::PendingState;
    use crate::model::{PermissionRequest, QuestionInfo, QuestionOption, QuestionRequest};

    #[test]
    fn current_requests_are_scoped_to_the_active_session() {
        let mut state = PendingState::default();
        state.permissions.push(PermissionRequest {
            id: "permission_1".to_owned(),
            session_id: "session_1".to_owned(),
            ..PermissionRequest::default()
        });
        state.questions.push(QuestionRequest {
            id: "question_1".to_owned(),
            session_id: "session_1".to_owned(),
            ..QuestionRequest::default()
        });

        assert!(state.current_permission(Some("session_1")).is_some());
        assert!(state.current_question(Some("session_1")).is_some());
        assert!(state.current_permission(Some("session_2")).is_none());
        assert!(state.current_question(Some("session_2")).is_none());
    }

    #[test]
    fn question_draft_tracks_and_clears_the_active_request() {
        let mut state = PendingState::default();
        state.set_questions(
            vec![QuestionRequest {
                id: "question_1".to_owned(),
                session_id: "session_1".to_owned(),
                questions: vec![QuestionInfo {
                    options: vec![QuestionOption {
                        label: "yes".to_owned(),
                        ..QuestionOption::default()
                    }],
                    ..QuestionInfo::default()
                }],
                ..QuestionRequest::default()
            }],
            Some("session_1"),
        );

        assert_eq!(state.question_answers, vec![Vec::<String>::new()]);
        state.remove_question("question_1");
        assert!(state.question_answers.is_empty());
        assert!(state.current_question(Some("session_1")).is_none());
    }
}
