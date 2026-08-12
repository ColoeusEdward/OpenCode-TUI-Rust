use crate::app::{App, Screen};
use crate::command::{matching_commands_with_server, slash_query};
use crate::command_palette::filter_commands;
use crate::dialog::{
    OverlayState, agent_options, model_options, skill_options, tool_override_names, variant_options,
};
use crate::markdown::{MarkdownRenderer, MarkdownTheme};
use crate::mention::{MentionKind, mention_options};
use crate::model::{
    FileDiff, MessageInfo, MessageWithParts, Part, PromptPart, Session, VcsFileDiff,
};
use crate::notification_state::NotificationLevel;
use crate::selection::SelectionPane;
use crate::theme::Theme;
use crate::transcript_view::TranscriptView;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let theme = app.theme;
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        frame.area(),
    );
    // Pane geometry changes between frames, so selectable panes re-register every
    // render rather than being patched in place.
    app.selection.begin_frame();
    match app.session.screen {
        Screen::Home => {
            app.sidebar_area = None;
            draw_home(frame, app, theme);
        }
        Screen::Session => draw_session(frame, app, theme),
    }
    // Painted after the panes so it covers their content, but before overlays so a
    // dialog is never tinted by a selection underneath it.
    draw_selection_highlight(frame, app, theme);
    draw_overlay(frame, app, theme);
}

/// Flattens a styled line back to plain text for selection. Copying should yield
/// what the user sees, not the styling used to draw it.
fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

/// Repaints the selected cells with the selection colors. Working on the finished
/// buffer keeps every pane's own rendering untouched, so a pane does not need to
/// know whether part of it is selected.
fn draw_selection_highlight(frame: &mut Frame<'_>, app: &App, theme: Theme) {
    let spans = app.selection.highlight_spans();
    if spans.is_empty() {
        return;
    }
    let area = frame.area();
    let buffer = frame.buffer_mut();
    for span in spans {
        if span.row < area.y || span.row >= area.bottom() {
            continue;
        }
        for column in span.start_column..span.end_column.min(area.right()) {
            if column < area.x {
                continue;
            }
            let cell = &mut buffer[(column, span.row)];
            cell.set_bg(theme.selection_background);
            cell.set_fg(theme.selection_text);
        }
    }
}

fn draw_home(frame: &mut Frame<'_>, app: &App, theme: Theme) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(2),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "OpenCode",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  Rust TUI", Style::default().fg(theme.accent)),
        Span::styled(
            format!("  [{}]", current_directory(app)),
            Style::default().fg(theme.text_muted),
        ),
        Span::styled(
            format!("  {}", app.connection_label()),
            Style::default().fg(connection_color(app, theme)),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(theme.border)),
    );
    frame.render_widget(header, layout[0]);

    let items = app
        .session
        .sessions
        .iter()
        .map(|session| {
            let markers = match (
                session.parent_id.is_some(),
                session.share_url().is_some(),
                session.time.archived.is_some(),
            ) {
                (true, true, true) => "[fork] [shared] [archived] ",
                (true, true, false) => "[fork] [shared] ",
                (true, false, true) => "[fork] [archived] ",
                (true, false, false) => "[fork] ",
                (false, true, true) => "[shared] [archived] ",
                (false, true, false) => "[shared] ",
                (false, false, true) => "[archived] ",
                (false, false, false) => "",
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{markers}{}", session.display_title()),
                    Style::default().fg(theme.text),
                ),
                Span::styled(
                    format!("  {}", short_id(&session.id)),
                    Style::default().fg(theme.text_muted),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let list = if app.session.sessions.is_empty() {
        List::new(vec![ListItem::new(Line::from(Span::styled(
            if app.session.show_archived {
                "No archived sessions found. Press A for active sessions."
            } else {
                "No sessions found. Press n to create one."
            },
            Style::default().fg(theme.text_muted),
        )))])
    } else {
        List::new(items)
    }
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(if app.session.show_archived {
                "Archived sessions"
            } else {
                "Sessions"
            }),
    )
    .highlight_style(
        Style::default()
            .bg(theme.background_element)
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("  ");
    let mut state = ListState::default();
    if !app.session.sessions.is_empty() {
        state.select(Some(
            app.session
                .selected_session
                .min(app.session.sessions.len() - 1),
        ));
    }
    frame.render_stateful_widget(list, layout[1], &mut state);
    draw_footer(frame, app, layout[2], "? shortcuts", theme);
}

fn draw_session(frame: &mut Frame<'_>, app: &mut App, theme: Theme) {
    if !app.runtime.sidebar_visible {
        app.sidebar_area = None;
        draw_transcript(frame, app, frame.area(), theme);
        return;
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(74), Constraint::Percentage(26)])
        .split(frame.area());
    app.sidebar_area = Some(columns[1]);
    draw_transcript(frame, app, columns[0], theme);
    draw_sidebar(frame, app, columns[1], theme);
}

fn draw_overlay(frame: &mut Frame<'_>, app: &App, theme: Theme) {
    let Some(overlay) = app.overlay.as_ref() else {
        return;
    };
    match overlay {
        OverlayState::Slash { selected } => {
            let text = app.prompt.composer.text();
            let query = slash_query(&text).unwrap_or_default();
            let options = matching_commands_with_server(query, &app.catalog.commands);
            let area = dialog_area(frame.area(), 64, dialog_height(options.len(), 0));
            frame.render_widget(Clear, area);
            let items = if options.is_empty() {
                vec![ListItem::new(Line::from(Span::styled(
                    "No matching commands",
                    Style::default().fg(theme.text_muted),
                )))]
            } else {
                options
                    .iter()
                    .map(|option| {
                        ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("/{:<8}", option.name()),
                                Style::default().fg(theme.text),
                            ),
                            Span::styled(
                                option.description(),
                                Style::default().fg(theme.text_muted),
                            ),
                        ]))
                    })
                    .collect()
            };
            let list = List::new(items)
                .block(dialog_block("Commands", theme))
                .highlight_style(purple_selection_style(theme))
                .highlight_symbol("> ");
            let mut state = ListState::default();
            if !options.is_empty() {
                state.select(Some((*selected).min(options.len() - 1)));
            }
            frame.render_stateful_widget(list, area, &mut state);
        }
        OverlayState::Model { query, selected } => {
            let options = model_options(&app.catalog.providers, &app.catalog.recent_models, query);
            draw_catalog_dialog_with_selection_style(
                frame,
                "Select model",
                query,
                *selected,
                theme,
                options
                    .iter()
                    .map(|option| {
                        let current = app.catalog.selected_model.as_ref().is_some_and(|model| {
                            model.provider_id == option.provider_id && model.id == option.model_id
                        });
                        let marker = if current { "*" } else { " " };
                        let value = format!(
                            "{marker} {}/{}  {}  ctx {}",
                            option.provider_id,
                            option.model_id,
                            option.model_name,
                            format_count(option.context_limit)
                        );
                        (value, option.provider_name.clone())
                    })
                    .collect(),
                purple_selection_style(theme),
            );
        }
        OverlayState::Skill { query, selected } => {
            let options = skill_options(&app.catalog.skills, query);
            draw_catalog_dialog(
                frame,
                "Select skill",
                query,
                *selected,
                theme,
                options
                    .iter()
                    .map(|option| (option.name.clone(), option.description.clone()))
                    .collect(),
            );
        }
        OverlayState::Agent { query, selected } => {
            let options = agent_options(&app.catalog.agents, query);
            draw_catalog_dialog(
                frame,
                "Select agent",
                query,
                *selected,
                theme,
                options
                    .iter()
                    .map(|option| {
                        let marker = if app.catalog.selected_agent.as_deref()
                            == Some(option.name.as_str())
                        {
                            "*"
                        } else {
                            " "
                        };
                        let source = if option.native {
                            "native"
                        } else {
                            "configured"
                        };
                        (
                            format!("{marker} {}", option.name),
                            if option.description.is_empty() {
                                source.to_owned()
                            } else {
                                format!("{}  {}", source, option.description)
                            },
                        )
                    })
                    .collect(),
            );
        }
        OverlayState::Variant { query, selected } => {
            let model = app.active_model_ref();
            let current_variant = model
                .as_ref()
                .and_then(|model| model.variant.as_deref())
                .unwrap_or("default");
            let options = variant_options(&app.catalog.providers, model.as_ref(), query);
            draw_catalog_dialog(
                frame,
                "Select variant",
                query,
                *selected,
                theme,
                options
                    .iter()
                    .map(|option| {
                        let marker = if current_variant == option.name.as_str() {
                            "*"
                        } else {
                            " "
                        };
                        (format!("{marker} {}", option.name), String::new())
                    })
                    .collect(),
            );
        }
        OverlayState::Mcp { selected } => {
            let entries = app
                .integrations
                .mcp
                .iter()
                .map(|server| {
                    let state =
                        if app.integrations.mcp_action.as_deref() == Some(server.name.as_str()) {
                            "working...".to_owned()
                        } else if let Some(error) = server.error.as_deref() {
                            format!("{}  {error}", server.status)
                        } else {
                            server.status.clone()
                        };
                    (server.name.clone(), state)
                })
                .collect();
            draw_catalog_dialog(frame, "MCP servers", "", *selected, theme, entries);
        }
        OverlayState::CommandPalette { query, selected } => {
            let options = filter_commands(query);
            draw_catalog_dialog_with_selection_style(
                frame,
                "Command palette",
                query,
                *selected,
                theme,
                options
                    .iter()
                    .map(|command| {
                        let key = command.keybinding.unwrap_or("");
                        (
                            format!("{:<24} {}", command.name, key),
                            format!("{}  {}", command.category.label(), command.description),
                        )
                    })
                    .collect(),
                purple_selection_style(theme),
            );
        }
        OverlayState::Theme { selected } => draw_theme_dialog(frame, *selected, theme),
        OverlayState::Mention {
            query, selected, ..
        } => {
            let files = app.catalog.mention_files();
            let options =
                mention_options(&files, &app.catalog.references, &app.catalog.agents, query);
            draw_catalog_dialog(
                frame,
                "Mention file, reference, or agent",
                query,
                *selected,
                theme,
                options
                    .iter()
                    .map(|option| {
                        let marker = match option.kind {
                            MentionKind::File => "file",
                            MentionKind::Reference => "reference",
                            MentionKind::Agent => "agent",
                        };
                        (
                            format!("@{}", option.name),
                            format!("{marker}  {}", option.description),
                        )
                    })
                    .collect(),
            );
        }
        OverlayState::RenameSession { value } => {
            draw_text_dialog(
                frame,
                "Rename session",
                "Title",
                value,
                "Enter save  Esc cancel",
                theme,
            );
        }
        OverlayState::DeleteSession { session_id } => {
            let title = app
                .session
                .sessions
                .iter()
                .find(|session| session.id == *session_id)
                .map(|session| session.display_title())
                .unwrap_or("this session");
            draw_text_dialog(
                frame,
                "Delete session",
                "This permanently deletes",
                title,
                "Enter/y confirm  Esc/n cancel",
                theme,
            );
        }
        OverlayState::ArchiveSession {
            session_id,
            restore,
        } => {
            let title = app
                .session
                .sessions
                .iter()
                .find(|session| session.id == *session_id)
                .map(|session| session.display_title())
                .unwrap_or("this session");
            draw_text_dialog(
                frame,
                if *restore {
                    "Restore session"
                } else {
                    "Archive session"
                },
                "Session",
                title,
                "Enter/y confirm  Esc/n cancel",
                theme,
            );
        }
        OverlayState::MoveSession {
            session_id,
            destination,
            move_changes,
        } => draw_move_session_dialog(frame, app, session_id, destination, *move_changes, theme),
        OverlayState::SessionDiff { selected, scroll } => {
            draw_session_diff_dialog(frame, app, *selected, *scroll, theme)
        }
        OverlayState::VcsDiff {
            mode,
            selected,
            scroll,
        } => draw_vcs_diff_dialog(frame, app, *mode, *selected, *scroll, theme),
        OverlayState::SessionShare { url } => draw_session_share_dialog(frame, url, theme),
        OverlayState::AttachFile { value } => {
            draw_text_dialog(
                frame,
                "Attach file",
                "Path",
                value,
                "Tab browse  Enter attach  Esc cancel",
                theme,
            );
        }
        OverlayState::FilePicker {
            path,
            entries,
            selected,
            loading,
        } => {
            let query = if *loading {
                format!("{path}  loading...")
            } else {
                path.clone()
            };
            draw_catalog_dialog(
                frame,
                "Choose attachment",
                &query,
                *selected,
                theme,
                entries
                    .iter()
                    .map(|entry| {
                        let marker = if entry.is_directory {
                            "[dir]"
                        } else {
                            "[file]"
                        };
                        (format!("{marker} {}", entry.path), String::new())
                    })
                    .collect(),
            );
        }
        OverlayState::Timeline { selected } => {
            let entries = app.timeline_entries();
            draw_catalog_dialog(
                frame,
                "Timeline",
                "",
                *selected,
                theme,
                entries
                    .iter()
                    .map(|entry| {
                        (
                            truncate_timeline_text(&entry.text),
                            format!(
                                "{}  {}",
                                short_id(&entry.message_id),
                                format_timeline_time(entry.created)
                            ),
                        )
                    })
                    .collect(),
            );
        }
        OverlayState::ForkSession { selected } => {
            let mut entries = vec![("Full session".to_owned(), "Fork all messages".to_owned())];
            entries.extend(app.timeline_entries().iter().map(|entry| {
                (
                    truncate_timeline_text(&entry.text),
                    format!(
                        "from {}  {}",
                        short_id(&entry.message_id),
                        format_timeline_time(entry.created)
                    ),
                )
            }));
            draw_catalog_dialog(frame, "Fork session", "", *selected, theme, entries);
        }
        OverlayState::Subtask { prompt, selected } => {
            draw_subtask_dialog(frame, app, prompt, *selected, theme);
        }
        OverlayState::PromptOptions { selected } => {
            draw_prompt_options_dialog(frame, app, *selected, theme);
        }
        OverlayState::PromptPanel { selected } => {
            draw_prompt_panel(frame, app, *selected, theme);
        }
        OverlayState::PromptTools { selected } => {
            draw_prompt_tools_dialog(frame, app, *selected, theme);
        }
        OverlayState::PromptToolName { value } => {
            draw_text_dialog(
                frame,
                "Add tool override",
                "Tool",
                value,
                "Enter add  Esc back",
                theme,
            );
        }
        OverlayState::PromptSystem { value } => {
            draw_text_dialog(
                frame,
                "System prompt",
                "Instructions",
                value,
                "Enter save  Esc back",
                theme,
            );
        }
        OverlayState::Diagnostics => draw_diagnostics_dialog(frame, app, theme),
        OverlayState::Help => draw_help_dialog(frame, theme),
    }
}

