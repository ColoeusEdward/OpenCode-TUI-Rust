use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Tracks the visual layout of composer text when wrapped to a specific width.
#[derive(Debug, Clone)]
pub struct ComposerLayout {
    /// Logical lines (actual content lines)
    logical_lines: Vec<String>,
    /// Visual lines (how content appears when wrapped)
    visual_lines: Vec<VisualLine>,
    /// Available width for text content (excluding borders)
    content_width: usize,
    /// Visual line the cursor is on (0-based)
    cursor_visual_line: usize,
    /// Display column within the visual line (0-based)
    cursor_visual_col: usize,
    /// Grapheme index within the visual line where the cursor is located
    cursor_visual_grapheme: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct VisualLine {
    /// Index of the logical line this visual line belongs to
    logical_line: usize,
    /// Start character column in the logical line (inclusive)
    start_col: usize,
    /// End character column in the logical line (exclusive)
    end_col: usize,
    /// The actual text content of this visual line
    text: String,
    /// Terminal display width of this visual line
    #[allow(dead_code)]
    width: usize,
}

impl ComposerLayout {
    /// Create a new layout from logical lines and cursor position.
    pub fn new(
        logical_lines: Vec<String>,
        cursor_row: usize,
        cursor_col: usize,
        content_width: u16,
    ) -> Self {
        let mut layout = Self {
            logical_lines,
            visual_lines: Vec::new(),
            content_width: usize::from(content_width.max(1)),
            cursor_visual_line: 0,
            cursor_visual_col: 0,
            cursor_visual_grapheme: 0,
        };
        layout.recompute_layout(cursor_row, cursor_col);
        layout
    }

    /// Get the visual line index where the cursor is located.
    pub fn cursor_visual_line(&self) -> usize {
        self.cursor_visual_line
    }

    /// Get the display column where the cursor is located within its visual line.
    #[allow(dead_code)]
    pub fn cursor_visual_col(&self) -> usize {
        self.cursor_visual_col
    }

    /// Get the grapheme index where the cursor is located within its visual line.
    pub fn cursor_visual_grapheme(&self) -> usize {
        self.cursor_visual_grapheme
    }

    /// Get the total number of visual lines.
    pub fn visual_line_count(&self) -> usize {
        self.visual_lines.len()
    }

    /// Get the text content of a visual line.
    pub fn visual_line_text(&self, index: usize) -> Option<&str> {
        self.visual_lines.get(index).map(|vl| vl.text.as_str())
    }

    /// Get the terminal display width of a visual line.
    #[allow(dead_code)]
    pub fn visual_line_width(&self, index: usize) -> Option<usize> {
        self.visual_lines.get(index).map(|vl| vl.width)
    }

    /// Recompute the visual layout for the current logical lines and cursor position.
    fn recompute_layout(&mut self, cursor_row: usize, cursor_col: usize) {
        self.visual_lines.clear();
        let width = self.content_width;

        for (logical_idx, line) in self.logical_lines.clone().into_iter().enumerate() {
            if line.is_empty() {
                // Empty logical line becomes one empty visual line
                self.visual_lines.push(VisualLine {
                    logical_line: logical_idx,
                    start_col: 0,
                    end_col: 0,
                    text: String::new(),
                    width: 0,
                });
                continue;
            }

            // Wrap by terminal display cells while preserving grapheme boundaries.
            let graphemes: Vec<&str> = line.graphemes(true).collect();
            let mut current = Vec::new();
            let mut current_width: usize = 0;
            let mut current_start_col: usize = 0;
            let mut char_col: usize = 0;

            for grapheme in graphemes {
                let grapheme_chars = grapheme.chars().count();
                let grapheme_width = grapheme.width();

                if !current.is_empty() && current_width.saturating_add(grapheme_width) > width {
                    self.push_visual_line(
                        logical_idx,
                        current_start_col,
                        char_col,
                        &current,
                        current_width,
                    );
                    current.clear();
                    current_width = 0;
                    current_start_col = char_col;
                }

                if current.is_empty() {
                    current_start_col = char_col;
                }
                current.push(grapheme);
                current_width = current_width.saturating_add(grapheme_width);
                char_col = char_col.saturating_add(grapheme_chars);
            }

            if !current.is_empty() {
                self.push_visual_line(
                    logical_idx,
                    current_start_col,
                    char_col,
                    &current,
                    current_width,
                );
            }
        }

        // Calculate cursor visual position
        self.update_cursor_visual_position(cursor_row, cursor_col);
    }

    fn push_visual_line(
        &mut self,
        logical_line: usize,
        start_col: usize,
        end_col: usize,
        graphemes: &[&str],
        width: usize,
    ) {
        self.visual_lines.push(VisualLine {
            logical_line,
            start_col,
            end_col,
            text: graphemes.iter().copied().collect(),
            width,
        });
    }

