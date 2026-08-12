use crate::composer_layout::ComposerLayout;
use crate::cursor_blink::CursorBlink;
use crate::prompt_history::PromptHistory;
use crate::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph};
use tui_textarea::{CursorMove, TextArea};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerAction {
    None,
    Changed,
    Submit(String),
}

pub struct Composer {
    textarea: TextArea<'static>,
    /// Scroll offset in visual lines (0 = show from top)
    scroll_offset: u16,
    /// Prompt history
    history: PromptHistory,
    /// Blink phase for the hand-drawn cursor.
    blink: CursorBlink,
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    pub fn new() -> Self {
        let mut composer = Self {
            textarea: TextArea::default(),
            scroll_offset: 0,
            history: PromptHistory::new(100, PromptHistory::default_file_path()),
            blink: CursorBlink::new(),
        };
        composer.configure(Theme::default());
        composer
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    pub fn set_text(&mut self, text: &str) {
        self.textarea = TextArea::from(text.split('\n'));
        self.configure(Theme::default());
        self.textarea.move_cursor(CursorMove::Bottom);
        self.textarea.move_cursor(CursorMove::End);
        self.scroll_offset = 0;
    }

    pub fn clear(&mut self) {
        self.set_text("");
    }

    #[cfg(test)]
    pub fn cursor(&self) -> (usize, usize) {
        self.textarea.cursor()
    }

    pub fn cursor_offset(&self) -> usize {
        let (row, col) = self.textarea.cursor();
        self.textarea
            .lines()
            .iter()
            .take(row)
            .map(|line| line.chars().count() + 1)
            .sum::<usize>()
            + col
    }

    pub fn replace_char_range(&mut self, start: usize, end: usize, replacement: &str) -> bool {
        let text_length = self.text().chars().count();
        let start = start.min(text_length);
        let end = end.min(text_length).max(start);
        let (row, col) = self.position_for_offset(start);

        self.textarea.cancel_selection();
        self.textarea.move_cursor(CursorMove::Jump(
            row.min(u16::MAX as usize) as u16,
            col.min(u16::MAX as usize) as u16,
        ));
        self.textarea.start_selection();
        for _ in start..end {
            self.textarea.move_cursor(CursorMove::Forward);
        }
        let changed = self.textarea.insert_str(replacement);
        self.normalize_cursor_to_grapheme_boundary();
        changed
    }

    /// Advances the cursor blink phase at the rate matching `thinking`.
    pub fn advance_blink(&mut self, delta: std::time::Duration, thinking: bool) {
        self.blink.advance(delta, thinking);
    }

    /// Returns how long the runtime can sleep before cursor visibility changes.
    pub fn next_blink_transition_in(&self, thinking: bool) -> Option<std::time::Duration> {
        self.blink.next_transition_in(thinking)
    }

    pub fn history_previous(&mut self) -> bool {
        let current = self.text();
        let Some(previous) = self.history.previous(&current) else {
            return false;
        };
        self.set_text(&previous);
        true
    }

    pub fn history_next(&mut self) -> bool {
        let Some(next) = self.history.next() else {
            return false;
        };
        self.set_text(&next);
        true
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ComposerAction {
        if key.kind != KeyEventKind::Press {
            return ComposerAction::None;
        }

        // Handle history navigation (Up/Down only when on first/last line and not slash command)
        if !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && !self.text().starts_with('/')
        {
            let (row, _) = self.textarea.cursor();
            let line_count = self.textarea.lines().len();

            match key.code {
                KeyCode::Up if row == 0 => {
                    // Only navigate history when on first line
                    if let Some(prev) = self.history.previous(&self.text()) {
                        self.set_text(&prev);
                        return ComposerAction::Changed;
                    }
                    return ComposerAction::None;
                }
                KeyCode::Down
                    if row == line_count.saturating_sub(1) && self.history.is_navigating() =>
                {
                    // Only navigate history when on last line and actively navigating
                    if let Some(next) = self.history.next() {
                        self.set_text(&next);
                    } else {
                        self.clear();
                    }
                    return ComposerAction::Changed;
                }
                _ => {}
            }
        }

        if key.code == KeyCode::Enter {
            if key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                self.textarea.insert_newline();
                return ComposerAction::Changed;
            }
            let text = self.text();
            if !text.trim().is_empty() {
                self.history.add(&text);
            }
            return ComposerAction::Submit(text);
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('a') {
            self.textarea.select_all();
            return ComposerAction::None;
        }

        if is_delete_word_backward(key) {
            let changed = self.textarea.delete_word();
            self.normalize_cursor_to_grapheme_boundary();
            return Self::changed(changed);
        }
        if is_delete_word_forward(key) {
            let changed = self.textarea.delete_next_word();
            self.normalize_cursor_to_grapheme_boundary();
            return Self::changed(changed);
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('u') {
            return Self::changed(self.textarea.undo());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
            return Self::changed(self.textarea.redo());
        }

        let action = if let Some(action) = self.handle_grapheme_key(key) {
            action
        } else {
            self.blink.restart();
            Self::changed(self.textarea.input(key))
        };
        self.normalize_cursor_to_grapheme_boundary();
        action
    }

    pub fn paste(&mut self, text: &str) -> bool {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.textarea.insert_str(normalized)
    }

    /// Renders the prompt box.
    ///
    /// The blink rate is chosen when the phase is advanced, not here, so drawing
    /// a frame never changes the blink.
    /// Draws the prompt and returns the region and rows that mouse selection can
    /// address: the area inside the border, and the visible text of each row.
    pub fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: Theme,
        thinking: bool,
    ) -> (Rect, Vec<String>) {
        let cursor_visible = !thinking || self.blink.is_visible();
        self.configure(theme);
        // The block owns the rail gap and content padding, so selection
        // coordinates and text wrapping use the exact rendered content area.
        let block = prompt_block(theme);
        let inner = block.inner(area);
        let content_width = inner.width.saturating_sub(1); // Reserve 1 for cursor
        let viewport_height = inner.height;

        // Build layout from current textarea state
        let (cursor_row, cursor_col) = self.textarea.cursor();
        let logical_lines: Vec<String> = self
            .textarea
            .lines()
            .iter()
            .map(|s| s.to_string())
            .collect();

        let layout = ComposerLayout::new(logical_lines, cursor_row, cursor_col, content_width);

        // Adjust scroll to keep cursor visible
        let cursor_visual_line = layout.cursor_visual_line() as u16;
        let total_visual_lines = layout.visual_line_count() as u16;

        // Auto-scroll to keep cursor in view
        if cursor_visual_line < self.scroll_offset {
            self.scroll_offset = cursor_visual_line;
        } else if cursor_visual_line >= self.scroll_offset + viewport_height {
            self.scroll_offset = cursor_visual_line.saturating_sub(viewport_height - 1);
        }

        // Clamp scroll offset to valid range
        let max_scroll = total_visual_lines.saturating_sub(viewport_height);
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        // Build visible lines
        let start = self.scroll_offset as usize;
        let end = (start + viewport_height as usize).min(layout.visual_line_count());

        let mut lines = Vec::new();
        for idx in start..end {
            let text = layout.visual_line_text(idx).unwrap_or("");

            // Check if cursor is on this visual line
            if idx == layout.cursor_visual_line() {
                let cursor_grapheme = layout.cursor_visual_grapheme();
                let graphemes: Vec<&str> = text.graphemes(true).collect();
                let before: String = graphemes.iter().take(cursor_grapheme).copied().collect();
                let at_cursor = graphemes.get(cursor_grapheme).copied().unwrap_or(" ");
                let after: String = graphemes
                    .iter()
                    .skip(cursor_grapheme + 1)
                    .copied()
                    .collect();

                lines.push(Line::from(vec![
                    Span::styled(before, Style::default().fg(theme.text)),
                    Span::styled(at_cursor.to_string(), cursor_style(theme, cursor_visible)),
                    Span::styled(after, Style::default().fg(theme.text)),
                ]));
            } else {
                lines.push(Line::from(Span::styled(
                    text.to_string(),
                    Style::default().fg(theme.text),
                )));
            }
        }

        // The selectable rows are the prompt's own text, captured before the
        // placeholder is substituted below. The placeholder is a prompt to the
        // user, not content, so selecting over it must copy nothing.
        let selectable_rows = (start..end)
            .map(|idx| layout.visual_line_text(idx).unwrap_or("").to_owned())
            .collect();

        // With no text there is nothing for the cursor span to sit on, so the
        // placeholder is drawn after an explicit cursor cell rather than in place
        // of the line. Replacing the whole line would hide the cursor exactly when
        // the user is looking for somewhere to start typing.
        if lines.is_empty() || (lines.len() == 1 && layout.visual_line_text(0) == Some("")) {
            lines = vec![Line::from(vec![
                Span::styled(" ", cursor_style(theme, cursor_visible)),
                Span::styled(
                    "Type a prompt and press Enter",
                    Style::default().fg(theme.text_muted),
                ),
            ])];
        }

        let paragraph = Paragraph::new(lines)
            .style(Style::default().fg(theme.text).bg(theme.background_element))
            .block(block);

        frame.render_widget(paragraph, area);
        render_prompt_rail(frame, area, theme);
        (inner, selectable_rows)
    }

    fn changed(changed: bool) -> ComposerAction {
        if changed {
            ComposerAction::Changed
        } else {
            ComposerAction::None
        }
    }

    fn handle_grapheme_key(&mut self, key: KeyEvent) -> Option<ComposerAction> {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return None;
        }

        match key.code {
            KeyCode::Left | KeyCode::Right => {
                let direction = if key.code == KeyCode::Left {
                    CursorMove::Back
                } else {
                    CursorMove::Forward
                };
                let steps = self.horizontal_grapheme_steps(key.code)?;
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    if !self.textarea.is_selecting() {
                        self.textarea.start_selection();
                    }
                } else if self.textarea.is_selecting() {
                    self.textarea.cancel_selection();
                }
                for _ in 0..steps {
                    self.textarea.move_cursor(direction);
                }
                Some(ComposerAction::None)
            }
            KeyCode::Backspace | KeyCode::Delete if !self.textarea.is_selecting() => {
                let (row, col) = self.textarea.cursor();
                match key.code {
                    KeyCode::Backspace if col > 0 => {
                        let steps = previous_grapheme_width(&self.textarea.lines()[row], col)?;
                        for _ in 0..steps {
                            self.textarea.move_cursor(CursorMove::Back);
                        }
                        Some(Self::changed(self.textarea.delete_str(steps)))
                    }
                    KeyCode::Delete if col < self.textarea.lines()[row].chars().count() => {
                        let steps = next_grapheme_width(&self.textarea.lines()[row], col)?;
                        Some(Self::changed(self.textarea.delete_str(steps)))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn horizontal_grapheme_steps(&self, direction: KeyCode) -> Option<usize> {
        let (row, col) = self.textarea.cursor();
        let line = &self.textarea.lines()[row];
        match direction {
            KeyCode::Left => {
                if col == 0 {
                    (row > 0).then_some(1)
                } else {
                    previous_grapheme_width(line, col)
                }
            }
            KeyCode::Right => {
                let line_len = line.chars().count();
                if col == line_len {
                    (row + 1 < self.textarea.lines().len()).then_some(1)
                } else {
                    next_grapheme_width(line, col)
                }
            }
            _ => None,
        }
    }

    fn normalize_cursor_to_grapheme_boundary(&mut self) {
        let (row, col) = self.textarea.cursor();
        let boundary = {
            let line = &self.textarea.lines()[row];
            let boundaries = grapheme_boundaries(line);
            match boundaries.binary_search(&col) {
                Ok(_) => return,
                Err(index) => boundaries[index.saturating_sub(1)],
            }
        };
        for _ in boundary..col {
            self.textarea.move_cursor(CursorMove::Back);
        }
    }

    fn position_for_offset(&self, offset: usize) -> (usize, usize) {
        let mut remaining = offset;
        for (row, line) in self.textarea.lines().iter().enumerate() {
            let line_length = line.chars().count();
            if remaining <= line_length {
                return (row, remaining);
            }
            remaining = remaining.saturating_sub(line_length + 1);
        }
        let row = self.textarea.lines().len().saturating_sub(1);
        (row, self.textarea.lines()[row].chars().count())
    }

    fn configure(&mut self, theme: Theme) {
        self.textarea
            .set_style(Style::default().fg(theme.text).bg(theme.background_element));
        self.textarea
            .set_placeholder_text("Type a prompt and press Enter");
        self.textarea
            .set_placeholder_style(Style::default().fg(theme.text_muted));
        self.textarea.set_cursor_style(cursor_style(theme, true));
        self.textarea.set_block(prompt_block(theme));
    }
}

fn prompt_block(theme: Theme) -> Block<'static> {
    Block::default()
        .style(Style::default().bg(theme.background_element))
        .padding(Padding::new(2, 1, 1, 1))
}

fn render_prompt_rail(frame: &mut Frame<'_>, area: Rect, theme: Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let rail = (0..area.height)
        .map(|_| Line::from(Span::styled("▍", Style::default().fg(theme.prompt_border))))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(rail).style(Style::default().bg(theme.background_element)),
        Rect {
            x: area.x,
            y: area.y,
            width: 1,
            height: area.height,
        },
    );
}

/// Style for the cell the cursor occupies.
///
/// While blinked off the cell keeps its glyph and normal colours, so the
/// character under the cursor stays readable instead of disappearing.
fn cursor_style(theme: Theme, visible: bool) -> Style {
    if visible {
        Style::default()
            .fg(theme.selected_list_item_text)
            .bg(theme.prompt_cursor)
    } else {
        Style::default().fg(theme.text)
    }
}

/// Whether this key means "delete the word before the cursor".
///
/// `Ctrl-Backspace` is the requested binding, but terminals disagree about what
/// they send for it, so several encodings map to the same action:
///
/// - Windows console reports it as `Ctrl` plus `Backspace`.
/// - Many Unix terminals send `\x08` (BS), which Crossterm decodes as `Ctrl-H`,
///   because `\x7F` (DEL) is already plain `Backspace`.
/// - `Alt-Backspace` is the long-standing readline binding and is accepted too.
fn is_delete_word_backward(key: KeyEvent) -> bool {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Backspace => control || alt,
        KeyCode::Char('h') => control,
        _ => false,
    }
}

/// Whether this key means "delete the word after the cursor".
///
/// Included so forward word deletion stays consistent with the new backward
/// binding rather than being reachable only through `tui-textarea`'s own
/// `Alt-Delete` / `Alt-D` defaults.
fn is_delete_word_forward(key: KeyEvent) -> bool {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Delete => control || alt,
        KeyCode::Char('d') => alt,
        _ => false,
    }
}

fn grapheme_boundaries(line: &str) -> Vec<usize> {
    let mut boundaries = vec![0];
    let mut chars = 0;
    for grapheme in line.graphemes(true) {
        chars += grapheme.chars().count();
        boundaries.push(chars);
    }
    boundaries
}

fn previous_grapheme_width(line: &str, col: usize) -> Option<usize> {
    let boundaries = grapheme_boundaries(line);
    let index = boundaries.binary_search(&col).ok()?;
    (index > 0).then(|| boundaries[index] - boundaries[index - 1])
}

fn next_grapheme_width(line: &str, col: usize) -> Option<usize> {
    let boundaries = grapheme_boundaries(line);
    let index = boundaries.binary_search(&col).ok()?;
    boundaries
        .get(index + 1)
        .map(|next| next - boundaries[index])
}

#[cfg(test)]
mod tests {
    use super::{Composer, ComposerAction};
    use crate::theme::Theme;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// Renders the composer and returns the background colour of every cell on the
    /// first content row, so tests can locate the drawn cursor by its highlight.
    fn first_row_backgrounds(
        composer: &mut Composer,
        thinking: bool,
    ) -> Vec<ratatui::style::Color> {
        let theme = Theme::default();
        let backend = TestBackend::new(40, 7);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| {
                composer.render(
                    frame,
                    Rect {
                        x: 0,
                        y: 0,
                        width: 40,
                        height: 7,
                    },
                    theme,
                    thinking,
                );
            })
            .expect("test draw should succeed");
        let buffer = terminal.backend().buffer();
        // The prompt has a visually narrow rail, one column of gap, and one
        // column of right padding around the content.
        (2..40)
            .map(|x| {
                buffer
                    .cell((x, 1))
                    .expect("test coordinates should be inside the buffer")
                    .bg
            })
            .collect()
    }

