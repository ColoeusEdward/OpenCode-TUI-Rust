use crate::markdown::{MarkdownRenderer, MarkdownTheme};
use crate::model::{MessageWithParts, Part};
use crate::scroll::ScrollAnchor;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;
use std::collections::HashSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A virtualized transcript view that only renders visible messages.
#[derive(Debug, Clone)]
pub struct TranscriptView {
    /// Pre-computed line ranges for each message
    message_line_ranges: Vec<MessageLineRange>,
    /// Total number of lines in the transcript
    total_lines: usize,
    /// Part IDs whose reasoning/tool details are hidden.
    collapsed_parts: HashSet<String>,
    theme: Theme,
}

#[derive(Debug, Clone)]
struct MessageLineRange {
    /// Index of the message in the transcript store
    message_index: usize,
    /// Start line (inclusive) in the global transcript
    start_line: usize,
    /// End line (exclusive) in the global transcript
    end_line: usize,
}

impl TranscriptView {
    /// Create a new virtualized view by computing line ranges for all messages.
    #[allow(dead_code)]
    pub fn new(messages: &[MessageWithParts], terminal_width: u16) -> Self {
        Self::with_theme(messages, terminal_width, &Theme::default())
    }

    pub fn with_theme(messages: &[MessageWithParts], terminal_width: u16, theme: &Theme) -> Self {
        Self::with_collapsed_theme(messages, terminal_width, &HashSet::new(), theme)
    }

    /// Create a view with selected reasoning/tool parts collapsed.
    #[allow(dead_code)]
    pub fn with_collapsed(
        messages: &[MessageWithParts],
        terminal_width: u16,
        collapsed_parts: &HashSet<String>,
    ) -> Self {
        Self::with_collapsed_theme(messages, terminal_width, collapsed_parts, &Theme::default())
    }

    pub fn with_collapsed_theme(
        messages: &[MessageWithParts],
        terminal_width: u16,
        collapsed_parts: &HashSet<String>,
        theme: &Theme,
    ) -> Self {
        let mut message_line_ranges = Vec::new();
        let mut current_line = 0;

        for (index, message) in messages.iter().enumerate() {
            let start_line = current_line;
            let lines = message_lines(message, terminal_width, collapsed_parts, theme);
            current_line += lines.len();

            message_line_ranges.push(MessageLineRange {
                message_index: index,
                start_line,
                end_line: current_line,
            });
        }

        Self {
            message_line_ranges,
            total_lines: current_line,
            collapsed_parts: collapsed_parts.clone(),
            theme: *theme,
        }
    }

    /// Get the total number of lines in the transcript.
    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    /// Render only the visible lines in the given range.
    pub fn render_lines(
        &self,
        messages: &[MessageWithParts],
        start_line: usize,
        end_line: usize,
        terminal_width: u16,
    ) -> Vec<Line<'static>> {
        let mut result = Vec::new();

        // Find messages that overlap with the visible range
        for range in &self.message_line_ranges {
            if range.end_line <= start_line {
                continue; // Message is above visible area
            }
            if range.start_line >= end_line {
                break; // Message is below visible area
            }

            // Render this message's lines
            let message = &messages[range.message_index];
            let all_lines =
                message_lines(message, terminal_width, &self.collapsed_parts, &self.theme);

            // Calculate which lines from this message are visible
            let message_start = start_line.saturating_sub(range.start_line);

            let message_end = if end_line < range.end_line {
                end_line - range.start_line
            } else {
                all_lines.len()
            };

            result.extend(all_lines[message_start..message_end].iter().cloned());
        }

