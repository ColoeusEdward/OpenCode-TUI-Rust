use crate::theme::Theme;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy)]
pub struct MarkdownTheme {
    pub body: Color,
    pub heading: Color,
    pub muted: Color,
    pub code: Color,
    pub code_keyword: Color,
    pub code_string: Color,
    pub code_comment: Color,
    pub code_number: Color,
    pub marker: Color,
}

impl MarkdownTheme {
    pub fn from_theme(theme: Theme) -> Self {
        Self {
            body: theme.markdown_text,
            heading: theme.markdown_heading,
            muted: theme.text_muted,
            code: theme.markdown_code_block,
            code_keyword: theme.syntax_keyword,
            code_string: theme.syntax_string,
            code_comment: theme.syntax_comment,
            code_number: theme.syntax_number,
            marker: theme.markdown_list_item,
        }
    }

    pub fn with_body(self, body: Color) -> Self {
        Self { body, ..self }
    }
}

pub struct MarkdownRenderer;

impl MarkdownRenderer {
    pub fn render(text: &str, theme: MarkdownTheme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut in_code = false;
        let mut code_language = String::new();

        for raw_line in expand_tables(text) {
            let trimmed = raw_line.trim();
            if let Some(fence) = trimmed.strip_prefix("```") {
                if in_code {
                    lines.push(Line::from(Span::styled(
                        "  [/code]",
                        Style::default().fg(theme.muted),
                    )));
                    in_code = false;
                    code_language.clear();
                } else {
                    code_language = fence.trim().to_owned();
                    let label = if code_language.is_empty() {
                        "  [code]".to_owned()
                    } else {
                        format!("  [code: {}]", code_language)
                    };
                    lines.push(Line::from(Span::styled(
                        label,
                        Style::default().fg(theme.muted),
                    )));
                    in_code = true;
                }
                continue;
            }

            if in_code {
                lines.push(highlight_code_line(&raw_line, &code_language, theme));
                continue;
            }

            if trimmed.is_empty() {
                lines.push(Line::from(""));
            } else if let Some(content) = heading_content(trimmed) {
                lines.push(Line::from(inline_spans(
                    &format!("  {content}"),
                    theme,
                    Style::default()
                        .fg(theme.heading)
                        .add_modifier(Modifier::BOLD),
                )));
            } else if let Some((prefix, content)) = unordered_item(&raw_line) {
                let mut spans = vec![Span::styled(prefix, Style::default().fg(theme.marker))];
                spans.extend(inline_spans(
                    content,
                    theme,
                    Style::default().fg(theme.body),
                ));
                lines.push(Line::from(spans));
            } else if let Some((prefix, content)) = ordered_item(&raw_line) {
                let mut spans = vec![Span::styled(prefix, Style::default().fg(theme.marker))];
                spans.extend(inline_spans(
                    content,
                    theme,
                    Style::default().fg(theme.body),
                ));
                lines.push(Line::from(spans));
            } else if let Some(content) = trimmed.strip_prefix('>') {
                let mut spans = vec![Span::styled("  | ", Style::default().fg(theme.muted))];
                spans.extend(inline_spans(
                    content.trim_start(),
                    theme,
                    Style::default().fg(theme.muted),
                ));
                lines.push(Line::from(spans));
            } else if is_rule(trimmed) {
                lines.push(Line::from(Span::styled(
                    "  ----------------",
                    Style::default().fg(theme.muted),
                )));
            } else {
                let mut spans = vec![Span::styled("  ", Style::default().fg(theme.body))];
                spans.extend(inline_spans(
                    raw_line.trim_end(),
                    theme,
                    Style::default().fg(theme.body),
                ));
                lines.push(Line::from(spans));
            }
        }

        lines
    }
}

