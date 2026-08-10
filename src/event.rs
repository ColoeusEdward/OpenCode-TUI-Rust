use crate::model::{CacheTokens, FileDiff, MessageInfo, ModelRef, Part, TodoItem, TokenUsage};
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MessageUpdatedEvent {
    pub session_id: String,
    pub info: MessageInfo,
}

#[derive(Debug, Clone)]
pub struct MessageRemovedEvent {
    pub session_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone)]
pub struct MessagePartUpdatedEvent {
    pub session_id: String,
    pub part: Part,
}

#[derive(Debug, Clone)]
pub struct MessagePartDeltaEvent {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
    pub field: String,
    pub delta: String,
}

#[derive(Debug, Clone)]
pub struct MessagePartRemovedEvent {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextAgentSwitchedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
    pub agent: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextModelSwitchedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
    pub model: ModelRef,
}

#[derive(Debug, Clone)]
pub struct SessionNextLocation {
    pub directory: String,
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionNextMovedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub location: SessionNextLocation,
    pub subdirectory: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PromptSource {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct PromptFileAttachment {
    pub uri: String,
    pub mime: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub source: Option<PromptSource>,
}

#[derive(Debug, Clone)]
pub struct PromptAgentAttachment {
    pub name: String,
    pub source: Option<PromptSource>,
}

#[derive(Debug, Clone)]
pub struct SessionPrompt {
    pub text: String,
    pub files: Vec<PromptFileAttachment>,
    pub agents: Vec<PromptAgentAttachment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionDelivery {
    Steer,
    Queue,
}

impl SessionDelivery {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::Queue => "queue",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionNextPromptedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
    pub prompt: SessionPrompt,
    pub delivery: SessionDelivery,
}

#[derive(Debug, Clone)]
pub struct SessionNextPromptAdmittedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
    pub prompt: SessionPrompt,
    pub delivery: SessionDelivery,
}

#[derive(Debug, Clone)]
pub struct SessionNextContextUpdatedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextSyntheticEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct RetryError {
    pub message: String,
    pub status_code: Option<f64>,
    pub is_retryable: bool,
    pub response_headers: HashMap<String, String>,
    pub response_body: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl RetryError {
    pub fn summary(&self) -> String {
        let status = self
            .status_code
            .map(|status| status.to_string())
            .unwrap_or_else(|| "unknown".to_owned());
        let body = self
            .response_body
            .as_deref()
            .filter(|body| !body.is_empty())
            .map(|_| "present")
            .unwrap_or("none");
        format!(
            "{} (retryable={}, status={}, headers={}, body={}, metadata={})",
            self.message,
            self.is_retryable,
            status,
            self.response_headers.len(),
            body,
            self.metadata.len()
        )
    }
}

#[derive(Debug, Clone)]
pub struct SessionNextRetriedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub attempt: f64,
    pub error: RetryError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevertFileStatus {
    Added,
    Modified,
    Deleted,
}

impl RevertFileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RevertFileDiff {
    pub path: String,
    pub status: RevertFileStatus,
    pub additions: u64,
    pub deletions: u64,
    pub patch: String,
}

#[derive(Debug, Clone)]
pub struct RevertState {
    pub message_id: String,
    pub part_id: Option<String>,
    pub snapshot: Option<String>,
    pub diff: Option<String>,
    pub files: Vec<RevertFileDiff>,
}

#[derive(Debug, Clone)]
pub struct SessionNextRevertStagedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub revert: RevertState,
}

#[derive(Debug, Clone)]
pub struct SessionNextRevertClearedEvent {
    pub timestamp: i64,
    pub session_id: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextRevertCommittedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone)]
pub struct TodoUpdatedEvent {
    pub session_id: String,
    pub todos: Vec<TodoItem>,
}

#[derive(Debug, Clone)]
pub struct SessionDiffUpdatedEvent {
    pub session_id: String,
    pub diffs: Vec<FileDiff>,
}

#[derive(Debug, Clone)]
pub struct VcsBranchUpdatedEvent {
    pub branch: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionNextShellStartedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
    pub call_id: String,
    pub command: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextShellEndedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub call_id: String,
    pub output: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextStepStartedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub agent: String,
    pub model: ModelRef,
    pub snapshot: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionNextStepEndedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub finish: String,
    pub cost: f64,
    pub tokens: TokenUsage,
    pub snapshot: Option<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionNextStepFailedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub error: EventError,
}

#[derive(Debug, Clone)]
pub struct SessionNextCompactionStartedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextCompactionDeltaEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextCompactionEndedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub message_id: String,
    pub reason: String,
    pub text: String,
    pub recent: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextTextStartedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub text_id: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextTextDeltaEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub text_id: String,
    pub delta: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextTextEndedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub text_id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextReasoningStartedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub reasoning_id: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextReasoningDeltaEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub reasoning_id: String,
    pub delta: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextReasoningEndedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub reasoning_id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextToolInputStartedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub call_id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextToolInputDeltaEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub call_id: String,
    pub delta: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextToolInputEndedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub call_id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SessionNextToolCalledEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub call_id: String,
    pub tool: String,
    pub input: Map<String, Value>,
    pub provider: ToolProvider,
}

#[derive(Debug, Clone)]
pub struct SessionNextToolProgressEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub call_id: String,
    pub structured: Map<String, Value>,
    pub content: Vec<ToolContent>,
}

#[derive(Debug, Clone)]
pub struct SessionNextToolSuccessEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub call_id: String,
    pub structured: Map<String, Value>,
    pub content: Vec<ToolContent>,
    pub output_paths: Vec<String>,
    pub result: Option<Value>,
    pub provider: ToolProvider,
}