fn draw_text_dialog(
    frame: &mut Frame<'_>,
    title: &str,
    label: &str,
    value: &str,
    footer: &str,
    theme: Theme,
) {
    let area = dialog_area(frame.area(), 72, 7);
    frame.render_widget(Clear, area);
    let block = dialog_block(title, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = vec![
        Line::from(vec![
            Span::styled(format!("{label}: "), Style::default().fg(theme.text_muted)),
            Span::styled(value.to_owned(), Style::default().fg(theme.text)),
        ]),
        Line::from(Span::styled(footer, Style::default().fg(theme.text_muted))),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_move_session_dialog(
    frame: &mut Frame<'_>,
    app: &App,
    session_id: &str,
    destination: &str,
    move_changes: bool,
    theme: Theme,
) {
    let area = dialog_area(frame.area(), 78, 9);
    frame.render_widget(Clear, area);
    let block = dialog_block("Move session", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let title = app
        .session
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .map(|session| session.display_title())
        .unwrap_or("this session");
    let transfer = if move_changes { "[x]" } else { "[ ]" };
    let lines = vec![
        Line::from(vec![
            Span::styled("Session: ", Style::default().fg(theme.text_muted)),
            Span::styled(title, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Destination: ", Style::default().fg(theme.text_muted)),
            Span::styled(destination, Style::default().fg(theme.text)),
        ]),
        Line::from(Span::styled(
            format!("{transfer} Transfer local changes with the session"),
            Style::default().fg(if move_changes {
                theme.primary
            } else {
                theme.text_muted
            }),
        )),
        Line::from(Span::styled(
            "Tab toggle changes  Enter move  Esc cancel",
            Style::default().fg(theme.text_muted),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_session_diff_dialog(
    frame: &mut Frame<'_>,
    app: &App,
    selected: usize,
    scroll: usize,
    theme: Theme,
) {
    let area = dialog_area(
        frame.area(),
        112,
        frame.area().height.saturating_sub(2).max(12),
    );
    frame.render_widget(Clear, area);
    let block = dialog_block("Session diff", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Min(1)])
        .split(inner);

    let entries = app
        .integrations
        .diffs
        .iter()
        .map(|diff| {
            (
                diff.file.clone(),
                format!("+{}  -{}", diff.additions, diff.deletions),
            )
        })
        .collect::<Vec<_>>();
    let list = if entries.is_empty() {
        List::new(vec![ListItem::new(Line::from(Span::styled(
            "No changed files",
            Style::default().fg(theme.text_muted),
        )))])
    } else {
        List::new(
            entries
                .into_iter()
                .map(|(file, summary)| {
                    ListItem::new(Line::from(vec![
                        Span::styled(file, Style::default().fg(theme.text)),
                        Span::styled(
                            format!("  {summary}"),
                            Style::default().fg(theme.text_muted),
                        ),
                    ]))
                })
                .collect::<Vec<_>>(),
        )
    }
    .block(dialog_block("Files", theme))
    .highlight_style(selection_style(theme))
    .highlight_symbol("> ");
    let mut list_state = ListState::default();
    if !app.integrations.diffs.is_empty() {
        list_state.select(Some(
            selected.min(app.integrations.diffs.len().saturating_sub(1)),
        ));
    }
    frame.render_stateful_widget(list, columns[0], &mut list_state);

    let selected_diff = app
        .integrations
        .diffs
        .get(selected.min(app.integrations.diffs.len().saturating_sub(1)));
    let mut lines = vec![Line::from(Span::styled(
        "Up/Down files  PageUp/PageDown scroll  r refresh  Esc close",
        Style::default().fg(theme.text_muted),
    ))];
    let title = selected_diff
        .map(|diff| format!("{}  +{} -{}", diff.file, diff.additions, diff.deletions))
        .unwrap_or_else(|| "No diff loaded".to_owned());
    if let Some(diff) = selected_diff {
        lines.extend(diff_lines(diff, theme));
    } else {
        lines.push(Line::from(Span::styled(
            "Open a session with changed files to review its diff.",
            Style::default().fg(theme.text_muted),
        )));
    }
    let detail_block = dialog_block(&title, theme);
    let detail_inner = detail_block.inner(columns[1]);
    frame.render_widget(detail_block, columns[1]);
    let max_start = lines.len().saturating_sub(detail_inner.height as usize);
    let start = scroll.min(max_start);
    frame.render_widget(
        Paragraph::new(lines.into_iter().skip(start).collect::<Vec<_>>())
            .wrap(Wrap { trim: false }),
        detail_inner,
    );
}

fn diff_lines(diff: &FileDiff, theme: Theme) -> Vec<Line<'static>> {
    let before = diff.before.lines().collect::<Vec<_>>();
    let after = diff.after.lines().collect::<Vec<_>>();
    let mut lines = Vec::new();
    for index in 0..before.len().max(after.len()) {
        match (before.get(index), after.get(index)) {
            (Some(previous), Some(current)) if previous == current => {
                lines.push(styled_diff_line(
                    "  ",
                    previous,
                    theme.diff_context,
                    theme.diff_context_bg,
                ));
            }
            (Some(previous), Some(current)) => {
                lines.push(styled_diff_line(
                    "- ",
                    previous,
                    theme.diff_removed,
                    theme.diff_removed_bg,
                ));
                lines.push(styled_diff_line(
                    "+ ",
                    current,
                    theme.diff_added,
                    theme.diff_added_bg,
                ));
            }
            (Some(previous), None) => lines.push(styled_diff_line(
                "- ",
                previous,
                theme.diff_removed,
                theme.diff_removed_bg,
            )),
            (None, Some(current)) => lines.push(styled_diff_line(
                "+ ",
                current,
                theme.diff_added,
                theme.diff_added_bg,
            )),
            (None, None) => {}
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(empty)",
            Style::default().fg(theme.text_muted),
        )));
    }
    lines
}

fn styled_diff_line(
    prefix: &str,
    value: &str,
    foreground: Color,
    background: Color,
) -> Line<'static> {
    Line::from(Span::styled(
        format!("{prefix}{value}"),
        Style::default().fg(foreground).bg(background),
    ))
}

fn draw_vcs_diff_dialog(
    frame: &mut Frame<'_>,
    app: &App,
    mode: crate::model::VcsDiffMode,
    selected: usize,
    scroll: usize,
    theme: Theme,
) {
    let area = dialog_area(
        frame.area(),
        112,
        frame.area().height.saturating_sub(2).max(12),
    );
    frame.render_widget(Clear, area);
    let block = dialog_block(&format!("VCS diff  {}", mode.label()), theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Min(1)])
        .split(inner);

    let entries = app
        .integrations
        .vcs_diffs
        .iter()
        .map(|diff| {
            (
                diff.file.clone(),
                format!(
                    "+{}  -{}  {}",
                    diff.additions,
                    diff.deletions,
                    if diff.status.is_empty() {
                        "modified"
                    } else {
                        diff.status.as_str()
                    }
                ),
            )
        })
        .collect::<Vec<_>>();
    let list = if entries.is_empty() {
        List::new(vec![ListItem::new(Line::from(Span::styled(
            "No changed files",
            Style::default().fg(theme.text_muted),
        )))])
    } else {
        List::new(
            entries
                .into_iter()
                .map(|(file, summary)| {
                    ListItem::new(Line::from(vec![
                        Span::styled(file, Style::default().fg(theme.text)),
                        Span::styled(
                            format!("  {summary}"),
                            Style::default().fg(theme.text_muted),
                        ),
                    ]))
                })
                .collect::<Vec<_>>(),
        )
    }
    .block(dialog_block("Files", theme))
    .highlight_style(selection_style(theme))
    .highlight_symbol("> ");
    let mut list_state = ListState::default();
    if !app.integrations.vcs_diffs.is_empty() {
        list_state.select(Some(
            selected.min(app.integrations.vcs_diffs.len().saturating_sub(1)),
        ));
    }
    frame.render_stateful_widget(list, columns[0], &mut list_state);

    let selected_diff = app
        .integrations
        .vcs_diffs
        .get(selected.min(app.integrations.vcs_diffs.len().saturating_sub(1)));
    let mut lines = vec![Line::from(Span::styled(
        "Up/Down files  PageUp/PageDown scroll  s source  r refresh  Esc close",
        Style::default().fg(theme.text_muted),
    ))];
    let title = selected_diff
        .map(|diff| format!("{}  +{} -{}", diff.file, diff.additions, diff.deletions))
        .unwrap_or_else(|| "No VCS diff loaded".to_owned());
    if let Some(diff) = selected_diff {
        lines.extend(vcs_patch_lines(diff, theme));
    } else {
        lines.push(Line::from(Span::styled(
            "No changed files are available for this source.",
            Style::default().fg(theme.text_muted),
        )));
    }
    let detail_block = dialog_block(&title, theme);
    let detail_inner = detail_block.inner(columns[1]);
    frame.render_widget(detail_block, columns[1]);
    let max_start = lines.len().saturating_sub(detail_inner.height as usize);
    let start = scroll.min(max_start);
    frame.render_widget(
        Paragraph::new(lines.into_iter().skip(start).collect::<Vec<_>>())
            .wrap(Wrap { trim: false }),
        detail_inner,
    );
}

fn vcs_patch_lines(diff: &VcsFileDiff, theme: Theme) -> Vec<Line<'static>> {
    if diff.patch.is_empty() {
        return vec![Line::from(Span::styled(
            "(patch unavailable)",
            Style::default().fg(theme.text_muted),
        ))];
    }

    diff.patch
        .lines()
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let (foreground, background) = if line.starts_with('+') && !line.starts_with("+++") {
                (theme.diff_added, theme.diff_added_bg)
            } else if line.starts_with('-') && !line.starts_with("---") {
                (theme.diff_removed, theme.diff_removed_bg)
            } else if line.starts_with("@@") {
                (theme.accent, theme.diff_context_bg)
            } else {
                (theme.diff_context, theme.diff_context_bg)
            };
            Line::from(Span::styled(
                line.to_owned(),
                Style::default().fg(foreground).bg(background),
            ))
        })
        .collect()
}

fn draw_session_share_dialog(frame: &mut Frame<'_>, url: &str, theme: Theme) {
    let area = dialog_area(frame.area(), 86, 9);
    frame.render_widget(Clear, area);
    let block = dialog_block("Session link", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "URL",
            Style::default().fg(theme.text_muted),
        ))),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(url.to_owned())
            .style(Style::default().fg(theme.text))
            .wrap(Wrap { trim: false }),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "u unshare  Enter/Esc close",
            Style::default().fg(theme.text_muted),
        ))),
        layout[2],
    );
}

