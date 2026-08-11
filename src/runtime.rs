use crate::api::ApiClient;
use crate::event::ServerEvent;
use crate::model::{
    AgentInfo, CommandInfo, FileDiff, LspStatus, McpStatus, MessageWithParts, PermissionRequest,
    PromptPart, PromptRequest, ProviderCatalog, QuestionRequest, ReferenceInfo, Session,
    SessionStatus, Skill, TodoItem, VcsDiffMode, VcsFileDiff, VcsFileStatus, VcsInfo,
    WorkspaceFile, file_mime,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use crossterm::event::Event;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub enum AppMsg {
    Terminal(Event),
    Server(ServerEvent),
    Api(Box<ApiResult>),
    Tick,
}

#[derive(Debug)]
pub enum Effect {
    Api(ApiRequest),
    ExportSession {
        session_id: String,
        title: String,
        content: String,
    },
    /// Text the user selected with the mouse. Writing it out touches the
    /// terminal, which the reducer must not do, so it travels as an effect.
    CopyToClipboard(String),
    Quit,
}

#[derive(Debug)]
pub enum ApiRequest {
    Health,
    ListSessions,
    ListArchivedSessions,
    ListPermissions,
    ListQuestions,
    ListProviders,
    ListSkills,
    ListCommands,
    ListAgents,
    ListReferences,
    ListMcp,
    ListLsp,
    ListSessionStatuses,
    ListVcs,
    ListVcsStatus,
    ListVcsDiff {
        mode: VcsDiffMode,
    },
    ConnectMcp(String),
    DisconnectMcp(String),
    ListWorkspaceFiles {
        directory: PathBuf,
    },
    ListWorkspaceDirectory {
        directory: PathBuf,
        path: String,
    },
    SearchWorkspaceFiles {
        query: String,
    },
    ReadAttachment {
        session_id: String,
        directory: PathBuf,
        path: String,
    },
    CreateSession,
    OpenSession(String),
    RefreshCurrent(String),
    ListSessionTodos(String),
    ListSessionDiff(String),
    ListSessionChildren(String),
    RenameSession {
        session_id: String,
        title: String,
    },
    ArchiveSession {
        session_id: String,
        archived: bool,
    },
    MoveSession {
        session_id: String,
        destination: String,
        move_changes: bool,
    },
    DeleteSession(String),
    ForkSession {
        session_id: String,
        message_id: Option<String>,
    },
    ShareSession(String),
    UnshareSession(String),
    CompactSession {
        session_id: String,
        model: crate::model::ModelRef,
    },
    Submit {
        session_id: Option<String>,
        request: Box<PromptRequest>,
    },
    Abort(String),
    ReplyPermission {
        request_id: String,
        reply: PermissionReply,
        message: Option<String>,
    },
    ReplyQuestion {
        request_id: String,
        answers: Vec<Vec<String>>,
    },
    RejectQuestion(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionReply {
    Once,
    Always,
    Reject,
}

impl PermissionReply {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Reject => "reject",
        }
    }
}

#[derive(Debug)]
pub enum ApiResult {
    Health(Result<String, String>),
    Sessions(Result<Vec<Session>, String>),
    ArchivedSessions(Result<Vec<Session>, String>),
    Permissions(Result<Vec<PermissionRequest>, String>),
    Questions(Result<Vec<QuestionRequest>, String>),
    Providers(Result<ProviderCatalog, String>),
    Skills(Result<Vec<Skill>, String>),
    Commands(Result<Vec<CommandInfo>, String>),
    Agents(Result<Vec<AgentInfo>, String>),
    References(Result<Vec<ReferenceInfo>, String>),
    Mcp(Result<HashMap<String, McpStatus>, String>),
    Lsp(Result<Vec<LspStatus>, String>),
    SessionStatuses(Result<HashMap<String, SessionStatus>, String>),
    Todos {
        session_id: String,
        result: Result<Vec<TodoItem>, String>,
    },
    SessionDiff {
        session_id: String,
        result: Result<Vec<FileDiff>, String>,
    },
    SessionChildren {
        session_id: String,
        result: Result<Vec<Session>, String>,
    },
    Vcs(Result<VcsInfo, String>),
    VcsStatus(Result<Vec<VcsFileStatus>, String>),
    VcsDiff {
        mode: VcsDiffMode,
        result: Result<Vec<VcsFileDiff>, String>,
    },
    McpConnected {
        name: String,
        result: Result<(), String>,
    },
    McpDisconnected {
        name: String,
        result: Result<(), String>,
    },
    Files(Result<Vec<WorkspaceFile>, String>),
    Directory {
        path: String,
        result: Result<Vec<WorkspaceFile>, String>,
    },
    SearchedFiles {
        query: String,
        result: Result<Vec<WorkspaceFile>, String>,
    },
    Attachment {
        session_id: String,
        result: Result<PromptPart, String>,
    },
    CreatedSession(Result<Session, String>),
    OpenedSession(Result<SessionSnapshot, String>),
    RefreshedSession(Result<SessionSnapshot, String>),
    RenamedSession(Result<Session, String>),
    ArchivedSession {
        archived: bool,
        result: Result<Session, String>,
    },
    MovedSession {
        session_id: String,
        destination: String,
        result: Result<(), String>,
    },
    DeletedSession {
        session_id: String,
        result: Result<(), String>,
    },
    ForkedSession(Result<Session, String>),
    SharedSession(Result<Session, String>),
    UnsharedSession(Result<Session, String>),
    CompactedSession {
        session_id: String,
        result: Result<bool, String>,
    },
    Exported {
        result: Result<PathBuf, String>,
    },
    Submitted {
        session: Option<Session>,
        result: Result<(), String>,
    },
    Aborted(Result<(), String>),
    PermissionReplied {
        request_id: String,
        result: Result<(), String>,
    },
    QuestionReplied {
        request_id: String,
        result: Result<(), String>,
    },
    QuestionRejected {
        request_id: String,
        result: Result<(), String>,
    },
}

#[derive(Debug)]
pub struct SessionSnapshot {
    pub session: Session,
    pub messages: Vec<MessageWithParts>,
}

pub async fn execute_request(client: Arc<ApiClient>, request: ApiRequest) -> ApiResult {
    match request {
        ApiRequest::Health => {
            ApiResult::Health(client.health().await.map_err(|error| error.to_string()))
        }
        ApiRequest::ListSessions => ApiResult::Sessions(
            client
                .list_sessions()
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ListArchivedSessions => ApiResult::ArchivedSessions(
            client
                .list_archived_sessions()
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ListPermissions => ApiResult::Permissions(
            client
                .list_permissions()
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ListQuestions => ApiResult::Questions(
            client
                .list_questions()
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ListProviders => ApiResult::Providers(
            client
                .list_providers()
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ListSkills => ApiResult::Skills(
            client
                .list_skills()
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ListCommands => ApiResult::Commands(
            client
                .list_commands()
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ListAgents => ApiResult::Agents(
            client
                .list_agents()
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ListReferences => ApiResult::References(
            client
                .list_references()
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ListMcp => {
            ApiResult::Mcp(client.list_mcp().await.map_err(|error| error.to_string()))
        }
        ApiRequest::ListLsp => {
            ApiResult::Lsp(client.list_lsp().await.map_err(|error| error.to_string()))
        }
        ApiRequest::ListSessionStatuses => ApiResult::SessionStatuses(
            client
                .list_session_statuses()
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ListVcs => {
            ApiResult::Vcs(client.vcs_info().await.map_err(|error| error.to_string()))
        }
        ApiRequest::ListVcsStatus => {
            ApiResult::VcsStatus(client.vcs_status().await.map_err(|error| error.to_string()))
        }
        ApiRequest::ConnectMcp(name) => ApiResult::McpConnected {
            result: client
                .connect_mcp(&name)
                .await
                .map_err(|error| error.to_string()),
            name,
        },
        ApiRequest::DisconnectMcp(name) => ApiResult::McpDisconnected {
            result: client
                .disconnect_mcp(&name)
                .await
                .map_err(|error| error.to_string()),
            name,
        },
        ApiRequest::ListWorkspaceFiles { directory } => {
            ApiResult::Files(scan_workspace_files(&directory))
        }
        ApiRequest::ListWorkspaceDirectory { directory, path } => ApiResult::Directory {
            result: scan_workspace_directory(&directory, &path),
            path,
        },
        ApiRequest::SearchWorkspaceFiles { query } => ApiResult::SearchedFiles {
            result: client
                .find_workspace_files(&query, 20)
                .await
                .map_err(|error| error.to_string()),
            query,
        },
        ApiRequest::ReadAttachment {
            session_id,
            directory,
            path,
        } => {
            let result = tokio::task::spawn_blocking(move || read_attachment(&directory, &path))
                .await
                .map_err(|error| format!("attachment worker failed: {error}"))
                .and_then(|result| result);
            ApiResult::Attachment { session_id, result }
        }
        ApiRequest::CreateSession => ApiResult::CreatedSession(
            client
                .create_session(None)
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::OpenSession(session_id) => {
            ApiResult::OpenedSession(load_session(client, session_id).await)
        }
        ApiRequest::RefreshCurrent(session_id) => {
            ApiResult::RefreshedSession(load_session(client, session_id).await)
        }
        ApiRequest::ListSessionTodos(session_id) => ApiResult::Todos {
            result: client
                .list_session_todos(&session_id)
                .await
                .map_err(|error| error.to_string()),
            session_id,
        },
        ApiRequest::ListSessionDiff(session_id) => ApiResult::SessionDiff {
            result: client
                .list_session_diff(&session_id, None)
                .await
                .map_err(|error| error.to_string()),
            session_id,
        },
        ApiRequest::ListSessionChildren(session_id) => ApiResult::SessionChildren {
            result: client
                .list_session_children(&session_id)
                .await
                .map_err(|error| error.to_string()),
            session_id,
        },
        ApiRequest::ListVcsDiff { mode } => ApiResult::VcsDiff {
            result: client
                .vcs_diff(mode, Some(12))
                .await
                .map_err(|error| error.to_string()),
            mode,
        },
        ApiRequest::RenameSession { session_id, title } => ApiResult::RenamedSession(
            client
                .update_session(&session_id, &title)
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ArchiveSession {
            session_id,
            archived,
        } => ApiResult::ArchivedSession {
            result: client
                .archive_session(&session_id, archived)
                .await
                .map_err(|error| error.to_string()),
            archived,
        },
        ApiRequest::MoveSession {
            session_id,
            destination,
            move_changes,
        } => ApiResult::MovedSession {
            result: client
                .move_session(&session_id, &destination, move_changes)
                .await
                .map_err(|error| error.to_string()),
            session_id,
            destination,
        },
        ApiRequest::DeleteSession(session_id) => ApiResult::DeletedSession {
            result: client
                .delete_session(&session_id)
                .await
                .map_err(|error| error.to_string()),
            session_id,
        },
        ApiRequest::ForkSession {
            session_id,
            message_id,
        } => ApiResult::ForkedSession(
            client
                .fork_session(&session_id, message_id.as_deref())
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ShareSession(session_id) => ApiResult::SharedSession(
            client
                .share_session(&session_id)
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::UnshareSession(session_id) => ApiResult::UnsharedSession(
            client
                .unshare_session(&session_id)
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::CompactSession { session_id, model } => ApiResult::CompactedSession {
            result: client
                .summarize_session(&session_id, &model)
                .await
                .map_err(|error| error.to_string()),
            session_id,
        },
        ApiRequest::Submit {
            session_id,
            request,
        } => {
            let (session_id, created_session) = match session_id {
                Some(session_id) => (session_id, None),
                None => {
                    let title = request
                        .text_content()
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .unwrap_or("New session")
                        .chars()
                        .take(42)
                        .collect::<String>();
                    match client.create_session(Some(&title)).await {
                        Ok(session) => (session.id.clone(), Some(session)),
                        Err(error) => {
                            return ApiResult::Submitted {
                                session: None,
                                result: Err(error.to_string()),
                            };
                        }
                    }
                }
            };
            ApiResult::Submitted {
                session: created_session,
                result: client
                    .prompt_async(&session_id, request.as_ref())
                    .await
                    .map_err(|error| error.to_string()),
            }
        }
        ApiRequest::Abort(session_id) => ApiResult::Aborted(
            client
                .abort(&session_id)
                .await
                .map_err(|error| error.to_string()),
        ),
        ApiRequest::ReplyPermission {
            request_id,
            reply,
            message,
        } => ApiResult::PermissionReplied {
            result: client
                .reply_permission(&request_id, reply.as_str(), message.as_deref())
                .await
                .map_err(|error| error.to_string()),
            request_id,
        },
        ApiRequest::ReplyQuestion {
            request_id,
            answers,
        } => ApiResult::QuestionReplied {
            result: client
                .reply_question(&request_id, &answers)
                .await
                .map_err(|error| error.to_string()),
            request_id,
        },
        ApiRequest::RejectQuestion(request_id) => ApiResult::QuestionRejected {
            result: client
                .reject_question(&request_id)
                .await
                .map_err(|error| error.to_string()),
            request_id,
        },
    }
}

async fn load_session(
    client: Arc<ApiClient>,
    session_id: String,
) -> Result<SessionSnapshot, String> {
    let (session, messages) = tokio::try_join!(
        client.get_session(&session_id),
        client.list_messages(&session_id),
    )
    .map_err(|error| error.to_string())?;
    Ok(SessionSnapshot { session, messages })
}

pub async fn execute_export(session_id: String, title: String, content: String) -> ApiResult {
    let export_session_id = session_id.clone();
    let result =
        tokio::task::spawn_blocking(move || write_export(&export_session_id, &title, &content))
            .await
            .map_err(|error| format!("export worker failed: {error}"))
            .and_then(|result| result);
    ApiResult::Exported { result }
}

fn write_export(session_id: &str, title: &str, content: &str) -> Result<PathBuf, String> {
    let directories = directories::ProjectDirs::from("", "", "opencode-tui-rust")
        .ok_or_else(|| "unable to determine an application data directory".to_owned())?;
    let export_directory = directories.data_local_dir().join("exports");
    fs::create_dir_all(&export_directory)
        .map_err(|error| format!("failed to create export directory: {error}"))?;

    let filename = format!(
        "{}-{}.md",
        safe_filename(title, "session"),
        safe_filename(session_id, "unknown")
    );
    let path = export_directory.join(filename);
    fs::write(&path, content).map_err(|error| format!("failed to write export: {error}"))?;
    Ok(path)
}

fn safe_filename(value: &str, fallback: &str) -> String {
    let mut filename = value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .take(64)
        .collect::<String>();
    if filename.is_empty() || filename == "." || filename == ".." {
        filename = fallback.to_owned();
    }
    filename
}

fn read_attachment(directory: &Path, requested_path: &str) -> Result<PromptPart, String> {
    const MAX_ATTACHMENT_BYTES: u64 = 16 * 1024 * 1024;
    let requested_path = requested_path.trim();
    if requested_path.is_empty() {
        return Err("attachment path cannot be empty".to_owned());
    }
    let requested = Path::new(requested_path);
    let path = if requested.is_absolute() {
        requested.to_owned()
    } else {
        directory.join(requested)
    };
    let path = fs::canonicalize(&path)
        .map_err(|error| format!("failed to resolve attachment path: {error}"))?;
    let metadata =
        fs::metadata(&path).map_err(|error| format!("failed to inspect attachment: {error}"))?;
    if !metadata.is_file() {
        return Err("attachments must point to a regular file".to_owned());
    }
    if metadata.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "attachment is too large (maximum {} MiB)",
            MAX_ATTACHMENT_BYTES / (1024 * 1024)
        ));
    }
    let bytes = fs::read(&path).map_err(|error| format!("failed to read attachment: {error}"))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "attachment has no valid filename".to_owned())?;
    let mime = file_mime(&path);
    let url = format!("data:{mime};base64,{}", BASE64.encode(bytes));
    Ok(PromptPart::file(mime, url, Some(filename.to_owned())))
}

fn scan_workspace_files(root: &Path) -> Result<Vec<WorkspaceFile>, String> {
    let root = fs::canonicalize(root).map_err(|error| format!("workspace scan failed: {error}"))?;
    if !root.is_dir() {
        return Err("workspace path is not a directory".to_owned());
    }

    const MAX_FILES: usize = 512;
    const MAX_DEPTH: usize = 4;
    let mut pending = vec![(root.clone(), 0usize)];
    let mut files = Vec::new();

    while let Some((directory, depth)) = pending.pop() {
        let entries =
            fs::read_dir(&directory).map_err(|error| format!("workspace scan failed: {error}"))?;
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if name.starts_with('.') || matches!(name.as_ref(), "target" | "node_modules") {
                continue;
            }
            let path = entry.path();
            let is_directory = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let relative = path
                .strip_prefix(&root)
                .map_err(|error| format!("workspace path conversion failed: {error}"))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(WorkspaceFile {
                path: relative,
                is_directory,
            });
            if is_directory && depth < MAX_DEPTH {
                pending.push((path, depth + 1));
            }
            if files.len() >= MAX_FILES {
                break;
            }
        }
        if files.len() >= MAX_FILES {
            break;
        }
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn scan_workspace_directory(
    root: &Path,
    requested_path: &str,
) -> Result<Vec<WorkspaceFile>, String> {
    let root = fs::canonicalize(root).map_err(|error| format!("workspace scan failed: {error}"))?;
    if !root.is_dir() {
        return Err("workspace path is not a directory".to_owned());
    }

    let requested_path = requested_path.trim().replace('\\', "/");
    let relative_path = requested_path.trim_matches('/');
    if Path::new(relative_path).is_absolute() {
        return Err("file picker path must stay within the workspace".to_owned());
    }
    let directory = if relative_path.is_empty() || relative_path == "." {
        root.clone()
    } else {
        root.join(relative_path)
    };
    let directory = fs::canonicalize(&directory)
        .map_err(|error| format!("workspace directory unavailable: {error}"))?;
    if !directory.starts_with(&root) {
        return Err("file picker path must stay within the workspace".to_owned());
    }
    if !directory.is_dir() {
        return Err("file picker path is not a directory".to_owned());
    }

    const MAX_ENTRIES: usize = 512;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory)
        .map_err(|error| format!("workspace directory scan failed: {error}"))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules") {
            continue;
        }
        let path = entry.path();
        let is_directory = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        let relative = path
            .strip_prefix(&root)
            .map_err(|error| format!("workspace path conversion failed: {error}"))?
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(WorkspaceFile {
            path: relative,
            is_directory,
        });
        if entries.len() >= MAX_ENTRIES {
            break;
        }
    }

    entries.sort_by(|left, right| {
        right
            .is_directory
            .cmp(&left.is_directory)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::{read_attachment, scan_workspace_directory};
    use crate::model::PromptPart;

    #[test]
    fn reads_small_files_as_data_url_prompt_parts() {
        let directory = std::env::temp_dir().join(format!(
            "opencode-tui-rust-attachment-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("temporary attachment directory");
        let path = directory.join("notes.md");
        std::fs::write(&path, b"hello").expect("temporary attachment file");

        let part = read_attachment(&directory, "notes.md").expect("attachment should load");
        assert!(matches!(
            part,
            PromptPart::File {
                mime,
                url,
                filename: Some(filename),
                ..
            } if mime == "text/markdown"
                && url == "data:text/markdown;base64,aGVsbG8="
                && filename == "notes.md"
        ));

        std::fs::remove_dir_all(directory).expect("temporary attachment cleanup");
    }

    #[test]
    fn lists_only_direct_children_inside_the_workspace() {
        let root =
            std::env::temp_dir().join(format!("opencode-tui-rust-picker-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).expect("temporary picker directory");
        std::fs::write(root.join("README.md"), b"read me").expect("temporary picker file");
        std::fs::write(root.join("src/main.rs"), b"fn main() {}").expect("nested picker file");
        std::fs::write(root.join(".hidden"), b"hidden").expect("hidden picker file");

        let entries = scan_workspace_directory(&root, ".").expect("workspace entries should load");
        assert_eq!(
            entries
                .iter()
                .map(|entry| (&entry.path, entry.is_directory))
                .collect::<Vec<_>>(),
            vec![(&"src".to_owned(), true), (&"README.md".to_owned(), false),]
        );
        assert!(scan_workspace_directory(&root, "../").is_err());

        std::fs::remove_dir_all(root).expect("temporary picker cleanup");
    }
}
