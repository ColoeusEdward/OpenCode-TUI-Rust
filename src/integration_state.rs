use crate::event::RevertState;
use crate::model::{
    FileDiff, LspStatus, McpServer, McpStatus, TodoItem, VcsDiffMode, VcsFileDiff, VcsFileStatus,
    VcsInfo,
};
use std::collections::HashMap;

#[derive(Default)]
pub struct IntegrationState {
    pub mcp: Vec<McpServer>,
    pub mcp_action: Option<String>,
    pub lsp: Vec<LspStatus>,
    pub todos: Vec<TodoItem>,
    pub diffs: Vec<FileDiff>,
    pub vcs: Option<VcsInfo>,
    pub vcs_status: Vec<VcsFileStatus>,
    pub vcs_diffs: Vec<VcsFileDiff>,
    pub vcs_diff_mode: VcsDiffMode,
    pub revert_state: Option<RevertState>,
}

impl IntegrationState {
    pub fn replace_mcp(&mut self, statuses: HashMap<String, McpStatus>) {
        self.mcp = statuses
            .into_iter()
            .map(|(name, status)| McpServer {
                name,
                status: status.status,
                error: status.error,
            })
            .collect();
        self.mcp.sort_by(|left, right| left.name.cmp(&right.name));
    }

    pub fn begin_mcp_action(&mut self, name: &str) -> bool {
        if self.mcp_action.is_some() {
            return false;
        }
        self.mcp_action = Some(name.to_owned());
        true
    }

    pub fn finish_mcp_action(&mut self, name: &str) {
        if self.mcp_action.as_deref() == Some(name) {
            self.mcp_action = None;
        }
    }

    pub fn replace_lsp(&mut self, statuses: Vec<LspStatus>) {
        self.lsp = statuses;
    }

    pub fn replace_todos(&mut self, todos: Vec<TodoItem>) {
        self.todos = todos;
    }

    pub fn replace_diffs(&mut self, mut diffs: Vec<FileDiff>) {
        diffs.sort_by(|left, right| left.file.cmp(&right.file));
        self.diffs = diffs;
    }

    pub fn replace_vcs(&mut self, vcs: VcsInfo) {
        self.vcs = Some(vcs);
    }

    pub fn update_vcs_branch(&mut self, branch: Option<String>) {
        let vcs = self.vcs.get_or_insert_with(VcsInfo::default);
        if let Some(branch) = branch {
            vcs.branch = branch;
        }
    }

    pub fn replace_vcs_status(&mut self, mut statuses: Vec<VcsFileStatus>) {
        statuses.sort_by(|left, right| left.file.cmp(&right.file));
        self.vcs_status = statuses;
    }

    pub fn replace_vcs_diffs(&mut self, mode: VcsDiffMode, mut diffs: Vec<VcsFileDiff>) {
        diffs.sort_by(|left, right| left.file.cmp(&right.file));
        self.vcs_diff_mode = mode;
        self.vcs_diffs = diffs;
    }

    pub fn clear_session_panels(&mut self) {
        self.todos.clear();
        self.diffs.clear();
        self.revert_state = None;
    }

    pub fn stage_revert(&mut self, revert: RevertState) {
        self.revert_state = Some(revert);
    }

    pub fn clear_revert(&mut self) {
        self.revert_state = None;
    }
}

#[cfg(test)]
mod tests {
    use super::IntegrationState;
    use crate::model::{FileDiff, McpStatus, TodoItem, VcsDiffMode, VcsFileDiff, VcsInfo};
    use std::collections::HashMap;

    #[test]
    fn replacing_mcp_statuses_keeps_sidebar_entries_sorted() {
        let mut state = IntegrationState::default();
        state.replace_mcp(HashMap::from([
            (
                "zeta".to_owned(),
                McpStatus {
                    status: "connected".to_owned(),
                    ..McpStatus::default()
                },
            ),
            (
                "alpha".to_owned(),
                McpStatus {
                    status: "failed".to_owned(),
                    error: Some("offline".to_owned()),
                },
            ),
        ]));

        assert_eq!(
            state
                .mcp
                .iter()
                .map(|server| server.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert_eq!(state.mcp[0].error.as_deref(), Some("offline"));
    }

    #[test]
    fn session_panels_preserve_server_order_and_can_be_cleared_without_losing_vcs_state() {
        let mut state = IntegrationState::default();
        state.replace_todos(vec![
            TodoItem {
                id: "z".to_owned(),
                ..TodoItem::default()
            },
            TodoItem {
                id: "a".to_owned(),
                ..TodoItem::default()
            },
        ]);
        state.replace_diffs(vec![
            FileDiff {
                file: "z.rs".to_owned(),
                ..FileDiff::default()
            },
            FileDiff {
                file: "a.rs".to_owned(),
                ..FileDiff::default()
            },
        ]);
        state.replace_vcs(VcsInfo {
            branch: "main".to_owned(),
            ..VcsInfo::default()
        });

        assert_eq!(state.todos[0].id, "z");
        assert_eq!(state.diffs[0].file, "a.rs");
        state.clear_session_panels();
        assert!(state.todos.is_empty());
        assert!(state.diffs.is_empty());
        assert_eq!(
            state.vcs.as_ref().map(|vcs| vcs.branch.as_str()),
            Some("main")
        );
    }

    #[test]
    fn replacing_vcs_diffs_keeps_files_sorted_and_tracks_the_source() {
        let mut state = IntegrationState::default();
        state.replace_vcs_diffs(
            VcsDiffMode::Branch,
            vec![
                VcsFileDiff {
                    file: "z.rs".to_owned(),
                    ..VcsFileDiff::default()
                },
                VcsFileDiff {
                    file: "a.rs".to_owned(),
                    ..VcsFileDiff::default()
                },
            ],
        );

        assert_eq!(state.vcs_diff_mode, VcsDiffMode::Branch);
        assert_eq!(
            state
                .vcs_diffs
                .iter()
                .map(|diff| diff.file.as_str())
                .collect::<Vec<_>>(),
            ["a.rs", "z.rs"]
        );
    }
}
