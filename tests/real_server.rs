use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde_json::{Value, json};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, timeout};
use url::Url;

struct RealServer {
    client: Client,
    base_url: String,
    username: String,
    password: Option<String>,
    directory: Option<String>,
    workspace: Option<String>,
}

impl RealServer {
    fn from_env() -> Result<Self> {
        let base_url = env::var("OPENCODE_TUI_REAL_SERVER_URL")
            .context("set OPENCODE_TUI_REAL_SERVER_URL to run real-server tests")?;
        let parsed_url = Url::parse(&base_url).context("invalid real server URL")?;
        let mut builder = Client::builder().user_agent("opencode-tui-rust-real-server-test/0.1");
        if parsed_url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1"))
        {
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .context("failed to build real-server client")?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            username: env::var("OPENCODE_SERVER_USERNAME")
                .unwrap_or_else(|_| "opencode".to_owned()),
            password: env::var("OPENCODE_SERVER_PASSWORD").ok(),
            directory: env::var("OPENCODE_TUI_REAL_SERVER_DIRECTORY").ok(),
            workspace: env::var("OPENCODE_TUI_REAL_SERVER_WORKSPACE").ok(),
        })
    }

    fn request(&self, method: Method, path: &str, include_context: bool) -> RequestBuilder {
        let url = format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let mut request = self
            .client
            .request(method, url)
            .basic_auth(&self.username, self.password.as_deref());
        if include_context {
            let mut query = Vec::new();
            if let Some(directory) = self.directory.as_deref() {
                query.push(("directory", directory));
            }
            if let Some(workspace) = self.workspace.as_deref() {
                query.push(("workspace", workspace));
            }
            if !query.is_empty() {
                request = request.query(&query);
            }
        }
        request
    }

    async fn json(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let request = self.request(method, path, true);
        let request = if let Some(body) = body {
            request.json(&body)
        } else {
            request
        };
        decode_json(request.send().await?).await
    }

    async fn success(&self, method: Method, path: &str, body: Option<Value>) -> Result<()> {
        let request = self.request(method, path, true);
        let request = if let Some(body) = body {
            request.json(&body)
        } else {
            request
        };
        let response = request
            .send()
            .await
            .context("failed to send real-server request")?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .context("failed to read real-server error response")?;
            bail!("real server returned {status}: {}", compact_body(&body));
        }
        Ok(())
    }
}

async fn decode_json(response: Response) -> Result<Value> {
    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read real-server response")?;
    if !status.is_success() {
        bail!("real server returned {status}: {}", compact_body(&body));
    }
    serde_json::from_str(&body).context("real server returned invalid JSON")
}

fn data(value: Value) -> Value {
    value.get("data").cloned().unwrap_or(value)
}

fn compact_body(body: &str) -> String {
    let body = body.replace(['\r', '\n'], " ");
    let body = body.trim();
    if body.chars().count() > 240 {
        format!("{}...", body.chars().take(240).collect::<String>())
    } else {
        body.to_owned()
    }
}