fn draw_subtask_dialog(
    frame: &mut Frame<'_>,
    app: &App,
    prompt: &str,
    selected: usize,
    theme: Theme,
) {
    let agents = mention_options(&[], &[], &app.catalog.agents, "");
    let area = dialog_area(frame.area(), 82, dialog_height(agents.len(), 4));
    frame.render_widget(Clear, area);
    let block = dialog_block("Add subtask", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Prompt: ", Style::default().fg(theme.text_muted)),
            Span::styled(prompt.to_owned(), Style::default().fg(theme.text)),
        ])),
        layout[0],
    );
    let selected_name = agents
        .get(selected.min(agents.len().saturating_sub(1)))
        .map(|agent| agent.name.as_str())
        .unwrap_or("no agent");
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Agent: ", Style::default().fg(theme.text_muted)),
            Span::styled(selected_name.to_owned(), Style::default().fg(theme.accent)),
        ])),
        layout[1],
    );
    let items = if agents.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No mentionable sub-agents loaded",
            Style::default().fg(theme.text_muted),
        )))]
    } else {
        agents
            .iter()
            .map(|agent| {
                ListItem::new(Line::from(vec![
                    Span::styled(agent.name.clone(), Style::default().fg(theme.text)),
                    Span::styled(
                        format!("  {}", agent.description),
                        Style::default().fg(theme.text_muted),
                    ),
                ]))
            })
            .collect()
    };
    let mut state = ListState::default();
    if !agents.is_empty() {
        state.select(Some(selected.min(agents.len() - 1)));
    }
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(selection_style(theme))
            .highlight_symbol("> "),
        layout[2],
        &mut state,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Up/Down agent  Enter add  Esc cancel",
            Style::default().fg(theme.text_muted),
        ))),
        layout[3],
    );
}

fn draw_prompt_panel(frame: &mut Frame<'_>, app: &App, selected: usize, theme: Theme) {
    let area = dialog_area(
        frame.area(),
        112,
        frame.area().height.saturating_sub(2).max(12),
    );
    frame.render_widget(Clear, area);
    let block = dialog_block("Prompt panel", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items = app.prompt.panel_items();
    let entries = items
        .iter()
        .map(|item| prompt_panel_entry(item, app))
        .collect::<Vec<_>>();
    let selected = selected.min(entries.len().saturating_sub(1));
    let columns = if inner.width >= 72 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(43), Constraint::Min(1)])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Min(1)])
            .split(inner)
    };
    let mut list_state = ListState::default();
    if !entries.is_empty() {
        list_state.select(Some(selected));
    }
    let list = List::new(
        entries
            .iter()
            .map(|(title, detail)| {
                ListItem::new(Line::from(vec![
                    Span::styled(title.clone(), Style::default().fg(theme.text)),
                    Span::styled(format!("  {detail}"), Style::default().fg(theme.text_muted)),
                ]))
            })
            .chain(std::iter::once(ListItem::new(Line::from(Span::styled(
                "Up/Down/j/k select  Enter edit  d/x/Delete remove  Esc close",
                Style::default().fg(theme.text_muted),
            ))))),
    )
    .block(dialog_block("Prompt structure", theme))
    .highlight_style(selection_style(theme))
    .highlight_symbol("> ");
    frame.render_stateful_widget(list, columns[0], &mut list_state);

    let detail = items
        .get(selected)
        .map(|item| prompt_panel_detail(item, app, theme))
        .unwrap_or_else(|| {
            vec![Line::from(Span::styled(
                "No prompt items",
                Style::default().fg(theme.text_muted),
            ))]
        });
    frame.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: false })
            .block(dialog_block("Details", theme)),
        columns[1],
    );
}

fn prompt_panel_entry(item: &crate::prompt_state::PromptPanelItem, app: &App) -> (String, String) {
    use crate::prompt_state::PromptPanelItem;
    match item {
        PromptPanelItem::Draft => (
            "Draft".to_owned(),
            format!("{} chars", app.prompt.composer.text().chars().count()),
        ),
        PromptPanelItem::Model => {
            let value = app
                .active_model_ref()
                .map(|model| format!("{}/{}", model.provider_id, model.id))
                .unwrap_or_else(|| "unset".to_owned());
            ("Model".to_owned(), value)
        }
        PromptPanelItem::Agent => (
            "Agent".to_owned(),
            app.catalog
                .selected_agent
                .as_deref()
                .unwrap_or("unset")
                .to_owned(),
        ),
        PromptPanelItem::Variant => {
            let value = app
                .active_model_ref()
                .and_then(|model| model.variant)
                .unwrap_or_else(|| "default".to_owned());
            ("Variant".to_owned(), value)
        }
        PromptPanelItem::Format => (
            "Format".to_owned(),
            if app.prompt.options.output_format.is_some() {
                "json object"
            } else {
                "text"
            }
            .to_owned(),
        ),
        PromptPanelItem::NoReply => (
            "No-reply".to_owned(),
            if app.prompt.options.no_reply {
                "on"
            } else {
                "off"
            }
            .to_owned(),
        ),
        PromptPanelItem::System => (
            "System".to_owned(),
            if app.prompt.options.system.is_some() {
                "configured"
            } else {
                "unset"
            }
            .to_owned(),
        ),
        PromptPanelItem::Tools => (
            "Tools".to_owned(),
            if app.prompt.options.tool_overrides.is_empty() {
                "default".to_owned()
            } else {
                format!("{} overrides", app.prompt.options.tool_overrides.len())
            },
        ),
        PromptPanelItem::AddAttachment => {
            ("+ Attachment".to_owned(), "open path picker".to_owned())
        }
        PromptPanelItem::Attachment(index) => {
            let value = app.prompt.attachments.get(*index).map(PromptPart::display);
            value
                .map(|display| (format!("Attachment {}", index + 1), display.title))
                .unwrap_or_else(|| ("Attachment".to_owned(), "missing".to_owned()))
        }
        PromptPanelItem::AddSubtask => ("+ Subtask".to_owned(), "choose sub-agent".to_owned()),
        PromptPanelItem::Subtask(index) => {
            let value = app.prompt.subtasks.get(*index).map(PromptPart::display);
            value
                .map(|display| (format!("Subtask {}", index + 1), display.title))
                .unwrap_or_else(|| ("Subtask".to_owned(), "missing".to_owned()))
        }
    }
}

fn prompt_panel_detail(
    item: &crate::prompt_state::PromptPanelItem,
    app: &App,
    theme: Theme,
) -> Vec<Line<'static>> {
    use crate::prompt_state::PromptPanelItem;
    let mut lines = Vec::new();
    match item {
        PromptPanelItem::Draft => {
            lines.push(Line::from(Span::styled(
                "Current draft",
                Style::default().fg(theme.text),
            )));
            lines.push(Line::from(Span::styled(
                app.prompt.composer.text(),
                Style::default().fg(theme.accent),
            )));
        }
        PromptPanelItem::Attachment(index) => {
            if let Some(part) = app.prompt.attachments.get(*index) {
                let display = part.display();
                lines.push(Line::from(Span::styled(
                    display.title,
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(Span::styled(
                    display.detail,
                    Style::default().fg(theme.text_muted),
                )));
                lines.extend(display.preview.lines().map(|line| {
                    Line::from(Span::styled(
                        line.to_owned(),
                        Style::default().fg(theme.accent),
                    ))
                }));
            }
        }
        PromptPanelItem::Subtask(index) => {
            if let Some(part) = app.prompt.subtasks.get(*index) {
                let display = part.display();
                lines.push(Line::from(Span::styled(
                    display.title,
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(Span::styled(
                    display.detail,
                    Style::default().fg(theme.text_muted),
                )));
                lines.extend(display.preview.lines().map(|line| {
                    Line::from(Span::styled(
                        line.to_owned(),
                        Style::default().fg(theme.accent),
                    ))
                }));
            }
        }
        _ => {
            let (title, detail) = prompt_panel_entry(item, app);
            lines.push(Line::from(Span::styled(
                title,
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                detail,
                Style::default().fg(theme.accent),
            )));
            lines.push(Line::from(Span::styled(
                "Enter opens the existing editor for this option.",
                Style::default().fg(theme.text_muted),
            )));
        }
    }
    lines
}

fn draw_prompt_options_dialog(frame: &mut Frame<'_>, app: &App, selected: usize, theme: Theme) {
    let area = dialog_area(frame.area(), 72, 10);
    frame.render_widget(Clear, area);
    let block = dialog_block("Prompt options", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let system = app.prompt.options.system.as_deref().unwrap_or("unset");
    let format = if app.prompt.options.output_format.is_some() {
        "json object"
    } else {
        "text"
    };
    let tools = if app.prompt.options.tool_overrides.is_empty() {
        "default"
    } else {
        "overrides configured"
    };
    let entries = vec![
        format!(
            "No reply  {}",
            if app.prompt.options.no_reply {
                "on"
            } else {
                "off"
            }
        ),
        format!("Format    {format}"),
        format!("Tools     {tools}"),
        format!("System    {system}"),
    ];
    let mut state = ListState::default();
    state.select(Some(selected.min(entries.len() - 1)));
    let items = entries
        .into_iter()
        .map(|entry| {
            ListItem::new(Line::from(Span::styled(
                entry,
                Style::default().fg(theme.text),
            )))
        })
        .chain(std::iter::once(ListItem::new(Line::from(Span::styled(
            "Up/Down select  Enter toggle/edit  Esc close",
            Style::default().fg(theme.text_muted),
        )))))
        .collect::<Vec<_>>();
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(selection_style(theme))
            .highlight_symbol("> "),
        inner,
        &mut state,
    );
}

fn draw_prompt_tools_dialog(frame: &mut Frame<'_>, app: &App, selected: usize, theme: Theme) {
    let names = tool_override_names(&app.prompt.options.tool_overrides);
    let area = dialog_area(frame.area(), 82, dialog_height(names.len() + 2, 1));
    frame.render_widget(Clear, area);
    let block = dialog_block("Tool overrides", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut items = names
        .iter()
        .map(|name| {
            let state = match app.prompt.options.tool_overrides.get(name) {
                Some(true) => "on",
                Some(false) => "off",
                None => "default",
            };
            ListItem::new(Line::from(vec![
                Span::styled(name.clone(), Style::default().fg(theme.text)),
                Span::styled(format!("  {state}"), Style::default().fg(theme.text_muted)),
            ]))
        })
        .collect::<Vec<_>>();
    items.push(ListItem::new(Line::from(Span::styled(
        "Add custom tool...",
        Style::default().fg(theme.accent),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        "Clear overrides",
        Style::default().fg(theme.warning),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        "Up/Down select  Enter cycle/add/clear  Esc back",
        Style::default().fg(theme.text_muted),
    ))));
    let mut state = ListState::default();
    state.select(Some(selected.min(names.len() + 1)));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(selection_style(theme))
            .highlight_symbol("> "),
        inner,
        &mut state,
    );
}

fn draw_help_dialog(frame: &mut Frame<'_>, theme: Theme) {
    let area = dialog_area(frame.area(), 78, 16);
    frame.render_widget(Clear, area);
    let block = dialog_block("Keyboard help", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = [
        "Session: Enter send  Ctrl-X abort  Esc home",
        "Prompt: Shift/Ctrl/Alt-Enter newline  Ctrl-A select all",
        "Prompt: Ctrl-U undo  Ctrl-R redo  Up/Down history when empty",
        "Transcript: PageUp/PageDown  Home/End jump top/latest  Ctrl-Shift-B collapse",
        "Catalogs: /model  /skill  /agent  /variant",
        "Mentions: type @ for local files and sub-agents",
        "Sessions: F2/e rename  Delete/d remove  Ctrl-E export",
        "Attachments: Ctrl-Shift-U add  Ctrl-Shift-Backspace remove",
        "Prompt: Ctrl-Shift-T subtask  Ctrl-Shift-O options",
        "Ctrl-P command palette  Ctrl-C quit",
        "",
        "Esc close",
    ]
    .into_iter()
    .map(|line| Line::from(Span::styled(line, Style::default().fg(theme.text))))
    .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_diagnostics_dialog(frame: &mut Frame<'_>, app: &App, theme: Theme) {
    let area = dialog_area(frame.area(), 96, 22);
    frame.render_widget(Clear, area);
    let block = dialog_block("Diagnostics", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("connection: ", Style::default().fg(theme.text_muted)),
            Span::styled(app.connection_detail(), Style::default().fg(theme.text)),
            Span::styled(
                format!("  server: {}", app.client.base_url()),
                Style::default().fg(theme.text_muted),
            ),
        ]),
        Line::from(vec![
            Span::styled("status: ", Style::default().fg(theme.text_muted)),
            Span::styled(app.status_label(), Style::default().fg(theme.text)),
            Span::styled(
                format!(
                    "  sidebar: {}",
                    if app.runtime.sidebar_visible {
                        "visible"
                    } else {
                        "hidden"
                    }
                ),
                Style::default().fg(theme.text_muted),
            ),
        ]),
        Line::from(Span::styled(
            format!(
                "sessions: {}  transcript messages: {}  permissions: {}  questions: {}",
                app.session.sessions.len(),
                app.transcript.iter().count(),
                app.pending.permissions.len(),
                app.pending.questions.len(),
            ),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            format!(
                "catalog: {} providers  {} skills  {} agents  {} workspace files",
                app.catalog.providers.len(),
                app.catalog.skills.len(),
                app.catalog.agents.len(),
                app.catalog.workspace_files.len(),
            ),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            format!(
                "integrations: {} MCP  {} LSP  {} todos  {} diffs  health: {}",
                app.integrations.mcp.len(),
                app.integrations.lsp.len(),
                app.integrations.todos.len(),
                app.integrations.diffs.len(),
                fallback_text(&app.runtime.server_health, "unknown"),
            ),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            format!(
                "vcs: {}  changed files: {}  tracked statuses: {}",
                app.integrations
                    .vcs
                    .as_ref()
                    .map(|vcs| fallback_text(&vcs.branch, "detached"))
                    .unwrap_or("unavailable"),
                app.integrations.diffs.len(),
                app.integrations.vcs_status.len(),
            ),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            format!("session statuses: {}", app.runtime.session_statuses.len()),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            format!("workspace VCS files: {}", app.integrations.vcs_status.len()),
            Style::default().fg(theme.text_muted),
        )),
        Line::from(""),
    ];

    let mut session_statuses = app.runtime.session_statuses.iter().collect::<Vec<_>>();
    session_statuses.sort_by(|left, right| left.0.cmp(right.0));
    if !session_statuses.is_empty() {
        lines.push(Line::from(Span::styled(
            "Session status details",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )));
    }
    for (session_id, status) in session_statuses.iter().take(6) {
        lines.push(Line::from(Span::styled(
            format!("{}: {}", short_id(session_id), status.label()),
            Style::default().fg(if status.is_working() {
                theme.warning
            } else {
                theme.text_muted
            }),
        )));
    }

    lines.push(Line::from(Span::styled(
        "Recent notifications",
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
    )));

    if app.notifications.history().is_empty() {
        lines.push(Line::from(Span::styled(
            "No notifications recorded",
            Style::default().fg(theme.text_muted),
        )));
    } else {
        for record in app.notifications.history().iter().rev().take(10) {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("#{:03} {} ", record.sequence, record.level.label()),
                    Style::default().fg(notification_level_color(record.level, theme)),
                ),
                Span::styled(record.message.clone(), Style::default().fg(theme.text)),
            ]));
        }
    }
    lines.push(Line::from(Span::styled(
        "r refresh  c clear history  Esc/q close",
        Style::default().fg(theme.text_muted),
    )));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_theme_dialog(frame: &mut Frame<'_>, selected: usize, theme: Theme) {
    let choices = Theme::choices();
    draw_catalog_dialog(
        frame,
        "Select theme",
        "",
        selected,
        theme,
        choices
            .iter()
            .map(|choice| {
                let marker = if choice.theme == theme { "*" } else { " " };
                let source = if choice.spec == choice.name {
                    "built-in"
                } else {
                    "JSON"
                };
                (format!("{marker} {}", choice.name), source.to_owned())
            })
            .collect(),
    );
}

