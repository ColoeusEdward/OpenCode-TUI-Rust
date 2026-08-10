//! Mouse text selection over the panes that hold copyable content.
//!
//! Arming console mouse input takes the terminal's own selection away, so the
//! application has to provide it. Doing it here rather than delegating also
//! makes the pane restriction expressible: the terminal has no idea where the
//! transcript ends and the runtime sidebar begins, but this module does, so a
//! drag can be confined to the pane it started in and the sidebar can be left
//! out of selection entirely.
//!
//! Selection is line-stream, matching a normal terminal: a drag from one point
//! to another takes the rest of the first row, all of the rows between, and the
//! start of the last row.

use ratatui::layout::Rect;

/// The panes whose content can be selected. The runtime sidebar is deliberately
/// absent: it renders derived status, not text worth copying, and excluding it
/// keeps a drag that overshoots the transcript from picking up sidebar rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionPane {
    Transcript,
    Prompt,
}

/// A pane's on-screen text as of the last render. `rows[i]` is the text drawn at
/// row `area.y + i`, already stripped of decoration, so selected text can be
/// sliced without reading back the frame buffer.
#[derive(Debug, Clone)]
struct PaneContent {
    pane: SelectionPane,
    area: Rect,
    rows: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Point {
    column: u16,
    row: u16,
}

impl Point {
    /// Ordered reading-order-first so a drag upward or leftward normalizes the
    /// same way as one downward or rightward.
    fn key(self) -> (u16, u16) {
        (self.row, self.column)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Range {
    pane: SelectionPane,
    anchor: Point,
    head: Point,
    /// True between press and release. A finished selection stays visible so the
    /// user can see what was copied, but no longer tracks the pointer.
    dragging: bool,
}

/// One highlighted run on a single row, in absolute terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightSpan {
    pub row: u16,
    pub start_column: u16,
    /// Exclusive.
    pub end_column: u16,
}

#[derive(Debug, Default, Clone)]
pub struct SelectionState {
    panes: Vec<PaneContent>,
    range: Option<Range>,
}

impl SelectionState {
    /// Called at the start of every render. Pane geometry can change between
    /// frames (resize, the sidebar toggling, the draft-parts row appearing), so
    /// the recorded content is rebuilt rather than patched.
    pub fn begin_frame(&mut self) {
        self.panes.clear();
    }

    /// Records a selectable pane's text area and visible rows. `area` must be
    /// the inner text region, excluding any border.
    pub fn record_pane(&mut self, pane: SelectionPane, area: Rect, rows: Vec<String>) {
        self.panes.retain(|content| content.pane != pane);
        self.panes.push(PaneContent { pane, area, rows });
    }

    fn content(&self, pane: SelectionPane) -> Option<&PaneContent> {
        self.panes.iter().find(|content| content.pane == pane)
    }

    fn pane_at(&self, column: u16, row: u16) -> Option<&PaneContent> {
        self.panes.iter().find(|content| {
            content
                .area
                .contains(ratatui::layout::Position { x: column, y: row })
        })
    }

    pub fn is_dragging(&self) -> bool {
        self.range.is_some_and(|range| range.dragging)
    }

    /// Test-only: production code clears unconditionally rather than checking
    /// first, since `clear` is idempotent.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn has_selection(&self) -> bool {
        self.range.is_some_and(|range| !self.is_empty_range(range))
    }

    pub fn clear(&mut self) {
        self.range = None;
    }

    /// Begins a selection if the press landed in a selectable pane. A press
    /// anywhere else — the sidebar, a border, the footer — clears the selection
    /// instead of starting one, so the sidebar can never become an anchor.
    pub fn press(&mut self, column: u16, row: u16) {
        match self.pane_at(column, row) {
            Some(content) => {
                let point = clamp_to(content.area, column, row);
                self.range = Some(Range {
                    pane: content.pane,
                    anchor: point,
                    head: point,
                    dragging: true,
                });
            }
            None => self.range = None,
        }
    }