    #[test]
    fn the_cursor_is_drawn_even_when_the_prompt_is_empty() {
        let theme = Theme::default();
        let mut composer = Composer::new();
        assert!(composer.is_empty());

        let backgrounds = first_row_backgrounds(&mut composer, false);
        assert_eq!(
            backgrounds
                .iter()
                .filter(|bg| **bg == theme.prompt_cursor)
                .count(),
            1,
            "an empty prompt should still show exactly one cursor cell; the \
             placeholder must not replace it"
        );
        assert_eq!(
            backgrounds[0], theme.prompt_cursor,
            "the cursor belongs at the start of the line, before the placeholder"
        );
    }

    #[test]
    fn the_empty_prompt_cursor_only_blinks_while_thinking() {
        let theme = Theme::default();
        let mut composer = Composer::new();

        assert_eq!(
            first_row_backgrounds(&mut composer, true)[0],
            theme.prompt_cursor,
            "a fresh blink phase starts visible"
        );

        let transition = composer
            .next_blink_transition_in(true)
            .expect("thinking should schedule a transition");
        composer.advance_blink(transition + std::time::Duration::from_millis(1), true);
        assert_ne!(
            first_row_backgrounds(&mut composer, true)[0],
            theme.prompt_cursor,
            "the empty-prompt cursor should follow the same blink phase as a \
              cursor sitting on text"
        );
        assert_eq!(
            first_row_backgrounds(&mut composer, false)[0],
            theme.prompt_cursor,
            "the cursor should stay visible while the session is idle"
        );
        assert!(composer.next_blink_transition_in(false).is_none());
    }