    /// Update the cursor's visual position based on its logical position.
    fn update_cursor_visual_position(&mut self, cursor_row: usize, cursor_col: usize) {
        // Find the visual line containing this logical position
        for (visual_idx, vline) in self.visual_lines.iter().enumerate() {
            if vline.logical_line == cursor_row
                && cursor_col >= vline.start_col
                && cursor_col <= vline.end_col
            {
                let (grapheme, display_col) = cursor_position(vline, cursor_col);
                self.cursor_visual_line = visual_idx;
                self.cursor_visual_grapheme = grapheme;
                self.cursor_visual_col = display_col;
                return;
            }
        }

        // Fallback: place cursor at the end
        if let Some((visual_idx, last)) = self.visual_lines.iter().enumerate().next_back() {
            let (grapheme, display_col) = cursor_position(last, last.end_col);
            self.cursor_visual_line = visual_idx;
            self.cursor_visual_grapheme = grapheme;
            self.cursor_visual_col = display_col;
        } else {
            self.cursor_visual_line = 0;
            self.cursor_visual_col = 0;
            self.cursor_visual_grapheme = 0;
        }
    }
}

fn cursor_position(line: &VisualLine, cursor_col: usize) -> (usize, usize) {
    let local_col = cursor_col
        .saturating_sub(line.start_col)
        .min(line.end_col.saturating_sub(line.start_col));
    let mut char_col: usize = 0;
    let mut grapheme_index: usize = 0;
    let mut display_col: usize = 0;

    for grapheme in line.text.graphemes(true) {
        let grapheme_chars = grapheme.chars().count();
        if local_col < char_col.saturating_add(grapheme_chars) {
            break;
        }
        char_col = char_col.saturating_add(grapheme_chars);
        display_col = display_col.saturating_add(grapheme.width());
        grapheme_index += 1;
    }

    (grapheme_index, display_col)
}

#[cfg(test)]
mod tests {
    use super::ComposerLayout;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn empty_content_has_one_visual_line() {
        let layout = ComposerLayout::new(vec![String::new()], 0, 0, 80);

        assert_eq!(layout.visual_line_count(), 1);
        assert_eq!(layout.cursor_visual_line(), 0);
        assert_eq!(layout.cursor_visual_col(), 0);
        assert_eq!(layout.cursor_visual_grapheme(), 0);
    }

    #[test]
    fn short_line_is_one_visual_line() {
        let layout = ComposerLayout::new(vec!["hello world".to_string()], 0, 5, 80);

        assert_eq!(layout.visual_line_count(), 1);
        assert_eq!(layout.visual_line_text(0), Some("hello world"));
        assert_eq!(layout.cursor_visual_line(), 0);
        assert_eq!(layout.cursor_visual_col(), 5);
        assert_eq!(layout.cursor_visual_grapheme(), 5);
    }

    #[test]
    fn long_line_wraps_into_multiple_visual_lines() {
        let text = "a".repeat(100);
        let layout = ComposerLayout::new(vec![text.clone()], 0, 75, 40);

        assert_eq!(layout.visual_line_count(), 3); // 40 + 40 + 20
        assert_eq!(layout.visual_line_text(0), Some("a".repeat(40).as_str()));
        assert_eq!(layout.visual_line_text(1), Some("a".repeat(40).as_str()));
        assert_eq!(layout.visual_line_text(2), Some("a".repeat(20).as_str()));
        assert_eq!(layout.cursor_visual_line(), 1); // cursor at col 75 = in second visual line
        assert_eq!(layout.cursor_visual_col(), 35); // 75 - 40 = 35
        assert_eq!(layout.cursor_visual_grapheme(), 35);
    }

    #[test]
    fn multiple_logical_lines_create_multiple_visual_lines() {
        let layout = ComposerLayout::new(
            vec![
                "first".to_string(),
                "second".to_string(),
                "third".to_string(),
            ],
            1,
            3,
            80,
        );

        assert_eq!(layout.visual_line_count(), 3);
        assert_eq!(layout.visual_line_text(0), Some("first"));
        assert_eq!(layout.visual_line_text(1), Some("second"));
        assert_eq!(layout.visual_line_text(2), Some("third"));
        assert_eq!(layout.cursor_visual_line(), 1);
        assert_eq!(layout.cursor_visual_col(), 3);
        assert_eq!(layout.cursor_visual_grapheme(), 3);
    }

    #[test]
    fn unicode_graphemes_use_terminal_display_width() {
        let text = "hello 你好 world";
        let layout = ComposerLayout::new(vec![text.to_string()], 0, 9, 80);

        assert_eq!(layout.visual_line_text(0), Some(text));
        assert_eq!(layout.cursor_visual_line(), 0);
        assert_eq!(layout.cursor_visual_col(), "hello 你好 ".width());
        assert_eq!(layout.cursor_visual_grapheme(), 9);
    }

    #[test]
    fn mixed_width_text_wraps_without_exceeding_display_width() {
        let layout = ComposerLayout::new(vec!["ab你好cd".to_string()], 0, 3, 6);

        assert_eq!(layout.visual_line_count(), 2);
        assert_eq!(layout.visual_line_text(0), Some("ab你好"));
        assert_eq!(layout.visual_line_text(1), Some("cd"));
        assert!(layout.visual_line_width(0).is_some_and(|width| width <= 6));
        assert!(layout.visual_line_width(1).is_some_and(|width| width <= 6));
        assert_eq!(layout.cursor_visual_line(), 0);
        assert_eq!(layout.cursor_visual_col(), "ab你".width());
        assert_eq!(layout.cursor_visual_grapheme(), 3);
    }

    #[test]
    fn narrow_width_keeps_progress_for_wide_graphemes() {
        let layout = ComposerLayout::new(vec!["你a".to_string()], 0, 0, 1);

        assert_eq!(layout.visual_line_count(), 2);
        assert_eq!(layout.visual_line_text(0), Some("你"));
        assert_eq!(layout.visual_line_text(1), Some("a"));
        assert_eq!(layout.visual_line_width(0), Some(2));
        assert_eq!(layout.visual_line_width(1), Some(1));
    }

    #[test]
    fn cursor_at_end_of_wrapped_line() {
        let text = "a".repeat(40);
        let layout = ComposerLayout::new(vec![text.clone()], 0, 40, 40);

        assert_eq!(layout.visual_line_count(), 1);
        assert_eq!(layout.cursor_visual_line(), 0);
        assert_eq!(layout.cursor_visual_col(), 40);
        assert_eq!(layout.cursor_visual_grapheme(), 40);
    }
}