fn draw_catalog_dialog(
    frame: &mut Frame<'_>,
    title: &str,
    query: &str,
    selected: usize,
    theme: Theme,
    entries: Vec<(String, String)>,
) {
    draw_catalog_dialog_with_selection_style(
        frame,
        title,
        query,
        selected,
        theme,
        entries,
        selection_style(theme),
    );
}

fn draw_catalog_dialog_with_selection_style(
    frame: &mut Frame<'_>,
    title: &str,
    query: &str,
    selected: usize,
    theme: Theme,
    entries: Vec<(String, String)>,
    selected_style: Style,
) {
    let area = dialog_area(frame.area(), 78, dialog_height(entries.len(), 1));
    let has_entries = !entries.is_empty();
    frame.render_widget(Clear, area);
    let block = dialog_block(title, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Search: ", Style::default().fg(theme.text_muted)),
            Span::styled(query.to_owned(), Style::default().fg(theme.text)),
        ])),
        layout[0],
    );
    let items = if entries.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            if query.is_empty() {
                "No entries loaded"
            } else {
                "No matching entries"
            },
            Style::default().fg(theme.text_muted),
        )))]
    } else {
        entries
            .into_iter()
            .map(|(value, description)| {
                ListItem::new(Line::from(vec![
                    Span::styled(value, Style::default().fg(theme.text)),
                    if description.is_empty() {
                        Span::raw("")
                    } else {
                        Span::styled(
                            format!("  {description}"),
                            Style::default().fg(theme.text_muted),
                        )
                    },
                ]))
            })
            .collect()
    };
    let count = items.len();
    let list = List::new(items)
        .highlight_style(selected_style)
        .highlight_symbol("> ");
    let mut state = ListState::default();
    if has_entries {
        state.select(Some(selected.min(count - 1)));
    }
    frame.render_stateful_widget(list, layout[1], &mut state);
}

fn dialog_block(title: &str, theme: Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active))
        .style(Style::default().bg(theme.background_menu))
        .title(title.to_owned())
}

fn selection_style(theme: Theme) -> Style {
    Style::default()
        .bg(theme.background_element)
        .fg(theme.selected_list_item_text)
        .add_modifier(Modifier::BOLD)
}

fn purple_selection_style(theme: Theme) -> Style {
    Style::default()
        .bg(theme.background_element)
        .fg(theme.secondary)
        .add_modifier(Modifier::BOLD)
}

fn dialog_height(entry_count: usize, extra_lines: u16) -> u16 {
    (entry_count.min(12) as u16 + 3 + extra_lines).max(5)
}

fn dialog_area(area: Rect, requested_width: u16, requested_height: u16) -> Rect {
    let width = requested_width.min(area.width.saturating_sub(2)).max(1);
    let height = requested_height.min(area.height.saturating_sub(2)).max(1);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn draw_transcript(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: Theme) {
    let pending_height = if app.current_permission().is_some() {
        6
    } else if app.current_question().is_some() {
        8
    } else {
        0
    };
    let attachment_height = if app.prompt.attachments.is_empty() && app.prompt.subtasks.is_empty() {
        0
    } else {
        2
    };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(pending_height),
            Constraint::Length(attachment_height),
            Constraint::Length(7),
            Constraint::Length(1),
        ])
        .split(area);
    let current_session = app.session.current_session.as_ref();
    let title = current_session
        .map(|session| session.display_title())
        .unwrap_or("Session");
    let share_status = current_session
        .filter(|session| session.share_url().is_some())
        .map(|_| "  shared")
        .unwrap_or("");
    let fork_status = current_session
        .filter(|session| session.parent_id.is_some())
        .map(|_| "  fork")
        .unwrap_or("");
    let archive_status = current_session
        .filter(|session| session.time.archived.is_some())
        .map(|_| "  archived")
        .unwrap_or("");
    let children_status = if app.session.children.is_empty() {
        String::new()
    } else {
        format!("  children:{}", app.session.children.len())
    };
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            title.to_owned(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", app.status_label()),
            Style::default().fg(status_color(app, theme)),
        ),
        Span::styled(
            format!("  [{}]", current_directory(app)),
            Style::default().fg(theme.text_muted),
        ),
        Span::styled(share_status, Style::default().fg(theme.success)),
        Span::styled(fork_status, Style::default().fg(theme.text_muted)),
        Span::styled(archive_status, Style::default().fg(theme.warning)),
        Span::styled(children_status, Style::default().fg(theme.text_muted)),
        Span::styled(
            if app.transcript.scroll.is_following() {
                "  live"
            } else {
                "  manual"
            },
            Style::default().fg(theme.text_muted),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(theme.border)),
    );
    frame.render_widget(header, layout[0]);

    // Use virtualized rendering for better performance with large transcripts
    let transcript_area = layout[1];
    // Reserve the bottom of the transcript for queued prompts. That area is
    // independent from transcript scrolling, so queued commands remain pinned.
    let transcript_width = transcript_area.width.saturating_sub(2).max(1);
    let transcript_inner = transcript_area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let queued_lines = queued_prompt_lines(
        app,
        transcript_width,
        transcript_inner.height as usize,
        theme,
    );
    let queue_height = queued_lines.len().min(u16::MAX as usize) as u16;
    let transcript_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(queue_height)])
        .split(transcript_inner);
    let message_area = transcript_sections[0];
    let queue_area = transcript_sections[1];
    let viewport_height = message_area.height as usize;

    let lines = if viewport_height == 0 {
        app.transcript.scroll.observe(0, 0);
        Vec::new()
    } else if app.transcript.is_empty() {
        app.transcript.scroll.observe(0, viewport_height as u16);
        app.transcript.scroll.clear_anchor();
        vec![Line::from(Span::styled(
            "This session has no messages yet.",
            Style::default().fg(theme.text_muted),
        ))]
    } else {
        // Build virtualized view
        let messages: Vec<MessageWithParts> = app.transcript.iter().cloned().collect();
        let previous_anchor = if app.transcript.scroll.is_following() {
            None
        } else {
            app.transcript.scroll.anchor()
        };
        let view = if app.transcript.collapsed_parts.is_empty() {
            TranscriptView::with_theme(&messages, transcript_width, &theme)
        } else {
            TranscriptView::with_collapsed_theme(
                &messages,
                transcript_width,
                &app.transcript.collapsed_parts,
                &theme,
            )
        };
        let total_lines = view.total_lines();

        // Update scroll state with actual content height
        app.transcript
            .scroll
            .observe(total_lines, viewport_height as u16);
        if let Some(anchor) = previous_anchor {
            if let Some(offset) = view.line_for_anchor(&messages, &anchor) {
                app.transcript.scroll.restore_offset(offset);
            } else {
                app.transcript.scroll.clear_anchor();
            }
        }

        // Render only visible lines
        let scroll_offset = app.transcript.scroll.offset() as usize;
        let end_offset = (scroll_offset + viewport_height).min(total_lines);
        let lines = view.render_lines(&messages, scroll_offset, end_offset, transcript_width);
        app.transcript
            .scroll
            .set_anchor(view.anchor_at_line(&messages, scroll_offset));
        lines
    };

    // Selection rows include padding between short transcript content and the
    // bottom-pinned queue so their indices continue to match screen coordinates.
    let mut selection_rows = lines.iter().map(line_text).collect::<Vec<_>>();
    selection_rows.resize(message_area.height as usize, String::new());
    selection_rows.extend(queued_lines.iter().map(line_text));
    app.selection
        .record_pane(SelectionPane::Transcript, transcript_inner, selection_rows);

    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title("Transcript"),
        transcript_area,
    );
    // Lines are pre-wrapped in transcript_view to account for terminal width.
    frame.render_widget(Paragraph::new(lines), message_area);
    if queue_height > 0 {
        frame.render_widget(Paragraph::new(queued_lines), queue_area);
    }

    if pending_height > 0 {
        draw_pending(frame, app, layout[2], theme);
    }

    if attachment_height > 0 {
        draw_attachments(frame, app, layout[3], theme);
    }

    let (prompt_inner, prompt_rows) =
        app.prompt
            .composer
            .render(frame, layout[4], theme, app.runtime.working);
    app.selection
        .record_pane(SelectionPane::Prompt, prompt_inner, prompt_rows);

    draw_footer(frame, app, layout[5], "? shortcuts", theme);
}

fn draw_attachments(frame: &mut Frame<'_>, app: &App, area: Rect, theme: Theme) {
    let labels = app
        .prompt
        .attachments
        .iter()
        .chain(app.prompt.subtasks.iter())
        .map(|part| {
            let display = part.display();
            format!("{} ({})", display.title, display.detail)
        })
        .collect::<Vec<_>>();
    let text = if labels.is_empty() {
        "file".to_owned()
    } else {
        labels.join("  ")
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Draft parts: ", Style::default().fg(theme.text_muted)),
            Span::styled(text, Style::default().fg(theme.accent)),
        ]))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme.border)),
        ),
        area,
    );
}