fn highlight_code_line(line: &str, language: &str, theme: MarkdownTheme) -> Line<'static> {
    let language = language
        .split(|character: char| character.is_whitespace() || character == ',')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let characters = line.char_indices().collect::<Vec<_>>();
    let mut spans = vec![Span::styled("    ", Style::default().fg(theme.code))];
    let mut index = 0;

    while index < characters.len() {
        let start = characters[index].0;
        let character = characters[index].1;

        if is_comment_start(&line[start..], &language) {
            push_span(
                &mut spans,
                &line[start..],
                Style::default().fg(theme.code_comment),
            );
            break;
        }

        if matches!(character, '"' | '\'' | '`') {
            let quote = character;
            let mut end = index + 1;
            let mut escaped = false;
            while end < characters.len() {
                let current = characters[end].1;
                if escaped {
                    escaped = false;
                } else if current == '\\' {
                    escaped = true;
                } else if current == quote {
                    end += 1;
                    break;
                }
                end += 1;
            }
            let end_byte = characters.get(end).map_or(line.len(), |entry| entry.0);
            push_span(
                &mut spans,
                &line[start..end_byte],
                Style::default().fg(theme.code_string),
            );
            index = end;
            continue;
        }

        if character.is_ascii_digit() {
            let mut end = index + 1;
            while end < characters.len()
                && (characters[end].1.is_ascii_alphanumeric()
                    || matches!(characters[end].1, '.' | '_'))
            {
                end += 1;
            }
            let end_byte = characters.get(end).map_or(line.len(), |entry| entry.0);
            push_span(
                &mut spans,
                &line[start..end_byte],
                Style::default().fg(theme.code_number),
            );
            index = end;
            continue;
        }

        if is_identifier_start(character) {
            let mut end = index + 1;
            while end < characters.len() && is_identifier_continue(characters[end].1) {
                end += 1;
            }
            let end_byte = characters.get(end).map_or(line.len(), |entry| entry.0);
            let word = &line[start..end_byte];
            let color = if is_keyword(&language, word) {
                theme.code_keyword
            } else {
                theme.code
            };
            push_span(&mut spans, word, Style::default().fg(color));
            index = end;
            continue;
        }

        let end_byte = characters
            .get(index + 1)
            .map_or(line.len(), |entry| entry.0);
        push_span(
            &mut spans,
            &line[start..end_byte],
            Style::default().fg(theme.code),
        );
        index += 1;
    }

    Line::from(spans)
}

fn is_comment_start(rest: &str, language: &str) -> bool {
    rest.starts_with("//")
        || (rest.starts_with('#')
            && matches!(
                language,
                "" | "py" | "python" | "rb" | "ruby" | "sh" | "bash" | "shell" | "yaml" | "yml"
            ))
        || (rest.starts_with("--") && matches!(language, "sql" | "lua"))
}

fn is_identifier_start(character: char) -> bool {
    character == '_' || character.is_alphabetic()
}

fn is_identifier_continue(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

fn is_keyword(language: &str, word: &str) -> bool {
    let keywords: &[&str] = match language {
        "rs" | "rust" => &[
            "as", "async", "await", "const", "crate", "else", "enum", "fn", "for", "if", "impl",
            "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self",
            "Self", "static", "struct", "trait", "type", "use", "where", "while", "true", "false",
        ],
        "py" | "python" => &[
            "and", "as", "assert", "async", "await", "class", "def", "elif", "else", "False",
            "for", "from", "if", "import", "in", "is", "lambda", "None", "not", "or", "pass",
            "raise", "return", "True", "try", "while", "with", "yield",
        ],
        "js" | "jsx" | "javascript" | "ts" | "tsx" | "typescript" => &[
            "as", "async", "await", "class", "const", "default", "else", "export", "extends",
            "false", "for", "from", "function", "if", "import", "in", "let", "new", "null", "of",
            "return", "this", "throw", "true", "try", "type", "var", "while",
        ],
        _ => &[
            "as", "class", "const", "def", "else", "false", "for", "from", "function", "if",
            "import", "in", "let", "new", "null", "return", "true", "try", "type", "var", "while",
        ],
    };
    keywords.contains(&word)
}

fn heading_content(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ') {
        Some(line[hashes + 1..].trim())
    } else {
        None
    }
}