    #[test]
    fn the_prompt_uses_a_theme_background_and_only_a_thick_left_border() {
        let theme = Theme::default();
        let mut composer = Composer::new();
        let backend = TestBackend::new(40, 7);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");

        terminal
            .draw(|frame| {
                composer.render(
                    frame,
                    Rect {
                        x: 0,
                        y: 0,
                        width: 40,
                        height: 7,
                    },
                    theme,
                    false,
                );
            })
            .expect("test draw should succeed");

        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer
                .cell((0, 0))
                .expect("left rail should be rendered")
                .symbol(),
            "▍",
            "the prompt should render a visually narrow left rail"
        );
        assert_eq!(
            buffer
                .cell((1, 1))
                .expect("left gap should be inside the prompt")
                .bg,
            theme.background_element,
            "the prompt should leave space between the rail and text"
        );
        assert_eq!(
            buffer
                .cell((10, 1))
                .expect("prompt content should be rendered")
                .bg,
            theme.background_element,
            "the prompt content should use the active theme background"
        );
        assert_eq!(
            buffer
                .cell((10, 0))
                .expect("top padding should be rendered")
                .bg,
            theme.background_element,
            "the prompt should have top padding"
        );
        assert_eq!(
            buffer
                .cell((10, 6))
                .expect("bottom padding should be rendered")
                .bg,
            theme.background_element,
            "the prompt should have bottom padding"
        );
        assert_eq!(
            buffer
                .cell((39, 1))
                .expect("right edge should be inside the prompt")
                .symbol(),
            " ",
            "the prompt should not render a right border"
        );
        assert!(
            buffer.content().iter().all(|cell| cell.symbol() != "P"),
            "the prompt title should not be rendered"
        );
    }

    #[test]
    fn paste_normalizes_all_line_endings() {
        let mut composer = Composer::new();

        assert!(composer.paste("first\r\nsecond\rthird"));

        assert_eq!(composer.text(), "first\nsecond\nthird");
    }

    #[test]
    fn enter_variants_submit_or_insert_newlines() {
        let mut composer = Composer::new();
        composer.paste("hello");

        assert_eq!(
            composer.handle_key(key(KeyCode::Enter, KeyModifiers::SHIFT)),
            ComposerAction::Changed
        );
        assert_eq!(
            composer.handle_key(key(KeyCode::Enter, KeyModifiers::CONTROL)),
            ComposerAction::Changed
        );
        assert_eq!(composer.text(), "hello\n\n");
        assert_eq!(
            composer.handle_key(key(KeyCode::Enter, KeyModifiers::ALT)),
            ComposerAction::Changed
        );
        assert_eq!(composer.text(), "hello\n\n\n");

        composer.set_text("send me");
        assert_eq!(
            composer.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            ComposerAction::Submit("send me".to_owned())
        );
    }

    #[test]
    fn selection_undo_and_redo_are_owned_by_the_composer() {
        let mut composer = Composer::new();
        composer.set_text("hello");

        assert_eq!(
            composer.handle_key(key(KeyCode::Char('a'), KeyModifiers::CONTROL)),
            ComposerAction::None
        );
        assert!(composer.paste("goodbye"));
        assert_eq!(composer.text(), "goodbye");

        composer.set_text("hello");
        composer.handle_key(key(KeyCode::End, KeyModifiers::NONE));
        assert!(composer.paste("!"));
        assert_eq!(composer.text(), "hello!");

        assert_eq!(
            composer.handle_key(key(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            ComposerAction::Changed
        );
        assert_eq!(composer.text(), "hello");
        assert_eq!(
            composer.handle_key(key(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            ComposerAction::Changed
        );
        assert_eq!(composer.text(), "hello!");
    }

    #[test]
    fn unicode_cursor_and_deletion_remain_on_text_boundaries() {
        let mut composer = Composer::new();
        composer.set_text("你好 world");

        composer.handle_key(key(KeyCode::End, KeyModifiers::NONE));
        composer.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));

        assert_eq!(composer.text(), "你好 worl");
        assert_eq!(composer.cursor(), (0, 7));
    }

    #[test]
    fn horizontal_movement_and_deletion_use_grapheme_boundaries() {
        let mut composer = Composer::new();
        composer.set_text("e\u{301}x");

        composer.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(composer.cursor(), (0, 2));
        composer.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(composer.cursor(), (0, 0));

        composer.handle_key(key(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(composer.cursor(), (0, 2));
        composer.handle_key(key(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(composer.text(), "e\u{301}");
        composer.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(composer.text(), "");
        assert_eq!(composer.cursor(), (0, 0));
    }

    #[test]
    fn shifted_horizontal_selection_does_not_split_a_grapheme() {
        let mut composer = Composer::new();
        composer.set_text("e\u{301}x");

        composer.handle_key(key(KeyCode::Left, KeyModifiers::SHIFT));
        assert_eq!(composer.cursor(), (0, 2));
        assert!(composer.paste("y"));
        assert_eq!(composer.text(), "e\u{301}y");
    }

    #[test]
    fn replaces_a_unicode_range_and_keeps_the_cursor_after_the_inserted_text() {
        let mut composer = Composer::new();
        composer.set_text("检查 @src");

        assert!(composer.replace_char_range(3, 7, "@src/main.rs "));
        assert_eq!(composer.text(), "检查 @src/main.rs ");
        assert_eq!(composer.cursor_offset(), 16);
    }

    #[test]
    fn vertical_movement_is_normalized_when_target_line_has_a_cluster() {
        let mut composer = Composer::new();
        composer.set_text("e\u{301}\nx");

        composer.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(composer.cursor(), (0, 0));
        composer.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(composer.cursor(), (1, 0));
    }
}