fn draw_pending(frame: &mut Frame<'_>, app: &App, area: Rect, theme: Theme) {
    if let Some(request) = app.current_permission() {
        let action = if app.is_responding(&request.id) {
            "sending response..."
        } else {
            "y allow once  a allow always  n reject  Esc reject"
        };
        let detail = if request.patterns.is_empty() {
            request.permission.clone()
        } else {
            format!("{}  {}", request.permission, request.patterns.join(", "))
        };
        let metadata = request
            .metadata
            .get("filepath")
            .or_else(|| request.metadata.get("command"))
            .or_else(|| request.metadata.get("url"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let detail = if metadata.is_empty() {
            detail
        } else {
            format!("{detail}  {metadata}")
        };
        let detail = if let Some(tool) = request.tool.as_ref() {
            format!("{detail}  {}:{}", tool.message_id, tool.call_id)
        } else {
            detail
        };
        let always = if request.always.is_empty() {
            String::new()
        } else {
            format!("always: {}", request.always.join(", "))
        };
        let lines = vec![
            Line::from(Span::styled(
                "permission required",
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(detail, Style::default().fg(theme.text))),
            Line::from(Span::styled(always, Style::default().fg(theme.text_muted))),
            Line::from(Span::styled(action, Style::default().fg(theme.text_muted))),
        ];
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.warning))
                    .title("Action required"),
            ),
            area,
        );
        return;
    }

    if let (Some(request), Some(question)) = (app.current_question(), app.current_question_info()) {
        let action = if app.is_responding(&request.id) {
            "sending response..."
        } else if question.multiple {
            "Up/Down select  Space toggle  Enter next/submit  Esc reject"
        } else {
            "Up/Down select  Enter confirm  Esc reject"
        };
        let mut lines = vec![Line::from(Span::styled(
            format!(
                "{}{}  {}/{}",
                question.header,
                if question.custom { "  custom" } else { "" },
                app.pending.question_index + 1,
                request.questions.len()
            ),
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(Span::styled(
            question.question.clone(),
            Style::default().fg(theme.text),
        )));
        for (index, option) in question.options.iter().enumerate().take(3) {
            let selected = index == app.pending.question_selected;
            let checked = question.multiple
                && app
                    .pending
                    .question_answers
                    .get(app.pending.question_index)
                    .is_some_and(|answers| answers.iter().any(|answer| answer == &option.label));
            let marker = if checked {
                "[x]"
            } else if selected {
                ">"
            } else {
                " "
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "{marker} {}  {}  {}",
                    index + 1,
                    option.label,
                    option.description
                ),
                Style::default().fg(if selected { theme.accent } else { theme.text }),
            )));
        }
        let tool = request
            .tool
            .as_ref()
            .map(|tool| format!("  {}:{}", tool.message_id, tool.call_id))
            .unwrap_or_default();
        lines.push(Line::from(Span::styled(
            format!("{action}{tool}"),
            Style::default().fg(theme.text_muted),
        )));
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.warning))
                    .title("Question"),
            ),
            area,
        );
    }
}

fn draw_sidebar(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Runtime");
    let inner = block.inner(area);
    let content = Paragraph::new(sidebar_lines(app, theme)).wrap(Wrap { trim: false });
    app.sidebar_scroll
        .observe(content.line_count(inner.width), inner.height);
    let sidebar = content
        .block(block)
        .scroll((app.sidebar_scroll.offset(), 0));
    frame.render_widget(sidebar, area);
}

fn sidebar_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    let session_tokens = app
        .session
        .current_session
        .as_ref()
        .map(|session| session.tokens.clone())
        .unwrap_or_default();
    let latest = latest_assistant(app);
    let latest_tokens = latest
        .map(|message| message.tokens.clone())
        .unwrap_or_default();
    let current_tokens = if latest_tokens.total_or_sum() > 0 {
        latest_tokens.clone()
    } else {
        session_tokens.clone()
    };
    let (provider_id, model_id, variant) = current_model(app, latest);
    let provider_label = provider_label(app, &provider_id);
    let model_info = selected_model_info(app, &provider_id, &model_id);
    let model_label = model_label(&model_id, model_info);
    let context_limit = model_context_limit(app, &provider_id, &model_id);
    let context_tokens = current_tokens.total_or_sum();
    let context_percent = if context_limit == 0 {
        None
    } else {
        Some((context_tokens as f64 / context_limit as f64 * 100.0).min(100.0))
    };
    let cost = app
        .session
        .current_session
        .as_ref()
        .map(|session| session.cost)
        .filter(|cost| *cost > 0.0)
        .or_else(|| latest.map(|message| message.cost))
        .unwrap_or(0.0);
    let mut lines = Vec::new();

    if let Some(session) = app.session.current_session.as_ref() {
        lines.push(Line::from(Span::styled(
            session.display_title().to_owned(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )));
        lines.push(detail_line(
            theme,
            "id",
            short_id(&session.id),
            theme.text_muted,
        ));
        let agent = app
            .catalog
            .selected_agent
            .as_deref()
            .or(session.agent.as_deref())
            .filter(|agent| !agent.is_empty());
        if let Some(agent) = agent {
            lines.push(detail_line(theme, "agent", agent, theme.text));
        }
        lines.push(Line::from(""));
    }

    lines.push(section_line(theme, "v Cache stats"));
    lines.push(detail_line(
        theme,
        "hit",
        format!(
            "{} {:.1}%",
            progress_bar(current_tokens.cache_hit_percent()),
            current_tokens.cache_hit_percent()
        ),
        theme.success,
    ));
    lines.push(detail_line(
        theme,
        "total hit",
        format!("{:.1}%", session_tokens.cache_hit_percent()),
        theme.text,
    ));
    lines.push(detail_line(
        theme,
        "cache read",
        compact_tokens(session_tokens.cache.read),
        theme.text,
    ));
    lines.push(detail_line(
        theme,
        "miss",
        compact_tokens(session_tokens.input),
        theme.text,
    ));
    lines.push(detail_line(
        theme,
        "output",
        compact_tokens(session_tokens.output),
        theme.text,
    ));
    if session_tokens.cache.write > 0 {
        lines.push(detail_line(
            theme,
            "cache write",
            compact_tokens(session_tokens.cache.write),
            theme.text_muted,
        ));
    }

    lines.push(section_line(theme, "v Model"));
    lines.push(detail_line(theme, "cost", format_money(cost), theme.text));
    lines.push(detail_line(theme, "provider", provider_label, theme.text));
    lines.push(detail_line(theme, "model", model_label, theme.text));
    if let Some(variant) = variant.filter(|variant| !variant.is_empty()) {
        lines.push(detail_line(theme, "variant", variant, theme.text_muted));
    }
    if let Some(model) = model_info.filter(|model| model.limit.output > 0) {
        lines.push(detail_line(
            theme,
            "max output",
            format_count(model.limit.output),
            theme.text_muted,
        ));
    }

    lines.push(section_line(theme, "v Token distribution"));
    lines.push(detail_line(
        theme,
        "input",
        compact_tokens(current_tokens.input),
        theme.text_muted,
    ));
    lines.push(detail_line(
        theme,
        "reasoning",
        compact_tokens(current_tokens.reasoning),
        theme.text_muted,
    ));
    lines.push(detail_line(
        theme,
        "output",
        compact_tokens(current_tokens.output),
        theme.text_muted,
    ));

    lines.push(section_line(theme, "v Context"));
    lines.push(detail_line(
        theme,
        "tokens",
        format_count(context_tokens),
        theme.text,
    ));
    lines.push(detail_line(
        theme,
        "used",
        context_percent
            .map(|percent| format!("{percent:.0}%"))
            .unwrap_or_else(|| "unknown".to_owned()),
        theme.text,
    ));
    lines.push(detail_line(theme, "spent", format_money(cost), theme.text));

    lines.push(section_line(
        theme,
        &format!("v Todo ({})", app.integrations.todos.len()),
    ));
    if app.integrations.todos.is_empty() {
        lines.push(detail_line(
            theme,
            "status",
            "none loaded",
            theme.text_muted,
        ));
    } else {
        for todo in app.integrations.todos.iter().take(6) {
            let status = fallback_text(&todo.status, "pending");
            let priority = if todo.priority.is_empty() {
                String::new()
            } else {
                format!(" [{}]", todo.priority)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", todo_status_marker(status)),
                    Style::default().fg(todo_status_color(status, theme)),
                ),
                Span::styled(
                    format!("{}{}", truncate_sidebar_text(&todo.content, 46), priority),
                    Style::default().fg(theme.text),
                ),
            ]));
        }
        if app.integrations.todos.len() > 6 {
            lines.push(detail_line(
                theme,
                "more",
                format!("{} items", app.integrations.todos.len() - 6),
                theme.text_muted,
            ));
        }
    }

    lines.push(section_line(
        theme,
        &format!("v Modified files ({})", app.integrations.diffs.len()),
    ));
    if app.integrations.diffs.is_empty() {
        lines.push(detail_line(
            theme,
            "status",
            "none loaded",
            theme.text_muted,
        ));
    } else {
        for diff in app.integrations.diffs.iter().take(6) {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", truncate_sidebar_text(&diff.file, 42)),
                    Style::default().fg(theme.text),
                ),
                Span::styled(
                    format!("+{}", diff.additions),
                    Style::default().fg(theme.diff_added),
                ),
                Span::styled(" ", Style::default().fg(theme.text_muted)),
                Span::styled(
                    format!("-{}", diff.deletions),
                    Style::default().fg(theme.diff_removed),
                ),
            ]));
        }
        if app.integrations.diffs.len() > 6 {
            lines.push(detail_line(
                theme,
                "more",
                format!("{} files", app.integrations.diffs.len() - 6),
                theme.text_muted,
            ));
        }
    }

    lines.push(section_line(theme, "v VCS"));
    if let Some(vcs) = app.integrations.vcs.as_ref() {
        lines.push(detail_line(
            theme,
            "branch",
            fallback_text(&vcs.branch, "detached"),
            theme.text,
        ));
        let additions = app
            .integrations
            .vcs_status
            .iter()
            .map(|file| file.additions)
            .sum::<u64>();
        let deletions = app
            .integrations
            .vcs_status
            .iter()
            .map(|file| file.deletions)
            .sum::<u64>();
        lines.push(detail_line(
            theme,
            "changes",
            format!(
                "{} files  +{} -{}",
                app.integrations.vcs_status.len(),
                additions,
                deletions
            ),
            theme.text_muted,
        ));
    } else {
        lines.push(detail_line(
            theme,
            "status",
            "not available",
            theme.text_muted,
        ));
    }

    if let Some(revert) = app.integrations.revert_state.as_ref() {
        lines.push(section_line(theme, "v Revert"));
        lines.push(detail_line(
            theme,
            "message",
            short_id(&revert.message_id),
            theme.text,
        ));
        if let Some(part_id) = revert.part_id.as_deref() {
            lines.push(detail_line(
                theme,
                "part",
                short_id(part_id),
                theme.text_muted,
            ));
        }
        lines.push(detail_line(
            theme,
            "snapshot",
            if revert
                .snapshot
                .as_deref()
                .is_some_and(|snapshot| !snapshot.is_empty())
            {
                "available"
            } else {
                "none"
            },
            theme.text_muted,
        ));
        lines.push(detail_line(
            theme,
            "files",
            revert.files.len().to_string(),
            theme.text,
        ));
        lines.push(detail_line(
            theme,
            "diff",
            if revert.diff.as_deref().is_some_and(|diff| !diff.is_empty()) {
                "staged"
            } else {
                "metadata only"
            },
            theme.text_muted,
        ));
        for file in revert.files.iter().take(4) {
            lines.push(Line::from(Span::styled(
                format!(
                    "  * {} {} +{} -{} ({} patch lines)",
                    file.status.as_str(),
                    file.path,
                    file.additions,
                    file.deletions,
                    file.patch.lines().count()
                ),
                Style::default().fg(theme.text),
            )));
        }
        if revert.files.len() > 4 {
            lines.push(detail_line(
                theme,
                "more",
                format!("{} files", revert.files.len() - 4),
                theme.text_muted,
            ));
        }
    }

    lines.push(section_line(theme, "v MCP"));
    if app.integrations.mcp.is_empty() {
        lines.push(detail_line(
            theme,
            "status",
            "none configured",
            theme.text_muted,
        ));
    } else {
        for server in &app.integrations.mcp {
            let status = server
                .error
                .as_deref()
                .filter(|error| !error.is_empty())
                .unwrap_or(&server.status);
            lines.push(Line::from(Span::styled(
                format!("  * {} {}", server.name, mcp_status_label(status)),
                Style::default().fg(mcp_status_color(&server.status, theme)),
            )));
        }
    }

    lines.push(section_line(theme, "LSP"));
    if app.integrations.lsp.is_empty() {
        lines.push(detail_line(
            theme,
            "status",
            "LSPs are disabled",
            theme.text_muted,
        ));
    } else {
        for server in &app.integrations.lsp {
            lines.push(Line::from(Span::styled(
                format!(
                    "  * {} {}{}",
                    display_lsp_name(server),
                    server.status,
                    lsp_root_label(server)
                ),
                Style::default().fg(if server.status == "connected" {
                    theme.success
                } else {
                    theme.warning
                }),
            )));
        }
    }

    if !app.runtime.server_health.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            app.runtime.server_health.clone(),
            Style::default().fg(theme.text_muted),
        )));
    }
    lines
}