    /// Extends an in-progress selection. The point is clamped into the pane the
    /// drag started in, so dragging into the sidebar or past the pane edge
    /// extends along that edge rather than escaping the pane.
    pub fn drag(&mut self, column: u16, row: u16) {
        let Some(range) = self.range.as_mut() else {
            return;
        };
        if !range.dragging {
            return;
        }
        let pane = range.pane;
        let Some(area) = self.content(pane).map(|content| content.area) else {
            return;
        };
        if let Some(range) = self.range.as_mut() {
            range.head = clamp_to(area, column, row);
        }
    }

    /// Ends the drag and returns the selected text, or `None` when the gesture
    /// selected nothing (a plain click).
    pub fn release(&mut self) -> Option<String> {
        let range = self.range.as_mut()?;
        range.dragging = false;
        let range = *range;
        if self.is_empty_range(range) {
            self.range = None;
            return None;
        }
        self.selected_text()
    }

    /// The selected text, with each row's trailing padding removed and rows
    /// joined by newlines.
    pub fn selected_text(&self) -> Option<String> {
        let range = self.range?;
        let content = self.content(range.pane)?;
        let rows = self
            .row_spans(range, content)?
            .into_iter()
            .map(|span| {
                let text = row_text(content, span.row);
                let slice = slice_columns(text, span.start_column, span.end_column, content.area.x);
                slice.trim_end().to_owned()
            })
            .collect::<Vec<_>>();
        if rows.iter().all(String::is_empty) {
            return None;
        }
        Some(rows.join("\n"))
    }

    /// Highlight runs for the renderer, clipped to each row's actual content so
    /// the highlight shows exactly what would be copied rather than painting a
    /// block of trailing blanks.
    pub fn highlight_spans(&self) -> Vec<HighlightSpan> {
        let Some(range) = self.range else {
            return Vec::new();
        };
        let Some(content) = self.content(range.pane) else {
            return Vec::new();
        };
        let Some(spans) = self.row_spans(range, content) else {
            return Vec::new();
        };
        spans
            .into_iter()
            .filter_map(|span| {
                let text = row_text(content, span.row);
                let filled = content.area.x + display_columns(text);
                let end = span.end_column.min(filled);
                (end > span.start_column).then_some(HighlightSpan {
                    end_column: end,
                    ..span
                })
            })
            .collect()
    }

    fn is_empty_range(&self, range: Range) -> bool {
        range.anchor == range.head
    }

