use crate::composer::Composer;
use crate::model::{PromptOptions, PromptPart, PromptRequest};
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptPanelItem {
    Draft,
    Model,
    Agent,
    Variant,
    Format,
    NoReply,
    System,
    Tools,
    AddAttachment,
    Attachment(usize),
    AddSubtask,
    Subtask(usize),
}

#[allow(dead_code)]
impl PromptPanelItem {
    pub fn is_attachment(&self) -> bool {
        matches!(self, Self::Attachment(_))
    }

    pub fn is_subtask(&self) -> bool {
        matches!(self, Self::Subtask(_))
    }
}

#[derive(Default)]
pub struct PromptState {
    pub composer: Composer,
    pub attachments: Vec<PromptPart>,
    pub subtasks: Vec<PromptPart>,
    pub options: PromptOptions,
    pending: Option<PendingPrompt>,
    queued: VecDeque<PromptSubmission>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PromptSubmission {
    pub(crate) session_id: Option<String>,
    pub(crate) request: PromptRequest,
    pub(crate) prompt: String,
    pub(crate) attachments: Vec<PromptPart>,
    pub(crate) subtasks: Vec<PromptPart>,
}

struct PendingPrompt {
    prompt: String,
    attachments: Vec<PromptPart>,
    subtasks: Vec<PromptPart>,
}

impl PromptState {
    pub fn panel_items(&self) -> Vec<PromptPanelItem> {
        let mut items = vec![
            PromptPanelItem::Draft,
            PromptPanelItem::Model,
            PromptPanelItem::Agent,
            PromptPanelItem::Variant,
            PromptPanelItem::Format,
            PromptPanelItem::NoReply,
            PromptPanelItem::System,
            PromptPanelItem::Tools,
            PromptPanelItem::AddAttachment,
        ];
        items.extend((0..self.attachments.len()).map(PromptPanelItem::Attachment));
        items.push(PromptPanelItem::AddSubtask);
        items.extend((0..self.subtasks.len()).map(PromptPanelItem::Subtask));
        items
    }

    pub fn remove_attachment(&mut self, index: usize) -> Option<PromptPart> {
        (index < self.attachments.len()).then(|| self.attachments.remove(index))
    }

    pub fn remove_subtask(&mut self, index: usize) -> Option<PromptPart> {
        (index < self.subtasks.len()).then(|| self.subtasks.remove(index))
    }

    pub fn take_attachments(&mut self) -> Vec<PromptPart> {
        std::mem::take(&mut self.attachments)
    }

    pub fn take_subtasks(&mut self) -> Vec<PromptPart> {
        std::mem::take(&mut self.subtasks)
    }

    pub fn stage_submission(
        &mut self,
        prompt: String,
        attachments: Vec<PromptPart>,
        subtasks: Vec<PromptPart>,
    ) {
        self.pending = Some(PendingPrompt {
            prompt,
            attachments,
            subtasks,
        });
    }

    pub fn clear_pending(&mut self) {
        self.pending = None;
    }

    pub(crate) fn enqueue(&mut self, submission: PromptSubmission) {
        self.queued.push_back(submission);
    }

    pub(crate) fn dequeue_for_session(&mut self, session_id: &str) -> Option<PromptSubmission> {
        self.queued
            .front()
            .is_some_and(|submission| submission.session_id.as_deref() == Some(session_id))
            .then(|| self.queued.pop_front())
            .flatten()
    }

    pub fn queued_len(&self) -> usize {
        self.queued.len()
    }

    pub fn restore_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.composer.set_text(&pending.prompt);

        let mut attachments = pending.attachments;
        attachments.append(&mut self.attachments);
        self.attachments = attachments;

        let mut subtasks = pending.subtasks;
        subtasks.append(&mut self.subtasks);
        self.subtasks = subtasks;
    }

    pub fn clear_queued_parts(&mut self) {
        self.attachments.clear();
        self.subtasks.clear();
        self.clear_pending();
        self.queued.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{PromptPanelItem, PromptState};
    use crate::model::PromptPart;

    #[test]
    fn panel_items_keep_actions_and_queued_parts_in_stable_order() {
        let mut state = PromptState::default();
        state.attachments.push(PromptPart::file(
            "text/plain",
            "data:text/plain;base64,aGVsbG8=",
            Some("notes.txt".to_owned()),
        ));
        state
            .subtasks
            .push(PromptPart::subtask("Inspect", "Inspect", "explore"));

        assert_eq!(
            state.panel_items(),
            vec![
                PromptPanelItem::Draft,
                PromptPanelItem::Model,
                PromptPanelItem::Agent,
                PromptPanelItem::Variant,
                PromptPanelItem::Format,
                PromptPanelItem::NoReply,
                PromptPanelItem::System,
                PromptPanelItem::Tools,
                PromptPanelItem::AddAttachment,
                PromptPanelItem::Attachment(0),
                PromptPanelItem::AddSubtask,
                PromptPanelItem::Subtask(0),
            ]
        );
    }

    #[test]
    fn removing_a_queued_part_uses_its_panel_index() {
        let mut state = PromptState::default();
        state.attachments.extend([
            PromptPart::file(
                "text/plain",
                "data:text/plain;base64,YQ==",
                Some("a".to_owned()),
            ),
            PromptPart::file(
                "text/plain",
                "data:text/plain;base64,Yg==",
                Some("b".to_owned()),
            ),
        ]);
        state.subtasks.extend([
            PromptPart::subtask("one", "one", "explore"),
            PromptPart::subtask("two", "two", "build"),
        ]);

        let removed_attachment = state.remove_attachment(1).expect("attachment exists");
        let removed_subtask = state.remove_subtask(0).expect("subtask exists");
        assert!(
            matches!(removed_attachment, PromptPart::File { filename: Some(name), .. } if name == "b")
        );
        assert!(matches!(removed_subtask, PromptPart::Subtask { prompt, .. } if prompt == "one"));
        assert_eq!(state.attachments.len(), 1);
        assert_eq!(state.subtasks.len(), 1);
        assert!(state.remove_attachment(5).is_none());
        assert!(state.remove_subtask(5).is_none());
    }

    #[test]
    fn failed_submission_restores_prompt_parts_in_original_order() {
        let mut state = PromptState::default();
        state.composer.set_text("Review the change");
        state.attachments.push(PromptPart::file(
            "text/plain",
            "data:text/plain;base64,aGVsbG8=",
            Some("notes.txt".to_owned()),
        ));
        state
            .subtasks
            .push(PromptPart::subtask("Inspect", "Inspect", "explore"));

        let attachments = state.take_attachments();
        let subtasks = state.take_subtasks();
        state.stage_submission("Review the change".to_owned(), attachments, subtasks);
        state.restore_pending();

        assert_eq!(state.composer.text(), "Review the change");
        assert!(matches!(
            state.attachments.as_slice(),
            [PromptPart::File {
                filename: Some(filename),
                ..
            }] if filename == "notes.txt"
        ));
        assert!(matches!(
            state.subtasks.as_slice(),
            [PromptPart::Subtask { agent, .. }] if agent == "explore"
        ));
    }
}