fn section_line(theme: Theme, title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_owned(),
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
    ))
}

fn detail_line(theme: Theme, label: &str, value: impl Into<String>, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(theme.text_muted)),
        Span::styled(value.into(), Style::default().fg(color)),
    ])
}

fn latest_assistant(app: &App) -> Option<&MessageInfo> {
    app.transcript
        .iter()
        .rev()
        .map(|message| &message.info)
        .find(|info| info.role == "assistant" && info.tokens.total_or_sum() > 0)
}

fn current_model(app: &App, latest: Option<&MessageInfo>) -> (String, String, Option<String>) {
    let selected = app.catalog.selected_model.as_ref();
    let provider = selected
        .map(|model| model.provider_id.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            latest
                .map(|message| message.provider_id.clone())
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            app.session
                .current_session
                .as_ref()
                .and_then(|session| session.model.as_ref())
                .map(|model| model.provider_id.clone())
        })
        .unwrap_or_default();
    let model = selected
        .map(|model| model.id.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            latest
                .map(|message| message.model_id.clone())
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            app.session
                .current_session
                .as_ref()
                .and_then(|session| session.model.as_ref())
                .map(|model| model.id.clone())
        })
        .unwrap_or_default();
    let model = if model.is_empty() {
        app.catalog
            .provider_defaults
            .get(&provider)
            .cloned()
            .unwrap_or_default()
    } else {
        model
    };
    let variant = selected
        .and_then(|model| model.variant.clone())
        .or_else(|| {
            app.session
                .current_session
                .as_ref()
                .and_then(|session| session.model.as_ref())
                .and_then(|model| model.variant.clone())
        });
    (provider, model, variant)
}

fn model_context_limit(app: &App, provider_id: &str, model_id: &str) -> u64 {
    selected_model_info(app, provider_id, model_id)
        .map(|model| model.limit.context)
        .unwrap_or_default()
}

fn provider_label(app: &App, provider_id: &str) -> String {
    app.catalog
        .providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .map(|provider| fallback_text(&provider.name, &provider.id).to_owned())
        .unwrap_or_else(|| fallback_text(provider_id, "unknown").to_owned())
}

fn selected_model_info<'a>(
    app: &'a App,
    provider_id: &str,
    model_id: &str,
) -> Option<&'a crate::model::ModelInfo> {
    app.catalog
        .providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .and_then(|provider| {
            provider.models.iter().find_map(|(key, model)| {
                let id_matches = key == model_id || model.id == model_id;
                let provider_matches =
                    model.provider_id.is_empty() || model.provider_id == provider_id;
                id_matches.then_some(model).filter(|_| provider_matches)
            })
        })
}

fn model_label(model_id: &str, model: Option<&crate::model::ModelInfo>) -> String {
    model
        .map(|model| fallback_text(&model.name, &model.id).to_owned())
        .unwrap_or_else(|| fallback_text(model_id, "unknown").to_owned())
}

fn progress_bar(percent: f64) -> String {
    const WIDTH: usize = 10;
    let filled = ((percent.clamp(0.0, 100.0) / 100.0) * WIDTH as f64).round() as usize;
    format!("[{}{}]", "#".repeat(filled), "-".repeat(WIDTH - filled))
}

fn compact_tokens(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M tok", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K tok", value as f64 / 1_000.0)
    } else {
        format!("{value} tok")
    }
}

fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    output
}

fn format_money(value: f64) -> String {
    format!("${value:.4}")
}

fn fallback_text<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() { fallback } else { value }
}

fn truncate_sidebar_text(text: &str, limit: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut value = text.chars().take(limit).collect::<String>();
    if text.chars().count() > limit {
        value.push_str("...");
    }
    value
}

fn queued_prompt_lines(app: &App, width: u16, max_rows: usize, theme: Theme) -> Vec<Line<'static>> {
    app.prompt
        .queued_prompts()
        .take(max_rows)
        .enumerate()
        .map(|(index, prompt)| {
            let prefix = format!("[queued {}] ", index + 1);
            let prefix_width = UnicodeWidthStr::width(prefix.as_str());
            let available = usize::from(width).saturating_sub(prefix_width);
            let prompt = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
            let prompt = if prompt.is_empty() {
                "(non-text prompt)".to_owned()
            } else {
                truncate_to_display_width(&prompt, available)
            };
            Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default()
                        .fg(theme.secondary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(prompt, Style::default().fg(theme.text)),
            ])
            .style(Style::default().bg(theme.background_element))
        })
        .collect()
}

fn truncate_to_display_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }

    let suffix = if width >= 3 { "..." } else { "" };
    let content_width = width.saturating_sub(suffix.len());
    let mut used = 0;
    let mut value = String::new();
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > content_width {
            break;
        }
        value.push(character);
        used += character_width;
    }
    value.push_str(suffix);
    value
}

fn todo_status_marker(status: &str) -> &'static str {
    match status {
        "completed" => "[x]",
        "cancelled" => "[-]",
        "in_progress" => "[>]",
        _ => "[ ]",
    }
}

fn todo_status_color(status: &str, theme: Theme) -> Color {
    match status {
        "completed" => theme.success,
        "cancelled" => theme.text_muted,
        "in_progress" => theme.warning,
        _ => theme.text,
    }
}

fn mcp_status_label(status: &str) -> &str {
    match status {
        "connected" => "Connected",
        "disabled" => "Disabled",
        "needs_auth" => "Needs auth",
        "needs_client_registration" => "Needs client ID",
        "failed" => "Failed",
        value => value,
    }
}

fn mcp_status_color(status: &str, theme: Theme) -> Color {
    match status {
        "connected" => theme.success,
        "failed" | "needs_client_registration" => theme.warning,
        _ => theme.text_muted,
    }
}

fn display_lsp_name(server: &crate::model::LspStatus) -> &str {
    if server.name.is_empty() {
        &server.id
    } else {
        &server.name
    }
}

fn lsp_root_label(server: &crate::model::LspStatus) -> String {
    if server.root.is_empty() {
        String::new()
    } else {
        format!(
            "  ({})",
            server
                .root
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(&server.root)
        )
    }
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect, keys: &str, theme: Theme) {
    let mut line = Line::default();
    if let Some(response) = response_footer(app) {
        line.spans.push(Span::styled(
            response,
            Style::default().fg(theme.text_muted),
        ));
        line.spans.push(Span::raw("  "));
    }
    let message = app.notifications.active().unwrap_or(keys);
    let style = if app.notifications.active().is_some() {
        Style::default().fg(notification_level_color(
            app.notifications.active_level(),
            theme,
        ))
    } else {
        Style::default().fg(theme.text_muted)
    };
    line.spans.push(Span::styled(message.to_owned(), style));
    frame.render_widget(Paragraph::new(line), area);
}

fn response_footer(app: &App) -> Option<String> {
    let response = &app.runtime.response;
    if !response.has_data() {
        return None;
    }
    let state = if app.runtime.working {
        "responding"
    } else {
        "last"
    };
    Some(format!(
        "{state} {}",
        format_response_duration(response.elapsed())
    ))
}