fn expand_tables(text: &str) -> Vec<String> {
    let source = text.split('\n').collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut index = 0;
    let mut in_code = false;

    while index < source.len() {
        let line = source[index];
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            output.push(line.to_owned());
            in_code = !in_code;
            index += 1;
            continue;
        }

        if !in_code
            && let (Some(header), Some(separator)) = (
                source.get(index).and_then(|line| table_cells(line)),
                source.get(index + 1).and_then(|line| table_cells(line)),
            )
            && is_table_separator(&separator)
        {
            let mut rows = vec![header, separator];
            index += 2;
            while let Some(row) = source.get(index).and_then(|line| table_cells(line)) {
                rows.push(row);
                index += 1;
            }
            output.extend(render_table(rows));
            continue;
        }

        output.push(line.to_owned());
        index += 1;
    }

    output
}

fn table_cells(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return None;
    }
    let content = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or_else(|| trimmed.strip_prefix('|').unwrap_or(trimmed));
    let cells = content
        .split('|')
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    (cells.len() >= 2).then_some(cells)
}

fn is_table_separator(cells: &[String]) -> bool {
    cells.iter().all(|cell| {
        let cell = cell.trim();
        let cell = cell.strip_prefix(':').unwrap_or(cell);
        let cell = cell.strip_suffix(':').unwrap_or(cell);
        cell.len() >= 3 && cell.chars().all(|character| character == '-')
    })
}

fn render_table(rows: Vec<Vec<String>>) -> Vec<String> {
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths = (0..column_count)
        .map(|column| {
            rows.iter()
                .enumerate()
                .filter_map(|(row, cells)| {
                    if row == 1 {
                        None
                    } else {
                        cells.get(column).map(|cell| table_cell_width(cell))
                    }
                })
                .max()
                .unwrap_or(3)
                .max(3)
        })
        .collect::<Vec<_>>();

    rows.into_iter()
        .enumerate()
        .map(|(row, cells)| {
            let values = (0..column_count)
                .map(|column| {
                    if row == 1 {
                        "-".repeat(widths[column])
                    } else {
                        let value = cells.get(column).map(String::as_str).unwrap_or("");
                        format!(
                            "{value}{}",
                            " ".repeat(widths[column].saturating_sub(table_cell_width(value)))
                        )
                    }
                })
                .collect::<Vec<_>>();
            format!("| {} |", values.join(" | "))
        })
        .collect()
}

fn table_cell_width(value: &str) -> usize {
    value
        .replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .chars()
        .count()
}

fn unordered_item(line: &str) -> Option<(String, &str)> {
    let leading = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if !matches!(marker, '-' | '*' | '+') || !trimmed[1..].starts_with(' ') {
        return None;
    }
    let prefix = format!("  {}{} ", " ".repeat(leading), marker);
    Some((prefix, trimmed[2..].trim_end()))
}

fn ordered_item(line: &str) -> Option<(String, &str)> {
    let leading = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    let dot = trimmed.find(". ")?;
    if dot == 0
        || !trimmed[..dot]
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return None;
    }
    let prefix = format!("  {}{} ", " ".repeat(leading), &trimmed[..=dot]);
    Some((prefix, trimmed[dot + 2..].trim_end()))
}

fn is_rule(line: &str) -> bool {
    line.len() >= 3
        && line
            .chars()
            .all(|character| matches!(character, '-' | '_' | '*'))
}

