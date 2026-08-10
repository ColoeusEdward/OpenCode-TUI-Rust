#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollAnchor {
    pub message_id: String,
    pub line_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollState {
    offset: u16,
    max_offset: u16,
    viewport_height: u16,
    follow_tail: bool,
    /// Whether growing content may pull the viewport back to the last line.
    /// Streaming panes want this; panes whose newest content is at the top, such
    /// as the runtime sidebar, would otherwise be pinned to their bottom edge on
    /// every render and could never be scrolled down.
    allow_follow: bool,
    anchor: Option<ScrollAnchor>,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            offset: 0,
            max_offset: 0,
            viewport_height: 1,
            follow_tail: true,
            allow_follow: true,
            anchor: None,
        }
    }
}

impl ScrollState {
    /// A scroll state that starts at the top and stays where the user left it as
    /// content grows.
    pub fn top_anchored() -> Self {
        Self {
            follow_tail: false,
            allow_follow: false,
            ..Self::default()
        }
    }

    pub fn offset(&self) -> u16 {
        self.offset
    }

    /// The largest valid offset for the measured content, i.e. how far this pane
    /// can scroll. Zero means the content fits its viewport.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn max_offset(&self) -> u16 {
        self.max_offset
    }

    pub fn is_following(&self) -> bool {
        self.follow_tail
    }

    pub fn anchor(&self) -> Option<ScrollAnchor> {
        self.anchor.clone()
    }

    pub fn set_anchor(&mut self, anchor: Option<ScrollAnchor>) {
        self.anchor = anchor;
    }

    pub fn clear_anchor(&mut self) {
        self.anchor = None;
    }

    pub fn jump_to_anchor(&mut self, anchor: ScrollAnchor) {
        self.follow_tail = false;
        self.anchor = Some(anchor);
    }

    pub fn restore_offset(&mut self, offset: usize) {
        self.offset = offset.min(self.max_offset as usize) as u16;
    }

    pub fn observe(&mut self, content_height: usize, viewport_height: u16) {
        self.viewport_height = viewport_height.max(1);
        self.max_offset = content_height
            .saturating_sub(self.viewport_height as usize)
            .min(u16::MAX as usize) as u16;
        if self.follow_tail {
            self.offset = self.max_offset;
        } else {
            self.offset = self.offset.min(self.max_offset);
        }
    }

    pub fn scroll_lines(&mut self, lines: i32) {
        if lines == 0 {
            return;
        }

        self.follow_tail = false;
        self.clear_anchor();
        let magnitude = lines.unsigned_abs().min(u16::MAX as u32) as u16;
        self.offset = if lines.is_negative() {
            self.offset.saturating_sub(magnitude)
        } else {
            self.offset.saturating_add(magnitude).min(self.max_offset)
        };
        if self.offset == self.max_offset {
            self.follow_tail = self.allow_follow;
        }
    }

    pub fn scroll_page(&mut self, direction: i32) {
        let page = self.viewport_height.saturating_sub(1).max(1) as i32;
        self.scroll_lines(direction.saturating_mul(page));
    }

    pub fn jump_to_top(&mut self) {
        self.offset = 0;
        self.follow_tail = false;
        self.clear_anchor();
    }

    pub fn jump_to_latest(&mut self) {
        self.offset = self.max_offset;
        self.follow_tail = self.allow_follow;
        self.clear_anchor();
    }

    pub fn reset(&mut self) {
        self.offset = 0;
        self.max_offset = 0;
        self.viewport_height = 1;
        self.follow_tail = self.allow_follow;
        self.clear_anchor();
    }
}

#[cfg(test)]
mod tests {
    use super::ScrollState;

    #[test]
    fn top_anchored_state_never_follows_growing_content() {
        let mut state = ScrollState::top_anchored();
        state.observe(200, 10);
        assert_eq!(state.offset(), 0);
        assert_eq!(state.max_offset(), 190);
        assert!(!state.is_following());

        state.scroll_lines(3);
        assert_eq!(state.offset(), 3);

        // Re-measuring the same content must not move a manual position, and new
        // content must not pull the viewport to the bottom.
        state.observe(200, 10);
        assert_eq!(state.offset(), 3);
        state.observe(400, 10);
        assert_eq!(state.offset(), 3);

        // Reaching the bottom edge stays there rather than re-arming tail following.
        state.jump_to_latest();
        assert_eq!(state.offset(), 390);
        assert!(!state.is_following());
        state.observe(600, 10);
        assert_eq!(state.offset(), 390);

        state.reset();
        assert_eq!(state.offset(), 0);
        assert!(!state.is_following());
    }

    #[test]
    fn follows_new_content_until_the_user_scrolls_up() {
        let mut state = ScrollState::default();
        state.observe(40, 10);
        assert_eq!(state.offset(), 30);
        assert!(state.is_following());

        state.scroll_lines(-3);
        assert_eq!(state.offset(), 27);
        assert!(!state.is_following());

        state.observe(50, 10);
        assert_eq!(state.offset(), 27);
        assert!(!state.is_following());
    }

    #[test]
    fn reaching_the_bottom_resumes_tail_following() {
        let mut state = ScrollState::default();
        state.observe(40, 10);
        state.scroll_lines(-20);
        assert_eq!(state.offset(), 10);
        assert!(!state.is_following());

        state.scroll_lines(20);
        assert_eq!(state.offset(), 30);
        assert!(state.is_following());
    }

    #[test]
    fn page_and_jump_commands_are_bounded() {
        let mut state = ScrollState::default();
        state.observe(100, 20);

        state.scroll_page(-1);
        assert_eq!(state.offset(), 61);
        state.jump_to_top();
        assert_eq!(state.offset(), 0);
        assert!(!state.is_following());
        state.scroll_page(1);
        assert_eq!(state.offset(), 19);
        state.jump_to_latest();
        assert_eq!(state.offset(), 80);
        assert!(state.is_following());
    }

    #[test]
    fn shrinking_content_clamps_manual_position() {
        let mut state = ScrollState::default();
        state.observe(100, 20);
        state.jump_to_top();
        state.scroll_lines(60);
        state.observe(30, 20);

        assert_eq!(state.offset(), 10);
        assert!(!state.is_following());
    }

    #[test]
    fn manual_scroll_refreshes_anchor_and_jumps_clear_it() {
        let mut state = ScrollState::default();
        state.observe(100, 20);
        state.jump_to_top();
        state.set_anchor(Some(super::ScrollAnchor {
            message_id: "message-1".to_owned(),
            line_offset: 2,
        }));
        assert_eq!(
            state.anchor(),
            Some(super::ScrollAnchor {
                message_id: "message-1".to_owned(),
                line_offset: 2,
            })
        );

        state.scroll_lines(1);
        assert!(state.anchor().is_none());
        state.set_anchor(Some(super::ScrollAnchor {
            message_id: "message-1".to_owned(),
            line_offset: 2,
        }));
        state.jump_to_latest();
        assert!(state.anchor().is_none());
    }
}