#[derive(Debug, Clone)]
pub struct SessionNextToolFailedEvent {
    pub timestamp: i64,
    pub session_id: String,
    pub assistant_message_id: String,
    pub call_id: String,
    pub error: EventError,
    pub result: Option<Value>,
    pub provider: ToolProvider,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventError {
    #[serde(rename = "type")]
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "file")]
    File {
        uri: String,
        mime: String,
        #[serde(default)]
        name: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProvider {
    pub executed: bool,
    #[serde(default)]
    pub metadata: Option<Map<String, Value>>,
}

#[derive(Debug, Clone)]
pub enum ServerEventData {
    MessageUpdated(MessageUpdatedEvent),
    MessageRemoved(MessageRemovedEvent),
    MessagePartUpdated(MessagePartUpdatedEvent),
    MessagePartDelta(MessagePartDeltaEvent),
    MessagePartRemoved(MessagePartRemovedEvent),
    SessionNextAgentSwitched(SessionNextAgentSwitchedEvent),
    SessionNextModelSwitched(SessionNextModelSwitchedEvent),
    SessionNextMoved(SessionNextMovedEvent),
    SessionNextPrompted(SessionNextPromptedEvent),
    SessionNextPromptAdmitted(SessionNextPromptAdmittedEvent),
    SessionNextContextUpdated(SessionNextContextUpdatedEvent),
    SessionNextSynthetic(SessionNextSyntheticEvent),
    SessionNextRetried(SessionNextRetriedEvent),
    SessionNextRevertStaged(SessionNextRevertStagedEvent),
    SessionNextRevertCleared(SessionNextRevertClearedEvent),
    SessionNextRevertCommitted(SessionNextRevertCommittedEvent),
    TodoUpdated(TodoUpdatedEvent),
    SessionDiffUpdated(SessionDiffUpdatedEvent),
    VcsBranchUpdated(VcsBranchUpdatedEvent),
    SessionNextShellStarted(SessionNextShellStartedEvent),
    SessionNextShellEnded(SessionNextShellEndedEvent),
    SessionNextStepStarted(SessionNextStepStartedEvent),
    SessionNextStepEnded(SessionNextStepEndedEvent),
    SessionNextStepFailed(SessionNextStepFailedEvent),
    SessionNextCompactionStarted(SessionNextCompactionStartedEvent),
    SessionNextCompactionDelta(SessionNextCompactionDeltaEvent),
    SessionNextCompactionEnded(SessionNextCompactionEndedEvent),
    SessionNextTextStarted(SessionNextTextStartedEvent),
    SessionNextTextDelta(SessionNextTextDeltaEvent),
    SessionNextTextEnded(SessionNextTextEndedEvent),
    SessionNextReasoningStarted(SessionNextReasoningStartedEvent),
    SessionNextReasoningDelta(SessionNextReasoningDeltaEvent),
    SessionNextReasoningEnded(SessionNextReasoningEndedEvent),
    SessionNextToolInputStarted(SessionNextToolInputStartedEvent),
    SessionNextToolInputDelta(SessionNextToolInputDeltaEvent),
    SessionNextToolInputEnded(SessionNextToolInputEndedEvent),
    SessionNextToolCalled(SessionNextToolCalledEvent),
    SessionNextToolProgress(SessionNextToolProgressEvent),
    SessionNextToolSuccess(SessionNextToolSuccessEvent),
    SessionNextToolFailed(SessionNextToolFailedEvent),
    Known,
    Unknown,
    Invalid { error: String },
}

#[derive(Debug, Clone)]
pub struct ServerEvent {
    pub kind: String,
    pub properties: Value,
    pub directory: Option<String>,
    pub workspace: Option<String>,
    pub data: Box<ServerEventData>,
}

impl ServerEvent {
    pub fn from_json(value: Value) -> Result<Self> {
        let directory = value
            .get("directory")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let workspace = value
            .get("workspace")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let payload = value.get("payload").cloned().unwrap_or(value);
        let kind = payload
            .get("type")
            .and_then(Value::as_str)
            .context("SSE event is missing type")?
            .to_owned();
        let properties = payload
            .get("properties")
            .or_else(|| payload.get("data"))
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        let data = ServerEventData::decode(&kind, &properties);

        Ok(Self {
            kind,
            properties,
            directory,
            workspace,
            data: Box::new(data),
        })
    }

    pub fn local_connected() -> Self {
        Self {
            kind: "client.connected".to_owned(),
            properties: Value::Object(Map::new()),
            directory: None,
            workspace: None,
            data: Box::new(ServerEventData::Known),
        }
    }

    pub fn local_reconnecting(
        message: impl Into<String>,
        attempt: u32,
        retry_in_secs: u64,
    ) -> Self {
        Self {
            kind: "client.reconnecting".to_owned(),
            properties: serde_json::json!({
                "message": message.into(),
                "attempt": attempt,
                "retryIn": retry_in_secs,
            }),
            directory: None,
            workspace: None,
            data: Box::new(ServerEventData::Known),
        }
    }
}

impl ServerEventData {
    fn decode(kind: &str, properties: &Value) -> Self {
        match kind {
            "message.updated" => match decode_payload::<RawMessageUpdatedEvent>(kind, properties) {
                Ok(raw) => match raw.into_event() {
                    Ok(event) => Self::MessageUpdated(event),
                    Err(error) => Self::invalid(kind, properties, error),
                },
                Err(error) => Self::invalid(kind, properties, error),
            },
            "message.removed" => match decode_payload::<RawMessageRemovedEvent>(kind, properties) {
                Ok(raw) => match raw.into_event() {
                    Ok(event) => Self::MessageRemoved(event),
                    Err(error) => Self::invalid(kind, properties, error),
                },
                Err(error) => Self::invalid(kind, properties, error),
            },
            "message.part.updated" => {
                match decode_payload::<RawMessagePartUpdatedEvent>(kind, properties) {
                    Ok(raw) => match raw.into_event() {
                        Ok(event) => Self::MessagePartUpdated(event),
                        Err(error) => Self::invalid(kind, properties, error),
                    },
                    Err(error) => Self::invalid(kind, properties, error),
                }
            }
            "message.part.delta" => {
                match decode_payload::<RawMessagePartDeltaEvent>(kind, properties) {
                    Ok(raw) => match raw.into_event() {
                        Ok(event) => Self::MessagePartDelta(event),
                        Err(error) => Self::invalid(kind, properties, error),
                    },
                    Err(error) => Self::invalid(kind, properties, error),
                }
            }
            "message.part.removed" => {
                match decode_payload::<RawMessagePartRemovedEvent>(kind, properties) {
                    Ok(raw) => match raw.into_event() {
                        Ok(event) => Self::MessagePartRemoved(event),
                        Err(error) => Self::invalid(kind, properties, error),
                    },
                    Err(error) => Self::invalid(kind, properties, error),
                }
            }
            "session.next.agent.switched" => decode_typed(
                kind,
                properties,
                RawSessionNextAgentSwitchedEvent::into_event,
                Self::SessionNextAgentSwitched,
            ),
            "session.next.model.switched" => decode_typed(
                kind,
                properties,
                RawSessionNextModelSwitchedEvent::into_event,
                Self::SessionNextModelSwitched,
            ),
            "session.next.moved" => decode_typed(
                kind,
                properties,
                RawSessionNextMovedEvent::into_event,
                Self::SessionNextMoved,
            ),
            "session.next.prompted" => decode_typed(
                kind,
                properties,
                RawSessionNextPromptedEvent::into_event,
                Self::SessionNextPrompted,
            ),
            "session.next.prompt.admitted" => decode_typed(
                kind,
                properties,
                RawSessionNextPromptAdmittedEvent::into_event,
                Self::SessionNextPromptAdmitted,
            ),
            "session.next.context.updated" => decode_typed(
                kind,
                properties,
                RawSessionNextContextUpdatedEvent::into_event,
                Self::SessionNextContextUpdated,
            ),
            "session.next.synthetic" => decode_typed(
                kind,
                properties,
                RawSessionNextSyntheticEvent::into_event,
                Self::SessionNextSynthetic,
            ),
            "session.next.retried" => decode_typed(
                kind,
                properties,
                RawSessionNextRetriedEvent::into_event,
                Self::SessionNextRetried,
            ),
            "session.next.revert.staged" => decode_typed(
                kind,
                properties,
                RawSessionNextRevertStagedEvent::into_event,
                Self::SessionNextRevertStaged,
            ),
            "session.next.revert.cleared" => decode_typed(
                kind,
                properties,
                RawSessionNextRevertClearedEvent::into_event,
                Self::SessionNextRevertCleared,
            ),
            "session.next.revert.committed" => decode_typed(
                kind,
                properties,
                RawSessionNextRevertCommittedEvent::into_event,
                Self::SessionNextRevertCommitted,
            ),
            "todo.updated" => decode_typed(
                kind,
                properties,
                RawTodoUpdatedEvent::into_event,
                Self::TodoUpdated,
            ),
            "session.diff" => decode_typed(
                kind,
                properties,
                RawSessionDiffUpdatedEvent::into_event,
                Self::SessionDiffUpdated,
            ),
            "vcs.branch.updated" => decode_typed(
                kind,
                properties,
                RawVcsBranchUpdatedEvent::into_event,
                Self::VcsBranchUpdated,
            ),
            "session.next.shell.started" => decode_typed(
                kind,
                properties,
                RawSessionNextShellStartedEvent::into_event,
                Self::SessionNextShellStarted,
            ),
            "session.next.shell.ended" => decode_typed(
                kind,
                properties,
                RawSessionNextShellEndedEvent::into_event,
                Self::SessionNextShellEnded,
            ),
            "session.next.step.started" => decode_typed(
                kind,
                properties,
                RawSessionNextStepStartedEvent::into_event,
                Self::SessionNextStepStarted,
            ),
            "session.next.step.ended" => decode_typed(
                kind,
                properties,
                RawSessionNextStepEndedEvent::into_event,
                Self::SessionNextStepEnded,
            ),
            "session.next.step.failed" => decode_typed(
                kind,
                properties,
                RawSessionNextStepFailedEvent::into_event,
                Self::SessionNextStepFailed,
            ),
            "session.next.compaction.started" => decode_typed(
                kind,
                properties,
                RawSessionNextCompactionStartedEvent::into_event,
                Self::SessionNextCompactionStarted,
            ),
            "session.next.compaction.delta" => decode_typed(
                kind,
                properties,
                RawSessionNextCompactionDeltaEvent::into_event,
                Self::SessionNextCompactionDelta,
            ),
            "session.next.compaction.ended" => decode_typed(
                kind,
                properties,
                RawSessionNextCompactionEndedEvent::into_event,
                Self::SessionNextCompactionEnded,
            ),
            "session.next.text.started" => decode_typed(
                kind,
                properties,
                RawSessionNextTextStartedEvent::into_event,
                Self::SessionNextTextStarted,
            ),
            "session.next.text.delta" => decode_typed(
                kind,
                properties,
                RawSessionNextTextDeltaEvent::into_event,
                Self::SessionNextTextDelta,
            ),
            "session.next.text.ended" => decode_typed(
                kind,
                properties,
                RawSessionNextTextEndedEvent::into_event,
                Self::SessionNextTextEnded,
            ),
            "session.next.reasoning.started" => decode_typed(
                kind,
                properties,
                RawSessionNextReasoningStartedEvent::into_event,
                Self::SessionNextReasoningStarted,
            ),
            "session.next.reasoning.delta" => decode_typed(
                kind,
                properties,
                RawSessionNextReasoningDeltaEvent::into_event,
                Self::SessionNextReasoningDelta,
            ),
            "session.next.reasoning.ended" => decode_typed(
                kind,
                properties,
                RawSessionNextReasoningEndedEvent::into_event,
                Self::SessionNextReasoningEnded,
            ),
            "session.next.tool.input.started" => decode_typed(
                kind,
                properties,
                RawSessionNextToolInputStartedEvent::into_event,
                Self::SessionNextToolInputStarted,
            ),
            "session.next.tool.input.delta" => decode_typed(
                kind,
                properties,
                RawSessionNextToolInputDeltaEvent::into_event,
                Self::SessionNextToolInputDelta,
            ),
            "session.next.tool.input.ended" => decode_typed(
                kind,
                properties,
                RawSessionNextToolInputEndedEvent::into_event,
                Self::SessionNextToolInputEnded,
            ),
            "session.next.tool.called" => decode_typed(
                kind,
                properties,
                RawSessionNextToolCalledEvent::into_event,
                Self::SessionNextToolCalled,
            ),
            "session.next.tool.progress" => decode_typed(
                kind,
                properties,
                RawSessionNextToolProgressEvent::into_event,
                Self::SessionNextToolProgress,
            ),
            "session.next.tool.success" => decode_typed(
                kind,
                properties,
                RawSessionNextToolSuccessEvent::into_event,
                Self::SessionNextToolSuccess,
            ),
            "session.next.tool.failed" => decode_typed(
                kind,
                properties,
                RawSessionNextToolFailedEvent::into_event,
                Self::SessionNextToolFailed,
            ),
            _ if is_known_kind(kind) => Self::Known,
            _ => Self::Unknown,
        }
    }

    fn invalid(kind: &str, properties: &Value, error: impl Into<String>) -> Self {
        let _ = (kind, properties);
        Self::Invalid {
            error: error.into(),
        }
    }

    pub fn validation_error(&self) -> Option<&str> {
        match self {
            Self::Invalid { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawMessageUpdatedEvent {
    #[serde(rename = "sessionID")]
    session_id: String,
    info: MessageInfo,
}

impl RawMessageUpdatedEvent {
    fn into_event(self) -> std::result::Result<MessageUpdatedEvent, String> {
        let mut info = self.info;
        validate_non_empty("message.updated", "sessionID", &self.session_id)?;
        validate_non_empty("message.updated", "info.id", &info.id)?;
        validate_non_empty("message.updated", "info.role", &info.role)?;
        if info.session_id.is_empty() {
            info.session_id = self.session_id.clone();
        } else if info.session_id != self.session_id {
            return Err("message.updated has mismatched sessionID and info.sessionID".to_owned());
        }
        Ok(MessageUpdatedEvent {
            session_id: self.session_id,
            info,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawMessageRemovedEvent {
    #[serde(rename = "sessionID")]
    session_id: String,
    #[serde(rename = "messageID")]
    message_id: String,
}

impl RawMessageRemovedEvent {
    fn into_event(self) -> std::result::Result<MessageRemovedEvent, String> {
        validate_non_empty("message.removed", "sessionID", &self.session_id)?;
        validate_non_empty("message.removed", "messageID", &self.message_id)?;
        Ok(MessageRemovedEvent {
            session_id: self.session_id,
            message_id: self.message_id,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawMessagePartUpdatedEvent {
    #[serde(rename = "sessionID")]
    session_id: String,
    part: Part,
}

impl RawMessagePartUpdatedEvent {
    fn into_event(self) -> std::result::Result<MessagePartUpdatedEvent, String> {
        let mut part = self.part;
        validate_non_empty("message.part.updated", "sessionID", &self.session_id)?;
        validate_non_empty("message.part.updated", "part.id", &part.id)?;
        validate_non_empty("message.part.updated", "part.messageID", &part.message_id)?;
        validate_non_empty("message.part.updated", "part.type", &part.kind)?;
        if part.session_id.is_empty() {
            part.session_id = self.session_id.clone();
        } else if part.session_id != self.session_id {
            return Err(
                "message.part.updated has mismatched sessionID and part.sessionID".to_owned(),
            );
        }
        Ok(MessagePartUpdatedEvent {
            session_id: self.session_id,
            part,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawMessagePartDeltaEvent {
    #[serde(rename = "sessionID")]
    session_id: String,
    #[serde(rename = "messageID")]
    message_id: String,
    #[serde(rename = "partID")]
    part_id: String,
    field: String,
    delta: String,
}

impl RawMessagePartDeltaEvent {
    fn into_event(self) -> std::result::Result<MessagePartDeltaEvent, String> {
        validate_non_empty("message.part.delta", "sessionID", &self.session_id)?;
        validate_non_empty("message.part.delta", "messageID", &self.message_id)?;
        validate_non_empty("message.part.delta", "partID", &self.part_id)?;
        validate_non_empty("message.part.delta", "field", &self.field)?;
        Ok(MessagePartDeltaEvent {
            session_id: self.session_id,
            message_id: self.message_id,
            part_id: self.part_id,
            field: self.field,
            delta: self.delta,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawMessagePartRemovedEvent {
    #[serde(rename = "sessionID")]
    session_id: String,
    #[serde(rename = "messageID")]
    message_id: String,
    #[serde(rename = "partID")]
    part_id: String,
}

impl RawMessagePartRemovedEvent {
    fn into_event(self) -> std::result::Result<MessagePartRemovedEvent, String> {
        validate_non_empty("message.part.removed", "sessionID", &self.session_id)?;
        validate_non_empty("message.part.removed", "messageID", &self.message_id)?;
        validate_non_empty("message.part.removed", "partID", &self.part_id)?;
        Ok(MessagePartRemovedEvent {
            session_id: self.session_id,
            message_id: self.message_id,
            part_id: self.part_id,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionEventBase {
    timestamp: i64,
    #[serde(rename = "sessionID")]
    session_id: String,
}

impl RawSessionEventBase {
    fn into_parts(self, kind: &str) -> std::result::Result<(i64, String), String> {
        validate_non_empty(kind, "sessionID", &self.session_id)?;
        Ok((self.timestamp, self.session_id))
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextAgentSwitchedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
    agent: String,
}

impl RawSessionNextAgentSwitchedEvent {
    fn into_event(self) -> std::result::Result<SessionNextAgentSwitchedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.agent.switched")?;
        validate_non_empty("session.next.agent.switched", "messageID", &self.message_id)?;
        validate_non_empty("session.next.agent.switched", "agent", &self.agent)?;
        Ok(SessionNextAgentSwitchedEvent {
            timestamp,
            session_id,
            message_id: self.message_id,
            agent: self.agent,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextModelSwitchedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
    model: ModelRef,
}

impl RawSessionNextModelSwitchedEvent {
    fn into_event(self) -> std::result::Result<SessionNextModelSwitchedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.model.switched")?;
        validate_non_empty("session.next.model.switched", "messageID", &self.message_id)?;
        validate_model("session.next.model.switched", &self.model)?;
        Ok(SessionNextModelSwitchedEvent {
            timestamp,
            session_id,
            message_id: self.message_id,
            model: self.model,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextLocation {
    directory: String,
    #[serde(rename = "workspaceID", default)]
    workspace_id: Option<String>,
}

impl RawSessionNextLocation {
    fn into_event(self, kind: &str) -> std::result::Result<SessionNextLocation, String> {
        validate_non_empty(kind, "location.directory", &self.directory)?;
        Ok(SessionNextLocation {
            directory: self.directory,
            workspace_id: self.workspace_id,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextMovedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    location: RawSessionNextLocation,
    #[serde(default)]
    subdirectory: Option<String>,
}

impl RawSessionNextMovedEvent {
    fn into_event(self) -> std::result::Result<SessionNextMovedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.moved")?;
        Ok(SessionNextMovedEvent {
            timestamp,
            session_id,
            location: self.location.into_event("session.next.moved")?,
            subdirectory: self.subdirectory,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawPromptSource {
    start: usize,
    end: usize,
    text: String,
}

impl RawPromptSource {
    fn into_event(self) -> PromptSource {
        PromptSource {
            start: self.start,
            end: self.end,
            text: self.text,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawPromptFileAttachment {
    uri: String,
    mime: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    source: Option<RawPromptSource>,
}

impl RawPromptFileAttachment {
    fn into_event(self, kind: &str) -> std::result::Result<PromptFileAttachment, String> {
        validate_non_empty(kind, "prompt.files[].uri", &self.uri)?;
        validate_non_empty(kind, "prompt.files[].mime", &self.mime)?;
        Ok(PromptFileAttachment {
            uri: self.uri,
            mime: self.mime,
            name: self.name,
            description: self.description,
            source: self.source.map(RawPromptSource::into_event),
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawPromptAgentAttachment {
    name: String,
    #[serde(default)]
    source: Option<RawPromptSource>,
}

impl RawPromptAgentAttachment {
    fn into_event(self, kind: &str) -> std::result::Result<PromptAgentAttachment, String> {
        validate_non_empty(kind, "prompt.agents[].name", &self.name)?;
        Ok(PromptAgentAttachment {
            name: self.name,
            source: self.source.map(RawPromptSource::into_event),
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionPrompt {
    text: String,
    #[serde(default)]
    files: Vec<RawPromptFileAttachment>,
    #[serde(default)]
    agents: Vec<RawPromptAgentAttachment>,
}

impl RawSessionPrompt {
    fn into_event(self, kind: &str) -> std::result::Result<SessionPrompt, String> {
        Ok(SessionPrompt {
            text: self.text,
            files: self
                .files
                .into_iter()
                .map(|file| file.into_event(kind))
                .collect::<std::result::Result<Vec<_>, _>>()?,
            agents: self
                .agents
                .into_iter()
                .map(|agent| agent.into_event(kind))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        })
    }
}

fn parse_delivery(kind: &str, value: &str) -> std::result::Result<SessionDelivery, String> {
    match value {
        "steer" => Ok(SessionDelivery::Steer),
        "queue" => Ok(SessionDelivery::Queue),
        _ => Err(format!("{kind} has unsupported delivery {value}")),
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextPromptedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
    prompt: RawSessionPrompt,
    delivery: String,
}

impl RawSessionNextPromptedEvent {
    fn into_parts(
        self,
        kind: &str,
    ) -> std::result::Result<(i64, String, String, SessionPrompt, SessionDelivery), String> {
        let (timestamp, session_id) = self.base.into_parts(kind)?;
        validate_non_empty(kind, "messageID", &self.message_id)?;
        let prompt = self.prompt.into_event(kind)?;
        let delivery = parse_delivery(kind, &self.delivery)?;
        Ok((timestamp, session_id, self.message_id, prompt, delivery))
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextPromptAdmittedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
    prompt: RawSessionPrompt,
    delivery: String,
}

impl RawSessionNextPromptAdmittedEvent {
    fn into_event(self) -> std::result::Result<SessionNextPromptAdmittedEvent, String> {
        let (timestamp, session_id, message_id, prompt, delivery) = RawSessionNextPromptedEvent {
            base: self.base,
            message_id: self.message_id,
            prompt: self.prompt,
            delivery: self.delivery,
        }
        .into_parts("session.next.prompt.admitted")?;
        Ok(SessionNextPromptAdmittedEvent {
            timestamp,
            session_id,
            message_id,
            prompt,
            delivery,
        })
    }
}

impl RawSessionNextPromptedEvent {
    fn into_event(self) -> std::result::Result<SessionNextPromptedEvent, String> {
        let (timestamp, session_id, message_id, prompt, delivery) =
            self.into_parts("session.next.prompted")?;
        Ok(SessionNextPromptedEvent {
            timestamp,
            session_id,
            message_id,
            prompt,
            delivery,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextContextUpdatedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
    text: String,
}

impl RawSessionNextContextUpdatedEvent {
    fn into_event(self) -> std::result::Result<SessionNextContextUpdatedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.context.updated")?;
        validate_non_empty(
            "session.next.context.updated",
            "messageID",
            &self.message_id,
        )?;
        Ok(SessionNextContextUpdatedEvent {
            timestamp,
            session_id,
            message_id: self.message_id,
            text: self.text,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextSyntheticEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
    text: String,
}

impl RawSessionNextSyntheticEvent {
    fn into_event(self) -> std::result::Result<SessionNextSyntheticEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.synthetic")?;
        validate_non_empty("session.next.synthetic", "messageID", &self.message_id)?;
        Ok(SessionNextSyntheticEvent {
            timestamp,
            session_id,
            message_id: self.message_id,
            text: self.text,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawRetryError {
    message: String,
    #[serde(rename = "statusCode", default)]
    status_code: Option<f64>,
    #[serde(rename = "isRetryable")]
    is_retryable: bool,
    #[serde(rename = "responseHeaders", default)]
    response_headers: HashMap<String, String>,
    #[serde(rename = "responseBody", default)]
    response_body: Option<String>,
    #[serde(default)]
    metadata: HashMap<String, String>,
}

impl RawRetryError {
    fn into_event(self, kind: &str) -> std::result::Result<RetryError, String> {
        validate_non_empty(kind, "error.message", &self.message)?;
        if self.status_code.is_some_and(|status| !status.is_finite()) {
            return Err(format!("{kind} has non-finite error.statusCode"));
        }
        Ok(RetryError {
            message: self.message,
            status_code: self.status_code,
            is_retryable: self.is_retryable,
            response_headers: self.response_headers,
            response_body: self.response_body,
            metadata: self.metadata,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextRetriedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    attempt: f64,
    error: RawRetryError,
}

impl RawSessionNextRetriedEvent {
    fn into_event(self) -> std::result::Result<SessionNextRetriedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.retried")?;
        if !self.attempt.is_finite() || self.attempt < 0.0 {
            return Err("session.next.retried has invalid attempt".to_owned());
        }
        Ok(SessionNextRetriedEvent {
            timestamp,
            session_id,
            attempt: self.attempt,
            error: self.error.into_event("session.next.retried")?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawRevertFileDiff {
    path: String,
    status: String,
    additions: u64,
    deletions: u64,
    patch: String,
}

impl RawRevertFileDiff {
    fn into_event(self, kind: &str) -> std::result::Result<RevertFileDiff, String> {
        validate_non_empty(kind, "revert.files[].path", &self.path)?;
        let status = match self.status.as_str() {
            "added" => RevertFileStatus::Added,
            "modified" => RevertFileStatus::Modified,
            "deleted" => RevertFileStatus::Deleted,
            _ => {
                return Err(format!(
                    "{kind} has unsupported revert file status {}",
                    self.status
                ));
            }
        };
        Ok(RevertFileDiff {
            path: self.path,
            status,
            additions: self.additions,
            deletions: self.deletions,
            patch: self.patch,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawRevertState {
    #[serde(rename = "messageID")]
    message_id: String,
    #[serde(rename = "partID", default)]
    part_id: Option<String>,
    #[serde(default)]
    snapshot: Option<String>,
    #[serde(default)]
    diff: Option<String>,
    #[serde(default)]
    files: Vec<RawRevertFileDiff>,
}

impl RawRevertState {
    fn into_event(self, kind: &str) -> std::result::Result<RevertState, String> {
        validate_non_empty(kind, "revert.messageID", &self.message_id)?;
        Ok(RevertState {
            message_id: self.message_id,
            part_id: self.part_id,
            snapshot: self.snapshot,
            diff: self.diff,
            files: self
                .files
                .into_iter()
                .map(|file| file.into_event(kind))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextRevertStagedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    revert: RawRevertState,
}

impl RawSessionNextRevertStagedEvent {
    fn into_event(self) -> std::result::Result<SessionNextRevertStagedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.revert.staged")?;
        Ok(SessionNextRevertStagedEvent {
            timestamp,
            session_id,
            revert: self.revert.into_event("session.next.revert.staged")?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextRevertClearedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
}

impl RawSessionNextRevertClearedEvent {
    fn into_event(self) -> std::result::Result<SessionNextRevertClearedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.revert.cleared")?;
        Ok(SessionNextRevertClearedEvent {
            timestamp,
            session_id,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextRevertCommittedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
}

impl RawSessionNextRevertCommittedEvent {
    fn into_event(self) -> std::result::Result<SessionNextRevertCommittedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.revert.committed")?;
        validate_non_empty(
            "session.next.revert.committed",
            "messageID",
            &self.message_id,
        )?;
        Ok(SessionNextRevertCommittedEvent {
            timestamp,
            session_id,
            message_id: self.message_id,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextShellStartedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
    #[serde(rename = "callID")]
    call_id: String,
    command: String,
}

impl RawSessionNextShellStartedEvent {
    fn into_event(self) -> std::result::Result<SessionNextShellStartedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.shell.started")?;
        validate_non_empty("session.next.shell.started", "messageID", &self.message_id)?;
        validate_non_empty("session.next.shell.started", "callID", &self.call_id)?;
        Ok(SessionNextShellStartedEvent {
            timestamp,
            session_id,
            message_id: self.message_id,
            call_id: self.call_id,
            command: self.command,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextShellEndedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "callID")]
    call_id: String,
    output: String,
}

impl RawSessionNextShellEndedEvent {
    fn into_event(self) -> std::result::Result<SessionNextShellEndedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.shell.ended")?;
        validate_non_empty("session.next.shell.ended", "callID", &self.call_id)?;
        Ok(SessionNextShellEndedEvent {
            timestamp,
            session_id,
            call_id: self.call_id,
            output: self.output,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextStepStartedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    agent: String,
    model: ModelRef,
    #[serde(default)]
    snapshot: Option<String>,
}

impl RawSessionNextStepStartedEvent {
    fn into_event(self) -> std::result::Result<SessionNextStepStartedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.step.started")?;
        validate_non_empty(
            "session.next.step.started",
            "assistantMessageID",
            &self.assistant_message_id,
        )?;
        validate_model("session.next.step.started", &self.model)?;
        Ok(SessionNextStepStartedEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            agent: self.agent,
            model: self.model,
            snapshot: self.snapshot,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawStepTokens {
    input: u64,
    output: u64,
    reasoning: u64,
    cache: RawCacheTokens,
}

#[derive(Debug, Deserialize)]
struct RawCacheTokens {
    read: u64,
    write: u64,
}

impl RawStepTokens {
    fn into_model(self) -> TokenUsage {
        TokenUsage {
            total: 0,
            input: self.input,
            output: self.output,
            reasoning: self.reasoning,
            cache: CacheTokens {
                read: self.cache.read,
                write: self.cache.write,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextStepEndedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    finish: String,
    cost: f64,
    tokens: RawStepTokens,
    #[serde(default)]
    snapshot: Option<String>,
    #[serde(default)]
    files: Vec<String>,
}

impl RawSessionNextStepEndedEvent {
    fn into_event(self) -> std::result::Result<SessionNextStepEndedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.step.ended")?;
        validate_non_empty(
            "session.next.step.ended",
            "assistantMessageID",
            &self.assistant_message_id,
        )?;
        Ok(SessionNextStepEndedEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            finish: self.finish,
            cost: self.cost,
            tokens: self.tokens.into_model(),
            snapshot: self.snapshot,
            files: self.files,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextStepFailedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    error: EventError,
}

impl RawSessionNextStepFailedEvent {
    fn into_event(self) -> std::result::Result<SessionNextStepFailedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.step.failed")?;
        validate_non_empty(
            "session.next.step.failed",
            "assistantMessageID",
            &self.assistant_message_id,
        )?;
        validate_error("session.next.step.failed", &self.error)?;
        Ok(SessionNextStepFailedEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            error: self.error,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextCompactionStartedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
    reason: String,
}

impl RawSessionNextCompactionStartedEvent {
    fn into_event(self) -> std::result::Result<SessionNextCompactionStartedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.compaction.started")?;
        validate_compaction_identity(
            "session.next.compaction.started",
            &self.message_id,
            &self.reason,
        )?;
        Ok(SessionNextCompactionStartedEvent {
            timestamp,
            session_id,
            message_id: self.message_id,
            reason: self.reason,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextCompactionDeltaEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
    text: String,
}

impl RawSessionNextCompactionDeltaEvent {
    fn into_event(self) -> std::result::Result<SessionNextCompactionDeltaEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.compaction.delta")?;
        validate_non_empty(
            "session.next.compaction.delta",
            "messageID",
            &self.message_id,
        )?;
        Ok(SessionNextCompactionDeltaEvent {
            timestamp,
            session_id,
            message_id: self.message_id,
            text: self.text,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextCompactionEndedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "messageID")]
    message_id: String,
    reason: String,
    text: String,
    recent: String,
}

impl RawSessionNextCompactionEndedEvent {
    fn into_event(self) -> std::result::Result<SessionNextCompactionEndedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.compaction.ended")?;
        validate_compaction_identity(
            "session.next.compaction.ended",
            &self.message_id,
            &self.reason,
        )?;
        Ok(SessionNextCompactionEndedEvent {
            timestamp,
            session_id,
            message_id: self.message_id,
            reason: self.reason,
            text: self.text,
            recent: self.recent,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextTextStartedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "textID")]
    text_id: String,
}

impl RawSessionNextTextStartedEvent {
    fn into_event(self) -> std::result::Result<SessionNextTextStartedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.text.started")?;
        validate_text_identity(
            "session.next.text.started",
            &self.assistant_message_id,
            &self.text_id,
        )?;
        Ok(SessionNextTextStartedEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            text_id: self.text_id,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextTextDeltaEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "textID")]
    text_id: String,
    delta: String,
}

impl RawSessionNextTextDeltaEvent {
    fn into_event(self) -> std::result::Result<SessionNextTextDeltaEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.text.delta")?;
        validate_text_identity(
            "session.next.text.delta",
            &self.assistant_message_id,
            &self.text_id,
        )?;
        Ok(SessionNextTextDeltaEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            text_id: self.text_id,
            delta: self.delta,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextTextEndedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "textID")]
    text_id: String,
    text: String,
}

impl RawSessionNextTextEndedEvent {
    fn into_event(self) -> std::result::Result<SessionNextTextEndedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.text.ended")?;
        validate_text_identity(
            "session.next.text.ended",
            &self.assistant_message_id,
            &self.text_id,
        )?;
        Ok(SessionNextTextEndedEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            text_id: self.text_id,
            text: self.text,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextReasoningStartedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "reasoningID")]
    reasoning_id: String,
}

impl RawSessionNextReasoningStartedEvent {
    fn into_event(self) -> std::result::Result<SessionNextReasoningStartedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.reasoning.started")?;
        validate_reasoning_identity(
            "session.next.reasoning.started",
            &self.assistant_message_id,
            &self.reasoning_id,
        )?;
        Ok(SessionNextReasoningStartedEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            reasoning_id: self.reasoning_id,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextReasoningDeltaEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "reasoningID")]
    reasoning_id: String,
    delta: String,
}

impl RawSessionNextReasoningDeltaEvent {
    fn into_event(self) -> std::result::Result<SessionNextReasoningDeltaEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.reasoning.delta")?;
        validate_reasoning_identity(
            "session.next.reasoning.delta",
            &self.assistant_message_id,
            &self.reasoning_id,
        )?;
        Ok(SessionNextReasoningDeltaEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            reasoning_id: self.reasoning_id,
            delta: self.delta,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextReasoningEndedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "reasoningID")]
    reasoning_id: String,
    text: String,
}

impl RawSessionNextReasoningEndedEvent {
    fn into_event(self) -> std::result::Result<SessionNextReasoningEndedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.reasoning.ended")?;
        validate_reasoning_identity(
            "session.next.reasoning.ended",
            &self.assistant_message_id,
            &self.reasoning_id,
        )?;
        Ok(SessionNextReasoningEndedEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            reasoning_id: self.reasoning_id,
            text: self.text,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextToolInputStartedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "callID")]
    call_id: String,
    name: String,
}

impl RawSessionNextToolInputStartedEvent {
    fn into_event(self) -> std::result::Result<SessionNextToolInputStartedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.tool.input.started")?;
        validate_tool_identity(
            "session.next.tool.input.started",
            &self.assistant_message_id,
            &self.call_id,
        )?;
        Ok(SessionNextToolInputStartedEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            call_id: self.call_id,
            name: self.name,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextToolInputDeltaEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "callID")]
    call_id: String,
    delta: String,
}

impl RawSessionNextToolInputDeltaEvent {
    fn into_event(self) -> std::result::Result<SessionNextToolInputDeltaEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.tool.input.delta")?;
        validate_tool_identity(
            "session.next.tool.input.delta",
            &self.assistant_message_id,
            &self.call_id,
        )?;
        Ok(SessionNextToolInputDeltaEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            call_id: self.call_id,
            delta: self.delta,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextToolInputEndedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "callID")]
    call_id: String,
    text: String,
}

impl RawSessionNextToolInputEndedEvent {
    fn into_event(self) -> std::result::Result<SessionNextToolInputEndedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.tool.input.ended")?;
        validate_tool_identity(
            "session.next.tool.input.ended",
            &self.assistant_message_id,
            &self.call_id,
        )?;
        Ok(SessionNextToolInputEndedEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            call_id: self.call_id,
            text: self.text,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextToolCalledEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "callID")]
    call_id: String,
    tool: String,
    input: Map<String, Value>,
    provider: ToolProvider,
}

impl RawSessionNextToolCalledEvent {
    fn into_event(self) -> std::result::Result<SessionNextToolCalledEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.tool.called")?;
        validate_tool_identity(
            "session.next.tool.called",
            &self.assistant_message_id,
            &self.call_id,
        )?;
        Ok(SessionNextToolCalledEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            call_id: self.call_id,
            tool: self.tool,
            input: self.input,
            provider: self.provider,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextToolProgressEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "callID")]
    call_id: String,
    structured: Map<String, Value>,
    content: Vec<ToolContent>,
}

impl RawSessionNextToolProgressEvent {
    fn into_event(self) -> std::result::Result<SessionNextToolProgressEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.tool.progress")?;
        validate_tool_identity(
            "session.next.tool.progress",
            &self.assistant_message_id,
            &self.call_id,
        )?;
        Ok(SessionNextToolProgressEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            call_id: self.call_id,
            structured: self.structured,
            content: self.content,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextToolSuccessEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "callID")]
    call_id: String,
    structured: Map<String, Value>,
    content: Vec<ToolContent>,
    #[serde(default)]
    output_paths: Vec<String>,
    #[serde(default)]
    result: Option<Value>,
    provider: ToolProvider,
}

impl RawSessionNextToolSuccessEvent {
    fn into_event(self) -> std::result::Result<SessionNextToolSuccessEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.tool.success")?;
        validate_tool_identity(
            "session.next.tool.success",
            &self.assistant_message_id,
            &self.call_id,
        )?;
        Ok(SessionNextToolSuccessEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            call_id: self.call_id,
            structured: self.structured,
            content: self.content,
            output_paths: self.output_paths,
            result: self.result,
            provider: self.provider,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionNextToolFailedEvent {
    #[serde(flatten)]
    base: RawSessionEventBase,
    #[serde(rename = "assistantMessageID")]
    assistant_message_id: String,
    #[serde(rename = "callID")]
    call_id: String,
    error: EventError,
    #[serde(default)]
    result: Option<Value>,
    provider: ToolProvider,
}

impl RawSessionNextToolFailedEvent {
    fn into_event(self) -> std::result::Result<SessionNextToolFailedEvent, String> {
        let (timestamp, session_id) = self.base.into_parts("session.next.tool.failed")?;
        validate_tool_identity(
            "session.next.tool.failed",
            &self.assistant_message_id,
            &self.call_id,
        )?;
        validate_error("session.next.tool.failed", &self.error)?;
        Ok(SessionNextToolFailedEvent {
            timestamp,
            session_id,
            assistant_message_id: self.assistant_message_id,
            call_id: self.call_id,
            error: self.error,
            result: self.result,
            provider: self.provider,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawTodoUpdatedEvent {
    #[serde(rename = "sessionID")]
    session_id: String,
    #[serde(default)]
    todos: Vec<TodoItem>,
}

impl RawTodoUpdatedEvent {
    fn into_event(self) -> std::result::Result<TodoUpdatedEvent, String> {
        validate_non_empty("todo.updated", "sessionID", &self.session_id)?;
        Ok(TodoUpdatedEvent {
            session_id: self.session_id,
            todos: self.todos,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawSessionDiffUpdatedEvent {
    #[serde(rename = "sessionID")]
    session_id: String,
    #[serde(default)]
    diff: Vec<FileDiff>,
}

impl RawSessionDiffUpdatedEvent {
    fn into_event(self) -> std::result::Result<SessionDiffUpdatedEvent, String> {
        validate_non_empty("session.diff", "sessionID", &self.session_id)?;
        Ok(SessionDiffUpdatedEvent {
            session_id: self.session_id,
            diffs: self.diff,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawVcsBranchUpdatedEvent {
    #[serde(default)]
    branch: Option<String>,
}

impl RawVcsBranchUpdatedEvent {
    fn into_event(self) -> std::result::Result<VcsBranchUpdatedEvent, String> {
        Ok(VcsBranchUpdatedEvent {
            branch: self.branch,
        })
    }
}

fn decode_payload<T>(kind: &str, properties: &Value) -> std::result::Result<T, String>
where
    T: DeserializeOwned,
{
    serde_json::from_value(properties.clone()).map_err(|error| format!("{kind} payload: {error}"))
}

fn decode_typed<T, U>(
    kind: &str,
    properties: &Value,
    convert: fn(T) -> std::result::Result<U, String>,
    wrap: fn(U) -> ServerEventData,
) -> ServerEventData
where
    T: DeserializeOwned,
{
    match decode_payload::<T>(kind, properties) {
        Ok(raw) => match convert(raw) {
            Ok(event) => wrap(event),
            Err(error) => ServerEventData::invalid(kind, properties, error),
        },
        Err(error) => ServerEventData::invalid(kind, properties, error),
    }
}

fn validate_non_empty(kind: &str, field: &str, value: &str) -> std::result::Result<(), String> {
    if value.is_empty() {
        Err(format!("{kind} is missing {field}"))
    } else {
        Ok(())
    }
}

fn validate_model(kind: &str, model: &ModelRef) -> std::result::Result<(), String> {
    validate_non_empty(kind, "model.id", &model.id)?;
    validate_non_empty(kind, "model.providerID", &model.provider_id)
}

fn validate_text_identity(
    kind: &str,
    assistant_message_id: &str,
    text_id: &str,
) -> std::result::Result<(), String> {
    validate_non_empty(kind, "assistantMessageID", assistant_message_id)?;
    validate_non_empty(kind, "textID", text_id)
}

fn validate_reasoning_identity(
    kind: &str,
    assistant_message_id: &str,
    reasoning_id: &str,
) -> std::result::Result<(), String> {
    validate_non_empty(kind, "assistantMessageID", assistant_message_id)?;
    validate_non_empty(kind, "reasoningID", reasoning_id)
}

fn validate_compaction_identity(
    kind: &str,
    message_id: &str,
    reason: &str,
) -> std::result::Result<(), String> {
    validate_non_empty(kind, "messageID", message_id)?;
    if matches!(reason, "auto" | "manual") {
        Ok(())
    } else {
        Err(format!("{kind} has unsupported reason {reason}"))
    }
}

fn validate_tool_identity(
    kind: &str,
    assistant_message_id: &str,
    call_id: &str,
) -> std::result::Result<(), String> {
    validate_non_empty(kind, "assistantMessageID", assistant_message_id)?;
    validate_non_empty(kind, "callID", call_id)
}

fn validate_error(kind: &str, error: &EventError) -> std::result::Result<(), String> {
    if error.kind != "unknown" {
        return Err(format!("{kind} has unsupported error type {}", error.kind));
    }
    validate_non_empty(kind, "error.message", &error.message)
}

fn is_known_kind(kind: &str) -> bool {
    matches!(
        kind,
        "sync"
            | "server.connected"
            | "server.heartbeat"
            | "server.instance.disposed"
            | "client.connected"
            | "client.error"
            | "client.reconnecting"
            | "session.created"
            | "session.updated"
            | "session.deleted"
            | "session.status"
            | "permission.asked"
            | "permission.replied"
            | "question.asked"
            | "question.replied"
            | "question.rejected"
            | "session.next.agent.switched"
            | "session.next.model.switched"
            | "session.next.moved"
            | "session.next.prompted"
            | "session.next.prompt.admitted"
            | "session.next.context.updated"
            | "session.next.synthetic"
            | "session.next.retried"
            | "session.next.revert.staged"
            | "session.next.revert.cleared"
            | "session.next.revert.committed"
            | "todo.updated"
            | "session.diff"
            | "vcs.branch.updated"
            | "session.next.step.started"
            | "session.next.text.started"
            | "session.next.text.delta"
            | "session.next.text.ended"
            | "session.next.step.ended"
            | "session.next.step.failed"
            | "session.next.compaction.started"
            | "session.next.compaction.delta"
            | "session.next.compaction.ended"
            | "session.next.tool.called"
            | "session.next.tool.input.started"
            | "session.next.tool.input.delta"
            | "session.next.tool.input.ended"
            | "session.next.tool.progress"
            | "session.next.tool.success"
            | "session.next.tool.failed"
            | "session.next.reasoning.started"
            | "session.next.reasoning.delta"
            | "session.next.reasoning.ended"
            | "lsp.updated"
            | "mcp.tools.changed"
    )
}

pub fn parse_sse_frame(frame: &str) -> Result<Option<ServerEvent>> {
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");

    if data.is_empty() {
        return Ok(None);
    }

    let value: Value = serde_json::from_str(&data).context("invalid SSE JSON payload")?;
    Ok(Some(ServerEvent::from_json(value)?))
}

pub fn require_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

pub fn property_object<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(value)
}

pub fn property_session_id(value: &Value) -> Option<&str> {
    require_string(value, "sessionID").or_else(|| {
        value
            .get("info")
            .and_then(|info| require_string(info, "sessionID"))
    })
}

pub fn property_message_id(value: &Value) -> Option<&str> {
    require_string(value, "messageID").or_else(|| {
        value
            .get("info")
            .and_then(|info| require_string(info, "id"))
    })
}

#[cfg(test)]
mod tests {
    use super::{ServerEvent, ServerEventData, parse_sse_frame};
    use serde_json::json;

    #[test]
    fn parses_direct_event() {
        let event = ServerEvent::from_json(json!({
            "id": "evt_1",
            "type": "server.connected",
            "properties": {}
        }))
        .expect("direct event should parse");

        assert_eq!(event.kind, "server.connected");
        assert!(event.directory.is_none());
        assert!(matches!(event.data.as_ref(), ServerEventData::Known));
    }

    #[test]
    fn unwraps_global_event_envelope() {
        let event = ServerEvent::from_json(json!({
            "directory": "E:/project",
            "workspace": "ws_1",
            "payload": {
                "type": "message.part.delta",
                "properties": {
                    "sessionID": "ses_1",
                    "messageID": "msg_1",
                    "partID": "prt_1",
                    "field": "text",
                    "delta": "hello"
                }
            }
        }))
        .expect("global event should parse");

        assert_eq!(event.kind, "message.part.delta");
        assert_eq!(event.directory.as_deref(), Some("E:/project"));
        assert_eq!(event.properties["delta"], "hello");
        assert!(matches!(
            event.data.as_ref(),
            ServerEventData::MessagePartDelta(_)
        ));
    }

    #[test]
    fn parses_sse_data_lines() {
        let event = parse_sse_frame(
            "event: message\ndata: {\"type\":\"server.heartbeat\",\"properties\":{}}",
        )
        .expect("SSE frame should parse")
        .expect("SSE data should produce an event");

        assert_eq!(event.kind, "server.heartbeat");
    }

    #[test]
    fn decodes_message_and_part_events_into_typed_payloads() {
        let message = ServerEvent::from_json(json!({
            "type": "message.updated",
            "properties": {
                "sessionID": "ses_1",
                "info": {
                    "id": "msg_1",
                    "sessionID": "ses_1",
                    "role": "assistant"
                }
            }
        }))
        .expect("message event should parse");
        assert!(matches!(
            message.data.as_ref(),
            ServerEventData::MessageUpdated(_)
        ));

        let part = ServerEvent::from_json(json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_1",
                    "sessionID": "ses_1",
                    "messageID": "msg_1",
                    "type": "text",
                    "text": "hello"
                },
                "time": 1
            }
        }))
        .expect("part event should parse");
        assert!(matches!(
            part.data.as_ref(),
            ServerEventData::MessagePartUpdated(_)
        ));
    }

    #[test]
    fn decodes_live_text_tool_shell_and_step_events() {
        let step = ServerEvent::from_json(json!({
            "type": "session.next.step.started",
            "properties": {
                "timestamp": 1,
                "sessionID": "ses_1",
                "assistantMessageID": "msg_1",
                "agent": "build",
                "model": { "id": "model_1", "providerID": "provider_1" },
                "snapshot": "snap_1"
            }
        }))
        .expect("step event should parse");
        assert!(matches!(
            step.data.as_ref(),
            ServerEventData::SessionNextStepStarted(payload)
                if payload.session_id == "ses_1"
                    && payload.assistant_message_id == "msg_1"
                    && payload.model.id == "model_1"
        ));

        let text = ServerEvent::from_json(json!({
            "type": "session.next.text.delta",
            "properties": {
                "timestamp": 2,
                "sessionID": "ses_1",
                "assistantMessageID": "msg_1",
                "textID": "txt_1",
                "delta": "hello"
            }
        }))
        .expect("text event should parse");
        assert!(matches!(
            text.data.as_ref(),
            ServerEventData::SessionNextTextDelta(payload) if payload.delta == "hello"
        ));

        let tool = ServerEvent::from_json(json!({
            "type": "session.next.tool.called",
            "properties": {
                "timestamp": 3,
                "sessionID": "ses_1",
                "assistantMessageID": "msg_1",
                "callID": "call_1",
                "tool": "bash",
                "input": { "command": "pwd" },
                "provider": { "executed": true }
            }
        }))
        .expect("tool event should parse");
        assert!(matches!(
            tool.data.as_ref(),
            ServerEventData::SessionNextToolCalled(payload)
                if payload.call_id == "call_1" && payload.input["command"] == "pwd"
        ));

        let shell = ServerEvent::from_json(json!({
            "type": "session.next.shell.ended",
            "properties": {
                "timestamp": 4,
                "sessionID": "ses_1",
                "callID": "shell_1",
                "output": "done"
            }
        }))
        .expect("shell event should parse");
        assert!(matches!(
            shell.data.as_ref(),
            ServerEventData::SessionNextShellEnded(payload) if payload.output == "done"
        ));
    }

    #[test]
    fn decodes_compaction_lifecycle_events_with_reason_validation() {
        let started = ServerEvent::from_json(json!({
            "type": "session.next.compaction.started",
            "properties": {
                "timestamp": 10,
                "sessionID": "ses_1",
                "messageID": "cmp_1",
                "reason": "auto"
            }
        }))
        .expect("compaction start should parse");
        assert!(matches!(
            started.data.as_ref(),
            ServerEventData::SessionNextCompactionStarted(payload)
                if payload.message_id == "cmp_1" && payload.reason == "auto"
        ));

        let ended = ServerEvent::from_json(json!({
            "type": "session.next.compaction.ended",
            "properties": {
                "timestamp": 12,
                "sessionID": "ses_1",
                "messageID": "cmp_1",
                "reason": "manual",
                "text": "Summary",
                "recent": "Recent context"
            }
        }))
        .expect("compaction end should parse");
        assert!(matches!(
            ended.data.as_ref(),
            ServerEventData::SessionNextCompactionEnded(payload)
                if payload.text == "Summary" && payload.recent == "Recent context"
        ));

        let invalid = ServerEvent::from_json(json!({
            "type": "session.next.compaction.started",
            "properties": {
                "timestamp": 10,
                "sessionID": "ses_1",
                "messageID": "cmp_1",
                "reason": "unknown"
            }
        }))
        .expect("invalid compaction reason should remain parseable");
        assert!(matches!(
            invalid.data.as_ref(),
            ServerEventData::Invalid { .. }
        ));
    }

    #[test]
    fn decodes_todo_session_diff_and_vcs_events_with_typed_payloads() {
        let todo = ServerEvent::from_json(json!({
            "type": "todo.updated",
            "properties": {
                "sessionID": "ses_1",
                "todos": [{
                    "id": "todo_1",
                    "content": "Review diff",
                    "status": "in_progress",
                    "priority": "high"
                }]
            }
        }))
        .expect("todo event should parse");
        assert!(matches!(
            todo.data.as_ref(),
            ServerEventData::TodoUpdated(payload)
                if payload.session_id == "ses_1" && payload.todos[0].content == "Review diff"
        ));

        let diff = ServerEvent::from_json(json!({
            "type": "session.diff",
            "properties": {
                "sessionID": "ses_1",
                "diff": [{
                    "file": "src/main.rs",
                    "before": "",
                    "after": "+line",
                    "additions": 1,
                    "deletions": 0
                }]
            }
        }))
        .expect("session diff event should parse");
        assert!(matches!(
            diff.data.as_ref(),
            ServerEventData::SessionDiffUpdated(payload)
                if payload.diffs[0].file == "src/main.rs"
        ));

        let branch = ServerEvent::from_json(json!({
            "type": "vcs.branch.updated",
            "properties": { "branch": "feature" }
        }))
        .expect("VCS branch event should parse");
        assert!(matches!(
            branch.data.as_ref(),
            ServerEventData::VcsBranchUpdated(payload)
                if payload.branch.as_deref() == Some("feature")
        ));
    }

    #[test]
    fn decodes_control_session_event_family_with_schema_validation() {
        let agent = ServerEvent::from_json(json!({
            "type": "session.next.agent.switched",
            "properties": {
                "timestamp": 20,
                "sessionID": "ses_1",
                "messageID": "evt_agent",
                "agent": "build"
            }
        }))
        .expect("agent switch should parse");
        assert!(matches!(
            agent.data.as_ref(),
            ServerEventData::SessionNextAgentSwitched(payload) if payload.agent == "build"
        ));

        let model = ServerEvent::from_json(json!({
            "type": "session.next.model.switched",
            "properties": {
                "timestamp": 21,
                "sessionID": "ses_1",
                "messageID": "evt_model",
                "model": { "id": "model_1", "providerID": "provider_1" }
            }
        }))
        .expect("model switch should parse");
        assert!(matches!(
            model.data.as_ref(),
            ServerEventData::SessionNextModelSwitched(payload)
                if payload.model.provider_id == "provider_1"
        ));

        let prompted = ServerEvent::from_json(json!({
            "type": "session.next.prompted",
            "properties": {
                "timestamp": 22,
                "sessionID": "ses_1",
                "messageID": "msg_user",
                "prompt": {
                    "text": "Review this",
                    "files": [{
                        "uri": "file:///workspace/README.md",
                        "mime": "text/markdown",
                        "name": "README.md",
                        "source": { "start": 0, "end": 14, "text": "@README.md" }
                    }],
                    "agents": [{
                        "name": "explore",
                        "source": { "start": 15, "end": 23, "text": "@explore" }
                    }]
                },
                "delivery": "queue"
            }
        }))
        .expect("prompt event should parse");
        assert!(matches!(
            prompted.data.as_ref(),
            ServerEventData::SessionNextPrompted(payload)
                if payload.prompt.text == "Review this"
                    && payload.prompt.files.len() == 1
                    && payload.prompt.agents[0].name == "explore"
                    && payload.delivery.as_str() == "queue"
        ));

        let moved = ServerEvent::from_json(json!({
            "type": "session.next.moved",
            "properties": {
                "timestamp": 23,
                "sessionID": "ses_1",
                "location": { "directory": "E:/workspace", "workspaceID": "ws_1" },
                "subdirectory": "packages/tui"
            }
        }))
        .expect("move event should parse");
        assert!(matches!(
            moved.data.as_ref(),
            ServerEventData::SessionNextMoved(payload)
                if payload.location.directory == "E:/workspace"
                    && payload.subdirectory.as_deref() == Some("packages/tui")
        ));

        let retry = ServerEvent::from_json(json!({
            "type": "session.next.retried",
            "properties": {
                "timestamp": 24,
                "sessionID": "ses_1",
                "attempt": 2,
                "error": {
                    "message": "rate limited",
                    "statusCode": 429,
                    "isRetryable": true,
                    "responseHeaders": { "retry-after": "1" },
                    "responseBody": "busy",
                    "metadata": { "provider": "one" }
                }
            }
        }))
        .expect("retry event should parse");
        assert!(matches!(
            retry.data.as_ref(),
            ServerEventData::SessionNextRetried(payload)
                if payload.attempt == 2.0
                    && payload.error.response_headers["retry-after"] == "1"
                    && payload.error.summary().contains("rate limited")
        ));

        let revert = ServerEvent::from_json(json!({
            "type": "session.next.revert.staged",
            "properties": {
                "timestamp": 25,
                "sessionID": "ses_1",
                "revert": {
                    "messageID": "msg_user",
                    "partID": "part_1",
                    "snapshot": "snap_1",
                    "diff": "diff --git",
                    "files": [{
                        "path": "README.md",
                        "status": "modified",
                        "additions": 2,
                        "deletions": 1,
                        "patch": "@@ -1 +1 @@"
                    }]
                }
            }
        }))
        .expect("revert event should parse");
        assert!(matches!(
            revert.data.as_ref(),
            ServerEventData::SessionNextRevertStaged(payload)
                if payload.revert.files[0].status.as_str() == "modified"
                    && payload.revert.files[0].additions == 2
        ));

        let invalid = ServerEvent::from_json(json!({
            "type": "session.next.prompted",
            "properties": {
                "timestamp": 26,
                "sessionID": "ses_1",
                "messageID": "msg_user",
                "prompt": { "text": "hello" },
                "delivery": "invalid"
            }
        }))
        .expect("invalid control event should remain parseable");
        assert!(matches!(
            invalid.data.as_ref(),
            ServerEventData::Invalid { .. }
        ));
    }

    #[test]
    fn malformed_live_event_stays_at_the_protocol_boundary() {
        let event = ServerEvent::from_json(json!({
            "type": "session.next.tool.called",
            "properties": {
                "timestamp": 3,
                "sessionID": "ses_1",
                "assistantMessageID": "msg_1",
                "tool": "bash",
                "input": {},
                "provider": { "executed": true }
            }
        }))
        .expect("malformed live event should remain parseable");

        assert!(matches!(
            event.data.as_ref(),
            ServerEventData::Invalid { .. }
        ));
        assert_eq!(event.properties["tool"], "bash");
    }

    #[test]
    fn preserves_unknown_and_malformed_event_payloads() {
        let unknown = ServerEvent::from_json(json!({
            "type": "future.event",
            "properties": { "sessionID": "ses_1", "value": 42 }
        }))
        .expect("unknown event should remain parseable");
        assert!(matches!(unknown.data.as_ref(), ServerEventData::Unknown));
        assert_eq!(unknown.kind, "future.event");
        assert_eq!(unknown.properties["value"], 42);

        let malformed = ServerEvent::from_json(json!({
            "type": "message.part.delta",
            "properties": { "sessionID": "ses_1", "delta": "missing ids" }
        }))
        .expect("malformed event should remain parseable");
        assert!(matches!(
            malformed.data.as_ref(),
            ServerEventData::Invalid { .. }
        ));
        assert_eq!(malformed.kind, "message.part.delta");
        assert_eq!(malformed.properties["delta"], "missing ids");
    }
}
