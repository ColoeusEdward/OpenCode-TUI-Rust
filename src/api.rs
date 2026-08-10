use crate::event::{ServerEvent, parse_sse_frame};
use crate::model::{
    AgentInfo, CommandInfo, FileDiff, LspStatus, McpStatus, MessageWithParts, PermissionRequest,
    PromptRequest, ProviderCatalog, QuestionRequest, ReferenceInfo, Session, SessionArchiveTime,
    SessionArchiveUpdate, SessionMoveDestination, SessionMoveRequest, SessionStatus, SessionUpdate,
    Skill, TodoItem, VcsDiffMode, VcsFileDiff, VcsFileStatus, VcsInfo, WorkspaceFile,
};
use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use reqwest::{Client, Method, Response, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use url::Url;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub base_url: String,
    pub username: String,
    pub password: Option<String>,
    pub directory: Option<String>,
    pub workspace: Option<String>,
}

#[derive(Clone)]
pub struct ApiClient {
    http: Client,
    config: Arc<ClientConfig>,
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    #[serde(default)]
    version: String,
}

#[derive(Debug, Deserialize)]
struct FileSystemEntry {
    #[serde(default)]
    path: String,
    #[serde(rename = "type", default)]
    kind: String,
}

/// Rewrites a filesystem path into the form the OpenCode server uses for
/// workspace routing: forward slashes and no trailing separator. Both the
/// `directory` query parameter and the session filter go through this so a
/// path typed as `E:\CCE` matches one the server reports as `E:/CCE`.
pub fn normalize_directory(directory: &str) -> String {
    if directory.is_empty() {
        return String::new();
    }
    let replaced = directory.replace('\\', "/");
    let trimmed = replaced.trim_end_matches('/');
    // A drive root (`E:`) and the POSIX root (``, from `/`) keep their separator
    // because stripping it would change which directory they name.
    if trimmed.is_empty() || trimmed.ends_with(':') {
        format!("{trimmed}/")
    } else {
        trimmed.to_owned()
    }
}

