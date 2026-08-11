use crate::theme::Theme;
use ratatui::style::Color;

/// Available commands in the command palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: CommandCategory,
    pub keybinding: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
    Navigation,
    Session,
    Editing,
    View,
    Help,
}

impl CommandCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Navigation => "Navigation",
            Self::Session => "Session",
            Self::Editing => "Editing",
            Self::View => "View",
            Self::Help => "Help",
        }
    }

    #[allow(dead_code)]
    pub fn color(&self) -> Color {
        self.color_with(Theme::default())
    }

    pub fn color_with(&self, theme: Theme) -> Color {
        match self {
            Self::Navigation => theme.primary,
            Self::Session => theme.success,
            Self::Editing => theme.warning,
            Self::View => theme.text,
            Self::Help => theme.text_muted,
        }
    }
}

/// Get all available commands.
pub fn all_commands() -> Vec<Command> {
    vec![
        // Navigation
        Command {
            id: "home",
            name: "Go to Home",
            description: "Return to session list",
            category: CommandCategory::Navigation,
            keybinding: Some("Esc"),
        },
        Command {
            id: "scroll_top",
            name: "Scroll to Top",
            description: "Jump to the beginning of the transcript",
            category: CommandCategory::Navigation,
            keybinding: Some("Home"),
        },
        Command {
            id: "scroll_bottom",
            name: "Scroll to Bottom",
            description: "Jump to the latest message",
            category: CommandCategory::Navigation,
            keybinding: Some("End"),
        },
        Command {
            id: "page_up",
            name: "Page Up",
            description: "Scroll up one page",
            category: CommandCategory::Navigation,
            keybinding: Some("PageUp"),
        },
        Command {
            id: "page_down",
            name: "Page Down",
            description: "Scroll down one page",
            category: CommandCategory::Navigation,
            keybinding: Some("PageDown"),
        },
        // Session
        Command {
            id: "new_session",
            name: "New Session",
            description: "Create a new session",
            category: CommandCategory::Session,
            keybinding: Some("n"),
        },
        Command {
            id: "refresh_sessions",
            name: "Refresh Sessions",
            description: "Reload the session list",
            category: CommandCategory::Session,
            keybinding: Some("r"),
        },
        Command {
            id: "rename_session",
            name: "Rename Session",
            description: "Change the title of the active session",
            category: CommandCategory::Session,
            keybinding: Some("F2 / e"),
        },
        Command {
            id: "delete_session",
            name: "Delete Session",
            description: "Permanently remove the active session",
            category: CommandCategory::Session,
            keybinding: Some("Delete / d"),
        },
        Command {
            id: "archive_session",
            name: "Archive Session",
            description: "Move the active session out of the active list",
            category: CommandCategory::Session,
            keybinding: None,
        },
        Command {
            id: "restore_session",
            name: "Restore Session",
            description: "Return an archived session to the active list",
            category: CommandCategory::Session,
            keybinding: None,
        },
        Command {
            id: "move_session",
            name: "Move Session",
            description: "Move the session to another project directory",
            category: CommandCategory::Session,
            keybinding: None,
        },
        Command {
            id: "session_diff",
            name: "Open Session Diff",
            description: "Review changed files and their full session diff",
            category: CommandCategory::View,
            keybinding: None,
        },
        Command {
            id: "vcs_diff",
            name: "Open VCS Diff",
            description: "Review working-tree changes or compare with the default branch",
            category: CommandCategory::View,
            keybinding: None,
        },
        Command {
            id: "show_archived_sessions",
            name: "Show Archived Sessions",
            description: "Browse sessions hidden from the active list",
            category: CommandCategory::Session,
            keybinding: Some("a on Home"),
        },
        Command {
            id: "show_active_sessions",
            name: "Show Active Sessions",
            description: "Browse the active session list",
            category: CommandCategory::Session,
            keybinding: Some("A on Home"),
        },
        Command {
            id: "export_session",
            name: "Export Session",
            description: "Write the active transcript to a Markdown file",
            category: CommandCategory::Session,
            keybinding: Some("Ctrl-E"),
        },
        Command {
            id: "abort_session",
            name: "Abort Session",
            description: "Interrupt the running session",
            category: CommandCategory::Session,
            keybinding: Some("Ctrl-X"),
        },
        Command {
            id: "compact_session",
            name: "Compact Session",
            description: "Summarize the current session to reduce its context",
            category: CommandCategory::Session,
            keybinding: Some("/compact"),
        },
        Command {
            id: "select_model",
            name: "Select Model",
            description: "Choose a provider/model for the next prompt",
            category: CommandCategory::Session,
            keybinding: Some("/model"),
        },
        Command {
            id: "select_agent",
            name: "Select Agent",
            description: "Choose an agent for the next prompt",
            category: CommandCategory::Session,
            keybinding: Some("/agent"),
        },
        Command {
            id: "select_variant",
            name: "Select Variant",
            description: "Choose a model variant",
            category: CommandCategory::Session,
            keybinding: Some("/variant"),
        },
        Command {
            id: "select_skill",
            name: "Select Skill",
            description: "Invoke a skill",
            category: CommandCategory::Session,
            keybinding: Some("/skill"),
        },
        Command {
            id: "session_timeline",
            name: "Jump to Timeline",
            description: "Jump to a user prompt in the current session",
            category: CommandCategory::Session,
            keybinding: Some("/timeline"),
        },
        Command {
            id: "fork_session",
            name: "Fork Session",
            description: "Create a new session from the current timeline",
            category: CommandCategory::Session,
            keybinding: Some("/fork"),
        },
        Command {
            id: "share_session",
            name: "Share Session",
            description: "Create a shareable link for the active session",
            category: CommandCategory::Session,
            keybinding: Some("/share"),
        },
        Command {
            id: "unshare_session",
            name: "Unshare Session",
            description: "Remove the shareable link from the active session",
            category: CommandCategory::Session,
            keybinding: Some("/unshare"),
        },
        // Editing
        Command {
            id: "submit_prompt",
            name: "Submit Prompt",
            description: "Send the current prompt",
            category: CommandCategory::Editing,
            keybinding: Some("Enter"),
        },
        Command {
            id: "insert_newline",
            name: "Insert Newline",
            description: "Add a line break in the prompt",
            category: CommandCategory::Editing,
            keybinding: Some("Shift-Enter"),
        },
        Command {
            id: "select_all",
            name: "Select All",
            description: "Select all prompt text",
            category: CommandCategory::Editing,
            keybinding: Some("Ctrl-A"),
        },
        Command {
            id: "undo",
            name: "Undo",
            description: "Undo the last edit",
            category: CommandCategory::Editing,
            keybinding: Some("Ctrl-U"),
        },
        Command {
            id: "redo",
            name: "Redo",
            description: "Redo the last undone edit",
            category: CommandCategory::Editing,
            keybinding: Some("Ctrl-R"),
        },
        Command {
            id: "attach_file",
            name: "Attach File",
            description: "Read a local file into the next prompt",
            category: CommandCategory::Editing,
            keybinding: Some("Ctrl-Shift-U"),
        },
        Command {
            id: "browse_files",
            name: "Browse Workspace Files",
            description: "Choose a file from the current workspace",
            category: CommandCategory::Editing,
            keybinding: Some("Tab in Attach File"),
        },
        Command {
            id: "remove_attachment",
            name: "Remove Last Attachment",
            description: "Remove the most recently attached file",
            category: CommandCategory::Editing,
            keybinding: Some("Ctrl-Shift-Backspace"),
        },
        Command {
            id: "add_subtask",
            name: "Add Subtask",
            description: "Queue a delegated subtask for the next prompt",
            category: CommandCategory::Editing,
            keybinding: Some("Ctrl-Shift-T"),
        },
        Command {
            id: "prompt_options",
            name: "Prompt Panel",
            description: "Review and edit the complete structured prompt",
            category: CommandCategory::Editing,
            keybinding: Some("Ctrl-Shift-O"),
        },
        Command {
            id: "history_previous",
            name: "Previous Prompt",
            description: "Navigate to previous prompt in history",
            category: CommandCategory::Editing,
            keybinding: Some("Up"),
        },
        Command {
            id: "history_next",
            name: "Next Prompt",
            description: "Navigate to next prompt in history",
            category: CommandCategory::Editing,
            keybinding: Some("Down"),
        },
        // View
        Command {
            id: "toggle_sidebar",
            name: "Toggle Sidebar",
            description: "Show/hide the runtime sidebar",
            category: CommandCategory::View,
            keybinding: None,
        },
        Command {
            id: "toggle_transcript_blocks",
            name: "Collapse Transcript Blocks",
            description: "Collapse or expand reasoning and tool details",
            category: CommandCategory::View,
            keybinding: Some("Ctrl-Shift-B"),
        },
        Command {
            id: "select_theme",
            name: "Select Theme",
            description: "Change the active terminal theme",
            category: CommandCategory::View,
            keybinding: None,
        },
        Command {
            id: "toggle_mcp",
            name: "Toggle MCPs",
            description: "Connect or disconnect configured MCP servers",
            category: CommandCategory::View,
            keybinding: None,
        },
        // Help
        Command {
            id: "show_help",
            name: "Show Help",
            description: "Display keyboard shortcuts and commands",
            category: CommandCategory::Help,
            keybinding: Some("?"),
        },
        Command {
            id: "show_diagnostics",
            name: "Show Diagnostics",
            description: "Inspect runtime status and recent notifications",
            category: CommandCategory::Help,
            keybinding: None,
        },
        Command {
            id: "refresh_diagnostics",
            name: "Refresh Diagnostics",
            description: "Reload runtime, catalog, and integration status",
            category: CommandCategory::Help,
            keybinding: None,
        },
        Command {
            id: "command_palette",
            name: "Command Palette",
            description: "Open the command palette",
            category: CommandCategory::Help,
            keybinding: Some("Ctrl-P"),
        },
        Command {
            id: "quit",
            name: "Quit",
            description: "Exit the application",
            category: CommandCategory::Help,
            keybinding: Some("Ctrl-C / q"),
        },
    ]
}