#[tokio::test]
#[ignore = "requires a running opencode serve instance"]
async fn real_server_catalog_and_sse_boundary_are_usable() -> Result<()> {
    let server = RealServer::from_env()?;
    let health = data(server.json(Method::GET, "/global/health", None).await?);
    assert!(
        health.is_object(),
        "health response should be an object: {health}"
    );

    for path in [
        "/config/providers",
        "/command",
        "/agent",
        "/skill",
        "/mcp",
        "/lsp",
        "/session/status",
        "/vcs",
        "/vcs/status",
        "/vcs/diff?mode=git&context=12",
        "/api/reference",
    ] {
        let value = data(server.json(Method::GET, path, None).await?);
        assert!(
            !value.is_null(),
            "catalog response for {path} should not be null"
        );
    }

    let files = data(
        server
            .json(
                Method::GET,
                "/api/fs/find?query=src&type=file&limit=20",
                None,
            )
            .await?,
    );
    assert!(
        files.is_array(),
        "file completion response should be an array: {files}"
    );

    let sessions = data(server.json(Method::GET, "/session", None).await?);
    assert!(
        sessions.is_array(),
        "session response should be an array: {sessions}"
    );

    let response = server
        .request(Method::GET, "/event", true)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .context("failed to open real-server event stream")?;
    if response.status() == StatusCode::NOT_FOUND {
        bail!("real server does not expose /event; use a compatible OpenCode version");
    }
    if !response.status().is_success() {
        bail!("real server event stream returned {}", response.status());
    }

    let mut stream = response.bytes_stream();
    let mut received = String::new();
    for _ in 0..8 {
        let next = timeout(Duration::from_secs(5), stream.next())
            .await
            .context("timed out waiting for real-server SSE data")?;
        let Some(chunk) = next else {
            break;
        };
        received.push_str(&String::from_utf8_lossy(
            &chunk.context("failed to read SSE data")?,
        ));
        if received.contains("\"type\"") || received.contains("data:") {
            break;
        }
    }
    assert!(
        received.contains("\"type\"") || received.contains("data:"),
        "real-server SSE stream did not yield an event: {received}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires a running opencode serve instance"]
async fn real_server_session_can_be_created_and_deleted() -> Result<()> {
    let server = RealServer::from_env()?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_millis();
    let title = format!("opencode-tui-rust integration {timestamp}");
    let created = data(
        server
            .json(Method::POST, "/session", Some(json!({ "title": title })))
            .await?,
    );
    let session_id = created
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("created session response has no id: {created}"))?
        .to_owned();

    for suffix in ["todo", "diff"] {
        let value = data(
            server
                .json(
                    Method::GET,
                    &format!("/session/{session_id}/{suffix}"),
                    None,
                )
                .await?,
        );
        assert!(
            value.is_array(),
            "session {suffix} response should be an array: {value}"
        );
    }

    let children = data(
        server
            .json(
                Method::GET,
                &format!("/session/{session_id}/children"),
                None,
            )
            .await?,
    );
    assert!(
        children.is_array(),
        "session children response should be an array: {children}"
    );

    let shared = data(
        server
            .json(Method::POST, &format!("/session/{session_id}/share"), None)
            .await?,
    );
    assert!(
        shared
            .get("share")
            .and_then(|share| share.get("url"))
            .and_then(Value::as_str)
            .is_some_and(|url| !url.is_empty()),
        "shared session response should include a URL: {shared}"
    );

    let unshared = data(
        server
            .json(
                Method::DELETE,
                &format!("/session/{session_id}/share"),
                None,
            )
            .await?,
    );
    assert_eq!(
        unshared.get("id").and_then(Value::as_str),
        Some(session_id.as_str()),
        "unshare response should identify the session: {unshared}"
    );

    let session = data(
        server
            .json(Method::GET, &format!("/session/{session_id}"), None)
            .await?,
    );
    let destination = session
        .get("directory")
        .and_then(Value::as_str)
        .or(session
            .get("location")
            .and_then(|location| location.get("directory"))
            .and_then(Value::as_str))
        .or(created.get("directory").and_then(Value::as_str))
        .or(server.directory.as_deref())
        .ok_or_else(|| anyhow!("created session response has no directory: {session}"))?
        .to_owned();
    server
        .success(
            Method::POST,
            "/experimental/control-plane/move-session",
            Some(json!({
                "sessionID": session_id,
                "destination": { "directory": destination },
                "moveChanges": false,
            })),
        )
        .await?;

    let archived = data(
        server
            .json(
                Method::PATCH,
                &format!("/session/{session_id}"),
                Some(json!({ "time": { "archived": timestamp as i64 } })),
            )
            .await?,
    );
    assert_eq!(
        archived.get("id").and_then(Value::as_str),
        Some(session_id.as_str()),
        "archive response should identify the session: {archived}"
    );
    assert!(
        archived
            .get("time")
            .and_then(|time| time.get("archived"))
            .and_then(Value::as_i64)
            .is_some_and(|value| value > 0),
        "archive response should include a timestamp: {archived}"
    );

    let archived_sessions = data(
        server
            .json(Method::GET, "/experimental/session?archived=true", None)
            .await?,
    );
    assert!(
        archived_sessions.as_array().is_some_and(|sessions| {
            sessions.iter().any(|session| {
                session.get("id").and_then(Value::as_str) == Some(session_id.as_str())
            })
        }),
        "archived session list should include the session: {archived_sessions}"
    );

    let restored = data(
        server
            .json(
                Method::PATCH,
                &format!("/session/{session_id}"),
                Some(json!({ "time": { "archived": 0 } })),
            )
            .await?,
    );
    assert_eq!(
        restored.get("id").and_then(Value::as_str),
        Some(session_id.as_str()),
        "restore response should identify the session: {restored}"
    );
    assert!(
        restored
            .get("time")
            .and_then(|time| time.get("archived"))
            .is_none_or(|value| value.as_i64() == Some(0)),
        "restore response should clear the archive timestamp: {restored}"
    );

    let delete_result = server
        .request(Method::DELETE, &format!("/session/{session_id}"), true)
        .send()
        .await
        .context("failed to delete integration session")?;
    if !delete_result.status().is_success() {
        bail!(
            "integration session cleanup returned {}",
            delete_result.status()
        );
    }
    Ok(())
}
