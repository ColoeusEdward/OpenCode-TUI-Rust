use crate::scroll::ScrollState;
use crate::transcript::TranscriptStore;
use std::collections::HashSet;
use std::ops::{Deref, DerefMut};

#[derive(Debug, Default)]
pub struct TranscriptState {
    pub store: TranscriptStore,
    pub collapsed_parts: HashSet<String>,
    pub scroll: ScrollState,
}

impl TranscriptState {
    pub fn clear(&mut self) {
        self.store.clear();
        self.collapsed_parts.clear();
        self.scroll.reset();
    }

    pub fn reset_scroll(&mut self) {
        self.scroll.reset();
    }
}

impl Deref for TranscriptState {
    type Target = TranscriptStore;

    fn deref(&self) -> &Self::Target {
        &self.store
    }
}

impl DerefMut for TranscriptState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.store
    }
}

#[cfg(test)]
mod tests {
    use super::TranscriptState;
    use crate::model::{MessageInfo, MessageWithParts};

    #[test]
    fn clearing_transcript_state_resets_data_fold_state_and_scroll() {
        let mut state = TranscriptState::default();
        state.store.replace(vec![MessageWithParts {
            info: MessageInfo {
                id: "message-1".to_owned(),
                ..MessageInfo::default()
            },
            ..MessageWithParts::default()
        }]);
        state.collapsed_parts.insert("part-1".to_owned());
        state.scroll.observe(100, 10);
        state.scroll.scroll_lines(-3);

        state.clear();

        assert!(state.store.is_empty());
        assert!(state.collapsed_parts.is_empty());
        assert_eq!(state.scroll.offset(), 0);
        assert!(state.scroll.is_following());
    }
}