/// Filter commands by query string.
pub fn filter_commands(query: &str) -> Vec<Command> {
    let query_lower = query.to_lowercase();
    if query_lower.is_empty() {
        return all_commands();
    }

    all_commands()
        .into_iter()
        .filter(|cmd| {
            cmd.name.to_lowercase().contains(&query_lower)
                || cmd.description.to_lowercase().contains(&query_lower)
                || cmd.id.to_lowercase().contains(&query_lower)
                || cmd
                    .keybinding
                    .map(|k| k.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_commands_returns_non_empty_list() {
        let commands = all_commands();
        assert!(!commands.is_empty());
        assert!(commands.len() > 10);
    }

    #[test]
    fn all_commands_have_unique_ids() {
        let commands = all_commands();
        let mut ids = std::collections::HashSet::new();
        for cmd in commands {
            assert!(ids.insert(cmd.id), "Duplicate command id: {}", cmd.id);
        }
    }

    #[test]
    fn filter_commands_by_name() {
        let results = filter_commands("session");
        assert!(!results.is_empty());
        assert!(results.iter().any(|c| c.id == "new_session"));
        assert!(results.iter().any(|c| c.id == "abort_session"));
    }

    #[test]
    fn filter_commands_by_keybinding() {
        let results = filter_commands("ctrl");
        assert!(!results.is_empty());
        assert!(results.iter().any(|c| c.keybinding == Some("Ctrl-A")));
    }

    #[test]
    fn empty_query_returns_all_commands() {
        let all = all_commands();
        let filtered = filter_commands("");
        assert_eq!(all.len(), filtered.len());
    }

    #[test]
    fn diagnostics_command_is_discoverable() {
        let results = filter_commands("diagnostics");
        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .any(|command| command.id == "show_diagnostics")
        );
        assert!(
            results
                .iter()
                .any(|command| command.id == "refresh_diagnostics")
        );
    }

    #[test]
    fn timeline_commands_are_discoverable() {
        let timeline = filter_commands("timeline");
        assert!(
            timeline
                .iter()
                .any(|command| command.id == "session_timeline")
        );
        let fork = filter_commands("fork");
        assert!(fork.iter().any(|command| command.id == "fork_session"));
    }

    #[test]
    fn categories_have_distinct_colors() {
        use std::collections::HashSet;
        let colors: HashSet<_> = vec![
            CommandCategory::Navigation.color(),
            CommandCategory::Session.color(),
            CommandCategory::Editing.color(),
            CommandCategory::View.color(),
            CommandCategory::Help.color(),
        ]
        .into_iter()
        .collect();

        assert_eq!(colors.len(), 5, "Categories should have distinct colors");
    }
}