        result
    }

    pub fn anchor_at_line(
        &self,
        messages: &[MessageWithParts],
        line: usize,
    ) -> Option<ScrollAnchor> {
        let range = self
            .message_line_ranges
            .iter()
            .find(|range| line >= range.start_line && line < range.end_line)?;
        Some(ScrollAnchor {
            message_id: messages.get(range.message_index)?.info.id.clone(),
            line_offset: line.saturating_sub(range.start_line),
        })
    }

    pub fn line_for_anchor(
        &self,
        messages: &[MessageWithParts],
        anchor: &ScrollAnchor,
    ) -> Option<usize> {
        self.message_line_ranges.iter().find_map(|range| {
            let message = messages.get(range.message_index)?;
            if message.info.id != anchor.message_id {
                return None;
            }
            let last_line = range
                .end_line
                .saturating_sub(range.start_line)
                .saturating_sub(1);
            Some(range.start_line + anchor.line_offset.min(last_line))
        })
    }

    /// Find the message index that contains the given line.
    #[allow(dead_code)]
    pub fn message_at_line(&self, line: usize) -> Option<usize> {
        self.message_line_ranges
            .iter()
            .find(|range| line >= range.start_line && line < range.end_line)
            .map(|range| range.message_index)
    }
}

/// Render all lines for a single message (used for line counting and rendering).
fn message_lines(
    message: &MessageWithParts,
    terminal_width: u16,
    collapsed_parts: &HashSet<String>,
    palette: &Theme,
) -> Vec<Line<'static>> {
    let theme = MarkdownTheme::from_theme(*palette);

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    let role = &message.info.role;
    let model = &message.info.model_id;
    let label = format!("[{role}  {model}]");
    lines.push(Line::from(Span::styled(
        label,
        Style::default()
            .fg(palette.text_muted)
            .add_modifier(Modifier::BOLD),
    )));

    for part in &message.parts {
        lines.extend(part_lines(part, &theme, collapsed_parts, palette));
    }

    // Wrap long lines to fit terminal width
    wrap_lines(lines, terminal_width)
}

/// Wrap lines that exceed terminal width into multiple lines.
fn wrap_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    if width == 0 {
        return lines;
    }

    let mut wrapped = Vec::new();
    for line in lines {
        let line_width = line.spans.iter().map(|s| s.content.width()).sum::<usize>();

        if line_width <= width as usize {
            wrapped.push(line);
        } else {
            // Line is too long, need to wrap
            let mut current_line_spans = Vec::new();
            let mut current_width = 0;

            for span in line.spans {
                let span_width = span.content.width();

                if current_width + span_width <= width as usize {
                    // Span fits in current line
                    current_line_spans.push(span);
                    current_width += span_width;
                } else if span_width > width as usize {
                    // Span itself is too long, need to split it
                    if !current_line_spans.is_empty() {
                        wrapped.push(Line::from(current_line_spans));
                        current_line_spans = Vec::new();
                        current_width = 0;
                    }

                    let mut remaining = span.content.as_ref();
                    let style = span.style;

                    while !remaining.is_empty() {
                        let mut take_len = 0;
                        let mut take_width = 0;

                        for (idx, ch) in remaining.char_indices() {
                            let ch_width = ch.width().unwrap_or(0);
                            if take_width + ch_width > width as usize {
                                break;
                            }
                            take_len = idx + ch.len_utf8();
                            take_width += ch_width;
                        }

                        if take_len == 0 {
                            // Can't fit even one character, force take one
                            if let Some(ch) = remaining.chars().next() {
                                take_len = ch.len_utf8();
                            } else {
                                break;
                            }
                        }

                        let chunk = &remaining[..take_len];
                        wrapped.push(Line::from(Span::styled(chunk.to_string(), style)));
                        remaining = &remaining[take_len..];
                    }
                } else {
                    // Start a new line with this span
                    if !current_line_spans.is_empty() {
                        wrapped.push(Line::from(current_line_spans));
                    }
                    current_line_spans = vec![span];
                    current_width = span_width;
                }
            }

            if !current_line_spans.is_empty() {
                wrapped.push(Line::from(current_line_spans));
            }
        }
    }

    wrapped
}