fn inline_spans(text: &str, theme: MarkdownTheme, body_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = text;

    while !rest.is_empty() {
        let code_start = rest.find('`');
        let strong_start = rest.find("**");
        let emphasis_start = rest.find("__");
        let next = [code_start, strong_start, emphasis_start]
            .into_iter()
            .flatten()
            .min();

        let Some(start) = next else {
            push_span(&mut spans, rest, body_style);
            break;
        };
        if start > 0 {
            push_span(&mut spans, &rest[..start], body_style);
            rest = &rest[start..];
            continue;
        }

        if rest.starts_with('`') {
            if let Some(end) = rest[1..].find('`') {
                push_span(
                    &mut spans,
                    &rest[1..end + 1],
                    Style::default().fg(theme.code),
                );
                rest = &rest[end + 2..];
                continue;
            }
        } else if rest.starts_with("**") || rest.starts_with("__") {
            let marker = &rest[..2];
            if let Some(end) = rest[2..].find(marker) {
                push_span(
                    &mut spans,
                    &rest[2..end + 2],
                    body_style.add_modifier(Modifier::BOLD),
                );
                rest = &rest[end + 4..];
                continue;
            }
        }

        let character = rest.chars().next().expect("non-empty text has a character");
        push_span(&mut spans, &character.to_string(), body_style);
        rest = &rest[character.len_utf8()..];
    }

    spans
}

fn push_span(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if !text.is_empty() {
        spans.push(Span::styled(text.to_owned(), style));
    }
}

#[cfg(test)]
mod tests {
    use super::{MarkdownRenderer, MarkdownTheme};
    use ratatui::style::Color;
    use ratatui::text::Line;

    fn theme() -> MarkdownTheme {
        MarkdownTheme {
            body: Color::White,
            heading: Color::Cyan,
            muted: Color::Gray,
            code: Color::Yellow,
            code_keyword: Color::Cyan,
            code_string: Color::Green,
            code_comment: Color::DarkGray,
            code_number: Color::Magenta,
            marker: Color::Green,
        }
    }

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.to_string())
            .collect()
    }

    #[test]
    fn renders_markdown_blocks_and_inline_styles_as_stable_lines() {
        let lines = MarkdownRenderer::render(
            "# Title\n- one `inline`\n1. two\n> quote\n```rust\nfn main() {}\n```",
            theme(),
        );
        let text = lines.iter().map(line_text).collect::<Vec<_>>();

        assert_eq!(text[0], "  Title");
        assert_eq!(text[1], "  - one inline");
        assert_eq!(text[2], "  1. two");
        assert_eq!(text[3], "  | quote");
        assert_eq!(text[4], "  [code: rust]");
        assert_eq!(text[5], "    fn main() {}");
        assert_eq!(text[6], "  [/code]");
    }

    #[test]
    fn preserves_empty_lines_and_unclosed_inline_markers() {
        let lines = MarkdownRenderer::render("first\n\nsecond `unfinished", theme());
        let text = lines.iter().map(line_text).collect::<Vec<_>>();

        assert_eq!(text, vec!["  first", "", "  second `unfinished"]);
    }

    #[test]
    fn highlights_common_code_tokens_without_changing_code_text() {
        let lines = MarkdownRenderer::render(
            "```rust\nfn main() { let count = 42; // note\n println!(\"ok\");\n```",
            theme(),
        );

        assert_eq!(
            line_text(&lines[1]),
            "    fn main() { let count = 42; // note"
        );
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|span| span.content == "fn" && span.style.fg == Some(Color::Cyan))
        );
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|span| span.content == "42" && span.style.fg == Some(Color::Magenta))
        );
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|span| span.content == "// note" && span.style.fg == Some(Color::DarkGray))
        );
        assert!(
            lines[2]
                .spans
                .iter()
                .any(|span| span.content == "\"ok\"" && span.style.fg == Some(Color::Green))
        );
    }

    #[test]
    fn renders_gfm_tables_as_aligned_lines_without_parsing_code_fences() {
        let lines = MarkdownRenderer::render(
            "| Name | Status |\n| --- | :---: |\n| api | **ready** |\n\n```\n| not | a table |\n```",
            theme(),
        );
        let text = lines.iter().map(line_text).collect::<Vec<_>>();

        assert_eq!(
            text,
            vec![
                "  | Name | Status |",
                "  | ---- | ------ |",
                "  | api  | ready  |",
                "",
                "  [code]",
                "    | not | a table |",
                "  [/code]",
            ]
        );
    }
}