fn format_response_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    let seconds = millis / 1_000;
    if seconds < 60 {
        format!("{seconds}.{:01}s", (millis % 1_000) / 100)
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

fn notification_level_color(level: NotificationLevel, theme: Theme) -> Color {
    match level {
        NotificationLevel::Info => theme.info,
        NotificationLevel::Success => theme.success,
        NotificationLevel::Warning => theme.warning,
        NotificationLevel::Error => theme.error,
    }
}

#[allow(dead_code)]
fn message_lines(message: &MessageWithParts) -> Vec<Line<'static>> {
    let theme = Theme::default();
    let (label, color) = if message.info.role == "user" {
        ("you", theme.accent)
    } else {
        ("assistant", theme.success)
    };
    let mut lines = vec![Line::from(Span::styled(
        format!("{label}  {}", short_id(&message.info.id)),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))];
    for part in &message.parts {
        match part.kind.as_str() {
            "text" => append_markdown(&mut lines, part.text.as_deref().unwrap_or(""), theme.text),
            "reasoning" => append_markdown(
                &mut lines,
                part.text.as_deref().unwrap_or(""),
                theme.text_muted,
            ),
            "tool" => append_tool_lines(&mut lines, part),
            "shell" => {
                let command = part.command.as_deref().unwrap_or("shell");
                let state = part
                    .state
                    .as_ref()
                    .and_then(|state| state.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                lines.push(Line::from(Span::styled(
                    format!("  [{state}] $ {command}"),
                    Style::default().fg(theme.warning),
                )));
                append_markdown(
                    &mut lines,
                    part.text.as_deref().unwrap_or(""),
                    theme.text_muted,
                );
            }
            "step-finish" => {}
            _ => {
                if let Some(text) = part.text.as_deref() {
                    append_markdown(&mut lines, text, theme.text_muted);
                }
            }
        }
    }
    lines.push(Line::from(""));
    lines
}

#[allow(dead_code)]
fn append_markdown(lines: &mut Vec<Line<'static>>, text: &str, color: Color) {
    if text.is_empty() {
        return;
    }
    lines.extend(MarkdownRenderer::render(
        text,
        MarkdownTheme::from_theme(Theme::default()).with_body(color),
    ));
}

#[allow(dead_code)]
fn append_tool_lines(lines: &mut Vec<Line<'static>>, part: &Part) {
    let theme = Theme::default();
    let tool = part.tool.as_deref().unwrap_or("tool");
    let state = part
        .state
        .as_ref()
        .and_then(|state| state.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    lines.push(Line::from(Span::styled(
        format!("  [{state}] {tool}"),
        Style::default().fg(theme.warning),
    )));

    let Some(state) = part.state.as_ref() else {
        return;
    };
    if let Some(input) = state.get("input")
        && (!input.is_string() || !input.as_str().is_some_and(str::is_empty))
    {
        append_tool_value(lines, "input", input);
    }
    if let Some(structured) = state.get("structured")
        && structured
            .as_object()
            .is_some_and(|object| !object.is_empty())
    {
        append_tool_value(lines, "structured", structured);
    }
    if let Some(content) = state.get("content").and_then(Value::as_array) {
        for item in content {
            match item.get("type").and_then(Value::as_str) {
                Some("text") => {
                    append_markdown(
                        lines,
                        item.get("text").and_then(Value::as_str).unwrap_or(""),
                        theme.text_muted,
                    );
                }
                Some("file") => {
                    let uri = item.get("uri").and_then(Value::as_str).unwrap_or("file");
                    let mime = item.get("mime").and_then(Value::as_str).unwrap_or("");
                    lines.push(Line::from(Span::styled(
                        format!("  file  {uri}  {mime}"),
                        Style::default().fg(theme.text_muted),
                    )));
                }
                _ => append_tool_value(lines, "content", item),
            }
        }
    }
    if let Some(error) = state.get("error").and_then(|error| error.get("message")) {
        lines.push(Line::from(Span::styled(
            format!("  error  {}", error.as_str().unwrap_or("tool failed")),
            Style::default().fg(theme.warning),
        )));
    }
    if let Some(result) = state.get("result").filter(|result| !result.is_null()) {
        append_tool_value(lines, "result", result);
    }
}

#[allow(dead_code)]
fn append_tool_value(lines: &mut Vec<Line<'static>>, label: &str, value: &Value) {
    let theme = Theme::default();
    lines.push(Line::from(Span::styled(
        format!("  {label}"),
        Style::default().fg(theme.text_muted),
    )));
    let rendered = if let Some(text) = value.as_str() {
        text.to_owned()
    } else {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    };
    append_markdown(lines, &rendered, theme.markdown_code_block);
}

fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

fn truncate_timeline_text(text: &str) -> String {
    const LIMIT: usize = 72;
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut value = text.chars().take(LIMIT).collect::<String>();
    if text.chars().count() > LIMIT {
        value.push_str("...");
    }
    value
}

fn format_timeline_time(timestamp: i64) -> String {
    if timestamp <= 0 {
        return "unknown".to_owned();
    }
    let timestamp_millis = if timestamp < 10_000_000_000 {
        timestamp.saturating_mul(1_000)
    } else {
        timestamp
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(timestamp_millis);
    let delta = now.saturating_sub(timestamp_millis);
    let seconds = delta / 1_000;
    if timestamp_millis > now {
        return format!("in {}s", timestamp_millis.saturating_sub(now) / 1_000);
    }
    if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3_600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3_600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

fn connection_color(app: &App, theme: Theme) -> Color {
    if app.runtime.is_connected() {
        theme.success
    } else {
        theme.warning
    }
}

fn status_color(app: &App, theme: Theme) -> Color {
    if app.runtime.working {
        theme.warning
    } else {
        theme.success
    }
}

fn current_directory(app: &App) -> String {
    app.session
        .current_session
        .as_ref()
        .and_then(Session::directory)
        .map(str::to_owned)
        .or_else(|| app.client.directory().map(str::to_owned))
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|directory| directory.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{draw, purple_selection_style};
    use crate::api::{ApiClient, ClientConfig};
    use crate::app::{App, Screen};
    use crate::dialog::OverlayState;
    use crate::event::{RevertFileDiff, RevertFileStatus, RevertState};
    use crate::model::{
        AgentInfo, CacheTokens, FileDiff, McpServer, MessageInfo, MessageTime, MessageWithParts,
        ModelInfo, ModelLimit, ModelRef, Part, PermissionRequest, PromptPart, PromptRequest,
        ProviderInfo, Session, Skill, TodoItem, TokenUsage, VcsDiffMode, VcsFileDiff,
        VcsFileStatus, VcsInfo, WorkspaceFile,
    };
    use crate::prompt_state::PromptSubmission;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn purple_selection_uses_the_theme_secondary_color() {
        let theme = crate::theme::Theme::default();
        let style = purple_selection_style(theme);

        assert_eq!(style.fg, Some(theme.secondary));
        assert_eq!(style.bg, Some(theme.background_element));
    }

    fn app() -> App {
        let client = ApiClient::new(ClientConfig {
            base_url: "http://127.0.0.1:4096".to_owned(),
            username: "opencode".to_owned(),
            password: None,
            directory: None,
            workspace: None,
        })
        .expect("test client should build");
        App::new_for_tests(Arc::new(client))
    }

    fn rendered(app: &mut App) -> String {
        rendered_at(app, 80, 24)
    }

    fn rendered_at(app: &mut App, width: u16, height: u16) -> String {
        rendered_rows_at(app, width, height).concat()
    }

    fn rendered_rows_at(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| draw(frame, app))
            .expect("test draw should succeed");
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| {
                        buffer
                            .cell((x, y))
                            .expect("test coordinates should be inside the buffer")
                            .symbol()
                    })
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn queued_prompts_are_pinned_to_the_transcript_bottom_in_fifo_order() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.runtime.sidebar_visible = false;
        app.session.current_session = Some(Session {
            id: "ses_queue".to_owned(),
            ..Session::default()
        });
        for prompt in ["first queued", "second queued"] {
            app.prompt.enqueue(PromptSubmission {
                session_id: Some("ses_queue".to_owned()),
                request: PromptRequest::from_text(prompt, None, None),
                prompt: prompt.to_owned(),
                attachments: Vec::new(),
                subtasks: Vec::new(),
            });
        }

        let rows = rendered_rows_at(&mut app, 80, 24);
        let first_row = rows
            .iter()
            .position(|row| row.contains("[queued 1] first queued"))
            .expect("the first queued prompt should render");
        let second_row = rows
            .iter()
            .position(|row| row.contains("[queued 2] second queued"))
            .expect("the second queued prompt should render");

        assert_eq!(second_row, first_row + 1);
        assert!(
            first_row >= 24 / 2,
            "the queue should be pinned near the bottom"
        );
    }

    #[test]
    fn sidebar_scroll_uses_wrapped_inner_dimensions() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_sidebar_wrap".to_owned(),
            title: "Sidebar width test".to_owned(),
            ..Session::default()
        });

        let _ = rendered_at(&mut app, 40, 18);

        assert!(
            app.sidebar_scroll.max_offset() > 0,
            "sidebar content should exceed its inner viewport"
        );
        assert_eq!(
            app.sidebar_scroll.offset(),
            0,
            "the sidebar should open at its first line instead of its last"
        );
    }
    #[test]
    fn renders_home_screen_with_session_list() {
        let mut app = app();
        app.session.sessions.push(Session {
            id: "ses_123456789".to_owned(),
            title: "Review the runtime".to_owned(),
            ..Session::default()
        });

        let output = rendered(&mut app);

        assert!(output.contains("OpenCode"));
        assert!(output.contains("Review the runtime"));
    }

    #[test]
    fn renders_directory_and_single_help_shortcut_in_the_header_and_footer() {
        let client = ApiClient::new(ClientConfig {
            base_url: "http://127.0.0.1:4096".to_owned(),
            username: "opencode".to_owned(),
            password: None,
            directory: Some("E:/workspace".to_owned()),
            workspace: None,
        })
        .expect("test client should build");
        let mut app = App::new_for_tests(Arc::new(client));

        let output = rendered(&mut app);

        assert!(output.contains("[E:/workspace]"));
        assert!(output.contains("? shortcuts"));
        assert!(!output.contains("Enter open"));
    }

    #[test]
    fn renders_home_screen_at_narrow_terminal_sizes() {
        let mut app = app();
        app.session.sessions.push(Session {
            id: "ses_narrow".to_owned(),
            title: "Narrow terminal".to_owned(),
            ..Session::default()
        });

        for width in [24, 32, 48] {
            let output = rendered_at(&mut app, width, 10);
            assert!(
                !output.trim().is_empty(),
                "rendered output should not be empty"
            );
            assert!(
                output.contains("OpenCode"),
                "header should survive width {width}"
            );
        }
    }

    #[test]
    fn renders_session_transcript_and_prompt() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            title: "Runtime refactor".to_owned(),
            ..Session::default()
        });
        app.transcript.replace(vec![MessageWithParts {
            info: MessageInfo {
                id: "msg_1".to_owned(),
                session_id: "ses_1".to_owned(),
                role: "assistant".to_owned(),
                time: MessageTime::default(),
                ..MessageInfo::default()
            },
            parts: vec![
                Part {
                    id: "part_1".to_owned(),
                    session_id: "ses_1".to_owned(),
                    message_id: "msg_1".to_owned(),
                    kind: "text".to_owned(),
                    text: Some("HTTP work runs in the background.".to_owned()),
                    ..Part::default()
                },
                Part {
                    id: "shell_1".to_owned(),
                    session_id: "ses_1".to_owned(),
                    message_id: "msg_1".to_owned(),
                    kind: "shell".to_owned(),
                    command: Some("pwd".to_owned()),
                    text: Some("E:/project".to_owned()),
                    state: Some(serde_json::json!({ "status": "completed" })),
                    ..Part::default()
                },
            ],
        }]);
        app.prompt.composer.set_text("check status");

        let output = rendered_at(&mut app, 120, 48);

        assert!(output.contains("Runtime refactor"));
        assert!(output.contains("HTTP work runs in the background."));
        assert!(output.contains("$ pwd"));
        assert!(output.contains("check status"));
    }

    #[test]
    fn renders_response_duration_without_sent_or_received_counts() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_metrics".to_owned(),
            title: "Response metrics".to_owned(),
            ..Session::default()
        });
        app.runtime.begin_response("hello world");
        app.runtime.set_response_message("msg_metrics");
        app.runtime.add_response_output("answer");

        let output = rendered_at(&mut app, 120, 24);

        assert!(output.contains("responding"));
        assert!(!output.contains("sent "));
        assert!(!output.contains("recv "));

        app.runtime.finish_response_tokens(
            "msg_metrics",
            TokenUsage {
                input: 120,
                output: 45,
                ..TokenUsage::default()
            },
        );
        app.runtime.set_working(false);
        let output = rendered_at(&mut app, 120, 24);

        assert!(output.contains("last"));
        assert!(!output.contains("sent "));
        assert!(!output.contains("recv "));
    }

    #[test]
    fn renders_cjk_and_emoji_in_a_narrow_session() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_unicode".to_owned(),
            title: "中文会话 🚀".to_owned(),
            ..Session::default()
        });
        app.transcript.replace(vec![MessageWithParts {
            info: MessageInfo {
                id: "msg_unicode".to_owned(),
                session_id: "ses_unicode".to_owned(),
                role: "assistant".to_owned(),
                time: MessageTime::default(),
                ..MessageInfo::default()
            },
            parts: vec![Part {
                id: "part_unicode".to_owned(),
                session_id: "ses_unicode".to_owned(),
                message_id: "msg_unicode".to_owned(),
                kind: "text".to_owned(),
                text: Some("你好，世界。Status: ready 🚀".to_owned()),
                ..Part::default()
            }],
        }]);
        app.prompt.composer.set_text("检查状态 ✅");

        let output = rendered_at(&mut app, 48, 18);
        let compact = output
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();

        assert!(compact.contains("中文会话"), "rendered output: {output:?}");
        assert!(compact.contains("你好"), "rendered output: {output:?}");
        assert!(compact.contains("检查状态"), "rendered output: {output:?}");
    }

    #[test]
    fn transcript_keeps_text_at_the_inner_right_edge_visible() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_transcript_edge".to_owned(),
            title: "Transcript edge".to_owned(),
            ..Session::default()
        });
        let text = "123456789012345678901234567";
        app.transcript.replace(vec![MessageWithParts {
            info: MessageInfo {
                id: "msg_transcript_edge".to_owned(),
                session_id: "ses_transcript_edge".to_owned(),
                role: "assistant".to_owned(),
                time: MessageTime::default(),
                ..MessageInfo::default()
            },
            parts: vec![Part {
                id: "part_transcript_edge".to_owned(),
                session_id: "ses_transcript_edge".to_owned(),
                message_id: "msg_transcript_edge".to_owned(),
                kind: "text".to_owned(),
                text: Some(text.to_owned()),
                ..Part::default()
            }],
        }]);

        let output = rendered_at(&mut app, 40, 18);

        assert!(
            output.contains(text),
            "the complete line should be visible at the transcript inner edge: {output:?}"
        );
    }

    #[test]
    fn renders_markdown_and_rich_tool_state() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            title: "Markdown output".to_owned(),
            ..Session::default()
        });
        app.transcript.replace(vec![MessageWithParts {
            info: MessageInfo {
                id: "msg_1".to_owned(),
                session_id: "ses_1".to_owned(),
                role: "assistant".to_owned(),
                time: MessageTime::default(),
                ..MessageInfo::default()
            },
            parts: vec![
                Part {
                    id: "text_1".to_owned(),
                    session_id: "ses_1".to_owned(),
                    message_id: "msg_1".to_owned(),
                    kind: "text".to_owned(),
                    text: Some("# Result\n\n- done\n```rust\nlet ready = true;\n```".to_owned()),
                    ..Part::default()
                },
                Part {
                    id: "tool_1".to_owned(),
                    session_id: "ses_1".to_owned(),
                    message_id: "msg_1".to_owned(),
                    kind: "tool".to_owned(),
                    tool: Some("bash".to_owned()),
                    state: Some(serde_json::json!({
                        "status": "completed",
                        "input": { "command": "pwd" },
                        "structured": { "exitCode": 0 },
                        "content": [
                            { "type": "text", "text": "command completed" },
                            { "type": "file", "uri": "file:///tmp/out", "mime": "text/plain" }
                        ],
                        "result": "ok"
                    })),
                    ..Part::default()
                },
            ],
        }]);

        let output = rendered_at(&mut app, 120, 60);

        assert!(output.contains("[code: rust]"));
        assert!(output.contains("let ready = true;"));
        assert!(output.contains("input"));
        assert!(output.contains("command completed"));
        assert!(output.contains("file:///tmp/out"));
        assert!(output.contains("result"));
    }

    #[test]
    fn renders_runtime_sidebar_status_sections() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            title: "Runtime status".to_owned(),
            agent: Some("build".to_owned()),
            cost: 0.125,
            tokens: TokenUsage {
                total: 1_024,
                input: 512,
                output: 256,
                reasoning: 128,
                cache: CacheTokens {
                    read: 128,
                    write: 64,
                },
            },
            model: Some(ModelRef {
                id: "gpt-test".to_owned(),
                provider_id: "auto-gpt-pro".to_owned(),
                variant: Some("fast".to_owned()),
            }),
            ..Session::default()
        });
        app.catalog.skills.push(Skill {
            name: "demo-skill".to_owned(),
            ..Skill::default()
        });
        app.integrations.mcp.push(McpServer {
            name: "chrome-devtools".to_owned(),
            status: "connected".to_owned(),
            ..McpServer::default()
        });
        app.runtime.server_health = "healthy / 1.18.15".to_owned();
        app.integrations.todos.push(TodoItem {
            id: "todo_1".to_owned(),
            content: "Review the current diff".to_owned(),
            status: "in_progress".to_owned(),
            priority: "high".to_owned(),
        });
        app.integrations.diffs.push(FileDiff {
            file: "src/main.rs".to_owned(),
            additions: 4,
            deletions: 1,
            ..FileDiff::default()
        });
        app.integrations.vcs = Some(VcsInfo {
            branch: "feature/runtime".to_owned(),
            ..VcsInfo::default()
        });
        app.integrations.vcs_status.push(VcsFileStatus {
            file: "src/main.rs".to_owned(),
            additions: 4,
            deletions: 1,
            status: "modified".to_owned(),
        });
        app.integrations.revert_state = Some(RevertState {
            message_id: "msg_revert".to_owned(),
            part_id: Some("part_revert".to_owned()),
            snapshot: Some("snap_revert".to_owned()),
            diff: Some("diff --git".to_owned()),
            files: vec![RevertFileDiff {
                path: "README.md".to_owned(),
                status: RevertFileStatus::Modified,
                additions: 2,
                deletions: 1,
                patch: "@@ -1 +1 @@".to_owned(),
            }],
        });
        app.catalog.providers.push(ProviderInfo {
            id: "auto-gpt-pro".to_owned(),
            name: "Auto GPT Pro".to_owned(),
            models: HashMap::from([(
                "gpt-test".to_owned(),
                ModelInfo {
                    id: "gpt-test".to_owned(),
                    provider_id: "auto-gpt-pro".to_owned(),
                    name: "GPT Test".to_owned(),
                    limit: ModelLimit {
                        context: 8_192,
                        output: 1_024,
                    },
                    ..ModelInfo::default()
                },
            )]),
        });

        let output = rendered_at(&mut app, 120, 80);

        assert!(output.contains("Cache stats"));
        assert!(output.contains("Token distribution"));
        assert!(output.contains("Context"));
        assert!(output.contains("Todo (1)"));
        assert!(output.contains("Review the current diff"));
        assert!(output.contains("Modified files (1)"));
        assert!(output.contains("feature/runtime"));
        assert!(!output.contains("Skills ("));
        assert!(!output.contains("demo-skill"));
        assert!(output.contains("MCP"));
        assert!(output.contains("chrome-devtools"));
        assert!(output.contains("LSPs are disabled"));
        assert!(output.contains("Auto GPT Pro"));
        assert!(output.contains("GPT Test"));
        assert!(output.contains("Revert"));
        assert!(output.contains("README.md"));
    }

    #[test]
    fn new_session_sidebar_uses_the_latest_selected_provider_and_model() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_new".to_owned(),
            title: "New session".to_owned(),
            ..Session::default()
        });
        app.catalog.selected_model = Some(ModelRef {
            id: "latest_model".to_owned(),
            provider_id: "latest_provider".to_owned(),
            ..ModelRef::default()
        });
        app.catalog.providers.push(ProviderInfo {
            id: "latest_provider".to_owned(),
            name: "Latest Provider".to_owned(),
            models: HashMap::from([(
                "latest_model".to_owned(),
                ModelInfo {
                    id: "latest_model".to_owned(),
                    provider_id: "latest_provider".to_owned(),
                    name: "Latest Model".to_owned(),
                    ..ModelInfo::default()
                },
            )]),
        });

        let output = rendered_at(&mut app, 120, 30);

        assert!(output.contains("Latest Provider"));
        assert!(output.contains("Latest Model"));
        assert!(!output.contains("provider: unknown"));
        assert!(!output.contains("model: unknown"));
    }

    #[test]
    fn renders_permission_action_panel() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            title: "Needs approval".to_owned(),
            ..Session::default()
        });
        app.pending.permissions.push(PermissionRequest {
            id: "per_1".to_owned(),
            session_id: "ses_1".to_owned(),
            permission: "bash".to_owned(),
            patterns: vec!["git status".to_owned()],
            ..PermissionRequest::default()
        });

        let output = rendered(&mut app);

        assert!(output.contains("permission required"));
        assert!(output.contains("allow once"));
        assert!(output.contains("git status"));
    }

    #[test]
    fn renders_model_and_skill_overlays() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            title: "Overlay test".to_owned(),
            model: Some(ModelRef {
                id: "model_1".to_owned(),
                provider_id: "provider_1".to_owned(),
                ..ModelRef::default()
            }),
            ..Session::default()
        });
        app.catalog.providers.push(ProviderInfo {
            id: "provider_1".to_owned(),
            name: "Provider One".to_owned(),
            models: HashMap::from([(
                "model_1".to_owned(),
                ModelInfo {
                    id: "model_1".to_owned(),
                    name: "Model One".to_owned(),
                    variants: HashMap::from([("fast".to_owned(), serde_json::json!({}))]),
                    ..ModelInfo::default()
                },
            )]),
        });
        app.catalog.skills.push(Skill {
            name: "review".to_owned(),
            description: "Review the current change".to_owned(),
        });
        app.catalog.agents.push(AgentInfo {
            name: "build".to_owned(),
            description: "Build the project".to_owned(),
            mode: "primary".to_owned(),
            ..AgentInfo::default()
        });

        app.overlay = Some(OverlayState::Model {
            query: String::new(),
            selected: 0,
        });
        let model_output = rendered_at(&mut app, 120, 40);
        assert!(model_output.contains("Select model"));
        assert!(model_output.contains("provider_1/model_1"));

        app.overlay = Some(OverlayState::Skill {
            query: String::new(),
            selected: 0,
        });
        let skill_output = rendered_at(&mut app, 120, 40);
        assert!(skill_output.contains("Select skill"));
        assert!(skill_output.contains("review"));

        app.overlay = Some(OverlayState::Agent {
            query: String::new(),
            selected: 0,
        });
        let agent_output = rendered_at(&mut app, 120, 40);
        assert!(agent_output.contains("Select agent"));
        assert!(agent_output.contains("build"));

        app.overlay = Some(OverlayState::Variant {
            query: String::new(),
            selected: 0,
        });
        let variant_output = rendered_at(&mut app, 120, 40);
        assert!(variant_output.contains("Select variant"));
        assert!(variant_output.contains("fast"));
    }

    #[test]
    fn renders_diagnostics_overlay_with_recent_notifications() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_diagnostics".to_owned(),
            title: "Diagnostics".to_owned(),
            ..Session::default()
        });
        app.runtime.mark_connected();
        app.runtime.server_health = "healthy".to_owned();
        app.notifications.success("Connected to server");
        app.notifications.error("Request failed");
        app.overlay = Some(OverlayState::Diagnostics);

        let output = rendered_at(&mut app, 120, 40);

        assert!(output.contains("Diagnostics"));
        assert!(output.contains("healthy"));
        assert!(output.contains("Recent notifications"));
        assert!(output.contains("ERROR"));
        assert!(output.contains("Request failed"));
    }

    #[test]
    fn renders_theme_selector_with_builtin_choices() {
        let mut app = app();
        app.overlay = Some(OverlayState::Theme { selected: 0 });

        let output = rendered_at(&mut app, 100, 24);

        assert!(output.contains("Select theme"));
        assert!(output.contains("material"));
        assert!(output.contains("material-light"));
    }

    #[test]
    fn renders_session_actions_and_mention_overlay() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            title: "Action overlays".to_owned(),
            ..Session::default()
        });
        app.session
            .sessions
            .push(app.session.current_session.clone().expect("session"));
        app.catalog.workspace_files.push(WorkspaceFile {
            path: "src/main.rs".to_owned(),
            is_directory: false,
        });
        app.catalog.agents.push(AgentInfo {
            name: "explore".to_owned(),
            description: "Inspect files".to_owned(),
            mode: "subagent".to_owned(),
            ..AgentInfo::default()
        });
        app.prompt.attachments.push(PromptPart::file(
            "text/plain",
            "data:text/plain;base64,aGVsbG8=",
            Some("notes.txt".to_owned()),
        ));
        assert!(rendered_at(&mut app, 100, 30).contains("notes.txt"));
        app.prompt
            .subtasks
            .push(PromptPart::subtask("Inspect", "Inspect", "explore"));
        assert!(rendered_at(&mut app, 100, 30).contains("subtask:explore"));

        app.overlay = Some(OverlayState::RenameSession {
            value: "Renamed".to_owned(),
        });
        assert!(rendered_at(&mut app, 100, 30).contains("Renamed"));

        app.overlay = Some(OverlayState::DeleteSession {
            session_id: "ses_1".to_owned(),
        });
        assert!(rendered_at(&mut app, 100, 30).contains("permanently deletes"));

        app.overlay = Some(OverlayState::MoveSession {
            session_id: "ses_1".to_owned(),
            destination: "E:/workspace/next".to_owned(),
            move_changes: true,
        });
        let move_output = rendered_at(&mut app, 100, 30);
        assert!(move_output.contains("Move session"));
        assert!(move_output.contains("E:/workspace/next"));
        assert!(move_output.contains("Transfer local changes"));

        app.integrations.diffs.push(FileDiff {
            file: "src/main.rs".to_owned(),
            before: "old line".to_owned(),
            after: "new line".to_owned(),
            additions: 1,
            deletions: 1,
        });
        app.overlay = Some(OverlayState::SessionDiff {
            selected: 0,
            scroll: 0,
        });
        let diff_output = rendered_at(&mut app, 100, 30);
        assert!(diff_output.contains("Session diff"));
        assert!(diff_output.contains("src/main.rs"));
        assert!(diff_output.contains("- old line"));
        assert!(diff_output.contains("+ new line"));

        app.integrations.vcs_diffs.push(VcsFileDiff {
            file: "src/lib.rs".to_owned(),
            patch: "diff --git a/src/lib.rs b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n".to_owned(),
            additions: 1,
            deletions: 1,
            status: "modified".to_owned(),
        });
        app.overlay = Some(OverlayState::VcsDiff {
            mode: VcsDiffMode::Git,
            selected: 0,
            scroll: 0,
        });
        let vcs_output = rendered_at(&mut app, 100, 30);
        assert!(vcs_output.contains("VCS diff"));
        assert!(vcs_output.contains("working tree"));
        assert!(vcs_output.contains("src/lib.rs"));
        assert!(vcs_output.contains("-old"));
        assert!(vcs_output.contains("+new"));

        app.overlay = Some(OverlayState::Mention {
            query: "src".to_owned(),
            selected: 0,
            start: 0,
            end: 4,
        });
        assert!(rendered_at(&mut app, 100, 30).contains("@src/main.rs"));

        app.overlay = Some(OverlayState::AttachFile {
            value: "notes.txt".to_owned(),
        });
        assert!(rendered_at(&mut app, 100, 30).contains("Attach file"));

        app.overlay = Some(OverlayState::Subtask {
            prompt: "Inspect the diff".to_owned(),
            selected: 0,
        });
        assert!(rendered_at(&mut app, 100, 30).contains("Add subtask"));

        app.overlay = Some(OverlayState::PromptPanel { selected: 0 });
        app.prompt.attachments.push(PromptPart::file(
            "text/plain",
            "data:text/plain;base64,aGVsbG8=",
            Some("notes.txt".to_owned()),
        ));
        app.prompt.subtasks.push(PromptPart::subtask(
            "Inspect the diff",
            "Inspect",
            "explore",
        ));
        let panel_output = rendered_at(&mut app, 100, 30);
        assert!(panel_output.contains("Prompt panel"));
        assert!(panel_output.contains("notes.txt"));
        assert!(panel_output.contains("subtask:explore"));

        app.overlay = Some(OverlayState::PromptOptions { selected: 0 });
        assert!(rendered_at(&mut app, 100, 30).contains("Prompt options"));

        app.prompt
            .options
            .tool_overrides
            .insert("bash".to_owned(), false);
        app.overlay = Some(OverlayState::PromptTools { selected: 0 });
        let tool_output = rendered_at(&mut app, 100, 30);
        assert!(tool_output.contains("Tool overrides"));
        assert!(tool_output.contains("bash"));
        assert!(tool_output.contains("off"));

        app.overlay = Some(OverlayState::PromptToolName {
            value: "mcp_search".to_owned(),
        });
        assert!(rendered_at(&mut app, 100, 30).contains("Add tool override"));
    }
}
