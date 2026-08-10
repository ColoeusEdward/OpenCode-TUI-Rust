use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use url::Url;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CacheTokens {
    #[serde(default)]
    pub read: u64,
    #[serde(default)]
    pub write: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TokenUsage {
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub reasoning: u64,
    #[serde(default)]
    pub cache: CacheTokens,
}

impl TokenUsage {
    pub fn total_or_sum(&self) -> u64 {
        if self.total > 0 {
            self.total
        } else {
            self.input + self.output + self.reasoning + self.cache.read + self.cache.write
        }
    }

    pub fn cache_hit_percent(&self) -> f64 {
        let total_input = self.input + self.cache.read;
        if total_input == 0 {
            0.0
        } else {
            self.cache.read as f64 / total_input as f64 * 100.0
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionTime {
    #[serde(default)]
    pub updated: i64,
    #[serde(default)]
    pub archived: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionLocation {
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(rename = "workspaceID", default)]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionShare {
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Session {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(rename = "workspaceID", default)]
    pub workspace_id: Option<String>,
    #[serde(rename = "parentID", default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub time: SessionTime,
    #[serde(default)]
    pub location: Option<SessionLocation>,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub tokens: TokenUsage,
    #[serde(default)]
    pub share: Option<SessionShare>,
    #[serde(default)]
    pub model: Option<ModelRef>,
    #[serde(default)]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionArchiveUpdate {
    pub time: SessionArchiveTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionArchiveTime {
    pub archived: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionMoveRequest {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub destination: SessionMoveDestination,
    #[serde(rename = "moveChanges")]
    pub move_changes: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionMoveDestination {
    pub directory: String,
}

impl Session {
    pub fn directory(&self) -> Option<&str> {
        self.directory
            .as_deref()
            .or_else(|| self.location.as_ref()?.directory.as_deref())
    }

    pub fn workspace_id(&self) -> Option<&str> {
        self.workspace_id
            .as_deref()
            .or_else(|| self.location.as_ref()?.workspace_id.as_deref())
    }

    pub fn share_url(&self) -> Option<&str> {
        self.share
            .as_ref()
            .map(|share| share.url.as_str())
            .filter(|url| !url.is_empty())
    }

    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            "Untitled session"
        } else {
            &self.title
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageTime {
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub completed: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageInfo {
    pub id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: String,
    pub role: String,
    #[serde(default)]
    pub time: MessageTime,
    #[serde(rename = "providerID", default)]
    pub provider_id: String,
    #[serde(rename = "modelID", default)]
    pub model_id: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub tokens: TokenUsage,
    #[serde(default)]
    pub finish: Option<String>,
    #[serde(default)]
    pub error: Option<Value>,
    #[serde(default)]
    pub snapshot: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Part {
    pub id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: String,
    #[serde(rename = "messageID", default)]
    pub message_id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(rename = "callID", default)]
    pub call_id: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub state: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PermissionTool {
    #[serde(rename = "messageID", default)]
    pub message_id: String,
    #[serde(rename = "callID", default)]
    pub call_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: String,
    #[serde(default)]
    pub permission: String,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub always: Vec<String>,
    #[serde(default)]
    pub tool: Option<PermissionTool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QuestionOption {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QuestionInfo {
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default = "default_true")]
    pub custom: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QuestionTool {
    #[serde(rename = "messageID", default)]
    pub message_id: String,
    #[serde(rename = "callID", default)]
    pub call_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QuestionRequest {
    pub id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: String,
    #[serde(default)]
    pub questions: Vec<QuestionInfo>,
    #[serde(default)]
    pub tool: Option<QuestionTool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageWithParts {
    pub info: MessageInfo,
    #[serde(default)]
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelRef {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "providerID", default)]
    pub provider_id: String,
    #[serde(default)]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PromptModelRef {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

impl From<&ModelRef> for PromptModelRef {
    fn from(model: &ModelRef) -> Self {
        Self {
            provider_id: model.provider_id.clone(),
            model_id: model.id.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PromptMentionSource {
    pub value: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PromptFileSourceText {
    pub value: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PromptRangePosition {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PromptRange {
    pub start: PromptRangePosition,
    pub end: PromptRangePosition,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum PromptFileSource {
    #[serde(rename = "file")]
    File {
        path: String,
        text: PromptFileSourceText,
    },
    #[serde(rename = "symbol")]
    Symbol {
        path: String,
        range: PromptRange,
        name: String,
        kind: u32,
        text: PromptFileSourceText,
    },
    #[serde(rename = "resource")]
    Resource {
        #[serde(rename = "clientName")]
        client_name: String,
        uri: String,
        text: PromptFileSourceText,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum PromptPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "file")]
    File {
        mime: String,
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<PromptFileSource>,
    },
    #[serde(rename = "agent")]
    Agent {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<PromptMentionSource>,
    },
    #[serde(rename = "subtask")]
    Subtask {
        prompt: String,
        description: String,
        agent: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<PromptModelRef>,
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
    },
}

#[allow(dead_code)]
impl PromptPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn file(mime: impl Into<String>, url: impl Into<String>, filename: Option<String>) -> Self {
        Self::File {
            mime: mime.into(),
            url: url.into(),
            filename,
            source: None,
        }
    }

    pub fn file_reference(
        path: &Path,
        display_name: &str,
        start: usize,
        end: usize,
    ) -> Option<Self> {
        Self::file_reference_path(path, display_name, start, end)
    }

    pub fn file_reference_path(
        path: &Path,
        display_name: &str,
        start: usize,
        end: usize,
    ) -> Option<Self> {
        let url = Url::from_file_path(path).ok()?.to_string();
        Some(Self::File {
            mime: file_mime(path).to_owned(),
            url,
            filename: Some(display_name.to_owned()),
            source: Some(PromptFileSource::File {
                path: display_name.to_owned(),
                text: PromptFileSourceText {
                    value: format!("@{display_name}"),
                    start,
                    end,
                },
            }),
        })
    }

    pub fn agent(name: impl Into<String>) -> Self {
        Self::Agent {
            name: name.into(),
            source: None,
        }
    }

    pub fn agent_mention(name: impl Into<String>, start: usize, end: usize) -> Self {
        let name = name.into();
        Self::Agent {
            source: Some(PromptMentionSource {
                value: format!("@{name}"),
                start,
                end,
            }),
            name,
        }
    }

    pub fn subtask(
        prompt: impl Into<String>,
        description: impl Into<String>,
        agent: impl Into<String>,
    ) -> Self {
        Self::Subtask {
            prompt: prompt.into(),
            description: description.into(),
            agent: agent.into(),
            model: None,
            command: None,
        }
    }

    pub fn display(&self) -> PromptPartDisplay {
        match self {
            Self::Text { text } => PromptPartDisplay {
                title: "text".to_owned(),
                detail: format!("{} characters", text.chars().count()),
                preview: truncate_preview(text),
                bytes: Some(text.len()),
            },
            Self::File {
                mime,
                url,
                filename,
                source,
            } => display_file_part(mime, url, filename.as_deref(), source.as_ref()),
            Self::Agent { name, source } => PromptPartDisplay {
                title: format!("@{name}"),
                detail: source
                    .as_ref()
                    .map(|source| format!("mention {}..{}", source.start, source.end))
                    .unwrap_or_else(|| "agent mention".to_owned()),
                preview: "Agent mention".to_owned(),
                bytes: None,
            },
            Self::Subtask {
                prompt,
                description,
                agent,
                model,
                command,
            } => {
                let mut detail = format!("agent {agent}");
                if let Some(model) = model {
                    detail.push_str(&format!("  model {}/{}", model.provider_id, model.model_id));
                }
                if let Some(command) = command {
                    detail.push_str(&format!("  command {command}"));
                }
                let preview = if description.trim().is_empty() {
                    truncate_preview(prompt)
                } else {
                    format!(
                        "{}\n{}",
                        truncate_preview(description),
                        truncate_preview(prompt)
                    )
                };
                PromptPartDisplay {
                    title: format!("subtask:{agent}"),
                    detail,
                    preview,
                    bytes: Some(prompt.len() + description.len()),
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPartDisplay {
    pub title: String,
    pub detail: String,
    pub preview: String,
    pub bytes: Option<usize>,
}

const MAX_INLINE_PREVIEW_ENCODED_BYTES: usize = 1024 * 1024;
const MAX_INLINE_PREVIEW_CHARS: usize = 512;

fn display_file_part(
    mime: &str,
    url: &str,
    filename: Option<&str>,
    source: Option<&PromptFileSource>,
) -> PromptPartDisplay {
    let title = filename.unwrap_or("file").to_owned();
    let source_label = source
        .and_then(prompt_file_source_label)
        .map(|source| format!("  source {source}"))
        .unwrap_or_default();
    let mut detail = format!("{mime}  size unavailable{source_label}");
    let mut preview = "Preview unavailable for this file reference.".to_owned();
    let mut bytes = None;

    if let Some(data) = parse_inline_data_url(url) {
        if let Some(size) = data.decoded_size {
            bytes = Some(size);
            detail = format!("{mime}  {}{source_label}", format_byte_count(size));
        }
        if data.is_textual {
            preview = match data.payload_state {
                InlinePayloadState::Invalid => {
                    "Preview unavailable: invalid base64 data URL.".to_owned()
                }
                InlinePayloadState::TooLarge => {
                    "Preview unavailable: inline data is too large to decode in the TUI.".to_owned()
                }
                InlinePayloadState::Decoded(ref bytes) => match String::from_utf8(bytes.clone()) {
                    Ok(text) => truncate_preview(&text),
                    Err(_) => "Preview unavailable: data is not valid UTF-8.".to_owned(),
                },
            };
        } else {
            preview = if data.payload_state == InlinePayloadState::Invalid {
                "Binary preview unavailable: invalid base64 data URL.".to_owned()
            } else {
                "Binary attachment; terminal preview disabled.".to_owned()
            };
        }
    } else if url.starts_with("data:") {
        preview = "Preview unavailable: data URL is not base64 encoded.".to_owned();
    } else if url.starts_with("file:") {
        preview = "Preview unavailable for a file reference; metadata only.".to_owned();
    }

    PromptPartDisplay {
        title,
        detail,
        preview,
        bytes,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InlineData {
    decoded_size: Option<usize>,
    payload_state: InlinePayloadState,
    is_textual: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InlinePayloadState {
    Invalid,
    TooLarge,
    Decoded(Vec<u8>),
}

fn parse_inline_data_url(url: &str) -> Option<InlineData> {
    let remainder = url.strip_prefix("data:")?;
    let (metadata, payload) = remainder.split_once(',')?;
    let is_textual = metadata.split(';').next().is_some_and(is_text_mime);
    let is_base64 = metadata
        .split(';')
        .any(|part| part.eq_ignore_ascii_case("base64"));
    if !is_base64 {
        return Some(InlineData {
            decoded_size: None,
            payload_state: InlinePayloadState::Invalid,
            is_textual,
        });
    }

    let decoded_size = base64_decoded_size(payload);
    if decoded_size.is_none() {
        return Some(InlineData {
            decoded_size: None,
            payload_state: InlinePayloadState::Invalid,
            is_textual,
        });
    }
    if payload.len() > MAX_INLINE_PREVIEW_ENCODED_BYTES {
        return Some(InlineData {
            decoded_size,
            payload_state: InlinePayloadState::TooLarge,
            is_textual,
        });
    }
    Some(InlineData {
        decoded_size,
        payload_state: match BASE64.decode(payload) {
            Ok(bytes) => InlinePayloadState::Decoded(bytes),
            Err(_) => InlinePayloadState::Invalid,
        },
        is_textual,
    })
}

fn base64_decoded_size(payload: &str) -> Option<usize> {
    if payload.is_empty() {
        return Some(0);
    }
    if !payload.len().is_multiple_of(4)
        || payload
            .bytes()
            .enumerate()
            .any(|(index, byte)| byte == b'=' && index < payload.len().saturating_sub(2))
        || payload.bytes().filter(|byte| *byte == b'=').count() > 2
        || payload.bytes().any(
            |byte| !matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'='),
        )
    {
        return None;
    }
    let padding = payload
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'=')
        .count();
    Some(payload.len() / 4 * 3 - padding)
}

fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/ld+json"
                | "application/javascript"
                | "application/xml"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
                | "application/markdown"
        )
}

fn prompt_file_source_label(source: &PromptFileSource) -> Option<String> {
    match source {
        PromptFileSource::File { path, .. } | PromptFileSource::Symbol { path, .. } => {
            Some(path.clone())
        }
        PromptFileSource::Resource {
            client_name, uri, ..
        } => Some(format!("{client_name}:{uri}")),
    }
}

fn truncate_preview(value: &str) -> String {
    let value = value.replace('\0', "�");
    let mut preview = value
        .chars()
        .take(MAX_INLINE_PREVIEW_CHARS)
        .collect::<String>();
    if value.chars().count() > MAX_INLINE_PREVIEW_CHARS {
        preview.push('…');
    }
    preview
}

fn format_byte_count(bytes: usize) -> String {
    const KIB: usize = 1024;
    const MIB: usize = KIB * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum PromptOutputFormat {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "json_schema")]
    JsonSchema {
        schema: Value,
        #[serde(rename = "retryCount", skip_serializing_if = "Option::is_none")]
        retry_count: Option<u32>,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PromptOptions {
    pub no_reply: bool,
    pub tool_overrides: HashMap<String, bool>,
    pub output_format: Option<PromptOutputFormat>,
    pub system: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PromptRequest {
    #[serde(rename = "messageID", skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<PromptModelRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(rename = "noReply", skip_serializing_if = "Option::is_none")]
    pub no_reply: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<HashMap<String, bool>>,
    #[serde(rename = "format", skip_serializing_if = "Option::is_none")]
    pub output_format: Option<PromptOutputFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub parts: Vec<PromptPart>,
}

impl PromptRequest {
    pub fn from_text(
        text: impl Into<String>,
        model: Option<&ModelRef>,
        agent: Option<&str>,
    ) -> Self {
        Self {
            model: model.map(PromptModelRef::from),
            agent: agent.map(str::to_owned),
            variant: model.and_then(|model| model.variant.clone()),
            parts: vec![PromptPart::text(text)],
            ..Self::default()
        }
    }

    pub fn from_text_with_mentions_and_references(
        text: impl Into<String>,
        model: Option<&ModelRef>,
        agent: Option<&str>,
        directory: Option<&Path>,
        agents: &[AgentInfo],
        references: &[ReferenceInfo],
    ) -> Self {
        let text = text.into();
        let mut request = Self::from_text(&text, model, agent);
        request
            .parts
            .extend(prompt_mention_parts(&text, directory, agents, references));
        request
    }

    pub fn text_content(&self) -> String {
        self.parts
            .iter()
            .filter_map(|part| match part {
                PromptPart::Text { text } => Some(text.as_str()),
                PromptPart::File { .. } | PromptPart::Agent { .. } | PromptPart::Subtask { .. } => {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn prompt_mention_parts(
    text: &str,
    directory: Option<&Path>,
    agents: &[AgentInfo],
    references: &[ReferenceInfo],
) -> Vec<PromptPart> {
    let mut parts = Vec::new();
    let mut cursor = 0;
    while let Some(relative_at) = text[cursor..].find('@') {
        let at = cursor + relative_at;
        if !is_mention_boundary(text, at) {
            cursor = at + 1;
            continue;
        }

        let token_start = at + 1;
        let token_end = text[token_start..]
            .char_indices()
            .find_map(|(offset, character)| {
                character.is_whitespace().then_some(token_start + offset)
            })
            .unwrap_or(text.len());
        let name_end = trim_mention_punctuation(text, token_start, token_end);
        if name_end == token_start {
            cursor = token_end;
            continue;
        }
        let name = &text[token_start..name_end];
        let start = text[..at].chars().count();
        let end = start + text[at..name_end].chars().count();

        if agents
            .iter()
            .any(|agent| agent.is_mentionable() && agent.name == name)
        {
            parts.push(PromptPart::agent_mention(name, start, end));
        } else if let Some(reference) = references
            .iter()
            .find(|reference| !reference.hidden && reference.name == name)
            && let Some(part) =
                PromptPart::file_reference_path(Path::new(&reference.path), name, start, end)
        {
            parts.push(part);
        } else if let Some(directory) = directory
            && let Some(path) = mention_path(directory, name)
            && let Some(part) = PromptPart::file_reference(&path, name, start, end)
        {
            parts.push(part);
        }
        cursor = name_end;
    }
    parts
}

fn is_mention_boundary(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        .is_none_or(|character| character.is_whitespace() || "([{\"'".contains(character))
}

fn trim_mention_punctuation(text: &str, start: usize, mut end: usize) -> usize {
    while end > start {
        let Some((previous, character)) = text[..end].char_indices().next_back() else {
            break;
        };
        if ",.;:!?)]}\"'".contains(character) {
            end = previous;
        } else {
            break;
        }
    }
    end
}

fn mention_path(directory: &Path, name: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(name);
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        directory.join(path)
    };
    std::fs::canonicalize(path).ok()
}

pub(crate) fn file_mime(path: &Path) -> &'static str {
    if path.is_dir() {
        return "application/x-directory";
    }
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        Some("json") => "application/json",
        Some("html" | "htm") => "text/html",
        Some("md" | "markdown") => "text/markdown",
        Some("css") => "text/css",
        Some("js" | "jsx" | "ts" | "tsx") => "text/javascript",
        Some(
            "c" | "cc" | "cpp" | "h" | "hpp" | "java" | "go" | "rs" | "py" | "rb" | "php" | "swift"
            | "kt" | "kts" | "sh" | "bat" | "ps1" | "txt" | "log",
        ) => "text/plain",
        Some("yaml" | "yml") => "text/yaml",
        Some("xml") => "application/xml",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProviderCatalog {
    #[serde(default)]
    pub providers: Vec<ProviderInfo>,
    #[serde(default)]
    pub default: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProviderInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub models: HashMap<String, ModelInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelInfo {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "providerID", default)]
    pub provider_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub limit: ModelLimit,
    #[serde(default)]
    pub variants: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelLimit {
    #[serde(default)]
    pub context: u64,
    #[serde(default)]
    pub output: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Skill {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CommandInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<ModelRef>,
    #[serde(default)]
    pub subtask: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub native: bool,
    #[serde(default)]
    pub hidden: bool,
}

impl AgentInfo {
    pub fn is_selectable(&self) -> bool {
        !self.name.is_empty() && !self.hidden && self.mode != "subagent"
    }

    pub fn is_mentionable(&self) -> bool {
        !self.name.is_empty() && !self.hidden && self.mode != "primary"
    }
}

#[derive(Debug, Clone, Default)]
pub struct McpServer {
    pub name: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct McpStatus {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LspStatus {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TodoItem {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct FileDiff {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub before: String,
    #[serde(default)]
    pub after: String,
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct VcsInfo {
    #[serde(default)]
    pub branch: String,
    #[serde(rename = "default_branch", default)]
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VcsDiffMode {
    #[default]
    Git,
    Branch,
}

impl VcsDiffMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::Branch => "branch",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Git => "working tree",
            Self::Branch => "default branch",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Git => Self::Branch,
            Self::Branch => Self::Git,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct VcsFileDiff {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub patch: String,
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct VcsFileStatus {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(tag = "type")]
pub enum SessionStatus {
    #[serde(rename = "idle")]
    #[default]
    Idle,
    #[serde(rename = "busy")]
    Busy,
    #[serde(rename = "retry")]
    Retry {
        #[serde(default)]
        attempt: u32,
        #[serde(default)]
        message: String,
        #[serde(default)]
        next: i64,
    },
}

impl SessionStatus {
    pub fn label(&self) -> String {
        match self {
            Self::Idle => "idle".to_owned(),
            Self::Busy => "busy".to_owned(),
            Self::Retry {
                attempt,
                message,
                next,
            } => {
                let detail = if message.is_empty() {
                    format!("attempt {attempt}")
                } else {
                    format!("attempt {attempt}: {message}")
                };
                if *next > 0 {
                    format!("retrying {detail} (next {next})")
                } else {
                    format!("retrying {detail}")
                }
            }
        }
    }

    pub fn is_working(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub path: String,
    pub is_directory: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReferenceInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub hidden: bool,
}

impl MessageWithParts {
    pub fn sort_key(&self) -> i64 {
        self.info.time.created
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{AgentInfo, ModelRef, ReferenceInfo};
    use super::{
        PromptFileSource, PromptFileSourceText, PromptOutputFormat, PromptPart, PromptRequest,
    };
    use base64::Engine as _;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn prompt_part_display_decodes_text_and_reports_binary_metadata() {
        let text = PromptPart::file(
            "text/plain",
            "data:text/plain;base64,aGVsbG8=",
            Some("notes.txt".to_owned()),
        )
        .display();
        assert_eq!(text.bytes, Some(5));
        assert_eq!(text.preview, "hello");
        assert!(text.detail.contains("5 B"));

        let binary = PromptPart::file(
            "image/png",
            "data:image/png;base64,iVBORw0KGgo=",
            Some("image.png".to_owned()),
        )
        .display();
        assert_eq!(binary.bytes, Some(8));
        assert!(binary.preview.contains("Binary attachment"));
    }

    #[test]
    fn prompt_part_display_degrades_invalid_and_file_urls_without_panicking() {
        let invalid = PromptPart::file(
            "text/plain",
            "data:text/plain;base64,not-base64",
            Some("bad.txt".to_owned()),
        )
        .display();
        assert!(invalid.preview.contains("invalid base64"));
        assert_eq!(invalid.bytes, None);

        let reference = PromptPart::file(
            "text/plain",
            "file:///workspace/notes.txt",
            Some("notes.txt".to_owned()),
        )
        .display();
        assert!(reference.preview.contains("metadata only"));
    }

    #[test]
    fn prompt_part_display_truncates_long_text_preview() {
        let text = "x".repeat(700);
        let display = PromptPart::file(
            "text/plain",
            format!(
                "data:text/plain;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(text)
            ),
            Some("long.txt".to_owned()),
        )
        .display();
        assert_eq!(display.preview.chars().count(), 513);
        assert!(display.preview.ends_with('…'));
    }
    #[test]
    fn prompt_file_source_and_output_format_keep_api_field_names() {
        let request = PromptRequest {
            output_format: Some(PromptOutputFormat::JsonSchema {
                schema: json!({ "type": "object" }),
                retry_count: Some(2),
            }),
            parts: vec![PromptPart::File {
                mime: "text/plain".to_owned(),
                url: "file:///workspace/notes.txt".to_owned(),
                filename: Some("notes.txt".to_owned()),
                source: Some(PromptFileSource::File {
                    path: "notes.txt".to_owned(),
                    text: PromptFileSourceText {
                        value: "@notes.txt".to_owned(),
                        start: 0,
                        end: 10,
                    },
                }),
            }],
            ..PromptRequest::default()
        };

        assert_eq!(
            serde_json::to_value(request).expect("prompt request serializes"),
            json!({
                "format": {
                    "type": "json_schema",
                    "schema": { "type": "object" },
                    "retryCount": 2
                },
                "parts": [{
                    "type": "file",
                    "mime": "text/plain",
                    "url": "file:///workspace/notes.txt",
                    "filename": "notes.txt",
                    "source": {
                        "type": "file",
                        "path": "notes.txt",
                        "text": {
                            "value": "@notes.txt",
                            "start": 0,
                            "end": 10
                        }
                    }
                }]
            })
        );
    }

    #[test]
    fn prompt_request_serializes_explicit_tool_overrides() {
        let request = PromptRequest {
            tools: Some(HashMap::from([
                ("bash".to_owned(), false),
                ("mcp_search".to_owned(), true),
            ])),
            parts: vec![PromptPart::text("Inspect the repository")],
            ..PromptRequest::default()
        };

        assert_eq!(
            serde_json::to_value(request).expect("prompt request serializes"),
            json!({
                "tools": {
                    "bash": false,
                    "mcp_search": true
                },
                "parts": [{
                    "type": "text",
                    "text": "Inspect the repository"
                }]
            })
        );
    }

    #[test]
    fn prompt_mentions_add_existing_files_and_subagents_as_parts() {
        let directory = std::env::current_dir().expect("test directory");
        let agents = vec![
            AgentInfo {
                name: "explore".to_owned(),
                mode: "subagent".to_owned(),
                ..AgentInfo::default()
            },
            AgentInfo {
                name: "build".to_owned(),
                mode: "primary".to_owned(),
                ..AgentInfo::default()
            },
        ];
        let request = PromptRequest::from_text_with_mentions_and_references(
            "Inspect @README.md with @explore.",
            Some(&ModelRef {
                provider_id: "provider".to_owned(),
                id: "model".to_owned(),
                ..ModelRef::default()
            }),
            Some("build"),
            Some(&directory),
            &agents,
            &[],
        );

        assert_eq!(request.parts.len(), 3);
        assert!(matches!(
            &request.parts[1],
            PromptPart::File {
                filename: Some(filename),
                source: Some(PromptFileSource::File { path, .. }),
                ..
            } if filename == "README.md" && path == "README.md"
        ));
        assert!(matches!(
            &request.parts[2],
            PromptPart::Agent { name, source: Some(source) }
                if name == "explore" && source.value == "@explore"
        ));
        assert_eq!(request.agent.as_deref(), Some("build"));
    }

    #[test]
    fn prompt_mentions_add_server_references_using_their_resolved_path() {
        let request = PromptRequest::from_text_with_mentions_and_references(
            "Read @docs",
            None,
            None,
            None,
            &[],
            &[ReferenceInfo {
                name: "docs".to_owned(),
                path: "C:/workspace/reference-docs".to_owned(),
                ..ReferenceInfo::default()
            }],
        );

        assert!(matches!(
            &request.parts[1],
            PromptPart::File {
                filename: Some(filename),
                url,
                source: Some(PromptFileSource::File { path, .. }),
                ..
            } if filename == "docs"
                && url.starts_with("file:")
                && path == "docs"
        ));
    }
}