fn part_lines(
    part: &Part,
    theme: &MarkdownTheme,
    collapsed_parts: &HashSet<String>,
    palette: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let collapsed = !part.id.is_empty() && collapsed_parts.contains(&part.id);

    match part.kind.as_str() {
        "text" => {
            if let Some(text) = &part.text {
                lines.extend(MarkdownRenderer::render(text, *theme));
            }
        }
        "reasoning" => {
            lines.push(Line::from(Span::styled(
                if collapsed {
                    "  [thinking] [collapsed]"
                } else {
                    "  [thinking]"
                },
                Style::default().fg(palette.text_muted),
            )));
            if !collapsed && let Some(text) = &part.text {
                for line in text.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  {line}"),
                        Style::default().fg(palette.text_muted),
                    )));
                }
            }
        }
        "compaction" => {
            lines.extend(compaction_lines(part, palette));
        }
        "tool" => {
            lines.extend(tool_lines(part, collapsed, palette));
        }
        "shell" => {
            lines.extend(shell_lines(part, palette));
        }
        _ => {}
    }

    lines
}

fn compaction_lines(part: &Part, palette: &Theme) -> Vec<Line<'static>> {
    let state = part.state.as_ref();
    let status = state
        .and_then(|state| state.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let reason = state
        .and_then(|state| state.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let status_color = if status == "completed" {
        palette.success
    } else {
        palette.warning
    };
    let title = if status == "completed" {
        "  [context compacted]"
    } else {
        "  [context compaction]"
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(title, Style::default().fg(status_color)),
        Span::styled(
            format!("  ({reason})"),
            Style::default().fg(palette.text_muted),
        ),
    ])];
    if let Some(text) = part.text.as_deref().filter(|text| !text.is_empty()) {
        for line in text.lines() {
            lines.push(Line::from(Span::styled(
                format!("    {line}"),
                Style::default().fg(palette.text),
            )));
        }
    }
    if let Some(recent) = state
        .and_then(|state| state.get("recent"))
        .and_then(Value::as_str)
        .filter(|recent| !recent.is_empty())
    {
        lines.push(Line::from(Span::styled(
            format!("  recent: {recent}"),
            Style::default().fg(palette.text_muted),
        )));
    }
    lines
}

fn tool_lines(part: &Part, collapsed: bool, palette: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let tool_name = part.tool.as_deref().unwrap_or("tool");
    let state = part.state.as_ref();
    let status = state
        .and_then(|s| s.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let status_color = match status {
        "completed" => palette.success,
        "failed" | "error" => palette.error,
        "running" => palette.warning,
        _ => palette.text_muted,
    };

    lines.push(Line::from(vec![
        Span::styled(
            "  tool: ".to_string(),
            Style::default().fg(palette.text_muted),
        ),
        Span::styled(tool_name.to_string(), Style::default().fg(palette.text)),
        Span::styled(format!("  [{status}]"), Style::default().fg(status_color)),
        Span::styled(
            if collapsed { "  [collapsed]" } else { "" },
            Style::default().fg(palette.text_muted),
        ),
    ]));

    if collapsed {
        return lines;
    }

    // Show input
    if let Some(state) = state {
        if let Some(input) = state.get("input") {
            // Check if input is a simple string or an object
            if let Some(input_str) = input.as_str() {
                if !input_str.is_empty() && input_str.len() < 200 {
                    lines.push(Line::from(Span::styled(
                        format!("    input: {input_str}"),
                        Style::default().fg(palette.markdown_code_block),
                    )));
                }
            } else if input.is_object() {
                // Show "input" label for object inputs (like command objects)
                lines.push(Line::from(Span::styled(
                    "    input".to_string(),
                    Style::default().fg(palette.markdown_code_block),
                )));
            }
        }

        // Show content array items
        if let Some(content_array) = state.get("content").and_then(Value::as_array) {
            for item in content_array {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    lines.push(Line::from(Span::styled(
                        format!("    {text}"),
                        Style::default().fg(palette.text_muted),
                    )));
                } else if let Some(uri) = item.get("uri").and_then(Value::as_str) {
                    lines.push(Line::from(Span::styled(
                        format!("    {uri}"),
                        Style::default().fg(palette.text_muted),
                    )));
                }
            }
        }

        // Show result from state if available
        if let Some(result) = state.get("result") {
            if let Some(result_str) = result.as_str() {
                if !result_str.is_empty() && result_str.len() < 500 {
                    lines.push(Line::from(Span::styled(
                        format!("    result: {result_str}"),
                        Style::default().fg(palette.text_muted),
                    )));
                }
            } else {
                // Show "result" label for non-string results
                lines.push(Line::from(Span::styled(
                    "    result".to_string(),
                    Style::default().fg(palette.text_muted),
                )));
            }
        }
    }

    lines
}