/// Windows paths are case-insensitive, so the same directory can arrive spelled
/// two ways depending on how the server or the shell resolved it.
fn directories_match(left: &str, right: &str) -> bool {
    let left = normalize_directory(left);
    let right = normalize_directory(right);
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

impl ApiClient {
    pub fn new(mut config: ClientConfig) -> Result<Self> {
        let http = Client::builder()
            .user_agent("opencode-tui-rust/0.1")
            .build()
            .context("failed to create HTTP client")?;
        Url::parse(config.base_url.trim_end_matches('/')).context("invalid server URL")?;
        config.directory = config
            .directory
            .as_deref()
            .map(normalize_directory)
            .filter(|directory| !directory.is_empty());
        Ok(Self {
            http,
            config: Arc::new(config),
        })
    }

    /// The server is asked to scope listings by `directory`, but older builds
    /// and the experimental archive endpoint do not always honour it, so the
    /// listing is filtered again here. Sessions that report no directory at all
    /// are kept: their location cannot be proven to differ, and dropping them
    /// would empty the list against a server that omits the field.
    fn session_matches_directory(&self, session: &Session) -> bool {
        let Some(expected) = self.config.directory.as_deref() else {
            return true;
        };
        let actual = session.directory.as_deref().or_else(|| {
            session
                .location
                .as_ref()
                .and_then(|location| location.directory.as_deref())
        });
        match actual {
            Some(actual) => directories_match(expected, actual),
            None => true,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.config.base_url
    }

    pub fn directory(&self) -> Option<&str> {
        self.config.directory.as_deref()
    }

    pub async fn health(&self) -> Result<String> {
        let response = self
            .request(Method::GET, "/global/health", false)
            .send()
            .await?;
        let health: HealthResponse = decode_json(response).await?;
        Ok(if health.version.is_empty() {
            "healthy".to_owned()
        } else {
            format!("healthy / {}", health.version)
        })
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let response = self
            .request(Method::GET, "/session", true)
            .query(&[("limit", "100")])
            .send()
            .await?;
        let mut sessions: Vec<Session> = decode_json(response).await?;
        sessions.retain(|session| self.session_matches_directory(session));
        sessions.sort_by_key(|session| Reverse(session.time.updated));
        Ok(sessions)
    }

    pub async fn list_archived_sessions(&self) -> Result<Vec<Session>> {
        let response = self
            .request(Method::GET, "/experimental/session", true)
            .query(&[("archived", "true"), ("limit", "100")])
            .send()
            .await?;
        let mut sessions: Vec<Session> = decode_json(response).await?;
        sessions.retain(|session| {
            session.time.archived.is_some() && self.session_matches_directory(session)
        });
        sessions.sort_by_key(|session| Reverse(session.time.updated));
        Ok(sessions)
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Session> {
        let path = format!("/session/{session_id}");
        let response = self.request(Method::GET, &path, true).send().await?;
        decode_json(response).await
    }

    pub async fn update_session(&self, session_id: &str, title: &str) -> Result<Session> {
        let path = format!("/session/{session_id}");
        let response = self
            .request(Method::PATCH, &path, true)
            .json(&SessionUpdate {
                title: Some(title.to_owned()),
            })
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn archive_session(&self, session_id: &str, archived: bool) -> Result<Session> {
        let path = format!("/session/{session_id}");
        let timestamp = if archived {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("system clock is before the Unix epoch")?
                .as_millis() as i64
        } else {
            0
        };
        let response = self
            .request(Method::PATCH, &path, true)
            .json(&SessionArchiveUpdate {
                time: SessionArchiveTime {
                    archived: timestamp,
                },
            })
            .send()
            .await?;
        let mut session: Session = decode_json(response).await?;
        if !archived {
            // OpenCode returns zero for the cleared archive timestamp on some versions.
            session.time.archived = None;
        }
        Ok(session)
    }

    pub async fn move_session(
        &self,
        session_id: &str,
        destination: &str,
        move_changes: bool,
    ) -> Result<()> {
        let response = self
            .request(
                Method::POST,
                "/experimental/control-plane/move-session",
                true,
            )
            .json(&SessionMoveRequest {
                session_id: session_id.to_owned(),
                destination: SessionMoveDestination {
                    directory: destination.to_owned(),
                },
                move_changes,
            })
            .send()
            .await?;
        ensure_success(response).await
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let path = format!("/session/{session_id}");
        let response = self.request(Method::DELETE, &path, true).send().await?;
        ensure_success(response).await
    }

    pub async fn fork_session(
        &self,
        session_id: &str,
        message_id: Option<&str>,
    ) -> Result<Session> {
        let path = format!("/session/{session_id}/fork");
        let body = message_id
            .map(|message_id| json!({ "messageID": message_id }))
            .unwrap_or_else(|| json!({}));
        let response = self
            .request(Method::POST, &path, true)
            .json(&body)
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn share_session(&self, session_id: &str) -> Result<Session> {
        let path = format!("/session/{session_id}/share");
        let response = self.request(Method::POST, &path, true).send().await?;
        decode_json(response).await
    }

    pub async fn unshare_session(&self, session_id: &str) -> Result<Session> {
        let path = format!("/session/{session_id}/share");
        let response = self.request(Method::DELETE, &path, true).send().await?;
        let mut session: Session = decode_json(response).await?;
        // OpenCode 1.18.x can return the pre-update projection after a successful unshare.
        session.share = None;
        Ok(session)
    }

    pub async fn list_session_children(&self, session_id: &str) -> Result<Vec<Session>> {
        let path = format!("/session/{session_id}/children");
        let response = self.request(Method::GET, &path, true).send().await?;
        decode_json(response).await
    }

    pub async fn list_messages(&self, session_id: &str) -> Result<Vec<MessageWithParts>> {
        let path = format!("/session/{session_id}/message");
        let response = self
            .request(Method::GET, &path, true)
            .query(&[("limit", "100")])
            .send()
            .await?;
        let mut messages: Vec<MessageWithParts> = decode_json(response).await?;
        messages.sort_by_key(MessageWithParts::sort_key);
        Ok(messages)
    }

    pub async fn list_providers(&self) -> Result<ProviderCatalog> {
        let response = self
            .request(Method::GET, "/config/providers", true)
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn list_skills(&self) -> Result<Vec<Skill>> {
        let response = self.request(Method::GET, "/skill", true).send().await?;
        decode_json(response).await
    }

    pub async fn list_commands(&self) -> Result<Vec<CommandInfo>> {
        let response = self.request(Method::GET, "/command", true).send().await?;
        decode_json(response).await
    }

    pub async fn find_workspace_files(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WorkspaceFile>> {
        let limit = limit.to_string();
        let response = self
            .request(Method::GET, "/api/fs/find", true)
            .query(&[
                ("query", query),
                ("type", "file"),
                ("limit", limit.as_str()),
            ])
            .send()
            .await?;
        let entries: Vec<FileSystemEntry> = decode_json(response).await?;
        Ok(entries
            .into_iter()
            .filter(|entry| !entry.path.is_empty())
            .map(|entry| WorkspaceFile {
                path: entry.path,
                is_directory: entry.kind == "directory",
            })
            .collect())
    }

    pub async fn list_references(&self) -> Result<Vec<ReferenceInfo>> {
        let response = self
            .request(Method::GET, "/api/reference", true)
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn list_agents(&self) -> Result<Vec<AgentInfo>> {
        let response = self.request(Method::GET, "/agent", true).send().await?;
        decode_json(response).await
    }

    pub async fn list_mcp(&self) -> Result<HashMap<String, McpStatus>> {
        let response = self.request(Method::GET, "/mcp", true).send().await?;
        decode_json(response).await
    }

    pub async fn connect_mcp(&self, name: &str) -> Result<()> {
        let path = format!("/mcp/{}/connect", encode_path_segment(name));
        let response = self.request(Method::POST, &path, true).send().await?;
        ensure_success(response).await
    }

    pub async fn disconnect_mcp(&self, name: &str) -> Result<()> {
        let path = format!("/mcp/{}/disconnect", encode_path_segment(name));
        let response = self.request(Method::POST, &path, true).send().await?;
        ensure_success(response).await
    }

    pub async fn list_lsp(&self) -> Result<Vec<LspStatus>> {
        let response = self.request(Method::GET, "/lsp", true).send().await?;
        decode_json(response).await
    }

    pub async fn list_session_statuses(&self) -> Result<HashMap<String, SessionStatus>> {
        let response = self
            .request(Method::GET, "/session/status", true)
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn list_session_todos(&self, session_id: &str) -> Result<Vec<TodoItem>> {
        let path = format!("/session/{session_id}/todo");
        let response = self.request(Method::GET, &path, true).send().await?;
        decode_json(response).await
    }

    pub async fn list_session_diff(
        &self,
        session_id: &str,
        message_id: Option<&str>,
    ) -> Result<Vec<FileDiff>> {
        let path = format!("/session/{session_id}/diff");
        let mut request = self.request(Method::GET, &path, true);
        if let Some(message_id) = message_id {
            request = request.query(&[("messageID", message_id)]);
        }
        let response = request.send().await?;
        decode_json(response).await
    }

    pub async fn vcs_info(&self) -> Result<VcsInfo> {
        let response = self.request(Method::GET, "/vcs", true).send().await?;
        decode_json(response).await
    }

    pub async fn vcs_status(&self) -> Result<Vec<VcsFileStatus>> {
        let response = self
            .request(Method::GET, "/vcs/status", true)
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn vcs_diff(
        &self,
        mode: VcsDiffMode,
        context: Option<u32>,
    ) -> Result<Vec<VcsFileDiff>> {
        let mut request = self
            .request(Method::GET, "/vcs/diff", true)
            .query(&[("mode", mode.as_str())]);
        if let Some(context) = context {
            request = request.query(&[("context", context)]);
        }
        let response = request.send().await?;
        decode_json(response).await
    }

    pub async fn create_session(&self, title: Option<&str>) -> Result<Session> {
        let body = title
            .map(|title| json!({ "title": title }))
            .unwrap_or_else(|| json!({}));
        let response = self
            .request(Method::POST, "/session", true)
            .json(&body)
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn prompt_async(&self, session_id: &str, request: &PromptRequest) -> Result<()> {
        let path = format!("/session/{session_id}/prompt_async");
        let response = self
            .request(Method::POST, &path, true)
            .json(request)
            .send()
            .await?;
        ensure_success(response).await
    }

    pub async fn abort(&self, session_id: &str) -> Result<()> {
        let path = format!("/session/{session_id}/abort");
        let response = self.request(Method::POST, &path, true).send().await?;
        ensure_success(response).await
    }

    pub async fn list_permissions(&self) -> Result<Vec<PermissionRequest>> {
        let response = self
            .request(Method::GET, "/permission", true)
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn reply_permission(
        &self,
        request_id: &str,
        reply: &str,
        message: Option<&str>,
    ) -> Result<()> {
        let path = format!("/permission/{request_id}/reply");
        let mut body = json!({ "reply": reply });
        if let Some(message) = message {
            body["message"] = Value::String(message.to_owned());
        }
        let response = self
            .request(Method::POST, &path, true)
            .json(&body)
            .send()
            .await?;
        ensure_success(response).await
    }

    pub async fn list_questions(&self) -> Result<Vec<QuestionRequest>> {
        let response = self.request(Method::GET, "/question", true).send().await?;
        decode_json(response).await
    }

    pub async fn reply_question(&self, request_id: &str, answers: &[Vec<String>]) -> Result<()> {
        let path = format!("/question/{request_id}/reply");
        let response = self
            .request(Method::POST, &path, true)
            .json(&json!({ "answers": answers }))
            .send()
            .await?;
        ensure_success(response).await
    }

    pub async fn reject_question(&self, request_id: &str) -> Result<()> {
        let path = format!("/question/{request_id}/reject");
        let response = self.request(Method::POST, &path, true).send().await?;
        ensure_success(response).await
    }

    pub async fn stream_events(&self, sender: mpsc::Sender<ServerEvent>, stop: CancellationToken) {
        let mut attempt = 0u32;
        while !stop.is_cancelled() {
            match self.stream_once(&sender, &stop).await {
                Ok(()) => attempt = 0,
                Err(error) => {
                    attempt = attempt.saturating_add(1);
                    let delay = retry_delay(attempt);
                    warn!(
                        attempt,
                        retry_in_secs = delay.as_secs(),
                        error = %error,
                        "event stream disconnected; retry scheduled"
                    );
                    if sender
                        .send(ServerEvent::local_reconnecting(
                            error.to_string(),
                            attempt,
                            delay.as_secs(),
                        ))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    tokio::select! {
                        _ = stop.cancelled() => return,
                        _ = sleep(delay) => {}
                    }
                }
            }
        }
    }

    async fn stream_once(
        &self,
        sender: &mpsc::Sender<ServerEvent>,
        stop: &CancellationToken,
    ) -> Result<()> {
        let mut response = self
            .request(Method::GET, "/event", true)
            .header("Accept", "text/event-stream")
            .send()
            .await?;
        if stop.is_cancelled() {
            return Ok(());
        }
        if response.status() == StatusCode::NOT_FOUND {
            response = self
                .request(Method::GET, "/global/event", false)
                .header("Accept", "text/event-stream")
                .send()
                .await?;
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("event stream returned {status}: {}", compact_body(&body));
        }
        sender
            .send(ServerEvent::local_connected())
            .await
            .map_err(|_| anyhow!("event receiver closed"))?;
        info!(server = %self.base_url(), "event stream connected");

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        loop {
            let Some(chunk) = (tokio::select! {
                _ = stop.cancelled() => return Ok(()),
                chunk = stream.next() => chunk,
            }) else {
                break;
            };
            let chunk = chunk.context("event stream read failed")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            buffer = buffer.replace("\r\n", "\n");
            while let Some(separator) = buffer.find("\n\n") {
                let frame = buffer[..separator].to_owned();
                buffer.drain(..separator + 2);
                if let Some(event) = parse_sse_frame(&frame)? {
                    sender
                        .send(event)
                        .await
                        .map_err(|_| anyhow!("event receiver closed"))?;
                }
            }
        }
        if !buffer.trim().is_empty()
            && let Some(event) = parse_sse_frame(&buffer)?
        {
            sender
                .send(event)
                .await
                .map_err(|_| anyhow!("event receiver closed"))?;
        }
        if stop.is_cancelled() {
            Ok(())
        } else {
            bail!("event stream closed")
        }
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        include_context: bool,
    ) -> reqwest::RequestBuilder {
        let url = format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let mut request = self.http.request(method, url);
        if let Some(password) = self.config.password.as_deref() {
            request = request.basic_auth(&self.config.username, Some(password));
        }
        if include_context {
            let mut query = Vec::new();
            if let Some(directory) = self.config.directory.as_deref() {
                query.push(("directory", directory));
            }
            if let Some(workspace) = self.config.workspace.as_deref() {
                query.push(("workspace", workspace));
            }
            if !query.is_empty() {
                request = request.query(&query);
            }
        }
        request
    }
}

fn retry_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(5);
    Duration::from_secs(1u64 << exponent).min(Duration::from_secs(30))
}

async fn ensure_success(response: Response) -> Result<()> {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("HTTP {status}: {}", compact_body(&body));
    }
    Ok(())
}

async fn decode_json<T: DeserializeOwned>(response: Response) -> Result<T> {
    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read HTTP response")?;
    if !status.is_success() {
        bail!("HTTP {status}: {}", compact_body(&body));
    }
    let value: Value = serde_json::from_str(&body).with_context(|| {
        format!(
            "invalid JSON response from OpenCode: {}",
            compact_body(&body)
        )
    })?;
    if let Ok(result) = serde_json::from_value::<T>(value.clone()) {
        return Ok(result);
    }
    if let Some(data) = value.get("data") {
        return serde_json::from_value(data.clone())
            .context("invalid data envelope in OpenCode response");
    }
    serde_json::from_value(value).context("invalid OpenCode response shape")
}

fn compact_body(body: &str) -> String {
    let compact = body.replace(['\r', '\n'], " ");
    let trimmed = compact.trim();
    if trimmed.chars().count() > 240 {
        let prefix = trimmed.chars().take(240).collect::<String>();
        format!("{prefix}...")
    } else {
        trimmed.to_owned()
    }
}

fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        ApiClient, ClientConfig, compact_body, encode_path_segment, normalize_directory,
        retry_delay,
    };
    use crate::model::{
        ModelRef, PromptPart, PromptRequest, Session, SessionLocation, VcsDiffMode,
    };
    use reqwest::Client;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use tokio::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn compacts_multibyte_error_text_without_slicing_a_char() {
        let body = "错误信息".repeat(100);
        let compact = compact_body(&body);

        assert!(compact.ends_with("..."));
        assert!(compact.chars().count() <= 243);
    }

    #[test]
    fn retry_delay_is_exponential_and_bounded() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(2), Duration::from_secs(2));
        assert_eq!(retry_delay(3), Duration::from_secs(4));
        assert_eq!(retry_delay(5), Duration::from_secs(16));
        assert_eq!(retry_delay(6), Duration::from_secs(30));
        assert_eq!(retry_delay(u32::MAX), Duration::from_secs(30));
    }

    #[test]
    fn encodes_mcp_names_as_path_segments() {
        assert_eq!(encode_path_segment("my server/1"), "my%20server%2F1");
    }

    #[tokio::test]
    async fn sessions_fixture_preserves_context_and_basic_auth() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("fixture request");
            let request = read_request(&mut socket).await;
            let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
            assert!(request.contains("/session?"));
            assert!(request.contains("directory="));
            assert!(request.contains("workspace=workspace-id"));
            assert!(request.contains("authorization: basic"));
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"data\":[]}",
                )
                .await
                .expect("fixture response");
            socket.shutdown().await.expect("fixture shutdown");
        });

        let client = fixture_client(ClientConfig {
            base_url: format!("http://{address}"),
            username: "opencode".to_owned(),
            password: Some("secret".to_owned()),
            directory: Some("E:/workspace/project".to_owned()),
            workspace: Some("workspace-id".to_owned()),
        });

        assert!(
            client
                .list_sessions()
                .await
                .expect("sessions should decode")
                .is_empty()
        );
        server.await.expect("fixture server should finish");
    }

    #[tokio::test]
    async fn archived_sessions_fixture_filters_active_rows() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("fixture request");
            let request = read_request(&mut socket).await;
            let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
            assert!(request.contains("/experimental/session?archived=true&limit=100"));
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n[{\"id\":\"ses_active\",\"time\":{\"updated\":3}},{\"id\":\"ses_archived\",\"time\":{\"updated\":2,\"archived\":1}}]",
                )
                .await
                .expect("fixture response");
            socket.shutdown().await.expect("fixture shutdown");
        });

        let client = fixture_client(ClientConfig {
            base_url: format!("http://{address}"),
            username: "opencode".to_owned(),
            password: None,
            directory: None,
            workspace: None,
        });
        let sessions = client
            .list_archived_sessions()
            .await
            .expect("archived sessions should decode");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "ses_archived");
        server.await.expect("fixture server should finish");
    }

    #[tokio::test]
    async fn move_session_fixture_posts_control_plane_payload() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("fixture request");
            let request = read_request(&mut socket).await;
            let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
            assert!(request.contains("post /experimental/control-plane/move-session "));
            assert!(request.contains("\"sessionid\":\"ses_move\""));
            assert!(request.contains("\"directory\":\"e:/target\""));
            assert!(request.contains("\"movechanges\":true"));
            socket
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await
                .expect("fixture response");
            socket.shutdown().await.expect("fixture shutdown");
        });

        let client = fixture_client(ClientConfig {
            base_url: format!("http://{address}"),
            username: "opencode".to_owned(),
            password: None,
            directory: None,
            workspace: None,
        });
        client
            .move_session("ses_move", "E:/target", true)
            .await
            .expect("move request should accept no content");
        server.await.expect("fixture server should finish");
    }

    #[tokio::test]
    async fn fork_fixture_posts_message_context_and_decodes_session() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("fixture request");
            let request = read_request(&mut socket).await;
            let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
            assert!(request.contains("post /session/ses_1/fork?"));
            assert!(request.contains("\"messageid\":\"msg_1\""));
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"id\":\"ses_forked\",\"title\":\"Forked\",\"time\":{\"updated\":2}}",
                )
                .await
                .expect("fixture response");
            socket.shutdown().await.expect("fixture shutdown");
        });

        let client = fixture_client(ClientConfig {
            base_url: format!("http://{address}"),
            username: "opencode".to_owned(),
            password: None,
            directory: Some("E:/workspace/project".to_owned()),
            workspace: Some("workspace-id".to_owned()),
        });
        let session = client
            .fork_session("ses_1", Some("msg_1"))
            .await
            .expect("fork should decode");

        assert_eq!(session.id, "ses_forked");
        server.await.expect("fixture server should finish");
    }

    #[tokio::test]
    async fn session_panels_and_vcs_fixture_decode_typed_data() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let responses = [
                (
                    "/session/status?",
                    br#"{"ses_1":{"type":"busy"}}"# as &[u8],
                ),
                (
                    "/session/ses_1/todo?",
                    br#"[{"id":"todo_1","content":"Review diff","status":"in_progress","priority":"high"}]"#,
                ),
                (
                    "/session/ses_1/diff?",
                    br#"[{"file":"src/main.rs","before":"","after":"+line","additions":1,"deletions":0}]"#,
                ),
                ("/vcs?", br#"{"branch":"main","default_branch":"main"}"#),
                (
                    "/vcs/status?",
                    br#"[{"file":"src/main.rs","additions":1,"deletions":0,"status":"modified"}]"#,
                ),
                (
                    "/vcs/diff?",
                    br#"[{"file":"src/main.rs","patch":"@@ -1 +1 @@\n-old\n+new\n","additions":1,"deletions":1,"status":"modified"}]"#,
                ),
            ];
            for (expected_path, body) in responses {
                let (mut socket, _) = listener.accept().await.expect("fixture request");
                let request = read_request(&mut socket).await;
                let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
                assert!(request.contains(expected_path), "request was {request}");
                if expected_path == "/vcs/diff?" {
                    assert!(request.contains("mode=git"), "request was {request}");
                    assert!(request.contains("context=12"), "request was {request}");
                }
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                            String::from_utf8_lossy(body)
                        )
                        .as_bytes(),
                    )
                    .await
                    .expect("fixture response");
                socket.shutdown().await.expect("fixture shutdown");
            }
        });

        let client = fixture_client(ClientConfig {
            base_url: format!("http://{address}"),
            username: "opencode".to_owned(),
            password: None,
            directory: Some("E:/workspace/project".to_owned()),
            workspace: Some("workspace-id".to_owned()),
        });
        let statuses = client
            .list_session_statuses()
            .await
            .expect("session statuses should decode");
        assert!(matches!(
            statuses.get("ses_1"),
            Some(crate::model::SessionStatus::Busy)
        ));
        let todos = client
            .list_session_todos("ses_1")
            .await
            .expect("todos should decode");
        assert_eq!(todos[0].content, "Review diff");
        let diffs = client
            .list_session_diff("ses_1", Some("msg_1"))
            .await
            .expect("session diffs should decode");
        assert_eq!(diffs[0].file, "src/main.rs");
        let vcs = client.vcs_info().await.expect("VCS info should decode");
        assert_eq!(vcs.branch, "main");
        let status = client.vcs_status().await.expect("VCS status should decode");
        assert_eq!(status[0].status, "modified");
        let vcs_diffs = client
            .vcs_diff(VcsDiffMode::Git, Some(12))
            .await
            .expect("VCS diffs should decode");
        assert_eq!(vcs_diffs[0].patch, "@@ -1 +1 @@\n-old\n+new\n");
        server.await.expect("fixture server should finish");
    }

    #[tokio::test]
    async fn mcp_toggle_fixture_uses_encoded_paths_and_context() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            for expected_path in [
                "/mcp/my%20server%2f1/connect?directory=",
                "/mcp/my%20server%2f1/disconnect?directory=",
            ] {
                let (mut socket, _) = listener.accept().await.expect("fixture request");
                let request = read_request(&mut socket).await;
                let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
                assert!(request.contains(expected_path));
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .expect("fixture response");
                socket.shutdown().await.expect("fixture shutdown");
            }
        });

        let client = fixture_client(ClientConfig {
            base_url: format!("http://{address}"),
            username: "opencode".to_owned(),
            password: None,
            directory: Some("E:/workspace/project".to_owned()),
            workspace: None,
        });
        client
            .connect_mcp("my server/1")
            .await
            .expect("MCP connect should succeed");
        client
            .disconnect_mcp("my server/1")
            .await
            .expect("MCP disconnect should succeed");
        server.await.expect("fixture server should finish");
    }

    #[tokio::test]
    async fn file_search_fixture_decodes_location_envelope_and_query_context() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("fixture request");
            let request = read_request(&mut socket).await;
            let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
            assert!(request.contains("/api/fs/find?"));
            assert!(request.contains("query=src"));
            assert!(request.contains("type=file"));
            assert!(request.contains("limit=20"));
            assert!(request.contains("directory="));
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"location\":{\"directory\":\"E:/workspace/project\"},\"data\":[{\"path\":\"src/main.rs\",\"type\":\"file\"},{\"path\":\"src\",\"type\":\"directory\"}]}",
                )
                .await
                .expect("fixture response");
            socket.shutdown().await.expect("fixture shutdown");
        });

        let client = fixture_client(ClientConfig {
            base_url: format!("http://{address}"),
            username: "opencode".to_owned(),
            password: None,
            directory: Some("E:/workspace/project".to_owned()),
            workspace: Some("workspace-id".to_owned()),
        });
        let files = client
            .find_workspace_files("src", 20)
            .await
            .expect("file search should decode");

        assert_eq!(files[0].path, "src/main.rs");
        assert!(!files[0].is_directory);
        assert!(files[1].is_directory);
        server.await.expect("fixture server should finish");
    }

    #[tokio::test]
    async fn reference_fixture_decodes_data_envelope() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("fixture request");
            let request = read_request(&mut socket).await;
            let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
            assert!(request.contains("/api/reference?"));
            assert!(request.contains("directory="));
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"location\":{\"directory\":\"E:/workspace/project\"},\"data\":[{\"name\":\"docs\",\"path\":\"E:/reference-docs\",\"description\":\"Project docs\"}]}",
                )
                .await
                .expect("fixture response");
            socket.shutdown().await.expect("fixture shutdown");
        });

        let client = fixture_client(ClientConfig {
            base_url: format!("http://{address}"),
            username: "opencode".to_owned(),
            password: None,
            directory: Some("E:/workspace/project".to_owned()),
            workspace: None,
        });
        let references = client
            .list_references()
            .await
            .expect("references should decode");

        assert_eq!(references[0].name, "docs");
        assert_eq!(references[0].path, "E:/reference-docs");
        server.await.expect("fixture server should finish");
    }

    #[tokio::test]
    async fn stream_fixture_reassembles_partial_sse_and_surfaces_reconnect() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("fixture request");
            let _ = read_request(&mut socket).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("fixture headers");
            socket
                .write_all(b"data: {\"type\":\"server.heart")
                .await
                .expect("partial SSE prefix");
            socket
                .write_all(b"beat\",\"properties\":{}}\n\n")
                .await
                .expect("partial SSE suffix");
            socket.shutdown().await.expect("fixture shutdown");
        });

        let client = fixture_client(ClientConfig {
            base_url: format!("http://{address}"),
            username: "opencode".to_owned(),
            password: None,
            directory: None,
            workspace: None,
        });
        let (sender, mut receiver) = mpsc::channel(8);
        let stop = CancellationToken::new();
        let stream_stop = stop.clone();
        let stream = tokio::spawn(async move {
            client.stream_events(sender, stream_stop).await;
        });

        assert_eq!(
            receiver.recv().await.expect("connected event").kind,
            "client.connected"
        );
        assert_eq!(
            receiver.recv().await.expect("heartbeat event").kind,
            "server.heartbeat"
        );
        let reconnecting = receiver.recv().await.expect("reconnecting event");
        assert_eq!(reconnecting.kind, "client.reconnecting");
        assert_eq!(reconnecting.properties["attempt"], json!(1));
        assert_eq!(reconnecting.properties["retryIn"], json!(1));

        stop.cancel();
        stream.await.expect("stream worker should stop");
        server.await.expect("fixture server should finish");
    }

    async fn read_request(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0u8; 512];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = socket
                .read(&mut buffer)
                .await
                .expect("fixture request read");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        let Some(header_end) = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| index + 4)
        else {
            return request;
        };
        let content_length = String::from_utf8_lossy(&request[..header_end])
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length:")
                    .or_else(|| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        while request.len() < header_end + content_length {
            let read = socket
                .read(&mut buffer)
                .await
                .expect("fixture request body read");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        request
    }

    fn fixture_client(config: ClientConfig) -> ApiClient {
        ApiClient {
            http: Client::builder()
                .no_proxy()
                .build()
                .expect("fixture HTTP client should build"),
            config: Arc::new(config),
        }
    }

    fn session_at(id: &str, directory: Option<&str>) -> Session {
        Session {
            id: id.to_owned(),
            directory: directory.map(str::to_owned),
            ..Session::default()
        }
    }

    #[test]
    fn directories_are_normalized_to_the_form_the_server_reports() {
        assert_eq!(normalize_directory(r"E:\CCE"), "E:/CCE");
        assert_eq!(normalize_directory("E:/CCE/"), "E:/CCE");
        assert_eq!(normalize_directory(r"E:\CCE\sub\"), "E:/CCE/sub");
        assert_eq!(normalize_directory(r"E:\"), "E:/");
        assert_eq!(normalize_directory("/home/user"), "/home/user");
        assert_eq!(normalize_directory("/"), "/");
        assert_eq!(normalize_directory(""), "");
    }

    #[test]
    fn the_client_normalizes_the_configured_directory_once() {
        let client = ApiClient::new(ClientConfig {
            base_url: "http://127.0.0.1:4096".to_owned(),
            username: "opencode".to_owned(),
            password: None,
            directory: Some(r"E:\CCE\".to_owned()),
            workspace: None,
        })
        .expect("client should build");
        assert_eq!(client.directory(), Some("E:/CCE"));
    }

    #[test]
    fn an_empty_directory_is_treated_as_unset() {
        let client = ApiClient::new(ClientConfig {
            base_url: "http://127.0.0.1:4096".to_owned(),
            username: "opencode".to_owned(),
            password: None,
            directory: Some(String::new()),
            workspace: None,
        })
        .expect("client should build");
        assert_eq!(client.directory(), None);
    }

    #[test]
    fn sessions_outside_the_configured_directory_are_filtered_out() {
        let client = fixture_client(ClientConfig {
            base_url: "http://127.0.0.1:4096".to_owned(),
            username: "opencode".to_owned(),
            password: None,
            directory: Some("E:/CCE".to_owned()),
            workspace: None,
        });

        assert!(client.session_matches_directory(&session_at("ses_here", Some("E:/CCE"))));
        // Separator and trailing-slash spelling must not decide membership.
        assert!(client.session_matches_directory(&session_at("ses_slash", Some(r"E:\CCE\"))));
        assert!(!client.session_matches_directory(&session_at("ses_other", Some("E:/Other"))));
        assert!(!client.session_matches_directory(&session_at("ses_child", Some("E:/CCE/sub"))));
        // A server that omits the field must not produce an empty list.
        assert!(client.session_matches_directory(&session_at("ses_unknown", None)));
    }

    #[test]
    fn a_session_location_supplies_the_directory_when_the_top_level_field_is_absent() {
        let client = fixture_client(ClientConfig {
            base_url: "http://127.0.0.1:4096".to_owned(),
            username: "opencode".to_owned(),
            password: None,
            directory: Some("E:/CCE".to_owned()),
            workspace: None,
        });
        let mut session = session_at("ses_location", None);
        session.location = Some(SessionLocation {
            directory: Some("E:/Other".to_owned()),
            workspace_id: None,
        });
        assert!(!client.session_matches_directory(&session));
    }

    #[test]
    fn every_session_is_kept_when_no_directory_is_configured() {
        let client = fixture_client(ClientConfig {
            base_url: "http://127.0.0.1:4096".to_owned(),
            username: "opencode".to_owned(),
            password: None,
            directory: None,
            workspace: None,
        });
        assert!(client.session_matches_directory(&session_at("ses_any", Some("E:/Anywhere"))));
    }

    #[test]
    fn prompt_body_includes_the_selected_model_and_variant() {
        let request = PromptRequest::from_text(
            "hello",
            Some(&ModelRef {
                provider_id: "provider".to_owned(),
                id: "model".to_owned(),
                variant: Some("fast".to_owned()),
            }),
            Some("build"),
        );
        let body = serde_json::to_value(request).expect("prompt request serializes");

        assert_eq!(
            body,
            json!({
                "parts": [{ "type": "text", "text": "hello" }],
                "model": { "providerID": "provider", "modelID": "model" },
                "variant": "fast",
                "agent": "build"
            })
        );
    }

    #[test]
    fn prompt_body_serializes_structured_parts() {
        let request = PromptRequest {
            parts: vec![
                PromptPart::text("Review these files"),
                PromptPart::file(
                    "text/plain",
                    "file:///workspace/notes.txt",
                    Some("notes.txt".to_owned()),
                ),
                PromptPart::agent("review"),
                PromptPart::subtask("Inspect the diff", "Review changed files", "explore"),
            ],
            ..PromptRequest::default()
        };
        let body = serde_json::to_value(request).expect("prompt request serializes");

        assert_eq!(
            body,
            json!({
                "parts": [
                    { "type": "text", "text": "Review these files" },
                    {
                        "type": "file",
                        "mime": "text/plain",
                        "url": "file:///workspace/notes.txt",
                        "filename": "notes.txt"
                    },
                    { "type": "agent", "name": "review" },
                    {
                        "type": "subtask",
                        "prompt": "Inspect the diff",
                        "description": "Review changed files",
                        "agent": "explore"
                    }
                ]
            })
        );
    }
}