    /// Splits a normalized range into one span per row using line-stream rules:
    /// the first row runs to the pane's right edge, inner rows are full width,
    /// and the last row starts at the pane's left edge.
    fn row_spans(&self, range: Range, content: &PaneContent) -> Option<Vec<HighlightSpan>> {
        let (start, end) = if range.anchor.key() <= range.head.key() {
            (range.anchor, range.head)
        } else {
            (range.head, range.anchor)
        };
        let left = content.area.x;
        let right = content.area.right();
        if right <= left {
            return None;
        }
        let spans = (start.row..=end.row)
            .map(|row| {
                let start_column = if row == start.row { start.column } else { left };
                // The head cell is included, which is what a user dragging over a
                // character expects, so the end is one past it.
                let end_column = if row == end.row {
                    end.column.saturating_add(1).min(right)
                } else {
                    right
                };
                HighlightSpan {
                    row,
                    start_column,
                    end_column,
                }
            })
            .collect();
        Some(spans)
    }
}

fn clamp_to(area: Rect, column: u16, row: u16) -> Point {
    Point {
        column: column.clamp(area.x, area.right().saturating_sub(1)),
        row: row.clamp(area.y, area.bottom().saturating_sub(1)),
    }
}

fn row_text(content: &PaneContent, row: u16) -> &str {
    let index = row.checked_sub(content.area.y).map(usize::from);
    index
        .and_then(|index| content.rows.get(index))
        .map(String::as_str)
        .unwrap_or("")
}

fn display_columns(text: &str) -> u16 {
    u16::try_from(unicode_width::UnicodeWidthStr::width(text)).unwrap_or(u16::MAX)
}

/// Slices `text` by terminal column rather than by byte or char, so wide CJK
/// glyphs and multi-byte characters are cut where they are actually drawn. A
/// wide glyph straddling a boundary is included, because copying half of one
/// would be meaningless.
fn slice_columns(text: &str, start_column: u16, end_column: u16, origin: u16) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let start = start_column.saturating_sub(origin) as usize;
    let end = end_column.saturating_sub(origin) as usize;
    let mut column = 0usize;
    let mut out = String::new();
    for grapheme in text.graphemes(true) {
        let width = UnicodeWidthStr::width(grapheme).max(1);
        let next = column + width;
        if next > start && column < end {
            out.push_str(grapheme);
        }
        column = next;
        if column >= end {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{SelectionPane, SelectionState, slice_columns};
    use ratatui::layout::Rect;

    fn transcript_area() -> Rect {
        Rect {
            x: 1,
            y: 1,
            width: 20,
            height: 3,
        }
    }

    fn sidebar_area() -> Rect {
        Rect {
            x: 30,
            y: 1,
            width: 20,
            height: 3,
        }
    }

    fn prompt_area() -> Rect {
        Rect {
            x: 1,
            y: 6,
            width: 20,
            height: 2,
        }
    }

    fn state_with_panes() -> SelectionState {
        let mut state = SelectionState::default();
        state.begin_frame();
        state.record_pane(
            SelectionPane::Transcript,
            transcript_area(),
            vec![
                "first line".to_owned(),
                "second line".to_owned(),
                "third line".to_owned(),
            ],
        );
        state.record_pane(
            SelectionPane::Prompt,
            prompt_area(),
            vec!["draft text".to_owned(), String::new()],
        );
        state
    }

    #[test]
    fn a_drag_inside_the_transcript_selects_the_dragged_text() {
        let mut state = state_with_panes();
        state.press(1, 1);
        state.drag(5, 1);
        assert_eq!(state.release().as_deref(), Some("first"));
    }

    #[test]
    fn a_multi_row_drag_follows_line_stream_rules() {
        let mut state = state_with_panes();
        // From "line" on row 1 through "second" on row 2.
        state.press(7, 1);
        state.drag(6, 2);
        assert_eq!(state.release().as_deref(), Some("line\nsecond"));
    }

    #[test]
    fn a_drag_normalizes_when_it_runs_backwards() {
        let mut state = state_with_panes();
        state.press(6, 2);
        state.drag(7, 1);
        assert_eq!(state.release().as_deref(), Some("line\nsecond"));
    }

    #[test]
    fn the_prompt_is_selectable_too() {
        let mut state = state_with_panes();
        state.press(1, 6);
        state.drag(5, 6);
        assert_eq!(state.release().as_deref(), Some("draft"));
    }

    #[test]
    fn a_press_on_the_sidebar_starts_no_selection() {
        let mut state = state_with_panes();
        state.record_pane(SelectionPane::Transcript, transcript_area(), Vec::new());
        // The sidebar is never recorded as a selectable pane, so a press inside
        // its rect finds no pane at all.
        state.press(sidebar_area().x + 2, sidebar_area().y);
        state.drag(sidebar_area().x + 8, sidebar_area().y);

        assert!(!state.is_dragging());
        assert!(!state.has_selection());
        assert_eq!(state.release(), None);
        assert!(state.highlight_spans().is_empty());
    }

    #[test]
    fn a_press_on_the_sidebar_clears_an_existing_selection() {
        let mut state = state_with_panes();
        state.press(1, 1);
        state.drag(5, 1);
        state.release();
        assert!(state.has_selection());

        state.press(sidebar_area().x + 2, sidebar_area().y);
        assert!(!state.has_selection());
    }

    #[test]
    fn dragging_from_the_transcript_into_the_sidebar_stays_inside_the_transcript() {
        let mut state = state_with_panes();
        state.press(1, 1);
        // Well past the transcript's right edge, horizontally into the sidebar.
        state.drag(sidebar_area().right(), 1);

        let text = state.release().expect("the drag selected transcript text");
        assert_eq!(text, "first line");
        for span in state.highlight_spans() {
            assert!(
                span.end_column <= transcript_area().right(),
                "highlight escaped the transcript: {span:?}"
            );
        }
    }

    #[test]
    fn dragging_below_the_transcript_stops_at_its_last_row() {
        let mut state = state_with_panes();
        state.press(1, 1);
        // Past the transcript's bottom, into the rows the prompt occupies.
        state.drag(11, prompt_area().bottom());

        let spans = state.highlight_spans();
        assert!(!spans.is_empty());
        for span in &spans {
            assert!(
                span.row < transcript_area().bottom(),
                "highlight escaped the transcript: {span:?}"
            );
        }
        let text = state.release().expect("the drag selected transcript text");
        assert_eq!(text, "first line\nsecond line\nthird line");
    }

    #[test]
    fn a_plain_click_selects_nothing() {
        let mut state = state_with_panes();
        state.press(3, 1);
        assert_eq!(state.release(), None);
        assert!(!state.has_selection());
    }

    #[test]
    fn the_highlight_stops_at_the_end_of_each_rows_text() {
        let mut state = state_with_panes();
        state.press(1, 1);
        state.drag(transcript_area().right() - 1, 1);

        let spans = state.highlight_spans();
        assert_eq!(spans.len(), 1);
        // "first line" is 10 columns wide starting at x = 1, so the highlight must
        // stop at 11 rather than running to the pane edge at 21.
        assert_eq!(spans[0].start_column, 1);
        assert_eq!(spans[0].end_column, 11);
    }

    #[test]
    fn a_row_with_no_text_produces_no_highlight() {
        let mut state = state_with_panes();
        // The prompt's second row is empty.
        state.press(1, 6);
        state.drag(10, 7);

        let spans = state.highlight_spans();
        assert!(spans.iter().all(|span| span.row == 6), "{spans:?}");
    }

    #[test]
    fn the_selection_stays_visible_after_the_drag_ends() {
        let mut state = state_with_panes();
        state.press(1, 1);
        state.drag(5, 1);
        state.release();

        assert!(!state.is_dragging());
        assert!(state.has_selection());
        assert_eq!(state.selected_text().as_deref(), Some("first"));
    }

    #[test]
    fn a_new_frame_keeps_the_selection_but_refreshes_pane_geometry() {
        let mut state = state_with_panes();
        state.press(1, 1);
        state.drag(5, 1);
        state.release();

        state.begin_frame();
        state.record_pane(
            SelectionPane::Transcript,
            transcript_area(),
            vec!["FIRST line".to_owned()],
        );
        // The selection is by position, so re-rendered content is re-sliced.
        assert_eq!(state.selected_text().as_deref(), Some("FIRST"));
    }

    #[test]
    fn a_selection_whose_pane_disappeared_yields_nothing() {
        let mut state = state_with_panes();
        state.press(1, 1);
        state.drag(5, 1);
        state.release();

        // The sidebar toggling can drop and re-add panes; a pane that is simply
        // gone must not panic or return stale text.
        state.begin_frame();
        assert_eq!(state.selected_text(), None);
        assert!(state.highlight_spans().is_empty());
    }

    #[test]
    fn columns_are_sliced_by_display_width_not_by_byte() {
        // Each CJK glyph is two columns wide, so columns 0..4 is the first two.
        assert_eq!(slice_columns("中文字", 0, 4, 0), "中文");
        // A boundary landing inside a wide glyph includes it rather than
        // splitting it, since half a glyph is not copyable text.
        assert_eq!(slice_columns("中文字", 0, 3, 0), "中文");
        assert_eq!(slice_columns("abc", 1, 3, 0), "bc");
        // The origin shifts absolute terminal columns into pane-relative ones.
        assert_eq!(slice_columns("abc", 6, 8, 5), "bc");
    }

    #[test]
    fn trailing_padding_is_trimmed_from_each_selected_row() {
        let mut state = SelectionState::default();
        state.begin_frame();
        state.record_pane(
            SelectionPane::Transcript,
            transcript_area(),
            vec!["ab   ".to_owned(), "cd".to_owned()],
        );
        state.press(1, 1);
        state.drag(transcript_area().right() - 1, 2);
        assert_eq!(state.release().as_deref(), Some("ab\ncd"));
    }
}