fn shell_lines(part: &Part, palette: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let command = part.command.as_deref().unwrap_or("shell");
    let state = part.state.as_ref();
    let status = state
        .and_then(|s| s.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let status_color = match status {
        "completed" => palette.success,
        "running" => palette.warning,
        _ => palette.text_muted,
    };

    // Format: "  [status] $ command"
    lines.push(Line::from(vec![
        Span::styled(
            format!("  [{status}] $ "),
            Style::default().fg(status_color),
        ),
        Span::styled(
            command.to_string(),
            Style::default().fg(palette.markdown_code_block),
        ),
    ]));

    if let Some(text) = &part.text
        && !text.is_empty()
    {
        for line in text.lines().take(20) {
            lines.push(Line::from(Span::styled(
                format!("    {line}"),
                Style::default().fg(palette.text_muted),
            )));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MessageInfo, MessageTime, MessageWithParts, Part};
    use std::collections::HashSet;

    fn make_message(id: &str, role: &str, text: &str) -> MessageWithParts {
        MessageWithParts {
            info: MessageInfo {
                id: id.to_string(),
                session_id: "session".to_string(),
                role: role.to_string(),
                time: MessageTime::default(),
                ..MessageInfo::default()
            },
            parts: vec![Part {
                id: format!("{id}-part"),
                session_id: "session".to_string(),
                message_id: id.to_string(),
                kind: "text".to_string(),
                text: Some(text.to_string()),
                ..Part::default()
            }],
        }
    }

    #[test]
    fn empty_transcript_has_zero_lines() {
        let view = TranscriptView::new(&[], 80);
        assert_eq!(view.total_lines(), 0);
    }

    #[test]
    fn computes_line_ranges_for_messages() {
        let messages = vec![
            make_message("m1", "user", "Hello"),
            make_message("m2", "assistant", "Hi there"),
        ];

        let view = TranscriptView::new(&messages, 80);
        assert!(view.total_lines() > 0);
        assert_eq!(view.message_line_ranges.len(), 2);
    }

    #[test]
    fn renders_only_visible_lines() {
        let messages = vec![
            make_message("m1", "user", "First"),
            make_message("m2", "assistant", "Second"),
            make_message("m3", "user", "Third"),
        ];

        let view = TranscriptView::new(&messages, 80);
        let total = view.total_lines();

        // Render first half
        let lines = view.render_lines(&messages, 0, total / 2, 80);
        assert!(!lines.is_empty());
        assert!(lines.len() <= total / 2);

        // Render second half
        let lines = view.render_lines(&messages, total / 2, total, 80);
        assert!(!lines.is_empty());
    }

    #[test]
    fn finds_message_at_line() {
        let messages = vec![
            make_message("m1", "user", "First"),
            make_message("m2", "assistant", "Second"),
        ];

        let view = TranscriptView::new(&messages, 80);

        // First message should be at line 0
        assert_eq!(view.message_at_line(0), Some(0));

        // Last line should have a message
        if view.total_lines() > 0 {
            let last_msg = view.message_at_line(view.total_lines() - 1);
            assert!(last_msg.is_some());
        }
    }

    #[test]
    fn collapsed_reasoning_and_tool_parts_reduce_height_and_keep_headers() {
        let messages = vec![MessageWithParts {
            info: MessageInfo {
                id: "m1".to_owned(),
                session_id: "session".to_owned(),
                role: "assistant".to_owned(),
                time: MessageTime::default(),
                ..MessageInfo::default()
            },
            parts: vec![
                Part {
                    id: "reasoning-1".to_owned(),
                    message_id: "m1".to_owned(),
                    kind: "reasoning".to_owned(),
                    text: Some("private thought\nmore thought".to_owned()),
                    ..Part::default()
                },
                Part {
                    id: "tool-1".to_owned(),
                    message_id: "m1".to_owned(),
                    kind: "tool".to_owned(),
                    tool: Some("bash".to_owned()),
                    state: Some(serde_json::json!({
                        "status": "completed",
                        "input": "private input",
                        "result": "private result"
                    })),
                    ..Part::default()
                },
            ],
        }];
        let expanded = TranscriptView::new(&messages, 80);
        let collapsed_parts = HashSet::from(["reasoning-1".to_owned(), "tool-1".to_owned()]);
        let collapsed = TranscriptView::with_collapsed(&messages, 80, &collapsed_parts);

        assert!(collapsed.total_lines() < expanded.total_lines());
        let rendered = collapsed
            .render_lines(&messages, 0, collapsed.total_lines(), 80)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|line| line.contains("[thinking] [collapsed]"))
        );
        assert!(
            rendered
                .iter()
                .any(|line| line.contains("tool: bash") && line.contains("[collapsed]"))
        );
        assert!(!rendered.iter().any(|line| line.contains("private thought")));
        assert!(!rendered.iter().any(|line| line.contains("private result")));
    }

    #[test]
    fn anchor_resolves_against_message_ids_after_previous_content_grows() {
        let messages = vec![
            make_message("m1", "assistant", "first"),
            make_message("m2", "assistant", "second"),
        ];
        let view = TranscriptView::new(&messages, 80);
        let second_start = view.message_line_ranges[1].start_line;
        let anchor = view
            .anchor_at_line(&messages, second_start)
            .expect("second message should have an anchor");

        let grown_messages = vec![
            make_message("m1", "assistant", "first\nmore\ncontent"),
            make_message("m2", "assistant", "second"),
        ];
        let grown_view = TranscriptView::new(&grown_messages, 80);
        let restored = grown_view
            .line_for_anchor(&grown_messages, &anchor)
            .expect("anchor should resolve after content changes");

        assert_eq!(anchor.message_id, "m2");
        assert_eq!(anchor.line_offset, 0);
        assert!(restored > second_start);
        assert_eq!(
            grown_view
                .anchor_at_line(&grown_messages, restored)
                .expect("restored line should have an anchor")
                .message_id,
            "m2"
        );
    }

    #[test]
    fn renders_compaction_summary_and_recent_context() {
        let messages = vec![MessageWithParts {
            info: MessageInfo {
                id: "cmp_1".to_owned(),
                session_id: "session".to_owned(),
                role: "system".to_owned(),
                ..MessageInfo::default()
            },
            parts: vec![Part {
                id: "cmp_1".to_owned(),
                message_id: "cmp_1".to_owned(),
                kind: "compaction".to_owned(),
                text: Some("Summary".to_owned()),
                state: Some(serde_json::json!({
                    "status": "completed",
                    "reason": "manual",
                    "recent": "Recent context"
                })),
                ..Part::default()
            }],
        }];
        let view = TranscriptView::new(&messages, 80);
        let text = view
            .render_lines(&messages, 0, view.total_lines(), 80)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("[context compacted]  (manual)"));
        assert!(text.contains("Summary"));
        assert!(text.contains("recent: Recent context"));
    }
}
