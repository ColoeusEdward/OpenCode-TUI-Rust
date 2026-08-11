use crate::api::ApiClient;
use crate::catalog_state::CatalogState;
use crate::command::{CommandOption, SlashCommand, matching_commands_with_server, slash_query};
use crate::command_palette::filter_commands;
use crate::composer::ComposerAction;
use crate::dialog::{
    OverlayState, agent_options, model_options, skill_options, tool_override_names, variant_options,
};
use crate::event::{
    MessagePartDeltaEvent, MessagePartRemovedEvent, ServerEvent, ServerEventData,
    SessionNextLocation, property_message_id, property_object, property_session_id, require_string,
};
use crate::integration_state::IntegrationState;
use crate::mention::{MentionKind, MentionOption, mention_context, mention_options};
use crate::model::{
    MessageInfo, ModelRef, Part, PermissionRequest, PromptOutputFormat, PromptPart, PromptRequest,
    QuestionInfo, QuestionRequest, Session, SessionStatus, VcsDiffMode,
};
use crate::notification_state::NotificationState;
use crate::pending_state::PendingState;
use crate::prompt_state::{PromptPanelItem, PromptState, PromptSubmission};
use crate::runtime::{ApiRequest, ApiResult, AppMsg, Effect, SessionSnapshot};
use crate::runtime_state::RuntimeState;
use crate::scroll::{ScrollAnchor, ScrollState};
use crate::selection::SelectionState;
pub use crate::session_state::Screen;
use crate::session_state::SessionState;
use crate::theme::Theme;
use crate::transcript_state::TranscriptState;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogDialog {
    Model,
    Skill,
    Agent,
    Variant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimelineEntry {
    pub(crate) message_id: String,
    pub(crate) text: String,
    pub(crate) created: i64,
}

pub struct App {
    pub client: Arc<ApiClient>,
    pub theme: Theme,
    pub session: SessionState,
    pub transcript: TranscriptState,
    pub prompt: PromptState,
    pub catalog: CatalogState,
    pub sidebar_scroll: ScrollState,
    pub sidebar_area: Option<Rect>,
    pub selection: SelectionState,
    pub runtime: RuntimeState,
    pub notifications: NotificationState,
    pub pending: PendingState,
    pub integrations: IntegrationState,
    pub overlay: Option<OverlayState>,
    prompt_panel_return: Option<usize>,
}

impl App {
    pub fn new(client: Arc<ApiClient>) -> Self {
        Self::with_theme(client, Theme::default())
    }

    pub fn with_theme(client: Arc<ApiClient>, theme: Theme) -> Self {
        Self::with_catalog(client, theme, CatalogState::persistent())
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(client: Arc<ApiClient>) -> Self {
        Self::with_catalog(client, Theme::default(), CatalogState::default())
    }

    fn with_catalog(client: Arc<ApiClient>, theme: Theme, catalog: CatalogState) -> Self {
        Self {
            client,
            theme,
            session: SessionState::default(),
            transcript: TranscriptState::default(),
            prompt: PromptState::default(),
            catalog,
            // The sidebar renders static status sections, so it stays where the
            // user scrolled instead of following the last line.
            sidebar_scroll: ScrollState::top_anchored(),
            sidebar_area: None,
            selection: SelectionState::default(),
            runtime: RuntimeState::default(),
            notifications: NotificationState::default(),
            pending: PendingState::default(),
            integrations: IntegrationState::default(),
            overlay: None,
            prompt_panel_return: None,
        }
    }

    pub fn initial_effects(&mut self, requested_session: Option<&str>) -> Vec<Effect> {
        self.runtime.mark_connecting();
        let mut effects = self.refresh_effects();
        if let Some(session_id) = requested_session {
            self.session.opening_session = Some(session_id.to_owned());
            effects.push(Effect::Api(ApiRequest::OpenSession(session_id.to_owned())));
        }
        effects
    }

    pub fn refresh_effects(&self) -> Vec<Effect> {
        let mut effects = vec![
            Effect::Api(ApiRequest::Health),
            self.session_list_effect(),
            Effect::Api(ApiRequest::ListPermissions),
            Effect::Api(ApiRequest::ListQuestions),
            Effect::Api(ApiRequest::ListProviders),
            Effect::Api(ApiRequest::ListSkills),
            Effect::Api(ApiRequest::ListCommands),
            Effect::Api(ApiRequest::ListAgents),
            Effect::Api(ApiRequest::ListReferences),
            Effect::Api(ApiRequest::ListMcp),
            Effect::Api(ApiRequest::ListLsp),
            Effect::Api(ApiRequest::ListSessionStatuses),
            Effect::Api(ApiRequest::ListVcs),
            Effect::Api(ApiRequest::ListVcsStatus),
        ];
        let directory = self
            .client
            .directory()
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok());
        if let Some(directory) = directory {
            effects.push(Effect::Api(ApiRequest::ListWorkspaceFiles { directory }));
        }
        effects
    }

    fn session_list_effect(&self) -> Effect {
        if self.session.show_archived {
            Effect::Api(ApiRequest::ListArchivedSessions)
        } else {
            Effect::Api(ApiRequest::ListSessions)
        }
    }

    pub fn update(&mut self, message: AppMsg) -> Vec<Effect> {
        match message {
            AppMsg::Terminal(event) => self.handle_terminal_event(event),
            AppMsg::Server(event) => self.handle_server_event(event),
            AppMsg::Api(result) => self.handle_api_result(*result),
            AppMsg::Tick => {
                self.notifications.tick();
                // Cursor timing is advanced directly by the runtime around every
                // event; this reducer tick remains for slow application state.
                Vec::new()
            }
        }
    }

    fn handle_terminal_event(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Paste(text) => {
                if (self.session.screen == Screen::Session
                    || matches!(
                        self.overlay,
                        Some(OverlayState::RenameSession { .. } | OverlayState::AttachFile { .. })
                    ))
                    && self.current_permission().is_none()
                    && self.current_question().is_none()
                {
                    self.handle_paste(&text);
                    return self.sync_prompt_overlays();
                }
                Vec::new()
            }
            Event::Mouse(mouse) if self.session.screen == Screen::Session => {
                self.handle_mouse(mouse)
            }
            Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {
                Vec::new()
            }
        }
    }

    /// Wheel events scroll the pane under the pointer; left-button press, drag,
    /// and release drive text selection over the transcript and prompt. The
    /// runtime sidebar is not a selectable pane, so a press there only clears an
    /// existing selection.
    fn handle_mouse(&mut self, mouse: MouseEvent) -> Vec<Effect> {
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                // A selection anchors to screen positions, so scrolling mid-drag
                // would leave the anchor pointing at whatever text moved under it.
                // Real terminals hold still during a drag for the same reason.
                if self.selection.is_dragging() {
                    return Vec::new();
                }
                let delta = if mouse.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                };
                if self.mouse_over_sidebar(mouse.column, mouse.row) {
                    self.sidebar_scroll.scroll_lines(delta);
                } else {
                    self.transcript.scroll.scroll_lines(delta);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.selection.press(mouse.column, mouse.row);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.selection.drag(mouse.column, mouse.row);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(text) = self.selection.release() {
                    let lines = text.lines().count();
                    self.notifications.set(if lines > 1 {
                        format!("Copied {lines} lines")
                    } else {
                        "Copied selection".to_owned()
                    });
                    return vec![Effect::CopyToClipboard(text)];
                }
            }
            _ => {}
        }
        Vec::new()
    }

    fn handle_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.kind != KeyEventKind::Press {
            return Vec::new();
        }
        // A visible selection is dismissed by any keypress, the way a terminal's own
        // selection is. The key is not consumed: `Esc` already rejects permissions
        // and closes overlays, and swallowing it here would make dismissing a
        // highlight silently cost the user one of those.
        self.selection.clear();
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return vec![Effect::Quit];
        }
        if key.code == KeyCode::Char('?')
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            self.overlay = Some(OverlayState::Help);
            return Vec::new();
        }
        if self.overlay.is_some() {
            return self.handle_overlay_key(key);
        }
        match self.session.screen {
            Screen::Home => self.handle_home_key(key),
            Screen::Session => self.handle_session_key(key),
        }
    }

    pub fn handle_server_event(&mut self, event: ServerEvent) -> Vec<Effect> {
        if event.kind == "sync" {
            return Vec::new();
        }
        if !self.event_matches_scope(&event) {
            return Vec::new();
        }
        if let Some(error) = event.data.validation_error() {
            self.notifications
                .set(format!("Ignored malformed {} event: {error}", event.kind));
            return Vec::new();
        }
        match event.kind.as_str() {
            "server.connected" | "client.connected" => {
                self.runtime.mark_connected();
                self.notifications
                    .success(format!("Connected to {}", self.client.base_url()));
                Vec::new()
            }
            "client.error" => {
                self.runtime.mark_disconnected();
                self.notifications.error(
                    event
                        .properties
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("OpenCode client error"),
                );
                Vec::new()
            }
            "client.reconnecting" => {
                let attempt = event
                    .properties
                    .get("attempt")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .min(u32::MAX as u64) as u32;
                let retry_in_secs = event
                    .properties
                    .get("retryIn")
                    .and_then(Value::as_u64)
                    .unwrap_or(1);
                let message = event
                    .properties
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("event stream disconnected");
                self.runtime.mark_reconnecting(attempt, retry_in_secs);
                self.notifications.warning(format!(
                    "Connection lost; retrying in {retry_in_secs}s (attempt {attempt}): {message}"
                ));
                Vec::new()
            }
            "server.heartbeat" => {
                self.runtime.mark_connected();
                Vec::new()
            }
            "server.instance.disposed" => {
                self.runtime.mark_disconnected();
                self.runtime.set_working(false);
                self.notifications
                    .set("OpenCode instance was disposed".to_owned());
                let mut effects = vec![
                    Effect::Api(ApiRequest::Health),
                    self.session_list_effect(),
                    Effect::Api(ApiRequest::ListProviders),
                    Effect::Api(ApiRequest::ListSkills),
                    Effect::Api(ApiRequest::ListCommands),
                    Effect::Api(ApiRequest::ListAgents),
                    Effect::Api(ApiRequest::ListReferences),
                    Effect::Api(ApiRequest::ListMcp),
                    Effect::Api(ApiRequest::ListLsp),
                    Effect::Api(ApiRequest::ListSessionStatuses),
                    Effect::Api(ApiRequest::ListVcs),
                    Effect::Api(ApiRequest::ListVcsStatus),
                ];
                if let Some(session_id) = self
                    .session
                    .current_session
                    .as_ref()
                    .map(|session| session.id.clone())
                {
                    effects.push(Effect::Api(ApiRequest::RefreshCurrent(session_id)));
                }
                effects
            }
            "session.updated" | "session.created" => {
                if let Some(session) = parse_session(property_object(&event.properties, "info")) {
                    let visible = self.session.session_visible(&session);
                    let is_current = self
                        .session
                        .current_session
                        .as_ref()
                        .is_some_and(|current| current.id == session.id);
                    if visible {
                        self.upsert_session(session.clone());
                        if is_current {
                            self.session.current_session = Some(session);
                        }
                    } else {
                        let session_id = session.id.clone();
                        self.session.sessions.retain(|item| item.id != session_id);
                        self.pending.remove_session(&session_id);
                        self.runtime.clear_session_status(&session_id);
                        if is_current {
                            self.clear_current_session();
                        }
                    }
                }
                Vec::new()
            }
            "session.deleted" => {
                let id = property_message_id(&event.properties).map(str::to_owned);
                if let Some(id) = id {
                    self.session.sessions.retain(|session| session.id != id);
                    self.pending.remove_session(&id);
                    self.runtime.clear_session_status(&id);
                    if self
                        .session
                        .current_session
                        .as_ref()
                        .is_some_and(|session| session.id == id)
                    {
                        self.session.current_session = None;
                        self.transcript.clear();
                        self.integrations.clear_session_panels();
                        self.runtime.set_working(false);
                        self.runtime.reset_response();
                        self.sidebar_scroll.reset();
                        self.session.screen = Screen::Home;
                    }
                }
                Vec::new()
            }
            "session.status" => {
                if self.event_session_matches(&event.properties) {
                    let session_id = property_session_id(&event.properties)
                        .map(str::to_owned)
                        .unwrap_or_default();
                    let status_value = event
                        .properties
                        .get("status")
                        .cloned()
                        .unwrap_or(Value::Null);
                    if let Ok(status) = serde_json::from_value::<SessionStatus>(status_value) {
                        let idle = matches!(status, SessionStatus::Idle);
                        let became_idle = idle
                            && self
                                .runtime
                                .session_statuses
                                .get(&session_id)
                                .map(SessionStatus::is_working)
                                .unwrap_or(self.runtime.working);
                        self.runtime.set_session_status(session_id, status.clone());
                        self.runtime.set_working(status.is_working());
                        if let SessionStatus::Retry { message, .. } = status {
                            self.notifications.warning(if message.is_empty() {
                                "Retrying session".to_owned()
                            } else {
                                message
                            });
                        }
                        if became_idle {
                            return self.dispatch_next_queued_prompt();
                        }
                    } else {
                        self.runtime.set_working(true);
                    }
                }
                Vec::new()
            }
            "session.next.agent.switched" => {
                if let ServerEventData::SessionNextAgentSwitched(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.catalog.select_agent(payload.agent.clone());
                    self.apply_agent_switch(&payload.session_id, &payload.agent);
                    self.touch_session(&payload.session_id, payload.timestamp);
                    self.transcript.append_event_message(
                        &payload.session_id,
                        &payload.message_id,
                        "system",
                        &format!("Agent switched to `{}`", payload.agent),
                        payload.timestamp,
                        None,
                    );
                    self.notifications
                        .set(format!("Agent switched to {}", payload.agent));
                }
                Vec::new()
            }
            "session.next.model.switched" => {
                if let ServerEventData::SessionNextModelSwitched(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.select_model_for_current_session(payload.model.clone());
                    self.apply_model_switch(&payload.session_id, &payload.model);
                    self.touch_session(&payload.session_id, payload.timestamp);
                    let model_label = payload
                        .model
                        .variant
                        .as_deref()
                        .map(|variant| {
                            format!(
                                "`{}/{} ({variant})`",
                                payload.model.provider_id, payload.model.id
                            )
                        })
                        .unwrap_or_else(|| {
                            format!("`{}/{}`", payload.model.provider_id, payload.model.id)
                        });
                    self.transcript.append_event_message(
                        &payload.session_id,
                        &payload.message_id,
                        "system",
                        &format!("Model switched to {model_label}"),
                        payload.timestamp,
                        None,
                    );
                    self.notifications.set(format!(
                        "Model switched to {}/{}",
                        payload.model.provider_id, payload.model.id
                    ));
                }
                Vec::new()
            }
            "session.next.moved" => {
                if let ServerEventData::SessionNextMoved(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.apply_session_location(
                        &payload.session_id,
                        &payload.location,
                        payload.timestamp,
                    );
                    self.notifications
                        .set(match payload.subdirectory.as_deref() {
                            Some(subdirectory) if !subdirectory.is_empty() => {
                                format!(
                                    "Session moved to {} ({subdirectory})",
                                    payload.location.directory
                                )
                            }
                            _ => format!("Session moved to {}", payload.location.directory),
                        });
                }
                Vec::new()
            }
            "session.next.prompted" => {
                if let ServerEventData::SessionNextPrompted(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.runtime.set_working(true);
                    self.runtime.set_response_input(&payload.prompt.text);
                    self.transcript.append_event_message(
                        &payload.session_id,
                        &payload.message_id,
                        "user",
                        &payload.prompt.text,
                        payload.timestamp,
                        Some(prompt_event_state(&payload.prompt, payload.delivery)),
                    );
                }
                Vec::new()
            }
            "session.next.prompt.admitted" => {
                if let ServerEventData::SessionNextPromptAdmitted(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.runtime.set_working(true);
                    self.runtime.set_response_input(&payload.prompt.text);
                    self.touch_session(&payload.session_id, payload.timestamp);
                    self.notifications.set(format!(
                        "Prompt {} admitted ({}) [files={}, agents={}]",
                        payload.message_id,
                        payload.delivery.as_str(),
                        payload.prompt.files.len(),
                        payload.prompt.agents.len()
                    ));
                }
                Vec::new()
            }
            "session.next.context.updated" => {
                if let ServerEventData::SessionNextContextUpdated(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.append_event_message(
                        &payload.session_id,
                        &payload.message_id,
                        "system",
                        &payload.text,
                        payload.timestamp,
                        Some(json!({ "kind": "context-updated" })),
                    );
                }
                Vec::new()
            }
            "session.next.synthetic" => {
                if let ServerEventData::SessionNextSynthetic(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.append_event_message(
                        &payload.session_id,
                        &payload.message_id,
                        "synthetic",
                        &payload.text,
                        payload.timestamp,
                        Some(json!({ "kind": "synthetic" })),
                    );
                }
                Vec::new()
            }
            "session.next.retried" => {
                if let ServerEventData::SessionNextRetried(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.runtime.set_working(true);
                    self.touch_session(&payload.session_id, payload.timestamp);
                    self.notifications.set(format!(
                        "Retrying attempt {}: {}",
                        payload.attempt,
                        payload.error.summary()
                    ));
                }
                Vec::new()
            }
            "session.next.revert.staged" => {
                if let ServerEventData::SessionNextRevertStaged(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.touch_session(&payload.session_id, payload.timestamp);
                    self.integrations.stage_revert(payload.revert.clone());
                    self.notifications.set(format!(
                        "Revert staged for {} file(s)",
                        payload.revert.files.len()
                    ));
                }
                Vec::new()
            }
            "session.next.revert.cleared" => {
                if let ServerEventData::SessionNextRevertCleared(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.touch_session(&payload.session_id, payload.timestamp);
                    self.integrations.clear_revert();
                    self.notifications.set("Revert cleared".to_owned());
                }
                Vec::new()
            }
            "session.next.revert.committed" => {
                if let ServerEventData::SessionNextRevertCommitted(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.touch_session(&payload.session_id, payload.timestamp);
                    self.integrations.clear_revert();
                    self.notifications
                        .set(format!("Revert committed for {}", payload.message_id));
                }
                Vec::new()
            }
            "permission.asked" => {
                if let Some(request) = parse_permission_request(&event.properties) {
                    if self.is_current_session(&request.session_id) {
                        self.runtime.set_working(true);
                    }
                    self.upsert_permission(request);
                }
                Vec::new()
            }
            "permission.replied" => {
                if let Some(request_id) = require_string(&event.properties, "requestID") {
                    self.remove_permission(request_id);
                }
                Vec::new()
            }
            "question.asked" => {
                if let Some(request) = parse_question_request(&event.properties) {
                    if self.is_current_session(&request.session_id) {
                        self.runtime.set_working(true);
                    }
                    self.upsert_question(request);
                }
                Vec::new()
            }
            "question.replied" | "question.rejected" => {
                if let Some(request_id) = require_string(&event.properties, "requestID") {
                    self.remove_question(request_id);
                }
                Vec::new()
            }
            "message.updated" => {
                if let ServerEventData::MessageUpdated(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    if payload.info.role == "assistant" {
                        self.runtime
                            .update_response_tokens(&payload.info.id, payload.info.tokens.clone());
                    }
                    self.upsert_message_info(payload.info.clone());
                }
                Vec::new()
            }
            "message.removed" => {
                if let ServerEventData::MessageRemoved(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.remove_message(&payload.message_id);
                }
                Vec::new()
            }
            "message.part.updated" => {
                if let ServerEventData::MessagePartUpdated(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.upsert_part(payload.part.clone());
                }
                Vec::new()
            }
            "message.part.delta" => {
                if let ServerEventData::MessagePartDelta(payload) = event.data.as_ref() {
                    self.apply_part_delta(payload);
                }
                Vec::new()
            }
            "message.part.removed" => {
                if let ServerEventData::MessagePartRemoved(payload) = event.data.as_ref() {
                    self.remove_part(payload);
                }
                Vec::new()
            }
            "session.next.step.started" => {
                if let ServerEventData::SessionNextStepStarted(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.runtime.set_working(true);
                    self.runtime
                        .set_response_message(&payload.assistant_message_id);
                    self.transcript.start_assistant(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.agent,
                        &payload.model,
                        payload.snapshot.as_deref(),
                        payload.timestamp,
                    );
                }
                Vec::new()
            }
            "session.next.text.started" => {
                if let ServerEventData::SessionNextTextStarted(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.runtime.set_working(true);
                    self.runtime
                        .set_response_message(&payload.assistant_message_id);
                    self.transcript.start_text(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.text_id,
                        payload.timestamp,
                    );
                }
                Vec::new()
            }
            "session.next.text.delta" => {
                if let ServerEventData::SessionNextTextDelta(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.append_text_delta(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.text_id,
                        payload.timestamp,
                        &payload.delta,
                    );
                    self.runtime.add_response_output(&payload.delta);
                }
                Vec::new()
            }
            "session.next.text.ended" => {
                if let ServerEventData::SessionNextTextEnded(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.finish_text(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.text_id,
                        payload.timestamp,
                        &payload.text,
                    );
                }
                Vec::new()
            }
            "session.next.step.ended" => {
                if let ServerEventData::SessionNextStepEnded(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.finish_assistant(
                        &payload.assistant_message_id,
                        &payload.finish,
                        payload.cost,
                        payload.tokens.clone(),
                        (payload.snapshot.is_some() || !payload.files.is_empty()).then(|| {
                            json!({
                                "end": payload.snapshot.clone(),
                                "files": payload.files.clone(),
                            })
                        }),
                        payload.timestamp,
                    );
                    self.runtime.finish_response_tokens(
                        &payload.assistant_message_id,
                        payload.tokens.clone(),
                    );
                    self.runtime.set_working(false);
                    return self.refresh_current_effect();
                }
                Vec::new()
            }
            "session.next.step.failed" => {
                if let ServerEventData::SessionNextStepFailed(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.fail_assistant(
                        &payload.assistant_message_id,
                        &payload.error.kind,
                        &payload.error.message,
                        payload.timestamp,
                    );
                    self.runtime.set_working(false);
                    self.notifications.set(payload.error.message.clone());
                    return self.refresh_current_effect();
                }
                Vec::new()
            }
            "session.next.compaction.started" => {
                if let ServerEventData::SessionNextCompactionStarted(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.runtime.set_working(true);
                    self.notifications
                        .set(format!("Compacting context ({})", payload.reason));
                    self.transcript.start_compaction(
                        &payload.session_id,
                        &payload.message_id,
                        &payload.reason,
                        payload.timestamp,
                    );
                }
                Vec::new()
            }
            "session.next.compaction.delta" => {
                if let ServerEventData::SessionNextCompactionDelta(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.append_compaction_delta(
                        &payload.message_id,
                        payload.timestamp,
                        &payload.text,
                    );
                }
                Vec::new()
            }
            "session.next.compaction.ended" => {
                if let ServerEventData::SessionNextCompactionEnded(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.finish_compaction(
                        &payload.session_id,
                        &payload.message_id,
                        &payload.reason,
                        &payload.text,
                        &payload.recent,
                        payload.timestamp,
                    );
                    self.notifications
                        .set(format!("Context compaction completed ({})", payload.reason));
                    self.runtime.set_working(false);
                }
                Vec::new()
            }
            "session.next.reasoning.started" => {
                if let ServerEventData::SessionNextReasoningStarted(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.runtime.set_working(true);
                    self.runtime
                        .set_response_message(&payload.assistant_message_id);
                    self.transcript.start_reasoning(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.reasoning_id,
                        payload.timestamp,
                    );
                }
                Vec::new()
            }
            "session.next.reasoning.delta" => {
                if let ServerEventData::SessionNextReasoningDelta(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.append_reasoning_delta(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.reasoning_id,
                        payload.timestamp,
                        &payload.delta,
                    );
                    self.runtime.add_response_output(&payload.delta);
                }
                Vec::new()
            }
            "session.next.reasoning.ended" => {
                if let ServerEventData::SessionNextReasoningEnded(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.finish_reasoning(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.reasoning_id,
                        payload.timestamp,
                        &payload.text,
                    );
                }
                Vec::new()
            }
            "session.next.shell.started" => {
                if let ServerEventData::SessionNextShellStarted(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.runtime.set_working(true);
                    self.notifications
                        .set(format!("Running {}", payload.command));
                    self.transcript.start_shell(
                        &payload.session_id,
                        &payload.message_id,
                        &payload.call_id,
                        &payload.command,
                        payload.timestamp,
                    );
                }
                Vec::new()
            }
            "session.next.shell.ended" => {
                if let ServerEventData::SessionNextShellEnded(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.finish_shell(
                        &payload.call_id,
                        &payload.output,
                        payload.timestamp,
                    );
                }
                Vec::new()
            }
            "session.next.tool.input.started" => {
                if let ServerEventData::SessionNextToolInputStarted(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.runtime.set_working(true);
                    self.transcript.start_tool(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.call_id,
                        &payload.name,
                        payload.timestamp,
                    );
                }
                Vec::new()
            }
            "session.next.tool.input.delta" => {
                if let ServerEventData::SessionNextToolInputDelta(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.append_tool_input(
                        &payload.assistant_message_id,
                        &payload.call_id,
                        payload.timestamp,
                        &payload.delta,
                    );
                }
                Vec::new()
            }
            "session.next.tool.input.ended" => {
                if let ServerEventData::SessionNextToolInputEnded(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.set_tool_input(
                        &payload.assistant_message_id,
                        &payload.call_id,
                        payload.timestamp,
                        &payload.text,
                    );
                }
                Vec::new()
            }
            "session.next.tool.called" => {
                if let ServerEventData::SessionNextToolCalled(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.start_tool(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.call_id,
                        &payload.tool,
                        payload.timestamp,
                    );
                    self.runtime.set_working(true);
                    self.notifications.set(format!("Running {}", payload.tool));
                    self.transcript.set_tool_state(
                        &payload.assistant_message_id,
                        &payload.call_id,
                        json!({
                            "status": "running",
                            "input": payload.input.clone(),
                            "structured": {},
                            "content": [],
                            "provider": payload.provider.clone(),
                        }),
                    );
                }
                Vec::new()
            }
            "session.next.tool.progress" => {
                if let ServerEventData::SessionNextToolProgress(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.start_tool(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.call_id,
                        "tool",
                        payload.timestamp,
                    );
                    let input = self
                        .transcript
                        .tool_state(&payload.assistant_message_id, &payload.call_id)
                        .and_then(|state| state.get("input"))
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    self.transcript.set_tool_state(
                        &payload.assistant_message_id,
                        &payload.call_id,
                        json!({
                            "status": "running",
                            "input": input,
                            "structured": payload.structured.clone(),
                            "content": payload.content.clone(),
                        }),
                    );
                }
                Vec::new()
            }
            "session.next.tool.success" => {
                if let ServerEventData::SessionNextToolSuccess(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.start_tool(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.call_id,
                        "tool",
                        payload.timestamp,
                    );
                    let input = self
                        .transcript
                        .tool_state(&payload.assistant_message_id, &payload.call_id)
                        .and_then(|state| state.get("input"))
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    self.transcript.set_tool_state(
                        &payload.assistant_message_id,
                        &payload.call_id,
                        json!({
                            "status": "completed",
                            "input": input,
                            "structured": payload.structured.clone(),
                            "content": payload.content.clone(),
                            "outputPaths": payload.output_paths.clone(),
                            "result": payload.result.clone(),
                            "provider": payload.provider.clone(),
                        }),
                    );
                }
                Vec::new()
            }
            "session.next.tool.failed" => {
                if let ServerEventData::SessionNextToolFailed(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.transcript.start_tool(
                        &payload.session_id,
                        &payload.assistant_message_id,
                        &payload.call_id,
                        "tool",
                        payload.timestamp,
                    );
                    let previous = self
                        .transcript
                        .tool_state(&payload.assistant_message_id, &payload.call_id)
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let input = previous.get("input").cloned().unwrap_or_else(|| json!({}));
                    let structured = previous
                        .get("structured")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let content = previous
                        .get("content")
                        .cloned()
                        .unwrap_or_else(|| json!([]));
                    self.transcript.set_tool_state(
                        &payload.assistant_message_id,
                        &payload.call_id,
                        json!({
                            "status": "error",
                            "input": if input.is_string() { json!({}) } else { input },
                            "structured": structured,
                            "content": content,
                            "error": payload.error.clone(),
                            "result": payload.result.clone(),
                            "provider": payload.provider.clone(),
                        }),
                    );
                    self.notifications.set(payload.error.message.clone());
                }
                Vec::new()
            }
            "lsp.updated" => vec![Effect::Api(ApiRequest::ListLsp)],
            "mcp.tools.changed" => vec![Effect::Api(ApiRequest::ListMcp)],
            "todo.updated" => {
                if let ServerEventData::TodoUpdated(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.integrations.replace_todos(payload.todos.clone());
                }
                Vec::new()
            }
            "session.diff" => {
                if let ServerEventData::SessionDiffUpdated(payload) = event.data.as_ref()
                    && self.is_current_session(&payload.session_id)
                {
                    self.integrations.replace_diffs(payload.diffs.clone());
                }
                Vec::new()
            }
            "vcs.branch.updated" => {
                if let ServerEventData::VcsBranchUpdated(payload) = event.data.as_ref() {
                    self.integrations.update_vcs_branch(payload.branch.clone());
                }
                vec![Effect::Api(ApiRequest::ListVcsStatus)]
            }
            _ => Vec::new(),
        }
    }

    fn handle_api_result(&mut self, result: ApiResult) -> Vec<Effect> {
        match result {
            ApiResult::Health(result) => {
                match result {
                    Ok(status) => {
                        self.runtime.mark_connected();
                        self.runtime.set_health(status.clone());
                        self.notifications.set(status);
                    }
                    Err(error) => {
                        self.runtime.mark_disconnected();
                        self.runtime.set_health("server unavailable");
                        self.notifications
                            .set(format!("Server unavailable: {error}"));
                    }
                }
                Vec::new()
            }
            ApiResult::Sessions(result) => {
                if self.session.show_archived {
                    return Vec::new();
                }
                match result {
                    Ok(sessions) => self.apply_sessions(sessions),
                    Err(error) => {
                        self.runtime.mark_disconnected();
                        self.notifications
                            .set(format!("Session list unavailable: {error}"));
                    }
                }
                Vec::new()
            }
            ApiResult::ArchivedSessions(result) => {
                if !self.session.show_archived {
                    return Vec::new();
                }
                match result {
                    Ok(sessions) => self.apply_sessions(sessions),
                    Err(error) => self
                        .notifications
                        .set(format!("Archived session list unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Permissions(result) => {
                match result {
                    Ok(permissions) => self.set_permissions(permissions),
                    Err(error) => self
                        .notifications
                        .set(format!("Permission list unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Questions(result) => {
                match result {
                    Ok(questions) => self.set_questions(questions),
                    Err(error) => self
                        .notifications
                        .set(format!("Question list unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Providers(result) => {
                match result {
                    Ok(catalog) => {
                        self.catalog.replace_providers(catalog);
                    }
                    Err(error) => self
                        .notifications
                        .set(format!("Provider catalog unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Skills(result) => {
                match result {
                    Ok(skills) => self.catalog.replace_skills(skills),
                    Err(error) => self
                        .notifications
                        .set(format!("Skill list unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Commands(result) => {
                match result {
                    Ok(commands) => self.catalog.replace_commands(commands),
                    Err(error) => self
                        .notifications
                        .set(format!("Command list unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Agents(result) => {
                match result {
                    Ok(agents) => self.catalog.replace_agents(agents),
                    Err(error) => self
                        .notifications
                        .set(format!("Agent catalog unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::References(result) => {
                match result {
                    Ok(references) => self.catalog.replace_references(references),
                    Err(error) => self
                        .notifications
                        .set(format!("Reference list unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Mcp(result) => {
                match result {
                    Ok(statuses) => self.integrations.replace_mcp(statuses),
                    Err(error) => self
                        .notifications
                        .set(format!("MCP status unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Lsp(result) => {
                match result {
                    Ok(lsp) => self.integrations.replace_lsp(lsp),
                    Err(error) => self
                        .notifications
                        .set(format!("LSP status unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::SessionStatuses(result) => {
                match result {
                    Ok(statuses) => {
                        self.runtime.replace_session_statuses(statuses);
                        let working = self
                            .session
                            .current_session
                            .as_ref()
                            .and_then(|session| self.runtime.session_statuses.get(&session.id))
                            .is_some_and(SessionStatus::is_working);
                        self.runtime.set_working(working);
                    }
                    Err(error) => self
                        .notifications
                        .set(format!("Session statuses unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Todos { session_id, result } => {
                if !self.is_current_session(&session_id) {
                    return Vec::new();
                }
                match result {
                    Ok(todos) => self.integrations.replace_todos(todos),
                    Err(error) => self
                        .notifications
                        .set(format!("Session todo unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::SessionDiff { session_id, result } => {
                if !self.is_current_session(&session_id) {
                    return Vec::new();
                }
                match result {
                    Ok(diffs) => {
                        self.integrations.replace_diffs(diffs);
                        if let Some(OverlayState::SessionDiff { selected, scroll }) =
                            self.overlay.as_mut()
                        {
                            *selected =
                                (*selected).min(self.integrations.diffs.len().saturating_sub(1));
                            *scroll = 0;
                        }
                    }
                    Err(error) => self
                        .notifications
                        .set(format!("Session diff unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::SessionChildren { session_id, result } => {
                if !self.is_current_session(&session_id) {
                    return Vec::new();
                }
                match result {
                    Ok(children) => self.session.replace_children(&session_id, children),
                    Err(error) => self
                        .notifications
                        .set(format!("Session children unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Vcs(result) => {
                match result {
                    Ok(vcs) => self.integrations.replace_vcs(vcs),
                    Err(error) => self
                        .notifications
                        .set(format!("VCS information unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::VcsStatus(result) => {
                match result {
                    Ok(statuses) => self.integrations.replace_vcs_status(statuses),
                    Err(error) => self
                        .notifications
                        .set(format!("VCS status unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::VcsDiff { mode, result } => {
                let is_current_overlay = matches!(
                    self.overlay.as_ref(),
                    Some(OverlayState::VcsDiff {
                        mode: current_mode,
                        ..
                    }) if *current_mode == mode
                );
                if !is_current_overlay {
                    return Vec::new();
                }
                match result {
                    Ok(diffs) => {
                        self.integrations.replace_vcs_diffs(mode, diffs);
                        if let Some(OverlayState::VcsDiff {
                            selected, scroll, ..
                        }) = self.overlay.as_mut()
                        {
                            *selected = (*selected)
                                .min(self.integrations.vcs_diffs.len().saturating_sub(1));
                            *scroll = 0;
                        }
                    }
                    Err(error) => self
                        .notifications
                        .set(format!("VCS diff unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::McpConnected { name, result } => {
                self.integrations.finish_mcp_action(&name);
                match result {
                    Ok(()) => self.notifications.set(format!("Connected MCP: {name}")),
                    Err(error) => self
                        .notifications
                        .set(format!("MCP connect failed for {name}: {error}")),
                }
                vec![Effect::Api(ApiRequest::ListMcp)]
            }
            ApiResult::McpDisconnected { name, result } => {
                self.integrations.finish_mcp_action(&name);
                match result {
                    Ok(()) => self.notifications.set(format!("Disconnected MCP: {name}")),
                    Err(error) => self
                        .notifications
                        .set(format!("MCP disconnect failed for {name}: {error}")),
                }
                vec![Effect::Api(ApiRequest::ListMcp)]
            }
            ApiResult::Files(result) => {
                match result {
                    Ok(files) => self.catalog.replace_workspace_files(files),
                    Err(error) => self
                        .notifications
                        .set(format!("Workspace files unavailable: {error}")),
                }
                Vec::new()
            }
            ApiResult::Directory { path, result } => {
                let error_message = {
                    let Some(OverlayState::FilePicker {
                        path: current_path,
                        entries,
                        selected,
                        loading,
                    }) = self.overlay.as_mut()
                    else {
                        return Vec::new();
                    };
                    if current_path != &path {
                        return Vec::new();
                    }
                    match result {
                        Ok(files) => {
                            *entries = files;
                            *selected = (*selected).min(entries.len().saturating_sub(1));
                            *loading = false;
                            None
                        }
                        Err(error) => {
                            entries.clear();
                            *selected = 0;
                            *loading = false;
                            Some(error)
                        }
                    }
                };
                if let Some(error) = error_message {
                    self.notifications
                        .set(format!("Workspace directory unavailable: {error}"));
                }
                Vec::new()
            }
            ApiResult::SearchedFiles { query, result } => {
                if self.current_mention_query().as_deref() == Some(query.as_str()) {
                    match result {
                        Ok(files) => self.catalog.replace_server_workspace_files(query, files),
                        Err(_) => self.catalog.clear_server_file_search(&query),
                    }
                }
                Vec::new()
            }
            ApiResult::Attachment { session_id, result } => {
                if !self.is_current_session(&session_id) {
                    return Vec::new();
                }
                let panel_return = self.prompt_panel_return.take();
                match result {
                    Ok(part) => {
                        let filename = prompt_part_filename(&part).unwrap_or("file").to_owned();
                        self.prompt.attachments.push(part);
                        self.notifications.set(format!("Attached {filename}"));
                    }
                    Err(error) => self
                        .notifications
                        .set(format!("Attachment failed: {error}")),
                }
                if let Some(selected) = panel_return {
                    self.overlay = Some(OverlayState::PromptPanel { selected });
                }
                Vec::new()
            }
            ApiResult::CreatedSession { session, providers } => {
                let provider_error = match providers {
                    Ok(catalog) => {
                        self.catalog.replace_providers(catalog);
                        None
                    }
                    Err(error) => Some(error),
                };
                match session {
                    Ok(session) => {
                        let session_id = session.id.clone();
                        self.upsert_session(session);
                        self.select_latest_model_for_new_session(&session_id);
                        self.select_session(&session_id);
                        self.session.opening_session = Some(session_id.clone());
                        self.notifications.set(match provider_error {
                            Some(error) => {
                                format!("Session created; provider catalog unavailable: {error}")
                            }
                            None => "Session created".to_owned(),
                        });
                        vec![Effect::Api(ApiRequest::OpenSession(session_id))]
                    }
                    Err(error) => {
                        self.notifications
                            .set(format!("Create session failed: {error}"));
                        Vec::new()
                    }
                }
            }
            ApiResult::OpenedSession(result) => {
                match result {
                    Ok(snapshot) => return self.apply_opened_session(snapshot),
                    Err(error) => self
                        .notifications
                        .set(format!("Open session failed: {error}")),
                }
                Vec::new()
            }
            ApiResult::RefreshedSession(result) => {
                match result {
                    Ok(snapshot) => {
                        if self.is_current_session(&snapshot.session.id) {
                            let session_id = snapshot.session.id.clone();
                            self.apply_snapshot(snapshot);
                            self.restore_model_for_session(&session_id);
                            return vec![
                                Effect::Api(ApiRequest::ListSessionTodos(session_id.clone())),
                                Effect::Api(ApiRequest::ListSessionDiff(session_id.clone())),
                                Effect::Api(ApiRequest::ListSessionChildren(session_id)),
                            ];
                        }
                    }
                    Err(error) => self.notifications.set(format!("Refresh failed: {error}")),
                }
                Vec::new()
            }
            ApiResult::RenamedSession(result) => match result {
                Ok(session) => {
                    let session_id = session.id.clone();
                    self.upsert_session(session.clone());
                    if self.is_current_session(&session_id) {
                        self.session.current_session = Some(session);
                    }
                    self.notifications.set("Session renamed".to_owned());
                    Vec::new()
                }
                Err(error) => {
                    self.notifications.set(format!("Rename failed: {error}"));
                    Vec::new()
                }
            },
            ApiResult::ArchivedSession { archived, result } => match result {
                Ok(session) => {
                    let session_id = session.id.clone();
                    let was_current = self.is_current_session(&session_id);
                    self.session.sessions.retain(|item| item.id != session_id);
                    self.pending.remove_session(&session_id);
                    self.runtime.clear_session_status(&session_id);
                    if was_current {
                        self.clear_current_session();
                    }
                    self.notifications.set(if archived {
                        "Session archived"
                    } else {
                        "Session restored"
                    });
                    vec![self.session_list_effect()]
                }
                Err(error) => {
                    self.notifications.set(if archived {
                        format!("Archive failed: {error}")
                    } else {
                        format!("Restore failed: {error}")
                    });
                    Vec::new()
                }
            },
            ApiResult::MovedSession {
                session_id,
                destination,
                result,
            } => match result {
                Ok(()) => {
                    let workspace_id = self
                        .session
                        .sessions
                        .iter()
                        .find(|session| session.id == session_id)
                        .and_then(|session| session.workspace_id.clone())
                        .or_else(|| {
                            self.session
                                .current_session
                                .as_ref()
                                .filter(|session| session.id == session_id)
                                .and_then(|session| session.workspace_id.clone())
                        });
                    self.session
                        .update_location(&session_id, &destination, workspace_id, 0);
                    self.notifications
                        .set(format!("Session moved to {destination}"));
                    vec![self.session_list_effect()]
                }
                Err(error) => {
                    self.notifications.set(format!("Move failed: {error}"));
                    Vec::new()
                }
            },
            ApiResult::DeletedSession { session_id, result } => match result {
                Ok(()) => {
                    let was_current = self.is_current_session(&session_id);
                    self.session
                        .sessions
                        .retain(|session| session.id != session_id);
                    self.pending.remove_session(&session_id);
                    self.runtime.clear_session_status(&session_id);
                    if was_current {
                        self.clear_current_session();
                    }
                    self.session.selected_session = self
                        .session
                        .selected_session
                        .min(self.session.sessions.len().saturating_sub(1));
                    self.notifications.set("Session deleted".to_owned());
                    Vec::new()
                }
                Err(error) => {
                    self.notifications.set(format!("Delete failed: {error}"));
                    Vec::new()
                }
            },
            ApiResult::ForkedSession(result) => match result {
                Ok(session) => {
                    let session_id = session.id.clone();
                    self.upsert_session(session);
                    self.select_session(&session_id);
                    self.session.opening_session = Some(session_id.clone());
                    self.notifications
                        .set("Opening forked session...".to_owned());
                    vec![Effect::Api(ApiRequest::OpenSession(session_id))]
                }
                Err(error) => {
                    self.notifications.set(format!("Fork failed: {error}"));
                    Vec::new()
                }
            },
            ApiResult::SharedSession(result) => match result {
                Ok(session) => {
                    let session_id = session.id.clone();
                    let url = session.share_url().map(str::to_owned);
                    self.upsert_session(session.clone());
                    if self.is_current_session(&session_id) {
                        self.session.current_session = Some(session);
                    }
                    if let Some(url) = url {
                        self.overlay = Some(OverlayState::SessionShare { url });
                        self.notifications.set("Session shared".to_owned());
                    } else {
                        self.notifications
                            .set("Session shared without a link".to_owned());
                    }
                    Vec::new()
                }
                Err(error) => {
                    self.notifications.set(format!("Share failed: {error}"));
                    Vec::new()
                }
            },
            ApiResult::UnsharedSession(result) => match result {
                Ok(session) => {
                    let session_id = session.id.clone();
                    self.upsert_session(session.clone());
                    if self.is_current_session(&session_id) {
                        self.session.current_session = Some(session);
                    }
                    self.overlay = None;
                    self.notifications.set("Session unshared".to_owned());
                    Vec::new()
                }
                Err(error) => {
                    self.notifications.set(format!("Unshare failed: {error}"));
                    Vec::new()
                }
            },
            ApiResult::CompactedSession { session_id, result } => {
                if self.is_current_session(&session_id) {
                    match result {
                        Ok(true) => self
                            .notifications
                            .set("Session compaction requested".to_owned()),
                        Ok(false) => {
                            self.runtime.set_working(false);
                            self.notifications
                                .set("Session compaction was not started".to_owned());
                        }
                        Err(error) => {
                            self.runtime.set_working(false);
                            self.notifications
                                .set(format!("Compaction failed: {error}"));
                        }
                    }
                }
                Vec::new()
            }
            ApiResult::Exported { result } => match result {
                Ok(path) => {
                    self.notifications
                        .set(format!("Session exported to {}", path.display()));
                    Vec::new()
                }
                Err(error) => {
                    self.notifications.set(format!("Export failed: {error}"));
                    Vec::new()
                }
            },
            ApiResult::Submitted { session, result } => {
                match result {
                    Ok(()) => {
                        if let Some(session) = session {
                            let session_id = session.id.clone();
                            self.upsert_session(session.clone());
                            self.select_session(&session_id);
                            self.session.current_session = Some(session);
                            self.session.screen = Screen::Session;
                        }
                        self.prompt.clear_pending();
                        self.runtime.set_working(true);
                        self.notifications.set("Prompt sent".to_owned());
                    }
                    Err(error) => {
                        self.prompt.restore_pending();
                        self.runtime.set_working(false);
                        self.notifications.set(format!("Prompt failed: {error}"));
                    }
                }
                Vec::new()
            }
            ApiResult::Aborted(result) => {
                match result {
                    Ok(()) => {
                        self.runtime.set_working(false);
                        self.notifications.set("Abort requested".to_owned());
                    }
                    Err(error) => self.notifications.set(format!("Abort failed: {error}")),
                }
                Vec::new()
            }
            ApiResult::PermissionReplied { request_id, result } => {
                match result {
                    Ok(()) => {
                        self.remove_permission(&request_id);
                        self.notifications
                            .set("Permission response sent".to_owned());
                    }
                    Err(error) => {
                        self.pending.clear_responding();
                        self.notifications
                            .set(format!("Permission response failed: {error}"));
                    }
                }
                Vec::new()
            }
            ApiResult::QuestionReplied { request_id, result } => {
                match result {
                    Ok(()) => {
                        self.remove_question(&request_id);
                        self.notifications.set("Question response sent".to_owned());
                    }
                    Err(error) => {
                        self.pending.clear_responding();
                        self.notifications
                            .set(format!("Question response failed: {error}"));
                    }
                }
                Vec::new()
            }
            ApiResult::QuestionRejected { request_id, result } => {
                match result {
                    Ok(()) => {
                        self.remove_question(&request_id);
                        self.notifications.set("Question rejected".to_owned());
                    }
                    Err(error) => {
                        self.pending.clear_responding();
                        self.notifications
                            .set(format!("Question rejection failed: {error}"));
                    }
                }
                Vec::new()
            }
        }
    }

    pub fn connection_label(&self) -> &str {
        // Keep the short label stable for headers; diagnostics can expose retry detail separately.
        match &self.runtime.connection {
            crate::runtime_state::ConnectionState::Disconnected => "disconnected",
            crate::runtime_state::ConnectionState::Connecting => "connecting",
            crate::runtime_state::ConnectionState::Connected => "connected",
            crate::runtime_state::ConnectionState::Reconnecting { .. } => "reconnecting",
        }
    }

    pub fn connection_detail(&self) -> String {
        self.runtime.connection_label()
    }

    pub fn status_label(&self) -> &str {
        if self.runtime.working {
            "working"
        } else {
            "idle"
        }
    }

    pub fn current_permission(&self) -> Option<&PermissionRequest> {
        self.pending.current_permission(
            self.session
                .current_session
                .as_ref()
                .map(|session| session.id.as_str()),
        )
    }

    pub fn current_question(&self) -> Option<&QuestionRequest> {
        self.pending.current_question(
            self.session
                .current_session
                .as_ref()
                .map(|session| session.id.as_str()),
        )
    }

    pub fn is_responding(&self, request_id: &str) -> bool {
        self.pending.is_responding(request_id)
    }

    pub fn current_question_info(&self) -> Option<&QuestionInfo> {
        self.pending.current_question_info(
            self.session
                .current_session
                .as_ref()
                .map(|session| session.id.as_str()),
        )
    }

    fn handle_home_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            self.open_command_palette();
            return Vec::new();
        }
        match key.code {
            KeyCode::Char('q') => vec![Effect::Quit],
            KeyCode::Char('r') => {
                self.notifications.set("Refreshing sessions...".to_owned());
                vec![self.session_list_effect()]
            }
            KeyCode::Char('n') => {
                self.notifications.set("Creating session...".to_owned());
                vec![Effect::Api(ApiRequest::CreateSession)]
            }
            KeyCode::Char('e') | KeyCode::F(2) => {
                self.open_rename_session();
                Vec::new()
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                self.open_delete_session();
                Vec::new()
            }
            KeyCode::Char('a') => self.show_archived_sessions(),
            KeyCode::Char('A') => self.show_active_sessions(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.session.selected_session = self.session.selected_session.saturating_sub(1);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') if !self.session.sessions.is_empty() => {
                self.session.selected_session =
                    (self.session.selected_session + 1).min(self.session.sessions.len() - 1);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') => Vec::new(),
            KeyCode::Enter => {
                if let Some(session) = self.session.sessions.get(self.session.selected_session) {
                    self.session.opening_session = Some(session.id.clone());
                    self.notifications.set("Opening session...".to_owned());
                    vec![Effect::Api(ApiRequest::OpenSession(session.id.clone()))]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    fn handle_session_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            self.open_command_palette();
            return Vec::new();
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('x') {
            if let Some(session) = self.session.current_session.as_ref() {
                self.notifications.set("Requesting abort...".to_owned());
                return vec![Effect::Api(ApiRequest::Abort(session.id.clone()))];
            }
            return Vec::new();
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('e') {
            return self.export_current_session();
        }
        if self.current_permission().is_some() {
            return self.handle_permission_key(key);
        }
        if self.current_question().is_some() {
            return self.handle_question_key(key);
        }
        if is_attach_key(key) {
            self.open_attach_file();
            return Vec::new();
        }
        if is_remove_attachment_key(key) {
            self.remove_last_attachment();
            return Vec::new();
        }
        if is_subtask_key(key) {
            self.open_subtask();
            return Vec::new();
        }
        if is_prompt_options_key(key) {
            self.open_prompt_options();
            return Vec::new();
        }
        if is_collapse_blocks_key(key) {
            self.toggle_collapsible_blocks();
            return Vec::new();
        }
        match key.code {
            KeyCode::Esc => {
                self.session.screen = Screen::Home;
                self.session.opening_session = None;
                self.notifications.clear();
                Vec::new()
            }
            KeyCode::Home if self.prompt.composer.is_empty() => {
                self.transcript.scroll.jump_to_top();
                Vec::new()
            }
            KeyCode::End if self.prompt.composer.is_empty() => {
                self.transcript.scroll.jump_to_latest();
                Vec::new()
            }
            KeyCode::PageUp => {
                self.transcript.scroll.scroll_page(-1);
                Vec::new()
            }
            KeyCode::PageDown => {
                self.transcript.scroll.scroll_page(1);
                Vec::new()
            }
            _ => self.handle_composer_key(key),
        }
    }

    fn handle_composer_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let action = self.prompt.composer.handle_key(key);
        let mut effects = match action {
            ComposerAction::Submit(prompt) => self.submit(prompt),
            ComposerAction::Changed | ComposerAction::None => Vec::new(),
        };
        effects.extend(self.sync_prompt_overlays());
        effects
    }

    fn handle_paste(&mut self, text: &str) {
        match self.overlay.as_mut() {
            Some(OverlayState::Model { query, selected })
            | Some(OverlayState::Skill { query, selected })
            | Some(OverlayState::Agent { query, selected })
            | Some(OverlayState::Variant { query, selected })
            | Some(OverlayState::CommandPalette { query, selected }) => {
                let pasted = text.replace(['\r', '\n'], " ");
                query.push_str(&pasted);
                *selected = 0;
            }
            Some(OverlayState::Slash { .. }) | None | Some(OverlayState::Mention { .. }) => {
                self.prompt.composer.paste(text);
            }
            Some(OverlayState::RenameSession { value }) => {
                value.push_str(&text.replace(['\r', '\n'], " "));
            }
            Some(OverlayState::AttachFile { value }) => {
                value.push_str(&text.replace(['\r', '\n'], " "));
            }
            Some(OverlayState::MoveSession { destination, .. }) => {
                destination.push_str(&text.replace(['\r', '\n'], " "));
            }
            Some(OverlayState::Subtask { prompt, .. }) => {
                prompt.push_str(text);
            }
            Some(OverlayState::PromptSystem { value }) => {
                value.push_str(text);
            }
            Some(OverlayState::PromptToolName { value }) => {
                value.push_str(&text.replace(['\r', '\n'], " "));
            }
            Some(OverlayState::PromptOptions { .. })
            | Some(OverlayState::PromptPanel { .. })
            | Some(OverlayState::PromptTools { .. })
            | Some(OverlayState::Mcp { .. })
            | Some(OverlayState::DeleteSession { .. })
            | Some(OverlayState::ArchiveSession { .. })
            | Some(OverlayState::SessionShare { .. })
            | Some(OverlayState::SessionDiff { .. })
            | Some(OverlayState::VcsDiff { .. })
            | Some(OverlayState::FilePicker { .. })
            | Some(OverlayState::Timeline { .. })
            | Some(OverlayState::ForkSession { .. })
            | Some(OverlayState::Theme { .. })
            | Some(OverlayState::Diagnostics)
            | Some(OverlayState::Help) => {}
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Some(overlay) = self.overlay.clone() else {
            return self.handle_composer_key(key);
        };
        match overlay {
            OverlayState::Slash { .. } => self.handle_slash_overlay_key(key),
            OverlayState::Model { .. } => {
                self.handle_catalog_overlay_key(key, CatalogDialog::Model)
            }
            OverlayState::Skill { .. } => {
                self.handle_catalog_overlay_key(key, CatalogDialog::Skill)
            }
            OverlayState::Agent { .. } => {
                self.handle_catalog_overlay_key(key, CatalogDialog::Agent)
            }
            OverlayState::Variant { .. } => {
                self.handle_catalog_overlay_key(key, CatalogDialog::Variant)
            }
            OverlayState::Mcp { .. } => self.handle_mcp_overlay_key(key),
            OverlayState::CommandPalette { .. } => self.handle_command_palette_key(key),
            OverlayState::Mention { .. } => self.handle_mention_overlay_key(key),
            OverlayState::RenameSession { .. } => self.handle_rename_overlay_key(key),
            OverlayState::DeleteSession { .. } => self.handle_delete_overlay_key(key),
            OverlayState::ArchiveSession { .. } => self.handle_archive_overlay_key(key),
            OverlayState::MoveSession { .. } => self.handle_move_session_overlay_key(key),
            OverlayState::SessionDiff { .. } => self.handle_session_diff_overlay_key(key),
            OverlayState::VcsDiff { .. } => self.handle_vcs_diff_overlay_key(key),
            OverlayState::SessionShare { .. } => self.handle_session_share_overlay_key(key),
            OverlayState::AttachFile { .. } => self.handle_attach_overlay_key(key),
            OverlayState::FilePicker { .. } => self.handle_file_picker_key(key),
            OverlayState::Timeline { .. } => self.handle_timeline_overlay_key(key, false),
            OverlayState::ForkSession { .. } => self.handle_timeline_overlay_key(key, true),
            OverlayState::Subtask { .. } => self.handle_subtask_overlay_key(key),
            OverlayState::PromptOptions { .. } => self.handle_prompt_options_key(key),
            OverlayState::PromptPanel { .. } => self.handle_prompt_panel_key(key),
            OverlayState::PromptTools { .. } => self.handle_prompt_tools_key(key),
            OverlayState::PromptToolName { .. } => self.handle_prompt_tool_name_key(key),
            OverlayState::PromptSystem { .. } => self.handle_prompt_system_key(key),
            OverlayState::Theme { .. } => self.handle_theme_overlay_key(key),
            OverlayState::Diagnostics => self.handle_diagnostics_overlay_key(key),
            OverlayState::Help => self.handle_help_overlay_key(key),
        }
    }

    fn handle_slash_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.prompt.composer.clear();
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Up => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Enter | KeyCode::Tab if key.modifiers.is_empty() => {
                let text = self.prompt.composer.text();
                let query = slash_query(&text).unwrap_or_default();
                let command = matching_commands_with_server(query, &self.catalog.commands)
                    .get(self.overlay.as_ref().map_or(0, OverlayState::selected))
                    .cloned();
                if let Some(command) = command {
                    self.prompt.composer.clear();
                    self.overlay = None;
                    match command {
                        CommandOption::BuiltIn(entry) => match entry.command {
                            SlashCommand::Build => self.switch_mode("build"),
                            SlashCommand::Plan => self.switch_mode("plan"),
                            SlashCommand::Model => {
                                self.overlay = Some(OverlayState::Model {
                                    query: String::new(),
                                    selected: 0,
                                })
                            }
                            SlashCommand::Skill => {
                                self.overlay = Some(OverlayState::Skill {
                                    query: String::new(),
                                    selected: 0,
                                })
                            }
                            SlashCommand::Agent => {
                                self.overlay = Some(OverlayState::Agent {
                                    query: String::new(),
                                    selected: 0,
                                })
                            }
                            SlashCommand::Variant => {
                                self.overlay = Some(OverlayState::Variant {
                                    query: String::new(),
                                    selected: 0,
                                })
                            }
                            SlashCommand::Compact => return self.compact_current_session(),
                            SlashCommand::Timeline => self.open_timeline(),
                            SlashCommand::Fork => self.open_fork_session(),
                            SlashCommand::Share => return self.share_current_session(),
                            SlashCommand::Unshare => return self.unshare_current_session(),
                        },
                        CommandOption::Server(command) => {
                            self.prompt
                                .composer
                                .set_text(&format!("/{} ", command.name));
                            self.notifications
                                .set(format!("Inserted /{} command", command.name));
                        }
                    }
                    Vec::new()
                } else {
                    self.handle_composer_key(key)
                }
            }
            _ => self.handle_composer_key(key),
        }
    }

    fn handle_mention_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Up => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Enter | KeyCode::Tab if key.modifiers.is_empty() => {
                self.select_mention_from_overlay();
                Vec::new()
            }
            _ => self.handle_composer_key(key),
        }
    }

    fn handle_rename_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => self.submit_rename(),
            KeyCode::Backspace if key.modifiers.is_empty() => {
                if let Some(OverlayState::RenameSession { value }) = self.overlay.as_mut() {
                    value.pop();
                }
                Vec::new()
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(OverlayState::RenameSession { value }) = self.overlay.as_mut() {
                    value.push(character);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn handle_delete_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Enter | KeyCode::Char('y') => {
                let Some(OverlayState::DeleteSession { session_id }) = self.overlay.take() else {
                    return Vec::new();
                };
                self.notifications.set("Deleting session...".to_owned());
                vec![Effect::Api(ApiRequest::DeleteSession(session_id))]
            }
            _ => Vec::new(),
        }
    }

    fn handle_archive_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Enter | KeyCode::Char('y') => {
                let Some(OverlayState::ArchiveSession {
                    session_id,
                    restore,
                }) = self.overlay.take()
                else {
                    return Vec::new();
                };
                self.notifications.set(if restore {
                    "Restoring session..."
                } else {
                    "Archiving session..."
                });
                vec![Effect::Api(ApiRequest::ArchiveSession {
                    session_id,
                    archived: !restore,
                })]
            }
            _ => Vec::new(),
        }
    }

    fn handle_move_session_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Tab if key.modifiers.is_empty() => {
                if let Some(OverlayState::MoveSession { move_changes, .. }) = self.overlay.as_mut()
                {
                    *move_changes = !*move_changes;
                }
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => self.submit_move_session(),
            KeyCode::Backspace if key.modifiers.is_empty() => {
                if let Some(OverlayState::MoveSession { destination, .. }) = self.overlay.as_mut() {
                    destination.pop();
                }
                Vec::new()
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(OverlayState::MoveSession { destination, .. }) = self.overlay.as_mut() {
                    destination.push(character);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn handle_session_diff_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') if key.modifiers.is_empty() => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                self.move_session_diff_selection(-1);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                self.move_session_diff_selection(1);
                Vec::new()
            }
            KeyCode::PageUp if key.modifiers.is_empty() => {
                if let Some(OverlayState::SessionDiff { scroll, .. }) = self.overlay.as_mut() {
                    *scroll = scroll.saturating_sub(10);
                }
                Vec::new()
            }
            KeyCode::PageDown if key.modifiers.is_empty() => {
                if let Some(OverlayState::SessionDiff { scroll, .. }) = self.overlay.as_mut() {
                    *scroll = scroll.saturating_add(10);
                }
                Vec::new()
            }
            KeyCode::Home if key.modifiers.is_empty() => {
                if let Some(OverlayState::SessionDiff { scroll, .. }) = self.overlay.as_mut() {
                    *scroll = 0;
                }
                Vec::new()
            }
            KeyCode::End if key.modifiers.is_empty() => {
                if let Some(OverlayState::SessionDiff { scroll, .. }) = self.overlay.as_mut() {
                    *scroll = usize::MAX;
                }
                Vec::new()
            }
            KeyCode::Char('r') if key.modifiers.is_empty() => self.refresh_session_diff(),
            _ => Vec::new(),
        }
    }

    fn handle_vcs_diff_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') if key.modifiers.is_empty() => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                self.move_vcs_diff_selection(-1);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                self.move_vcs_diff_selection(1);
                Vec::new()
            }
            KeyCode::PageUp if key.modifiers.is_empty() => {
                if let Some(OverlayState::VcsDiff { scroll, .. }) = self.overlay.as_mut() {
                    *scroll = scroll.saturating_sub(10);
                }
                Vec::new()
            }
            KeyCode::PageDown if key.modifiers.is_empty() => {
                if let Some(OverlayState::VcsDiff { scroll, .. }) = self.overlay.as_mut() {
                    *scroll = scroll.saturating_add(10);
                }
                Vec::new()
            }
            KeyCode::Home if key.modifiers.is_empty() => {
                if let Some(OverlayState::VcsDiff { scroll, .. }) = self.overlay.as_mut() {
                    *scroll = 0;
                }
                Vec::new()
            }
            KeyCode::End if key.modifiers.is_empty() => {
                if let Some(OverlayState::VcsDiff { scroll, .. }) = self.overlay.as_mut() {
                    *scroll = usize::MAX;
                }
                Vec::new()
            }
            KeyCode::Char('s') if key.modifiers.is_empty() => self.toggle_vcs_diff_mode(),
            KeyCode::Char('r') if key.modifiers.is_empty() => self.refresh_vcs_diff(),
            _ => Vec::new(),
        }
    }

    fn handle_session_share_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Char('u') if key.modifiers.is_empty() => self.unshare_current_session(),
            _ => Vec::new(),
        }
    }

    fn handle_attach_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                if let Some(selected) = self.prompt_panel_return.take() {
                    self.overlay = Some(OverlayState::PromptPanel { selected });
                } else {
                    self.overlay = None;
                }
                Vec::new()
            }
            KeyCode::Tab if key.modifiers.is_empty() => {
                let path = match self.overlay.as_ref() {
                    Some(OverlayState::AttachFile { value }) => picker_directory_path(value),
                    _ => ".".to_owned(),
                };
                self.open_file_picker(path)
            }
            KeyCode::Enter if key.modifiers.is_empty() => self.submit_attachment(),
            KeyCode::Backspace if key.modifiers.is_empty() => {
                if let Some(OverlayState::AttachFile { value }) = self.overlay.as_mut() {
                    value.pop();
                }
                Vec::new()
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(OverlayState::AttachFile { value }) = self.overlay.as_mut() {
                    value.push(character);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn handle_file_picker_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                if let Some(selected) = self.prompt_panel_return.take() {
                    self.overlay = Some(OverlayState::PromptPanel { selected });
                } else {
                    self.overlay = None;
                }
                Vec::new()
            }
            KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => self.select_file_picker_entry(),
            KeyCode::Backspace | KeyCode::Left if key.modifiers.is_empty() => {
                let path = match self.overlay.as_ref() {
                    Some(OverlayState::FilePicker { path, .. }) => parent_picker_path(path),
                    _ => ".".to_owned(),
                };
                self.open_file_picker(path)
            }
            KeyCode::Char('r') if key.modifiers.is_empty() => {
                let path = match self.overlay.as_ref() {
                    Some(OverlayState::FilePicker { path, .. }) => path.clone(),
                    _ => ".".to_owned(),
                };
                self.open_file_picker(path)
            }
            KeyCode::Char('p') if key.modifiers.is_empty() => {
                let value = match self.overlay.as_ref() {
                    Some(OverlayState::FilePicker { path, .. }) if path != "." => {
                        format!("{path}/")
                    }
                    _ => String::new(),
                };
                self.overlay = Some(OverlayState::AttachFile { value });
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn handle_timeline_overlay_key(&mut self, key: KeyEvent, fork: bool) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                if fork {
                    self.fork_session_from_timeline()
                } else {
                    self.jump_to_timeline_entry()
                }
            }
            _ => Vec::new(),
        }
    }

    fn open_timeline(&mut self) {
        if self.timeline_entries().is_empty() {
            self.notifications
                .set("No user prompts in the current session".to_owned());
            return;
        }
        self.overlay = Some(OverlayState::Timeline { selected: 0 });
    }

    fn open_fork_session(&mut self) {
        if self.session.current_session.is_none() {
            self.notifications
                .set("Open a session before forking it".to_owned());
            return;
        }
        self.overlay = Some(OverlayState::ForkSession { selected: 0 });
    }

    fn jump_to_timeline_entry(&mut self) -> Vec<Effect> {
        let Some(OverlayState::Timeline { selected }) = self.overlay.as_ref() else {
            return Vec::new();
        };
        let Some(entry) = self.timeline_entries().get(*selected).cloned() else {
            self.notifications
                .set("No timeline entry selected".to_owned());
            return Vec::new();
        };
        self.transcript.scroll.jump_to_anchor(ScrollAnchor {
            message_id: entry.message_id,
            line_offset: 0,
        });
        self.overlay = None;
        self.notifications
            .set("Jumped to timeline entry".to_owned());
        Vec::new()
    }

    fn fork_session_from_timeline(&mut self) -> Vec<Effect> {
        let Some(session_id) = self
            .session
            .current_session
            .as_ref()
            .map(|session| session.id.clone())
        else {
            self.overlay = None;
            self.notifications
                .set("Open a session before forking it".to_owned());
            return Vec::new();
        };
        let selected = self.overlay.as_ref().map_or(0, OverlayState::selected);
        let message_id = selected.checked_sub(1).and_then(|index| {
            self.timeline_entries()
                .get(index)
                .map(|entry| entry.message_id.clone())
        });
        self.overlay = None;
        self.notifications.set(if message_id.is_some() {
            "Forking session from timeline..."
        } else {
            "Forking full session..."
        });
        vec![Effect::Api(ApiRequest::ForkSession {
            session_id,
            message_id,
        })]
    }

    fn handle_help_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.overlay = None;
        }
        Vec::new()
    }

    fn handle_theme_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Up => self.move_overlay_selection(-1),
            KeyCode::Down => self.move_overlay_selection(1),
            KeyCode::Enter if key.modifiers.is_empty() => self.select_theme_from_overlay(),
            _ => {}
        }
        Vec::new()
    }

    fn handle_diagnostics_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.overlay = None,
            KeyCode::Char('c') => self.notifications.clear_history(),
            KeyCode::Char('r') => return self.refresh_diagnostics(),
            _ => {}
        }
        Vec::new()
    }

    fn handle_mcp_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Up => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Char('r') => {
                self.notifications
                    .set("Refreshing MCP status...".to_owned());
                vec![Effect::Api(ApiRequest::ListMcp)]
            }
            KeyCode::Enter | KeyCode::Char(' ') if key.modifiers.is_empty() => {
                self.toggle_selected_mcp()
            }
            _ => Vec::new(),
        }
    }

    fn toggle_selected_mcp(&mut self) -> Vec<Effect> {
        let Some(server) = self
            .integrations
            .mcp
            .get(self.overlay.as_ref().map_or(0, OverlayState::selected))
            .cloned()
        else {
            self.notifications.set("No MCP servers loaded".to_owned());
            return Vec::new();
        };
        if !self.integrations.begin_mcp_action(&server.name) {
            self.notifications
                .set("An MCP operation is already in progress".to_owned());
            return Vec::new();
        }
        let connected = server.status == "connected";
        self.notifications.set(if connected {
            format!("Disconnecting MCP: {}", server.name)
        } else {
            format!("Connecting MCP: {}", server.name)
        });
        if connected {
            vec![Effect::Api(ApiRequest::DisconnectMcp(server.name))]
        } else {
            vec![Effect::Api(ApiRequest::ConnectMcp(server.name))]
        }
    }

    fn open_mcp_dialog(&mut self) {
        if self.integrations.mcp.is_empty() {
            self.notifications.set("No MCP servers loaded".to_owned());
            return;
        }
        self.overlay = Some(OverlayState::Mcp { selected: 0 });
    }

    fn refresh_diagnostics(&mut self) -> Vec<Effect> {
        self.overlay = Some(OverlayState::Diagnostics);
        self.notifications.set("Refreshing diagnostics...");
        self.refresh_effects()
    }

    fn select_theme_from_overlay(&mut self) {
        let Some(OverlayState::Theme { selected }) = self.overlay.as_ref() else {
            return;
        };
        let Some(choice) = Theme::choices().get(*selected).cloned() else {
            self.notifications.error("No selectable themes found");
            return;
        };
        self.theme = choice.theme;
        self.notifications
            .set(format!("Theme changed to {}", choice.name));
        self.overlay = None;
    }

    fn open_theme_selector(&mut self) {
        self.overlay = Some(OverlayState::Theme { selected: 0 });
    }

    fn show_archived_sessions(&mut self) -> Vec<Effect> {
        if self.session.show_archived {
            self.notifications
                .set("Already showing archived sessions".to_owned());
            return Vec::new();
        }
        self.session.show_archived = true;
        self.session.screen = Screen::Home;
        self.session.opening_session = None;
        self.session.selected_session = 0;
        self.notifications
            .set("Loading archived sessions...".to_owned());
        vec![Effect::Api(ApiRequest::ListArchivedSessions)]
    }

    fn show_active_sessions(&mut self) -> Vec<Effect> {
        if !self.session.show_archived {
            self.notifications
                .set("Already showing active sessions".to_owned());
            return Vec::new();
        }
        self.session.show_archived = false;
        self.session.screen = Screen::Home;
        self.session.opening_session = None;
        self.session.selected_session = 0;
        self.notifications
            .set("Loading active sessions...".to_owned());
        vec![Effect::Api(ApiRequest::ListSessions)]
    }

    fn session_for_action(&self) -> Option<&Session> {
        if self.session.screen == Screen::Session {
            self.session.current_session.as_ref()
        } else {
            self.session.sessions.get(self.session.selected_session)
        }
    }

    fn open_rename_session(&mut self) {
        let session = self.session_for_action();
        let Some(session) = session else {
            self.notifications.set("No session selected".to_owned());
            return;
        };
        self.overlay = Some(OverlayState::RenameSession {
            value: session.title.clone(),
        });
    }

    fn open_delete_session(&mut self) {
        let session = self.session_for_action();
        let Some(session) = session else {
            self.notifications.set("No session selected".to_owned());
            return;
        };
        self.overlay = Some(OverlayState::DeleteSession {
            session_id: session.id.clone(),
        });
    }

    fn open_archive_session(&mut self, restore: bool) {
        let Some(session) = self.session_for_action() else {
            self.notifications.set("No session selected".to_owned());
            return;
        };
        self.overlay = Some(OverlayState::ArchiveSession {
            session_id: session.id.clone(),
            restore,
        });
    }

    fn open_move_session(&mut self) {
        let Some(session) = self.session_for_action() else {
            self.notifications.set("No session selected".to_owned());
            return;
        };
        self.overlay = Some(OverlayState::MoveSession {
            session_id: session.id.clone(),
            destination: String::new(),
            move_changes: false,
        });
    }

    fn submit_move_session(&mut self) -> Vec<Effect> {
        let Some(OverlayState::MoveSession {
            session_id,
            destination,
            move_changes,
        }) = self.overlay.as_ref()
        else {
            return Vec::new();
        };
        let destination = destination.trim().to_owned();
        if destination.is_empty() {
            self.notifications
                .set("Destination directory cannot be empty".to_owned());
            return Vec::new();
        }
        let session_id = session_id.clone();
        let move_changes = *move_changes;
        self.overlay = None;
        self.notifications.set("Moving session...".to_owned());
        vec![Effect::Api(ApiRequest::MoveSession {
            session_id,
            destination,
            move_changes,
        })]
    }

    fn open_session_diff(&mut self) -> Vec<Effect> {
        let Some(session_id) = self
            .session
            .current_session
            .as_ref()
            .map(|session| session.id.clone())
        else {
            self.notifications
                .set("Open a session before viewing its diff".to_owned());
            return Vec::new();
        };
        self.overlay = Some(OverlayState::SessionDiff {
            selected: 0,
            scroll: 0,
        });
        self.notifications.set("Loading session diff...".to_owned());
        vec![Effect::Api(ApiRequest::ListSessionDiff(session_id))]
    }

    fn refresh_session_diff(&mut self) -> Vec<Effect> {
        let Some(session_id) = self
            .session
            .current_session
            .as_ref()
            .map(|session| session.id.clone())
        else {
            self.notifications
                .set("Open a session before refreshing its diff".to_owned());
            return Vec::new();
        };
        self.notifications
            .set("Refreshing session diff...".to_owned());
        vec![Effect::Api(ApiRequest::ListSessionDiff(session_id))]
    }

    fn move_session_diff_selection(&mut self, direction: i32) {
        let count = self.integrations.diffs.len();
        if count == 0 {
            return;
        }
        let Some(OverlayState::SessionDiff { selected, scroll }) = self.overlay.as_mut() else {
            return;
        };
        *selected = if direction.is_negative() {
            selected.saturating_sub(direction.unsigned_abs() as usize)
        } else {
            selected
                .saturating_add(direction as usize)
                .min(count.saturating_sub(1))
        };
        *scroll = 0;
    }

    fn open_vcs_diff(&mut self) -> Vec<Effect> {
        self.overlay = Some(OverlayState::VcsDiff {
            mode: VcsDiffMode::Git,
            selected: 0,
            scroll: 0,
        });
        self.load_vcs_diff(VcsDiffMode::Git)
    }

    fn refresh_vcs_diff(&mut self) -> Vec<Effect> {
        let mode = match self.overlay.as_ref() {
            Some(OverlayState::VcsDiff { mode, .. }) => *mode,
            _ => return Vec::new(),
        };
        self.load_vcs_diff(mode)
    }

    fn toggle_vcs_diff_mode(&mut self) -> Vec<Effect> {
        let mode = match self.overlay.as_mut() {
            Some(OverlayState::VcsDiff {
                mode,
                selected,
                scroll,
            }) => {
                *mode = mode.toggle();
                *selected = 0;
                *scroll = 0;
                *mode
            }
            _ => return Vec::new(),
        };
        self.load_vcs_diff(mode)
    }

    fn load_vcs_diff(&mut self, mode: VcsDiffMode) -> Vec<Effect> {
        self.notifications
            .set(format!("Loading {} VCS diff...", mode.label()));
        vec![Effect::Api(ApiRequest::ListVcsDiff { mode })]
    }

    fn move_vcs_diff_selection(&mut self, direction: i32) {
        let count = self.integrations.vcs_diffs.len();
        if count == 0 {
            return;
        }
        let Some(OverlayState::VcsDiff {
            selected, scroll, ..
        }) = self.overlay.as_mut()
        else {
            return;
        };
        *selected = if direction.is_negative() {
            selected.saturating_sub(direction.unsigned_abs() as usize)
        } else {
            selected
                .saturating_add(direction as usize)
                .min(count.saturating_sub(1))
        };
        *scroll = 0;
    }

    fn share_current_session(&mut self) -> Vec<Effect> {
        let Some(session) = self.session.current_session.as_ref() else {
            self.notifications
                .set("Open a session before sharing it".to_owned());
            return Vec::new();
        };
        if let Some(url) = session.share_url() {
            self.overlay = Some(OverlayState::SessionShare {
                url: url.to_owned(),
            });
            return Vec::new();
        }
        self.notifications
            .set("Creating session link...".to_owned());
        vec![Effect::Api(ApiRequest::ShareSession(session.id.clone()))]
    }

    fn unshare_current_session(&mut self) -> Vec<Effect> {
        let Some(session) = self.session.current_session.as_ref() else {
            self.notifications
                .set("Open a session before unsharing it".to_owned());
            return Vec::new();
        };
        if session.share_url().is_none() {
            self.notifications
                .set("Current session is not shared".to_owned());
            return Vec::new();
        }
        self.overlay = None;
        self.notifications
            .set("Removing session link...".to_owned());
        vec![Effect::Api(ApiRequest::UnshareSession(session.id.clone()))]
    }

    fn open_attach_file(&mut self) {
        if self.session.current_session.is_none() {
            self.prompt_panel_return = None;
            self.notifications
                .set("Open a session before attaching a file".to_owned());
            return;
        }
        self.overlay = Some(OverlayState::AttachFile {
            value: String::new(),
        });
    }

    fn open_file_picker(&mut self, path: String) -> Vec<Effect> {
        if self.session.current_session.is_none() {
            self.notifications
                .set("Open a session before attaching a file".to_owned());
            return Vec::new();
        }
        let path = normalize_picker_path(&path);
        self.overlay = Some(OverlayState::FilePicker {
            path: path.clone(),
            entries: Vec::new(),
            selected: 0,
            loading: true,
        });
        self.notifications
            .set("Loading workspace files...".to_owned());
        let Some(directory) = self
            .client
            .directory()
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
        else {
            self.notifications
                .set("Unable to determine the workspace directory".to_owned());
            return Vec::new();
        };
        vec![Effect::Api(ApiRequest::ListWorkspaceDirectory {
            directory,
            path,
        })]
    }

    fn select_file_picker_entry(&mut self) -> Vec<Effect> {
        let Some((path, entry)) = self.overlay.as_ref().and_then(|overlay| {
            let OverlayState::FilePicker {
                entries, selected, ..
            } = overlay
            else {
                return None;
            };
            entries
                .get(*selected)
                .cloned()
                .map(|entry| (entry.path.clone(), entry))
        }) else {
            return Vec::new();
        };
        if entry.is_directory {
            self.open_file_picker(path)
        } else {
            self.submit_attachment_path(path)
        }
    }

    fn submit_attachment(&mut self) -> Vec<Effect> {
        let Some(OverlayState::AttachFile { value }) = self.overlay.as_ref() else {
            return Vec::new();
        };
        let path = value.trim().to_owned();
        if path.is_empty() {
            self.notifications
                .set("Attachment path cannot be empty".to_owned());
            return Vec::new();
        }
        self.submit_attachment_path(path)
    }

    fn submit_attachment_path(&mut self, path: String) -> Vec<Effect> {
        let Some(session_id) = self
            .session
            .current_session
            .as_ref()
            .map(|session| session.id.clone())
        else {
            self.overlay = None;
            self.notifications
                .set("Open a session before attaching a file".to_owned());
            return Vec::new();
        };
        let Some(directory) = self
            .client
            .directory()
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
        else {
            self.notifications
                .set("Unable to determine the workspace directory".to_owned());
            return Vec::new();
        };
        let panel_return = self.prompt_panel_return.take();
        self.overlay = None;
        self.notifications.set("Reading attachment...".to_owned());
        self.prompt_panel_return = panel_return;
        vec![Effect::Api(ApiRequest::ReadAttachment {
            session_id,
            directory,
            path,
        })]
    }

    fn remove_last_attachment(&mut self) {
        let Some(part) = self.prompt.attachments.pop() else {
            self.notifications
                .set("No attachments to remove".to_owned());
            return;
        };
        self.notifications.set(format!(
            "Removed {}",
            prompt_part_filename(&part).unwrap_or("attachment")
        ));
    }

    fn open_subtask(&mut self) {
        if mention_options(&[], &[], &self.catalog.agents, "").is_empty() {
            self.prompt_panel_return = None;
            self.notifications
                .set("No mentionable sub-agents are loaded".to_owned());
            return;
        }
        self.overlay = Some(OverlayState::Subtask {
            prompt: String::new(),
            selected: 0,
        });
    }

    fn handle_subtask_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                if let Some(selected) = self.prompt_panel_return.take() {
                    self.overlay = Some(OverlayState::PromptPanel { selected });
                } else {
                    self.overlay = None;
                }
                Vec::new()
            }
            KeyCode::Up => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => self.add_subtask_from_overlay(),
            KeyCode::Backspace if key.modifiers.is_empty() => {
                if let Some(OverlayState::Subtask { prompt, .. }) = self.overlay.as_mut() {
                    prompt.pop();
                }
                Vec::new()
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(OverlayState::Subtask { prompt, .. }) = self.overlay.as_mut() {
                    prompt.push(character);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn add_subtask_from_overlay(&mut self) -> Vec<Effect> {
        let Some(OverlayState::Subtask { prompt, selected }) = self.overlay.as_ref() else {
            return Vec::new();
        };
        let prompt = prompt.trim().to_owned();
        if prompt.is_empty() {
            self.notifications
                .set("Subtask prompt cannot be empty".to_owned());
            return Vec::new();
        }
        let agents = mention_options(&[], &[], &self.catalog.agents, "");
        let Some(agent) = agents.get(*selected) else {
            self.notifications
                .set("Select a sub-agent first".to_owned());
            return Vec::new();
        };
        self.prompt.subtasks.push(PromptPart::subtask(
            prompt.clone(),
            prompt.clone(),
            agent.name.clone(),
        ));
        if self.prompt_panel_return.is_some() {
            self.finish_prompt_secondary();
        } else {
            self.overlay = None;
        }
        self.notifications
            .set(format!("Queued subtask for {}", agent.name));
        Vec::new()
    }

    fn open_prompt_options(&mut self) {
        self.prompt_panel_return = None;
        self.overlay = Some(OverlayState::PromptPanel { selected: 0 });
    }

    fn remember_prompt_panel(&mut self) {
        if let Some(OverlayState::PromptPanel { selected }) = self.overlay.as_ref() {
            self.prompt_panel_return = Some(*selected);
        }
    }

    fn finish_prompt_secondary(&mut self) {
        self.overlay = self
            .prompt_panel_return
            .take()
            .map(|selected| OverlayState::PromptPanel { selected });
    }

    fn handle_prompt_panel_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                self.prompt_panel_return = None;
                Vec::new()
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Delete | KeyCode::Char('d') | KeyCode::Char('x') => {
                self.remove_selected_prompt_panel_part()
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.remember_prompt_panel();
                self.open_attach_file();
                Vec::new()
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                self.remember_prompt_panel();
                self.open_subtask();
                Vec::new()
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                self.remember_prompt_panel();
                self.overlay = Some(OverlayState::Model {
                    query: String::new(),
                    selected: 0,
                });
                Vec::new()
            }
            KeyCode::Char('g') | KeyCode::Char('G') => {
                self.remember_prompt_panel();
                self.overlay = Some(OverlayState::Agent {
                    query: String::new(),
                    selected: 0,
                });
                Vec::new()
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                self.remember_prompt_panel();
                self.overlay = Some(OverlayState::Variant {
                    query: String::new(),
                    selected: 0,
                });
                Vec::new()
            }
            KeyCode::Char('f') | KeyCode::Char('F') => {
                self.toggle_prompt_format();
                Vec::new()
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.prompt.options.no_reply = !self.prompt.options.no_reply;
                Vec::new()
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.remember_prompt_panel();
                self.open_prompt_system();
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => self.activate_prompt_panel_item(),
            _ => Vec::new(),
        }
    }

    fn activate_prompt_panel_item(&mut self) -> Vec<Effect> {
        let selected = self.overlay.as_ref().map_or(0, OverlayState::selected);
        let Some(item) = self.prompt.panel_items().get(selected).cloned() else {
            return Vec::new();
        };
        match item {
            PromptPanelItem::Draft => {
                self.overlay = None;
            }
            PromptPanelItem::Model => {
                self.remember_prompt_panel();
                self.overlay = Some(OverlayState::Model {
                    query: String::new(),
                    selected: 0,
                });
            }
            PromptPanelItem::Agent => {
                self.remember_prompt_panel();
                self.overlay = Some(OverlayState::Agent {
                    query: String::new(),
                    selected: 0,
                });
            }
            PromptPanelItem::Variant => {
                self.remember_prompt_panel();
                self.overlay = Some(OverlayState::Variant {
                    query: String::new(),
                    selected: 0,
                });
            }
            PromptPanelItem::Format => self.toggle_prompt_format(),
            PromptPanelItem::NoReply => {
                self.prompt.options.no_reply = !self.prompt.options.no_reply;
            }
            PromptPanelItem::System => {
                self.remember_prompt_panel();
                self.open_prompt_system()
            }
            PromptPanelItem::Tools => {
                self.remember_prompt_panel();
                self.open_prompt_tools()
            }
            PromptPanelItem::AddAttachment => {
                self.remember_prompt_panel();
                self.open_attach_file()
            }
            PromptPanelItem::Attachment(_) => {}
            PromptPanelItem::AddSubtask => {
                self.remember_prompt_panel();
                self.open_subtask()
            }
            PromptPanelItem::Subtask(_) => {}
        }
        Vec::new()
    }

    fn remove_selected_prompt_panel_part(&mut self) -> Vec<Effect> {
        let selected = self.overlay.as_ref().map_or(0, OverlayState::selected);
        let Some(item) = self.prompt.panel_items().get(selected).cloned() else {
            return Vec::new();
        };
        let removed = match item {
            PromptPanelItem::Attachment(index) => self.prompt.remove_attachment(index),
            PromptPanelItem::Subtask(index) => self.prompt.remove_subtask(index),
            _ => None,
        };
        if removed.is_none() {
            return Vec::new();
        }
        let count = self.prompt.panel_items().len();
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_selected(selected.min(count.saturating_sub(1)));
        }
        self.notifications.set("Prompt part removed".to_owned());
        Vec::new()
    }

    #[allow(dead_code)]
    fn handle_prompt_options_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Up => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                match self.overlay.as_ref().map_or(0, OverlayState::selected) {
                    0 => {
                        self.prompt.options.no_reply = !self.prompt.options.no_reply;
                        self.notifications.set(
                            if self.prompt.options.no_reply {
                                "No-reply enabled"
                            } else {
                                "No-reply disabled"
                            }
                            .to_owned(),
                        );
                    }
                    1 => self.toggle_prompt_format(),
                    2 => self.open_prompt_tools(),
                    3 => self.open_prompt_system(),
                    _ => {}
                }
                Vec::new()
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.prompt.options.no_reply = !self.prompt.options.no_reply;
                Vec::new()
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.open_prompt_system();
                Vec::new()
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                self.open_prompt_tools();
                Vec::new()
            }
            KeyCode::Char('f') | KeyCode::Char('F') => {
                self.toggle_prompt_format();
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn open_prompt_tools(&mut self) {
        if !matches!(self.overlay, Some(OverlayState::PromptPanel { .. })) {
            self.prompt_panel_return = None;
        }
        self.overlay = Some(OverlayState::PromptTools { selected: 0 });
    }

    fn handle_prompt_tools_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                let selected = self.prompt_panel_return.take().unwrap_or(2);
                self.overlay = Some(OverlayState::PromptPanel { selected });
                Vec::new()
            }
            KeyCode::Up => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                let selected = self.overlay.as_ref().map_or(0, OverlayState::selected);
                let names = tool_override_names(&self.prompt.options.tool_overrides);
                match selected {
                    index if index < names.len() => {
                        self.cycle_prompt_tool_override(&names[index]);
                    }
                    index if index == names.len() => self.open_prompt_tool_name(),
                    _ => {
                        self.prompt.options.tool_overrides.clear();
                        self.notifications.set("Tool overrides cleared".to_owned());
                    }
                }
                Vec::new()
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.open_prompt_tool_name();
                Vec::new()
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                self.prompt.options.tool_overrides.clear();
                self.notifications.set("Tool overrides cleared".to_owned());
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn cycle_prompt_tool_override(&mut self, name: &str) {
        match self.prompt.options.tool_overrides.get(name).copied() {
            None => {
                self.prompt
                    .options
                    .tool_overrides
                    .insert(name.to_owned(), true);
                self.notifications.set(format!("Tool enabled: {name}"));
            }
            Some(true) => {
                self.prompt
                    .options
                    .tool_overrides
                    .insert(name.to_owned(), false);
                self.notifications.set(format!("Tool disabled: {name}"));
            }
            Some(false) => {
                self.prompt.options.tool_overrides.remove(name);
                self.notifications.set(format!("Tool reset: {name}"));
            }
        }
    }

    fn open_prompt_tool_name(&mut self) {
        self.overlay = Some(OverlayState::PromptToolName {
            value: String::new(),
        });
    }

    fn handle_prompt_tool_name_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                if self.prompt_panel_return.is_some() {
                    self.overlay = Some(OverlayState::PromptPanel {
                        selected: self.prompt_panel_return.take().unwrap_or(7),
                    });
                } else {
                    self.overlay = Some(OverlayState::PromptTools { selected: 0 });
                }
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                let value = match self.overlay.take() {
                    Some(OverlayState::PromptToolName { value }) => value.trim().to_owned(),
                    _ => String::new(),
                };
                if value.is_empty() {
                    self.notifications
                        .set("Tool name cannot be empty".to_owned());
                    self.open_prompt_tool_name();
                } else if value.chars().any(char::is_whitespace) {
                    self.notifications
                        .set("Tool name cannot contain whitespace".to_owned());
                    self.open_prompt_tool_name();
                } else {
                    self.prompt
                        .options
                        .tool_overrides
                        .insert(value.clone(), true);
                    let selected = tool_override_names(&self.prompt.options.tool_overrides)
                        .iter()
                        .position(|name| name == &value)
                        .unwrap_or(0);
                    self.overlay = Some(OverlayState::PromptTools { selected });
                    self.notifications.set(format!("Tool enabled: {value}"));
                }
                Vec::new()
            }
            KeyCode::Backspace if key.modifiers.is_empty() => {
                if let Some(OverlayState::PromptToolName { value }) = self.overlay.as_mut() {
                    value.pop();
                }
                Vec::new()
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(OverlayState::PromptToolName { value }) = self.overlay.as_mut() {
                    value.push(character);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn toggle_prompt_format(&mut self) {
        self.prompt.options.output_format = match self.prompt.options.output_format.take() {
            Some(PromptOutputFormat::JsonSchema { .. }) | Some(PromptOutputFormat::Text) => None,
            None => Some(PromptOutputFormat::JsonSchema {
                schema: serde_json::json!({ "type": "object" }),
                retry_count: None,
            }),
        };
        self.notifications.set(
            if self.prompt.options.output_format.is_some() {
                "JSON object output enabled"
            } else {
                "Text output enabled"
            }
            .to_owned(),
        );
    }

    fn open_prompt_system(&mut self) {
        self.overlay = Some(OverlayState::PromptSystem {
            value: self.prompt.options.system.clone().unwrap_or_default(),
        });
    }

    fn handle_prompt_system_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.overlay = Some(OverlayState::PromptPanel { selected: 6 });
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                let value = match self.overlay.take() {
                    Some(OverlayState::PromptSystem { value }) => value.trim().to_owned(),
                    _ => String::new(),
                };
                self.prompt.options.system = (!value.is_empty()).then_some(value);
                self.notifications
                    .set("System prompt option updated".to_owned());
                Vec::new()
            }
            KeyCode::Backspace if key.modifiers.is_empty() => {
                if let Some(OverlayState::PromptSystem { value }) = self.overlay.as_mut() {
                    value.pop();
                }
                Vec::new()
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(OverlayState::PromptSystem { value }) = self.overlay.as_mut() {
                    value.push(character);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn submit_rename(&mut self) -> Vec<Effect> {
        let Some(OverlayState::RenameSession { value }) = self.overlay.as_ref() else {
            return Vec::new();
        };
        let title = value.trim().to_owned();
        if title.is_empty() {
            self.notifications
                .set("Session title cannot be empty".to_owned());
            return Vec::new();
        }
        let Some(session) = self
            .session
            .current_session
            .as_ref()
            .or_else(|| self.session.sessions.get(self.session.selected_session))
        else {
            self.overlay = None;
            return Vec::new();
        };
        self.overlay = None;
        self.notifications.set("Renaming session...".to_owned());
        vec![Effect::Api(ApiRequest::RenameSession {
            session_id: session.id.clone(),
            title,
        })]
    }

    fn select_mention_from_overlay(&mut self) {
        let Some(OverlayState::Mention {
            query,
            selected,
            start,
            end,
        }) = self.overlay.clone()
        else {
            return;
        };
        let Some(option) = self.mention_options(&query).get(selected).cloned() else {
            self.notifications.set("No matching mentions".to_owned());
            return;
        };
        let text = self.prompt.composer.text();
        let suffix_is_whitespace = text.chars().nth(end).is_some_and(char::is_whitespace);
        let replacement = if suffix_is_whitespace {
            format!("@{}", option.name)
        } else {
            format!("@{} ", option.name)
        };
        self.prompt
            .composer
            .replace_char_range(start, end, &replacement);
        self.overlay = None;
        let kind = match option.kind {
            MentionKind::File => "file",
            MentionKind::Reference => "reference",
            MentionKind::Agent => "agent",
        };
        self.notifications
            .set(format!("Mentioned {kind}: @{}", option.name));
    }

    fn export_current_session(&mut self) -> Vec<Effect> {
        let Some(session) = self.session.current_session.as_ref() else {
            self.notifications
                .set("Open a session before exporting it".to_owned());
            return Vec::new();
        };
        let content = format!(
            "# {}\n\nSession ID: `{}`\n\n{}",
            session.display_title(),
            session.id,
            self.transcript.export_markdown()
        );
        self.notifications.set("Exporting session...".to_owned());
        vec![Effect::ExportSession {
            session_id: session.id.clone(),
            title: session.display_title().to_owned(),
            content,
        }]
    }

    fn current_mention_query(&self) -> Option<String> {
        mention_context(
            &self.prompt.composer.text(),
            self.prompt.composer.cursor_offset(),
        )
        .map(|context| context.query)
    }

    pub(crate) fn timeline_entries(&self) -> Vec<TimelineEntry> {
        let mut entries = self
            .transcript
            .iter()
            .filter(|message| message.info.role == "user")
            .filter_map(|message| {
                let text = message
                    .parts
                    .iter()
                    .filter(|part| part.kind == "text")
                    .filter_map(|part| part.text.as_deref())
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                (!text.is_empty()).then_some(TimelineEntry {
                    message_id: message.info.id.clone(),
                    text,
                    created: message.info.time.created,
                })
            })
            .collect::<Vec<_>>();
        entries.reverse();
        entries
    }

    fn mention_options(&self, query: &str) -> Vec<MentionOption> {
        let files = self.catalog.mention_files();
        mention_options(
            &files,
            &self.catalog.references,
            &self.catalog.agents,
            query,
        )
    }

    fn sync_prompt_overlays(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.sync_slash_overlay();
        if self.overlay.as_ref().is_some_and(|overlay| {
            matches!(
                overlay,
                OverlayState::Slash { .. }
                    | OverlayState::Model { .. }
                    | OverlayState::Skill { .. }
                    | OverlayState::Agent { .. }
                    | OverlayState::Variant { .. }
                    | OverlayState::Mcp { .. }
                    | OverlayState::CommandPalette { .. }
                    | OverlayState::RenameSession { .. }
                    | OverlayState::DeleteSession { .. }
                    | OverlayState::AttachFile { .. }
                    | OverlayState::FilePicker { .. }
                    | OverlayState::Timeline { .. }
                    | OverlayState::ForkSession { .. }
                    | OverlayState::Subtask { .. }
                    | OverlayState::PromptOptions { .. }
                    | OverlayState::PromptPanel { .. }
                    | OverlayState::PromptTools { .. }
                    | OverlayState::PromptToolName { .. }
                    | OverlayState::PromptSystem { .. }
                    | OverlayState::Theme { .. }
                    | OverlayState::Diagnostics
                    | OverlayState::Help
            )
        }) {
            return effects;
        }
        let Some(context) = mention_context(
            &self.prompt.composer.text(),
            self.prompt.composer.cursor_offset(),
        ) else {
            if matches!(self.overlay, Some(OverlayState::Mention { .. })) {
                self.overlay = None;
            }
            return effects;
        };
        let count = self.mention_options(&context.query).len();
        if self.catalog.begin_server_file_search(&context.query) {
            effects.push(Effect::Api(ApiRequest::SearchWorkspaceFiles {
                query: context.query.clone(),
            }));
        }
        match self.overlay.as_mut() {
            Some(OverlayState::Mention {
                query,
                selected,
                start,
                end,
            }) => {
                *query = context.query;
                *start = context.start;
                *end = context.end;
                *selected = (*selected).min(count.saturating_sub(1));
            }
            _ => {
                self.overlay = Some(OverlayState::Mention {
                    query: context.query,
                    selected: 0,
                    start: context.start,
                    end: context.end,
                });
            }
        }
        effects
    }

    fn open_command_palette(&mut self) {
        self.overlay = Some(OverlayState::CommandPalette {
            query: String::new(),
            selected: 0,
        });
    }

    fn handle_command_palette_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Vec::new()
            }
            KeyCode::Up => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => self.run_selected_command(),
            KeyCode::Backspace if key.modifiers.is_empty() => {
                self.pop_overlay_query();
                Vec::new()
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.push_overlay_query(character);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn run_selected_command(&mut self) -> Vec<Effect> {
        let Some(OverlayState::CommandPalette { query, selected }) = self.overlay.as_ref() else {
            return Vec::new();
        };
        let Some(command) = filter_commands(query).get(*selected).cloned() else {
            self.notifications.set("No matching commands".to_owned());
            return Vec::new();
        };
        self.overlay = None;

        match command.id {
            "home" => {
                self.session.screen = Screen::Home;
                self.session.opening_session = None;
                self.notifications.clear();
                Vec::new()
            }
            "scroll_top" => {
                self.transcript.scroll.jump_to_top();
                Vec::new()
            }
            "scroll_bottom" => {
                self.transcript.scroll.jump_to_latest();
                Vec::new()
            }
            "page_up" => {
                self.transcript.scroll.scroll_page(-1);
                Vec::new()
            }
            "page_down" => {
                self.transcript.scroll.scroll_page(1);
                Vec::new()
            }
            "new_session" => vec![Effect::Api(ApiRequest::CreateSession)],
            "refresh_sessions" => {
                self.notifications.set("Refreshing sessions...".to_owned());
                vec![self.session_list_effect()]
            }
            "rename_session" => {
                self.open_rename_session();
                Vec::new()
            }
            "delete_session" => {
                self.open_delete_session();
                Vec::new()
            }
            "archive_session" => {
                self.open_archive_session(false);
                Vec::new()
            }
            "restore_session" => {
                self.open_archive_session(true);
                Vec::new()
            }
            "move_session" => {
                self.open_move_session();
                Vec::new()
            }
            "session_diff" => self.open_session_diff(),
            "vcs_diff" => self.open_vcs_diff(),
            "show_archived_sessions" => self.show_archived_sessions(),
            "show_active_sessions" => self.show_active_sessions(),
            "export_session" => self.export_current_session(),
            "session_timeline" => {
                self.open_timeline();
                Vec::new()
            }
            "fork_session" => {
                self.open_fork_session();
                Vec::new()
            }
            "share_session" => self.share_current_session(),
            "unshare_session" => self.unshare_current_session(),
            "abort_session" => {
                if let Some(session) = self.session.current_session.as_ref() {
                    self.notifications.set("Requesting abort...".to_owned());
                    vec![Effect::Api(ApiRequest::Abort(session.id.clone()))]
                } else {
                    self.notifications
                        .set("No active session to abort".to_owned());
                    Vec::new()
                }
            }
            "compact_session" => self.compact_current_session(),
            "select_model" => {
                self.overlay = Some(OverlayState::Model {
                    query: String::new(),
                    selected: 0,
                });
                Vec::new()
            }
            "select_agent" => {
                self.overlay = Some(OverlayState::Agent {
                    query: String::new(),
                    selected: 0,
                });
                Vec::new()
            }
            "select_variant" => {
                self.overlay = Some(OverlayState::Variant {
                    query: String::new(),
                    selected: 0,
                });
                Vec::new()
            }
            "select_skill" => {
                self.overlay = Some(OverlayState::Skill {
                    query: String::new(),
                    selected: 0,
                });
                Vec::new()
            }
            "attach_file" => {
                self.open_attach_file();
                Vec::new()
            }
            "browse_files" => self.open_file_picker(".".to_owned()),
            "remove_attachment" => {
                self.remove_last_attachment();
                Vec::new()
            }
            "add_subtask" => {
                self.open_subtask();
                Vec::new()
            }
            "prompt_options" => {
                self.open_prompt_options();
                Vec::new()
            }
            "toggle_sidebar" => {
                self.runtime.toggle_sidebar();
                if !self.runtime.sidebar_visible {
                    self.sidebar_area = None;
                }
                Vec::new()
            }
            "toggle_transcript_blocks" => {
                self.toggle_collapsible_blocks();
                Vec::new()
            }
            "select_theme" => {
                self.open_theme_selector();
                Vec::new()
            }
            "toggle_mcp" => {
                self.open_mcp_dialog();
                Vec::new()
            }
            "show_help" => {
                self.overlay = Some(OverlayState::Help);
                Vec::new()
            }
            "show_diagnostics" => {
                self.overlay = Some(OverlayState::Diagnostics);
                Vec::new()
            }
            "refresh_diagnostics" => self.refresh_diagnostics(),
            "history_previous" => {
                self.prompt.composer.history_previous();
                self.sync_prompt_overlays()
            }
            "history_next" => {
                self.prompt.composer.history_next();
                self.sync_prompt_overlays()
            }
            "command_palette" => {
                self.open_command_palette();
                Vec::new()
            }
            "quit" => vec![Effect::Quit],
            other => {
                self.notifications
                    .set(format!("{other} is only available as a key binding"));
                Vec::new()
            }
        }
    }

    fn handle_catalog_overlay_key(&mut self, key: KeyEvent, dialog: CatalogDialog) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                if self.prompt_panel_return.is_some() {
                    self.finish_prompt_secondary();
                } else {
                    self.overlay = None;
                }
                Vec::new()
            }
            KeyCode::Up => {
                self.move_overlay_selection(-1);
                Vec::new()
            }
            KeyCode::Down => {
                self.move_overlay_selection(1);
                Vec::new()
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                match dialog {
                    CatalogDialog::Model => self.select_model_from_overlay(),
                    CatalogDialog::Skill => self.select_skill_from_overlay(),
                    CatalogDialog::Agent => self.select_agent_from_overlay(),
                    CatalogDialog::Variant => self.select_variant_from_overlay(),
                }
                Vec::new()
            }
            KeyCode::Backspace if key.modifiers.is_empty() => {
                self.pop_overlay_query();
                Vec::new()
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.push_overlay_query(character);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn push_overlay_query(&mut self, character: char) {
        if let Some(
            OverlayState::Model { query, selected }
            | OverlayState::Skill { query, selected }
            | OverlayState::Agent { query, selected }
            | OverlayState::Variant { query, selected }
            | OverlayState::CommandPalette { query, selected },
        ) = self.overlay.as_mut()
        {
            query.push(character);
            *selected = 0;
        }
    }

    fn pop_overlay_query(&mut self) {
        if let Some(
            OverlayState::Model { query, selected }
            | OverlayState::Skill { query, selected }
            | OverlayState::Agent { query, selected }
            | OverlayState::Variant { query, selected }
            | OverlayState::CommandPalette { query, selected },
        ) = self.overlay.as_mut()
        {
            query.pop();
            *selected = 0;
        }
    }

    fn move_overlay_selection(&mut self, direction: i32) {
        let count = self.overlay_item_count();
        if count == 0 {
            return;
        }
        let current = self.overlay.as_ref().map_or(0, OverlayState::selected);
        let next = if direction < 0 {
            if current == 0 { count - 1 } else { current - 1 }
        } else if current + 1 >= count {
            0
        } else {
            current + 1
        };
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_selected(next);
        }
    }

    fn overlay_item_count(&self) -> usize {
        match self.overlay.as_ref() {
            Some(OverlayState::Slash { selected: _ }) => {
                let text = self.prompt.composer.text();
                let query = slash_query(&text).unwrap_or_default();
                matching_commands_with_server(query, &self.catalog.commands).len()
            }
            Some(OverlayState::Model { query, selected: _ }) => {
                model_options(&self.catalog.providers, &self.catalog.recent_models, query).len()
            }
            Some(OverlayState::Skill { query, selected: _ }) => {
                skill_options(&self.catalog.skills, query).len()
            }
            Some(OverlayState::Agent { query, selected: _ }) => {
                agent_options(&self.catalog.agents, query).len()
            }
            Some(OverlayState::Variant { query, selected: _ }) => variant_options(
                &self.catalog.providers,
                self.active_model_ref().as_ref(),
                query,
            )
            .len(),
            Some(OverlayState::CommandPalette { query, selected: _ }) => {
                filter_commands(query).len()
            }
            Some(OverlayState::Mention { query, .. }) => self.mention_options(query).len(),
            Some(OverlayState::Subtask { .. }) => {
                mention_options(&[], &[], &self.catalog.agents, "").len()
            }
            Some(OverlayState::Mcp { .. }) => self.integrations.mcp.len(),
            Some(OverlayState::FilePicker { entries, .. }) => entries.len(),
            Some(OverlayState::Timeline { .. }) => self.timeline_entries().len(),
            Some(OverlayState::ForkSession { .. }) => self.timeline_entries().len() + 1,
            Some(OverlayState::SessionDiff { .. }) => self.integrations.diffs.len(),
            Some(OverlayState::VcsDiff { .. }) => self.integrations.vcs_diffs.len(),
            Some(OverlayState::PromptPanel { .. }) => self.prompt.panel_items().len(),
            Some(OverlayState::PromptOptions { .. }) => 4,
            Some(OverlayState::PromptTools { .. }) => {
                tool_override_names(&self.prompt.options.tool_overrides).len() + 2
            }
            Some(OverlayState::Theme { .. }) => Theme::choices().len(),
            Some(
                OverlayState::RenameSession { .. }
                | OverlayState::DeleteSession { .. }
                | OverlayState::ArchiveSession { .. }
                | OverlayState::MoveSession { .. }
                | OverlayState::SessionShare { .. }
                | OverlayState::AttachFile { .. }
                | OverlayState::PromptToolName { .. }
                | OverlayState::PromptSystem { .. }
                | OverlayState::Diagnostics
                | OverlayState::Help,
            ) => 0,
            None => 0,
        }
    }

    fn select_model_for_current_session(&mut self, model: ModelRef) {
        let session_id = self
            .session
            .current_session
            .as_ref()
            .map(|session| session.id.clone());
        if let Some(session_id) = session_id {
            self.catalog.select_model_for_session(&session_id, model);
        } else {
            self.catalog.select_model(model);
        }
    }

    fn select_model_from_overlay(&mut self) {
        let Some(OverlayState::Model { query, selected }) = self.overlay.as_ref() else {
            return;
        };
        let Some(option) =
            model_options(&self.catalog.providers, &self.catalog.recent_models, query)
                .get(*selected)
                .cloned()
        else {
            self.notifications.set("No matching models".to_owned());
            return;
        };
        self.select_model_for_current_session(option.model_ref());
        self.finish_prompt_secondary();
        self.notifications.set(format!(
            "Model selected: {}/{}",
            option.provider_id, option.model_id
        ));
    }

    fn select_skill_from_overlay(&mut self) {
        let Some(OverlayState::Skill { query, selected }) = self.overlay.as_ref() else {
            return;
        };
        let Some(option) = skill_options(&self.catalog.skills, query)
            .get(*selected)
            .cloned()
        else {
            self.notifications.set("No matching skills".to_owned());
            return;
        };
        self.prompt.composer.set_text(&format!("/{} ", option.name));
        self.overlay = None;
        self.notifications
            .set(format!("Skill ready: /{}", option.name));
    }

    fn select_agent_from_overlay(&mut self) {
        let Some(OverlayState::Agent { query, selected }) = self.overlay.as_ref() else {
            return;
        };
        let Some(option) = agent_options(&self.catalog.agents, query)
            .get(*selected)
            .cloned()
        else {
            self.notifications.set("No matching agents".to_owned());
            return;
        };
        self.catalog.select_agent(option.name.clone());
        self.finish_prompt_secondary();
        self.notifications
            .set(format!("Agent selected: {}", option.name));
    }

    fn switch_mode(&mut self, mode: &str) {
        self.catalog.select_agent(mode.to_owned());
        self.notifications
            .success(format!("Switched to {mode} mode"));
    }

    fn select_variant_from_overlay(&mut self) {
        let Some(OverlayState::Variant { query, selected }) = self.overlay.as_ref() else {
            return;
        };
        let Some(model) = self.active_model_ref() else {
            self.notifications
                .set("Select a model before selecting a variant".to_owned());
            return;
        };
        let Some(option) = variant_options(&self.catalog.providers, Some(&model), query)
            .get(*selected)
            .cloned()
        else {
            self.notifications.set("No matching variants".to_owned());
            return;
        };
        let mut selected_model = model;
        selected_model.variant = (option.name != "default").then_some(option.name.clone());
        self.select_model_for_current_session(selected_model);
        self.finish_prompt_secondary();
        self.notifications
            .set(format!("Variant selected: {}", option.name));
    }

    pub fn active_model_ref(&self) -> Option<ModelRef> {
        self.catalog
            .selected_model
            .clone()
            .or_else(|| {
                self.session
                    .current_session
                    .as_ref()
                    .and_then(|session| session.model.clone())
            })
            .or_else(|| {
                self.transcript.iter().rev().find_map(|message| {
                    let info = &message.info;
                    (!info.provider_id.is_empty() && !info.model_id.is_empty()).then(|| ModelRef {
                        id: info.model_id.clone(),
                        provider_id: info.provider_id.clone(),
                        variant: None,
                    })
                })
            })
    }

    fn compact_current_session(&mut self) -> Vec<Effect> {
        let Some(session_id) = self
            .session
            .current_session
            .as_ref()
            .map(|session| session.id.clone())
        else {
            self.notifications
                .set("Open a session before compacting its context".to_owned());
            return Vec::new();
        };
        let Some(model) = self.active_model_ref() else {
            self.notifications
                .set("Select a model before compacting the session".to_owned());
            return Vec::new();
        };
        self.runtime.set_working(true);
        self.notifications
            .set("Requesting session compaction...".to_owned());
        vec![Effect::Api(ApiRequest::CompactSession {
            session_id,
            model,
        })]
    }

    fn sync_slash_overlay(&mut self) {
        let text = self.prompt.composer.text();
        let query = slash_query(&text);
        match (query, self.overlay.as_mut()) {
            (Some(_), Some(OverlayState::Slash { selected })) => {
                let count = matching_commands_with_server(
                    query.unwrap_or_default(),
                    &self.catalog.commands,
                )
                .len();
                *selected = (*selected).min(count.saturating_sub(1));
            }
            (Some(_), None) => {
                self.overlay = Some(OverlayState::Slash { selected: 0 });
            }
            (None, Some(OverlayState::Slash { .. })) => {
                self.overlay = None;
            }
            _ => {}
        }
    }

    fn handle_permission_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Some(request) = self.current_permission().cloned() else {
            return Vec::new();
        };
        if self.is_responding(&request.id) {
            return Vec::new();
        }
        let reply = match key.code {
            KeyCode::Char('y') | KeyCode::Char('1') => crate::runtime::PermissionReply::Once,
            KeyCode::Char('a') => crate::runtime::PermissionReply::Always,
            KeyCode::Char('n') | KeyCode::Char('r') | KeyCode::Esc => {
                crate::runtime::PermissionReply::Reject
            }
            _ => return Vec::new(),
        };
        self.pending.start_responding(request.id.clone());
        self.notifications
            .set("Sending permission response...".to_owned());
        vec![Effect::Api(ApiRequest::ReplyPermission {
            request_id: request.id,
            reply,
            message: None,
        })]
    }

    fn handle_question_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Some(request) = self.current_question().cloned() else {
            return Vec::new();
        };
        if self.is_responding(&request.id) {
            return Vec::new();
        }
        let Some(question) = request.questions.get(self.pending.question_index).cloned() else {
            return Vec::new();
        };
        match key.code {
            KeyCode::Esc => {
                self.pending.start_responding(request.id.clone());
                self.notifications.set("Rejecting question...".to_owned());
                vec![Effect::Api(ApiRequest::RejectQuestion(request.id))]
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.pending.question_selected = self.pending.question_selected.saturating_sub(1);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !question.options.is_empty() {
                    self.pending.question_selected =
                        (self.pending.question_selected + 1).min(question.options.len() - 1);
                }
                Vec::new()
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.pending.question_index = self.pending.question_index.saturating_sub(1);
                self.pending.question_selected = 0;
                Vec::new()
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.pending.question_index + 1 < request.questions.len() {
                    self.pending.question_index += 1;
                    self.pending.question_selected = 0;
                }
                Vec::new()
            }
            KeyCode::Char(character) if ('1'..='9').contains(&character) => {
                let index = character as usize - '1' as usize;
                if index < question.options.len() {
                    self.pending.question_selected = index;
                }
                Vec::new()
            }
            KeyCode::Char(' ') if question.multiple => {
                self.toggle_question_answer(&question);
                Vec::new()
            }
            KeyCode::Enter => self.submit_question(request, &question),
            _ => Vec::new(),
        }
    }

    fn submit_question(
        &mut self,
        request: QuestionRequest,
        question: &QuestionInfo,
    ) -> Vec<Effect> {
        let Some(option) = question.options.get(self.pending.question_selected) else {
            self.notifications.set("Select an answer first".to_owned());
            return Vec::new();
        };
        self.pending
            .question_answers
            .resize(request.questions.len(), Vec::new());
        if question.multiple {
            if self.pending.question_answers[self.pending.question_index].is_empty() {
                self.pending.question_answers[self.pending.question_index]
                    .push(option.label.clone());
            }
        } else {
            self.pending.question_answers[self.pending.question_index] = vec![option.label.clone()];
        }
        if self.pending.question_index + 1 < request.questions.len() {
            self.pending.question_index += 1;
            self.pending.question_selected = 0;
            return Vec::new();
        }
        if self.pending.question_answers.iter().any(Vec::is_empty) {
            self.notifications
                .set("Answer each question before submitting".to_owned());
            return Vec::new();
        }
        self.pending.start_responding(request.id.clone());
        self.notifications
            .set("Sending question response...".to_owned());
        vec![Effect::Api(ApiRequest::ReplyQuestion {
            request_id: request.id,
            answers: self.pending.question_answers.clone(),
        })]
    }

    fn toggle_question_answer(&mut self, question: &QuestionInfo) {
        let Some(option) = question.options.get(self.pending.question_selected) else {
            return;
        };
        self.pending
            .question_answers
            .resize(self.pending.question_index + 1, Vec::new());
        let answers = &mut self.pending.question_answers[self.pending.question_index];
        if let Some(index) = answers.iter().position(|answer| answer == &option.label) {
            answers.remove(index);
        } else {
            answers.push(option.label.clone());
        }
    }

    fn submit(&mut self, prompt: String) -> Vec<Effect> {
        let prompt = prompt.trim().to_owned();
        if prompt.is_empty()
            && self.prompt.attachments.is_empty()
            && self.prompt.subtasks.is_empty()
        {
            return Vec::new();
        }
        let session_id = self
            .session
            .current_session
            .as_ref()
            .map(|session| session.id.clone());
        let model = self.catalog.selected_model.clone();
        let agent = self.catalog.selected_agent.clone().or_else(|| {
            self.session
                .current_session
                .as_ref()
                .and_then(|session| session.agent.clone())
        });
        let directory = self
            .client
            .directory()
            .map(Path::new)
            .unwrap_or_else(|| Path::new("."));
        let mut request = PromptRequest::from_text_with_mentions_and_references(
            &prompt,
            model.as_ref(),
            agent.as_deref(),
            Some(directory),
            &self.catalog.agents,
            &self.catalog.references,
        );
        let attachments = self.prompt.take_attachments();
        let subtasks = self.prompt.take_subtasks();
        if prompt.is_empty() {
            request.parts.clear();
        }
        request.parts.extend(attachments.iter().cloned());
        request.parts.extend(subtasks.iter().cloned());
        request.no_reply = self.prompt.options.no_reply.then_some(true);
        request.tools = (!self.prompt.options.tool_overrides.is_empty())
            .then(|| self.prompt.options.tool_overrides.clone());
        request.system = self.prompt.options.system.clone();
        request.output_format = self.prompt.options.output_format.clone();
        let submission = PromptSubmission {
            session_id,
            request,
            prompt,
            attachments,
            subtasks,
        };
        self.prompt.composer.clear();
        if self.current_session_is_working() {
            self.prompt.enqueue(submission);
            self.notifications.set(format!(
                "Prompt queued ({} waiting)",
                self.prompt.queued_len()
            ));
            return Vec::new();
        }
        self.dispatch_prompt(submission, false)
    }

    fn current_session_is_working(&self) -> bool {
        self.session
            .current_session
            .as_ref()
            .and_then(|session| self.runtime.session_statuses.get(&session.id))
            .is_some_and(SessionStatus::is_working)
            || self.runtime.working
    }

    fn dispatch_next_queued_prompt(&mut self) -> Vec<Effect> {
        let Some(session_id) = self
            .session
            .current_session
            .as_ref()
            .map(|session| session.id.clone())
        else {
            return Vec::new();
        };
        let Some(submission) = self.prompt.dequeue_for_session(&session_id) else {
            return Vec::new();
        };
        self.dispatch_prompt(submission, true)
    }

    fn dispatch_prompt(&mut self, submission: PromptSubmission, queued: bool) -> Vec<Effect> {
        let PromptSubmission {
            session_id,
            request,
            prompt,
            attachments,
            subtasks,
        } = submission;
        self.prompt
            .stage_submission(prompt.clone(), attachments, subtasks);
        self.runtime.begin_response(&prompt);
        self.notifications.set(if queued {
            format!(
                "Sending queued prompt ({} waiting)",
                self.prompt.queued_len()
            )
        } else {
            "Sending prompt...".to_owned()
        });
        vec![Effect::Api(ApiRequest::Submit {
            session_id,
            request: Box::new(request),
        })]
    }

    fn refresh_current_effect(&self) -> Vec<Effect> {
        self.session
            .current_session
            .as_ref()
            .map(|session| vec![Effect::Api(ApiRequest::RefreshCurrent(session.id.clone()))])
            .unwrap_or_default()
    }

    /// Advances the prompt cursor blink phase.
    ///
    /// The runtime calls this before handling each event, charging elapsed time
    /// to the state that was visible while it waited.
    pub fn advance_cursor_blink(&mut self, delta: std::time::Duration) {
        // `runtime.working` is the thinking state: it is set while a session is
        // producing a response and cleared when the step ends or fails.
        self.prompt
            .composer
            .advance_blink(delta, self.runtime.working);
    }

    /// Returns how long the runtime can sleep before cursor visibility changes.
    pub fn next_cursor_blink_transition_in(&self) -> Option<std::time::Duration> {
        self.prompt
            .composer
            .next_blink_transition_in(self.runtime.working)
    }

    #[allow(dead_code)]
    pub fn scroll_lines(&mut self, lines: i32) {
        self.transcript.scroll.scroll_lines(lines);
    }

    fn clear_current_session(&mut self) {
        self.session.current_session = None;
        self.session.opening_session = None;
        self.session.children.clear();
        self.transcript.clear();
        self.prompt.clear_queued_parts();
        self.integrations.clear_session_panels();
        self.runtime.set_working(false);
        self.runtime.reset_response();
        self.sidebar_scroll.reset();
        self.session.screen = Screen::Home;
    }

    fn apply_sessions(&mut self, sessions: Vec<Session>) {
        self.session.replace_sessions(
            sessions
                .into_iter()
                .filter(|session| self.session.session_visible(session))
                .collect(),
        );
        self.runtime.mark_connected();
    }

    fn select_latest_model_for_new_session(&mut self, session_id: &str) {
        let catalog_loaded = !self.catalog.providers.is_empty();
        let available = |model: &ModelRef| !catalog_loaded || self.catalog.has_model(model);
        let selected = self
            .catalog
            .selected_model
            .clone()
            .filter(|model| available(model));
        let recent = self
            .catalog
            .recent_models
            .iter()
            .find(|model| available(model))
            .cloned();
        let latest_session = self
            .session
            .sessions
            .iter()
            .filter_map(|session| {
                session
                    .model
                    .as_ref()
                    .filter(|model| available(model))
                    .map(|model| (session.time.updated, model.clone()))
            })
            .max_by_key(|(updated, _)| *updated)
            .map(|(_, model)| model);
        let selected = selected.or(latest_session).or(recent);
        self.catalog.selected_model = selected.clone();
        if let Some(model) = selected {
            self.catalog.remember_model_for_session(session_id, model);
        }
    }

    fn restore_model_for_session(&mut self, session_id: &str) {
        let catalog_loaded = !self.catalog.providers.is_empty();
        let available = |model: &ModelRef| !catalog_loaded || self.catalog.has_model(model);
        let persisted = self
            .catalog
            .model_for_session(session_id)
            .filter(&available);
        let session_model = self
            .session
            .current_session
            .as_ref()
            .and_then(|session| session.model.clone())
            .filter(&available);
        let transcript_model = self
            .transcript
            .iter()
            .rev()
            .find_map(|message| {
                let info = &message.info;
                (!info.provider_id.is_empty() && !info.model_id.is_empty()).then(|| ModelRef {
                    id: info.model_id.clone(),
                    provider_id: info.provider_id.clone(),
                    variant: None,
                })
            })
            .filter(&available);
        let recent = self
            .catalog
            .recent_models
            .iter()
            .find(|model| available(model))
            .cloned();
        self.catalog.selected_model = persisted.or(session_model).or(transcript_model).or(recent);
    }

    fn set_permissions(&mut self, permissions: Vec<PermissionRequest>) {
        self.pending.set_permissions(permissions);
    }

    fn set_questions(&mut self, questions: Vec<QuestionRequest>) {
        self.pending.set_questions(
            questions,
            self.session
                .current_session
                .as_ref()
                .map(|session| session.id.as_str()),
        );
    }

    fn upsert_permission(&mut self, request: PermissionRequest) {
        self.pending.upsert_permission(request);
    }

    fn remove_permission(&mut self, request_id: &str) {
        self.pending.remove_permission(request_id);
    }

    fn upsert_question(&mut self, request: QuestionRequest) {
        self.pending.upsert_question(
            request,
            self.session
                .current_session
                .as_ref()
                .map(|session| session.id.as_str()),
        );
    }

    fn remove_question(&mut self, request_id: &str) {
        self.pending.remove_question(request_id);
    }

    fn prepare_current_question_draft(&mut self) {
        self.pending.prepare_current_question_draft(
            self.session
                .current_session
                .as_ref()
                .map(|session| session.id.as_str()),
        );
    }

    fn apply_opened_session(&mut self, snapshot: SessionSnapshot) -> Vec<Effect> {
        if self
            .session
            .opening_session
            .as_deref()
            .is_some_and(|session_id| session_id != snapshot.session.id)
        {
            return Vec::new();
        }
        let session_id = snapshot.session.id.clone();
        self.integrations.clear_session_panels();
        self.session.children.clear();
        self.apply_snapshot(snapshot);
        self.restore_model_for_session(&session_id);
        self.select_session(&session_id);
        self.session.screen = Screen::Session;
        self.prepare_current_question_draft();
        self.session.opening_session = None;
        self.runtime.set_working(false);
        self.runtime.reset_response();
        self.prompt.clear_queued_parts();
        self.transcript.reset_scroll();
        self.sidebar_scroll.reset();
        self.notifications.clear();
        vec![
            Effect::Api(ApiRequest::ListSessionTodos(session_id.clone())),
            Effect::Api(ApiRequest::ListSessionDiff(session_id.clone())),
            Effect::Api(ApiRequest::ListSessionChildren(session_id)),
        ]
    }

    fn apply_snapshot(&mut self, snapshot: SessionSnapshot) {
        let part_ids = snapshot
            .messages
            .iter()
            .flat_map(|message| message.parts.iter())
            .filter(|part| matches!(part.kind.as_str(), "reasoning" | "tool"))
            .map(|part| part.id.as_str())
            .collect::<HashSet<_>>();
        self.transcript
            .collapsed_parts
            .retain(|part_id| part_ids.contains(part_id.as_str()));
        self.upsert_session(snapshot.session.clone());
        self.session.current_session = Some(snapshot.session);
        self.transcript.replace(snapshot.messages);
    }

    fn toggle_collapsible_blocks(&mut self) {
        let ids = self
            .transcript
            .iter()
            .flat_map(|message| message.parts.iter())
            .filter(|part| matches!(part.kind.as_str(), "reasoning" | "tool"))
            .filter(|part| !part.id.is_empty())
            .map(|part| part.id.clone())
            .collect::<HashSet<_>>();
        if ids.is_empty() {
            self.notifications
                .set("No reasoning or tool blocks to collapse".to_owned());
            return;
        }
        let all_collapsed = ids
            .iter()
            .all(|part_id| self.transcript.collapsed_parts.contains(part_id));
        if all_collapsed {
            self.transcript
                .collapsed_parts
                .retain(|part_id| !ids.contains(part_id));
            self.notifications
                .set("Expanded reasoning and tool blocks".to_owned());
        } else {
            self.transcript.collapsed_parts.extend(ids);
            self.notifications
                .set("Collapsed reasoning and tool blocks".to_owned());
        }
    }

    fn select_session(&mut self, session_id: &str) {
        self.session.select_session(session_id);
    }

    fn event_matches_scope(&self, event: &ServerEvent) -> bool {
        if let Some(directory) = event.directory.as_deref() {
            if let Some(configured) = self.client.directory()
                && configured != directory
            {
                return false;
            }
            if let Some(current) = self
                .session
                .current_session
                .as_ref()
                .and_then(Session::directory)
                && self.client.directory().is_none()
                && current != directory
            {
                return false;
            }
        }
        if let Some(workspace) = event.workspace.as_deref()
            && let Some(current) = self
                .session
                .current_session
                .as_ref()
                .and_then(Session::workspace_id)
            && current != workspace
        {
            return false;
        }
        true
    }

    fn event_session_matches(&self, properties: &Value) -> bool {
        property_session_id(properties)
            .map(|session_id| self.is_current_session(session_id))
            .unwrap_or(false)
    }

    fn is_current_session(&self, session_id: &str) -> bool {
        self.session.is_current_session(session_id)
    }

    fn upsert_session(&mut self, session: Session) {
        self.session.upsert_session(session);
    }

    fn apply_agent_switch(&mut self, session_id: &str, agent: &str) {
        self.session.update_agent(session_id, agent);
    }

    fn apply_model_switch(&mut self, session_id: &str, model: &ModelRef) {
        self.session.update_model(session_id, model);
    }

    fn apply_session_location(
        &mut self,
        session_id: &str,
        location: &SessionNextLocation,
        timestamp: i64,
    ) {
        self.session.update_location(
            session_id,
            &location.directory,
            location.workspace_id.clone(),
            timestamp,
        );
    }

    fn touch_session(&mut self, session_id: &str, timestamp: i64) {
        self.session.touch(session_id, timestamp);
    }

    fn upsert_message_info(&mut self, info: MessageInfo) {
        self.transcript.upsert_message_info(info);
    }

    fn upsert_part(&mut self, part: Part) {
        self.transcript.upsert_part(part);
    }

    fn apply_part_delta(&mut self, event: &MessagePartDeltaEvent) {
        if !self.is_current_session(&event.session_id) {
            return;
        }
        self.transcript.apply_part_delta(
            &event.message_id,
            &event.part_id,
            &event.field,
            &event.delta,
        );
    }

    fn remove_part(&mut self, event: &MessagePartRemovedEvent) {
        if !self.is_current_session(&event.session_id) {
            return;
        }
        self.transcript
            .remove_part(&event.message_id, &event.part_id);
    }

    fn mouse_over_sidebar(&self, column: u16, row: u16) -> bool {
        // `sidebar_area` is the pane rect recorded by the last render, so wheel
        // events are routed by the geometry the user actually sees. The session
        // layout gives the sidebar the full frame height, which makes the column
        // the deciding term in practice.
        self.sidebar_area
            .is_some_and(|area| area.contains(ratatui::layout::Position { x: column, y: row }))
    }
}

fn prompt_part_filename(part: &PromptPart) -> Option<&str> {
    match part {
        PromptPart::File { filename, .. } => filename.as_deref(),
        PromptPart::Text { .. } | PromptPart::Agent { .. } | PromptPart::Subtask { .. } => None,
    }
}

fn prompt_event_state(
    prompt: &crate::event::SessionPrompt,
    delivery: crate::event::SessionDelivery,
) -> Value {
    let files = prompt
        .files
        .iter()
        .map(|file| {
            json!({
                "uri": file.uri,
                "mime": file.mime,
                "name": file.name,
                "description": file.description,
                "source": file.source.as_ref().map(prompt_source_value),
            })
        })
        .collect::<Vec<_>>();
    let agents = prompt
        .agents
        .iter()
        .map(|agent| {
            json!({
                "name": agent.name,
                "source": agent.source.as_ref().map(prompt_source_value),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "delivery": delivery.as_str(),
        "files": files,
        "agents": agents,
    })
}

fn prompt_source_value(source: &crate::event::PromptSource) -> Value {
    json!({
        "start": source.start,
        "end": source.end,
        "text": source.text,
    })
}

fn normalize_picker_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let path = path.trim_matches('/');
    if path.is_empty() || path == "." {
        ".".to_owned()
    } else {
        path.trim_start_matches("./").to_owned()
    }
}

fn picker_directory_path(value: &str) -> String {
    let value = value.trim();
    if value.ends_with('/') || value.ends_with('\\') {
        return normalize_picker_path(value);
    }
    let path = normalize_picker_path(value);
    if path == "." {
        return path;
    }
    let path = Path::new(&path);
    path.parent()
        .and_then(|parent| parent.to_str())
        .map(normalize_picker_path)
        .unwrap_or_else(|| ".".to_owned())
}

fn parent_picker_path(path: &str) -> String {
    let path = normalize_picker_path(path);
    if path == "." {
        return path;
    }
    Path::new(&path)
        .parent()
        .and_then(|parent| parent.to_str())
        .map(normalize_picker_path)
        .unwrap_or_else(|| ".".to_owned())
}

fn is_attach_key(key: KeyEvent) -> bool {
    key.modifiers
        .contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Char('u' | 'U'))
}

fn is_remove_attachment_key(key: KeyEvent) -> bool {
    key.modifiers
        .contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
        && key.code == KeyCode::Backspace
}

fn is_subtask_key(key: KeyEvent) -> bool {
    key.modifiers
        .contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Char('t' | 'T'))
}

fn is_prompt_options_key(key: KeyEvent) -> bool {
    key.modifiers
        .contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Char('o' | 'O'))
}

fn is_collapse_blocks_key(key: KeyEvent) -> bool {
    key.modifiers
        .contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Char('b' | 'B'))
}

fn parse_session(value: &Value) -> Option<Session> {
    serde_json::from_value(value.clone()).ok()
}

fn parse_permission_request(value: &Value) -> Option<PermissionRequest> {
    serde_json::from_value(property_object(value, "request").clone()).ok()
}

fn parse_question_request(value: &Value) -> Option<QuestionRequest> {
    serde_json::from_value(property_object(value, "request").clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::{App, Screen};
    use crate::api::{ApiClient, ClientConfig};
    use crate::dialog::OverlayState;
    use crate::event::ServerEvent;
    use crate::model::{
        AgentInfo, CommandInfo, FileDiff, McpServer, MessageInfo, MessageTime, MessageWithParts,
        ModelInfo, ModelRef, Part, PermissionRequest, PromptFileSource, PromptOutputFormat,
        PromptPart, ProviderCatalog, ProviderInfo, Session, SessionShare, SessionStatus,
        SessionTime, Skill, TodoItem, VcsDiffMode, VcsFileDiff, VcsFileStatus, VcsInfo,
        WorkspaceFile,
    };
    use crate::runtime::{ApiRequest, ApiResult, AppMsg, Effect, PermissionReply, SessionSnapshot};
    use crate::theme::ThemeMode;
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::layout::Rect;
    use serde_json::{Value, json};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn app() -> App {
        app_with_directory(None)
    }

    fn app_with_directory(directory: Option<&str>) -> App {
        let client = ApiClient::new(ClientConfig {
            base_url: "http://127.0.0.1:4096".to_owned(),
            username: "opencode".to_owned(),
            password: None,
            directory: directory.map(str::to_owned),
            workspace: None,
        })
        .expect("test client should build");
        App::new_for_tests(Arc::new(client))
    }

    #[test]
    fn question_opens_the_keyboard_help_overlay() {
        let mut app = app();

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::SHIFT,
        ))));

        assert_eq!(app.overlay, Some(OverlayState::Help));
    }

    fn timeline_message(id: &str, text: &str, created: i64) -> MessageWithParts {
        MessageWithParts {
            info: MessageInfo {
                id: id.to_owned(),
                role: "user".to_owned(),
                time: MessageTime {
                    created,
                    ..MessageTime::default()
                },
                ..MessageInfo::default()
            },
            parts: vec![Part {
                id: format!("{id}:text"),
                kind: "text".to_owned(),
                text: Some(text.to_owned()),
                ..Part::default()
            }],
        }
    }

    fn seed_timeline(app: &mut App) {
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_timeline".to_owned(),
            ..Session::default()
        });
        app.transcript.replace(vec![
            timeline_message("msg_old", "first prompt", 1),
            timeline_message("msg_new", "latest prompt", 2),
        ]);
    }

    #[test]
    fn timeline_jumps_to_the_newest_user_prompt_by_message_anchor() {
        let mut app = app();
        seed_timeline(&mut app);
        app.overlay = Some(OverlayState::Timeline { selected: 0 });

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert!(app.overlay.is_none());
        assert_eq!(
            app.transcript
                .scroll
                .anchor()
                .map(|anchor| anchor.message_id),
            Some("msg_new".to_owned())
        );
        assert!(!app.transcript.scroll.is_following());
    }

    #[test]
    fn fork_dialog_can_fork_from_a_selected_prompt_and_open_the_child() {
        let mut app = app();
        seed_timeline(&mut app);
        app.overlay = Some(OverlayState::ForkSession { selected: 0 });

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        ))));
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ForkSession {
                session_id,
                message_id: Some(message_id),
            })] if session_id == "ses_timeline" && message_id == "msg_new"
        ));
        assert!(app.overlay.is_none());

        let effects = app.update(AppMsg::Api(Box::new(ApiResult::ForkedSession(Ok(
            Session {
                id: "ses_forked".to_owned(),
                ..Session::default()
            },
        )))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::OpenSession(session_id))] if session_id == "ses_forked"
        ));
        assert_eq!(app.session.opening_session.as_deref(), Some("ses_forked"));
    }

    #[test]
    fn timeline_and_fork_slash_commands_open_their_session_overlays() {
        let mut app = app();
        seed_timeline(&mut app);
        for character in "/timeline".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.overlay, Some(OverlayState::Timeline { selected: 0 }));

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ))));
        for character in "/fork".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.overlay, Some(OverlayState::ForkSession { selected: 0 }));
    }

    #[test]
    fn compact_slash_command_uses_the_active_model_and_tracks_failure() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_compact".to_owned(),
            ..Session::default()
        });
        app.catalog.selected_model = Some(ModelRef {
            provider_id: "provider_1".to_owned(),
            id: "model_1".to_owned(),
            ..ModelRef::default()
        });
        for character in "/compact".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::CompactSession { session_id, model })]
                if session_id == "ses_compact"
                    && model.provider_id == "provider_1"
                    && model.id == "model_1"
        ));
        assert!(app.runtime.working);
        assert!(app.overlay.is_none());

        app.update(AppMsg::Api(Box::new(ApiResult::CompactedSession {
            session_id: "ses_compact".to_owned(),
            result: Err("model unavailable".to_owned()),
        })));
        assert!(!app.runtime.working);
        assert_eq!(
            app.notifications.active(),
            Some("Compaction failed: model unavailable")
        );
    }

    #[test]
    fn session_panels_apply_current_session_data_and_ignore_stale_results() {
        let mut app = app();
        seed_timeline(&mut app);

        app.update(AppMsg::Api(Box::new(ApiResult::Todos {
            session_id: "ses_timeline".to_owned(),
            result: Ok(vec![TodoItem {
                id: "todo_1".to_owned(),
                content: "Review diff".to_owned(),
                status: "in_progress".to_owned(),
                ..TodoItem::default()
            }]),
        })));
        app.update(AppMsg::Api(Box::new(ApiResult::SessionDiff {
            session_id: "ses_timeline".to_owned(),
            result: Ok(vec![FileDiff {
                file: "src/main.rs".to_owned(),
                additions: 2,
                ..FileDiff::default()
            }]),
        })));
        app.update(AppMsg::Api(Box::new(ApiResult::SessionChildren {
            session_id: "ses_timeline".to_owned(),
            result: Ok(vec![Session {
                id: "ses_child".to_owned(),
                parent_id: Some("ses_timeline".to_owned()),
                ..Session::default()
            }]),
        })));
        app.update(AppMsg::Api(Box::new(ApiResult::Vcs(Ok(VcsInfo {
            branch: "feature".to_owned(),
            ..VcsInfo::default()
        })))));
        app.update(AppMsg::Api(Box::new(ApiResult::VcsStatus(Ok(vec![
            VcsFileStatus {
                file: "src/main.rs".to_owned(),
                additions: 2,
                status: "modified".to_owned(),
                ..VcsFileStatus::default()
            },
        ])))));
        app.update(AppMsg::Api(Box::new(ApiResult::SessionStatuses(Ok(
            HashMap::from([("ses_timeline".to_owned(), SessionStatus::Busy)]),
        )))));

        assert_eq!(app.integrations.todos[0].content, "Review diff");
        assert_eq!(app.integrations.diffs[0].file, "src/main.rs");
        assert_eq!(app.session.children[0].id, "ses_child");
        assert_eq!(app.integrations.vcs.as_ref().unwrap().branch, "feature");
        assert!(app.runtime.working);

        app.update(AppMsg::Api(Box::new(ApiResult::Todos {
            session_id: "ses_other".to_owned(),
            result: Ok(vec![TodoItem {
                id: "stale".to_owned(),
                ..TodoItem::default()
            }]),
        })));
        assert_eq!(app.integrations.todos[0].id, "todo_1");

        let event = ServerEvent::from_json(json!({
            "type": "todo.updated",
            "properties": {
                "sessionID": "ses_timeline",
                "todos": [{
                    "id": "todo_2",
                    "content": "Ship it",
                    "status": "completed"
                }]
            }
        }))
        .expect("todo event should parse");
        app.update(AppMsg::Server(event));
        assert_eq!(app.integrations.todos[0].id, "todo_2");
    }

    #[test]
    fn command_palette_opens_diagnostics_overlay() {
        let mut app = app();
        app.overlay = Some(OverlayState::CommandPalette {
            query: "diagnostics".to_owned(),
            selected: 0,
        });

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert_eq!(app.overlay, Some(OverlayState::Diagnostics));

        app.overlay = Some(OverlayState::CommandPalette {
            query: "refresh diagnostics".to_owned(),
            selected: 0,
        });
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert_eq!(app.overlay, Some(OverlayState::Diagnostics));
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Api(ApiRequest::Health)))
        );
    }

    #[test]
    fn command_palette_opens_session_diff_and_keeps_file_selection_scoped() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_diff".to_owned(),
            ..Session::default()
        });
        app.integrations.diffs = vec![
            FileDiff {
                file: "a.rs".to_owned(),
                before: "old".to_owned(),
                after: "new".to_owned(),
                ..FileDiff::default()
            },
            FileDiff {
                file: "b.rs".to_owned(),
                ..FileDiff::default()
            },
        ];
        app.overlay = Some(OverlayState::CommandPalette {
            query: "Open Session Diff".to_owned(),
            selected: 0,
        });

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            app.overlay,
            Some(OverlayState::SessionDiff {
                selected: 0,
                scroll: 0,
            })
        );
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ListSessionDiff(session_id))] if session_id == "ses_diff"
        ));

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            app.overlay,
            Some(OverlayState::SessionDiff {
                selected: 1,
                scroll: 0,
            })
        );
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::PageDown,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            app.overlay,
            Some(OverlayState::SessionDiff { scroll, .. }) if scroll == 10
        ));
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ListSessionDiff(session_id))] if session_id == "ses_diff"
        ));
    }

    #[test]
    fn command_palette_opens_vcs_diff_and_switches_sources_without_accepting_stale_data() {
        let mut app = app();
        app.overlay = Some(OverlayState::CommandPalette {
            query: "Open VCS Diff".to_owned(),
            selected: 0,
        });

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            app.overlay,
            Some(OverlayState::VcsDiff {
                mode: VcsDiffMode::Git,
                selected: 0,
                scroll: 0,
            })
        );
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ListVcsDiff { mode })] if *mode == VcsDiffMode::Git
        ));

        app.update(AppMsg::Api(Box::new(ApiResult::VcsDiff {
            mode: VcsDiffMode::Git,
            result: Ok(vec![VcsFileDiff {
                file: "src/main.rs".to_owned(),
                patch: "@@ -1 +1 @@\n-old\n+new\n".to_owned(),
                ..VcsFileDiff::default()
            }]),
        })));
        assert_eq!(app.integrations.vcs_diffs[0].file, "src/main.rs");

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('s'),
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            app.overlay,
            Some(OverlayState::VcsDiff {
                mode: VcsDiffMode::Branch,
                selected: 0,
                scroll: 0,
            })
        );
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ListVcsDiff { mode })] if *mode == VcsDiffMode::Branch
        ));

        app.update(AppMsg::Api(Box::new(ApiResult::VcsDiff {
            mode: VcsDiffMode::Git,
            result: Ok(vec![VcsFileDiff {
                file: "stale.rs".to_owned(),
                ..VcsFileDiff::default()
            }]),
        })));
        assert_eq!(app.integrations.vcs_diffs[0].file, "src/main.rs");

        app.update(AppMsg::Api(Box::new(ApiResult::VcsDiff {
            mode: VcsDiffMode::Branch,
            result: Ok(vec![VcsFileDiff {
                file: "branch.rs".to_owned(),
                ..VcsFileDiff::default()
            }]),
        })));
        assert_eq!(app.integrations.vcs_diffs[0].file, "branch.rs");
    }

    #[test]
    fn command_palette_opens_theme_selector_and_applies_choice() {
        let mut app = app();
        app.overlay = Some(OverlayState::CommandPalette {
            query: "select theme".to_owned(),
            selected: 0,
        });

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.overlay, Some(OverlayState::Theme { selected: 0 }));

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        ))));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert!(app.overlay.is_none());
        assert_eq!(app.theme.mode, ThemeMode::Light);
    }

    #[test]
    fn diagnostics_overlay_can_clear_history_and_close() {
        let mut app = app();
        app.notifications.set("old notice");
        app.overlay = Some(OverlayState::Diagnostics);

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::NONE,
        ))));
        assert!(app.notifications.history().is_empty());

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        ))));
        assert!(app.overlay.is_none());
    }

    #[test]
    fn diagnostics_overlay_refreshes_runtime_catalog_and_integration_data() {
        let mut app = app();
        app.overlay = Some(OverlayState::Diagnostics);

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::NONE,
        ))));

        assert_eq!(app.overlay, Some(OverlayState::Diagnostics));
        assert_eq!(
            app.notifications.active(),
            Some("Refreshing diagnostics...")
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Api(ApiRequest::Health)))
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Api(ApiRequest::ListProviders)))
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Api(ApiRequest::ListMcp)))
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Api(ApiRequest::ListLsp)))
        );
    }

    #[test]
    fn mcp_dialog_toggles_a_server_and_refreshes_status() {
        let mut app = app();
        app.integrations.mcp.push(McpServer {
            name: "docs".to_owned(),
            status: "disabled".to_owned(),
            ..McpServer::default()
        });
        app.open_mcp_dialog();

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ConnectMcp(name))] if name == "docs"
        ));
        assert_eq!(app.integrations.mcp_action.as_deref(), Some("docs"));

        let effects = app.update(AppMsg::Api(Box::new(ApiResult::McpConnected {
            name: "docs".to_owned(),
            result: Ok(()),
        })));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ListMcp)]
        ));
        assert!(app.integrations.mcp_action.is_none());
        assert_eq!(app.notifications.active(), Some("Connected MCP: docs"));
    }

    #[test]
    fn reconnecting_events_are_visible_in_runtime_status_and_recover_on_connect() {
        let mut app = app();

        app.handle_server_event(crate::event::ServerEvent::local_reconnecting(
            "connection reset",
            2,
            2,
        ));
        assert_eq!(app.connection_label(), "reconnecting");
        assert_eq!(
            app.connection_detail(),
            "reconnecting (attempt 2, retry in 2s)"
        );
        assert_eq!(
            app.notifications.active(),
            Some("Connection lost; retrying in 2s (attempt 2): connection reset")
        );

        app.handle_server_event(crate::event::ServerEvent::local_connected());
        assert_eq!(app.connection_label(), "connected");
        assert!(app.runtime.is_connected());
    }

    #[test]
    fn tick_expires_active_notification_but_keeps_history() {
        let mut app = app();
        app.notifications.set("temporary notice");

        for _ in 0..11 {
            app.update(AppMsg::Tick);
            assert_eq!(app.notifications.active(), Some("temporary notice"));
        }
        app.update(AppMsg::Tick);

        assert!(app.notifications.active().is_none());
        assert_eq!(app.notifications.history().len(), 1);
    }

    #[test]
    fn cursor_deletes_unicode_by_char_boundary() {
        let mut app = app();
        app.prompt.composer.set_text("你好 world");
        app.prompt
            .composer
            .handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        app.prompt
            .composer
            .handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

        assert_eq!(app.prompt.composer.text(), "你好 worl");
        app.prompt
            .composer
            .handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        app.prompt
            .composer
            .handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.prompt.composer.text(), "好 worl");
    }

    #[test]
    fn paste_normalizes_line_endings_and_respects_pending_actions() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_current".to_owned(),
            ..Session::default()
        });

        let effects = app.update(AppMsg::Terminal(Event::Paste(
            "first\r\nsecond\rthird".to_owned(),
        )));
        assert!(effects.is_empty());
        assert_eq!(app.prompt.composer.text(), "first\nsecond\nthird");
        assert!(!app.runtime.working);

        app.pending.permissions.push(PermissionRequest {
            id: "per_1".to_owned(),
            session_id: "ses_current".to_owned(),
            ..PermissionRequest::default()
        });
        app.update(AppMsg::Terminal(Event::Paste("blocked".to_owned())));

        assert_eq!(app.prompt.composer.text(), "first\nsecond\nthird");
    }

    #[test]
    fn enter_returns_a_prompt_effect_without_waiting_for_http() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            ..Session::default()
        });
        app.prompt.composer.set_text("hello");

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert!(app.prompt.composer.is_empty());
        assert!(app.runtime.working);
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::Submit {
                session_id: Some(id),
                request,
            })]
                if id == "ses_1"
                    && request.text_content() == "hello"
                    && request.model.is_none()
                    && request.agent.is_none()
        ));
    }

    #[test]
    fn prompts_submitted_while_working_are_sent_fifo_when_session_becomes_idle() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_queue".to_owned(),
            ..Session::default()
        });
        app.catalog.selected_model = Some(ModelRef {
            provider_id: "provider_one".to_owned(),
            id: "model_one".to_owned(),
            ..ModelRef::default()
        });
        app.runtime
            .set_session_status("ses_queue", SessionStatus::Busy);

        app.prompt.composer.set_text("first queued");
        let first_queue_effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(first_queue_effects.is_empty());
        assert_eq!(app.prompt.queued_len(), 1);
        assert!(app.prompt.composer.is_empty());

        app.catalog.selected_model = Some(ModelRef {
            provider_id: "provider_two".to_owned(),
            id: "model_two".to_owned(),
            ..ModelRef::default()
        });
        app.prompt.composer.set_text("second queued");
        let second_queue_effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(second_queue_effects.is_empty());
        assert_eq!(app.prompt.queued_len(), 2);

        let idle = crate::event::ServerEvent::from_json(json!({
            "type": "session.status",
            "properties": {
                "sessionID": "ses_queue",
                "status": { "type": "idle" }
            }
        }))
        .expect("idle status should parse");
        let first_send = app.handle_server_event(idle);
        assert!(matches!(
            first_send.as_slice(),
            [Effect::Api(ApiRequest::Submit { request, .. })]
                if request.text_content() == "first queued"
                    && request.model.as_ref().is_some_and(|model| {
                        model.provider_id == "provider_one" && model.model_id == "model_one"
                    })
        ));
        assert_eq!(app.prompt.queued_len(), 1);
        assert!(app.runtime.working);

        let duplicate_idle = crate::event::ServerEvent::from_json(json!({
            "type": "session.status",
            "properties": {
                "sessionID": "ses_queue",
                "status": { "type": "idle" }
            }
        }))
        .expect("duplicate idle status should parse");
        assert!(app.handle_server_event(duplicate_idle).is_empty());
        assert_eq!(app.prompt.queued_len(), 1);

        app.update(AppMsg::Api(Box::new(ApiResult::Submitted {
            session: None,
            result: Ok(()),
        })));
        let busy = crate::event::ServerEvent::from_json(json!({
            "type": "session.status",
            "properties": {
                "sessionID": "ses_queue",
                "status": { "type": "busy" }
            }
        }))
        .expect("busy status should parse");
        app.handle_server_event(busy);
        let idle = crate::event::ServerEvent::from_json(json!({
            "type": "session.status",
            "properties": {
                "sessionID": "ses_queue",
                "status": { "type": "idle" }
            }
        }))
        .expect("idle status should parse");
        let second_send = app.handle_server_event(idle);
        assert!(matches!(
            second_send.as_slice(),
            [Effect::Api(ApiRequest::Submit { request, .. })]
                if request.text_content() == "second queued"
                    && request.model.as_ref().is_some_and(|model| {
                        model.provider_id == "provider_two" && model.model_id == "model_two"
                    })
        ));
        assert_eq!(app.prompt.queued_len(), 0);
    }

    #[test]
    fn model_slash_flow_selects_a_model_for_the_next_prompt() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
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
                    ..ModelInfo::default()
                },
            )]),
        });

        for character in "/model".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        assert!(matches!(app.overlay, Some(OverlayState::Slash { .. })));

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            app.overlay,
            Some(OverlayState::Model {
                ref query,
                selected: 0
            }) if query.is_empty()
        ));
        assert!(app.prompt.composer.is_empty());

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            app.catalog
                .selected_model
                .as_ref()
                .map(|model| (model.provider_id.as_str(), model.id.as_str())),
            Some(("provider_1", "model_1"))
        );
        assert!(app.overlay.is_none());

        app.prompt.composer.set_text("use the selected model");
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::Submit {
                session_id: Some(id),
                request,
            })] if id == "ses_1"
                && request.text_content() == "use the selected model"
                && request
                    .model
                    .as_ref()
                    .is_some_and(|model| {
                        model.provider_id == "provider_1" && model.model_id == "model_1"
                    })
                && request.agent.is_none()
        ));
    }

    #[test]
    fn opening_sessions_restores_their_selected_models_independently() {
        let mut app = app();
        app.session.sessions = vec![
            Session {
                id: "session-a".to_owned(),
                ..Session::default()
            },
            Session {
                id: "session-b".to_owned(),
                ..Session::default()
            },
        ];
        let model_a = ModelRef {
            provider_id: "provider-a".to_owned(),
            id: "model-a".to_owned(),
            variant: Some("high".to_owned()),
        };
        let model_b = ModelRef {
            provider_id: "provider-b".to_owned(),
            id: "model-b".to_owned(),
            ..ModelRef::default()
        };

        app.session.current_session = Some(app.session.sessions[0].clone());
        app.select_model_for_current_session(model_a.clone());
        app.session.current_session = Some(app.session.sessions[1].clone());
        app.select_model_for_current_session(model_b.clone());

        app.session.opening_session = Some("session-a".to_owned());
        app.apply_opened_session(SessionSnapshot {
            session: app.session.sessions[0].clone(),
            messages: Vec::new(),
        });
        assert_eq!(
            app.active_model_ref().map(|model| model.variant),
            Some(Some("high".to_owned()))
        );
        assert_eq!(
            app.active_model_ref().map(|model| model.id),
            Some("model-a".to_owned())
        );

        app.session.opening_session = Some("session-b".to_owned());
        app.apply_opened_session(SessionSnapshot {
            session: app.session.sessions[1].clone(),
            messages: Vec::new(),
        });
        assert_eq!(
            app.active_model_ref().map(|model| model.id),
            Some("model-b".to_owned())
        );
        assert!(
            app.active_model_ref()
                .is_some_and(|model| model.variant.is_none())
        );
    }

    #[test]
    fn creating_a_session_refreshes_providers_and_reuses_the_latest_available_model() {
        let mut app = app();
        app.catalog.providers.push(ProviderInfo {
            id: "stale_provider".to_owned(),
            models: HashMap::from([(
                "stale_model".to_owned(),
                ModelInfo {
                    id: "stale_model".to_owned(),
                    provider_id: "stale_provider".to_owned(),
                    ..ModelInfo::default()
                },
            )]),
            ..ProviderInfo::default()
        });
        app.session.sessions = vec![
            Session {
                id: "ses_older".to_owned(),
                model: Some(ModelRef {
                    id: "older_model".to_owned(),
                    provider_id: "fresh_provider".to_owned(),
                    ..ModelRef::default()
                }),
                time: SessionTime {
                    updated: 10,
                    ..SessionTime::default()
                },
                ..Session::default()
            },
            Session {
                id: "ses_latest".to_owned(),
                model: Some(ModelRef {
                    id: "latest_model".to_owned(),
                    provider_id: "fresh_provider".to_owned(),
                    variant: Some("high".to_owned()),
                }),
                time: SessionTime {
                    updated: 20,
                    ..SessionTime::default()
                },
                ..Session::default()
            },
        ];
        let providers = ProviderCatalog {
            providers: vec![ProviderInfo {
                id: "fresh_provider".to_owned(),
                name: "Fresh Provider".to_owned(),
                models: HashMap::from([
                    (
                        "older_model".to_owned(),
                        ModelInfo {
                            id: "older_model".to_owned(),
                            provider_id: "fresh_provider".to_owned(),
                            ..ModelInfo::default()
                        },
                    ),
                    (
                        "latest_model".to_owned(),
                        ModelInfo {
                            id: "latest_model".to_owned(),
                            provider_id: "fresh_provider".to_owned(),
                            ..ModelInfo::default()
                        },
                    ),
                ]),
            }],
            default: HashMap::from([("fresh_provider".to_owned(), "latest_model".to_owned())]),
        };

        let effects = app.update(AppMsg::Api(Box::new(ApiResult::CreatedSession {
            session: Ok(Session {
                id: "ses_new".to_owned(),
                time: SessionTime {
                    updated: 30,
                    ..SessionTime::default()
                },
                ..Session::default()
            }),
            providers: Ok(providers),
        })));

        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::OpenSession(session_id))] if session_id == "ses_new"
        ));
        assert_eq!(app.catalog.providers[0].id, "fresh_provider");
        assert_eq!(
            app.catalog.selected_model.as_ref().map(|model| (
                model.provider_id.as_str(),
                model.id.as_str(),
                model.variant.as_deref(),
            )),
            Some(("fresh_provider", "latest_model", Some("high")))
        );
    }

    #[test]
    fn skill_slash_flow_inserts_the_selected_skill_command() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            ..Session::default()
        });
        app.catalog.skills.push(Skill {
            name: "review".to_owned(),
            description: "Review the current change".to_owned(),
        });

        for character in "/skill".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(app.overlay, Some(OverlayState::Skill { .. })));

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.prompt.composer.text(), "/review ");
        assert!(app.overlay.is_none());
    }

    #[test]
    fn server_slash_command_is_loaded_and_inserted_into_the_composer() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            ..Session::default()
        });
        app.catalog.commands.push(CommandInfo {
            name: "review".to_owned(),
            template: "Review the current change".to_owned(),
            description: Some("Review the current change".to_owned()),
            ..CommandInfo::default()
        });

        for character in "/rev".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        assert!(matches!(app.overlay, Some(OverlayState::Slash { .. })));

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert_eq!(app.prompt.composer.text(), "/review ");
        assert!(app.overlay.is_none());
    }

    #[test]
    fn agent_slash_flow_injects_the_selected_agent_into_the_next_prompt() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            ..Session::default()
        });
        app.catalog.agents.push(AgentInfo {
            name: "build".to_owned(),
            description: "Build the project".to_owned(),
            mode: "primary".to_owned(),
            ..AgentInfo::default()
        });

        for character in "/agent".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(app.overlay, Some(OverlayState::Agent { .. })));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.catalog.selected_agent.as_deref(), Some("build"));

        app.prompt.composer.set_text("run with build");
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::Submit {
                session_id: Some(id),
                request,
            })] if id == "ses_1"
                && request.text_content() == "run with build"
                && request.agent.as_deref() == Some("build")
                && request.model.is_none()
        ));
    }

    #[test]
    fn plan_and_build_slash_commands_switch_the_prompt_mode() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_mode".to_owned(),
            ..Session::default()
        });

        for character in "/plan".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.catalog.selected_agent.as_deref(), Some("plan"));
        assert!(app.prompt.composer.is_empty());
        assert!(app.overlay.is_none());

        for character in "/build".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.catalog.selected_agent.as_deref(), Some("build"));
        assert_eq!(app.notifications.active(), Some("Switched to build mode"));

        app.prompt.composer.set_text("implement the plan");
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::Submit { request, .. })]
                if request.agent.as_deref() == Some("build")
                    && request.text_content() == "implement the plan"
        ));
    }

    #[test]
    fn prompt_mentions_become_structured_file_and_agent_parts() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
            ..Session::default()
        });
        app.catalog.agents.push(AgentInfo {
            name: "explore".to_owned(),
            mode: "subagent".to_owned(),
            ..AgentInfo::default()
        });
        app.prompt
            .composer
            .set_text("Inspect @README.md with @explore.");

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::Submit { request, .. })]
                if request.parts.len() == 3
                    && matches!(
                        &request.parts[1],
                        PromptPart::File {
                            source: Some(PromptFileSource::File { path, .. }),
                            ..
                        } if path == "README.md"
                    )
                    && matches!(
                        &request.parts[2],
                        PromptPart::Agent { name, .. } if name == "explore"
                    )
        ));
    }

    #[test]
    fn attachment_overlay_dispatches_file_read_without_blocking_input() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_attach".to_owned(),
            ..Session::default()
        });

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))));
        assert!(matches!(app.overlay, Some(OverlayState::AttachFile { .. })));
        for character in "notes.md".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ReadAttachment {
                session_id,
                path,
                ..
            })] if session_id == "ses_attach" && path == "notes.md"
        ));
        assert!(app.overlay.is_none());
    }

    #[test]
    fn file_picker_navigates_directories_before_reading_an_attachment() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_picker".to_owned(),
            ..Session::default()
        });

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))));
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Tab,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ListWorkspaceDirectory { path, .. })] if path == "."
        ));

        app.update(AppMsg::Api(Box::new(ApiResult::Directory {
            path: ".".to_owned(),
            result: Ok(vec![WorkspaceFile {
                path: "src".to_owned(),
                is_directory: true,
            }]),
        })));
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ListWorkspaceDirectory { path, .. })] if path == "src"
        ));

        app.update(AppMsg::Api(Box::new(ApiResult::Directory {
            path: "src".to_owned(),
            result: Ok(vec![WorkspaceFile {
                path: "src/main.rs".to_owned(),
                is_directory: false,
            }]),
        })));
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ReadAttachment { session_id, path, .. })]
                if session_id == "ses_picker" && path == "src/main.rs"
        ));
        assert!(app.overlay.is_none());
    }

    #[test]
    fn attached_files_are_sent_and_restored_when_submission_fails() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_attach".to_owned(),
            ..Session::default()
        });
        app.prompt.attachments.push(PromptPart::file(
            "text/plain",
            "data:text/plain;base64,aGVsbG8=",
            Some("notes.txt".to_owned()),
        ));
        app.prompt.composer.set_text("Review the attachment");

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::Submit { request, .. })]
                if request.parts.len() == 2
                    && matches!(&request.parts[1], PromptPart::File { filename: Some(name), .. } if name == "notes.txt")
        ));
        assert!(app.prompt.attachments.is_empty());

        app.update(AppMsg::Api(Box::new(ApiResult::Submitted {
            session: None,
            result: Err("offline".to_owned()),
        })));
        assert_eq!(app.prompt.composer.text(), "Review the attachment");
        assert_eq!(app.prompt.attachments.len(), 1);
    }

    #[test]
    fn collapse_blocks_shortcut_toggles_reasoning_and_tool_parts() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_collapse".to_owned(),
            ..Session::default()
        });
        app.transcript.replace(vec![MessageWithParts {
            info: MessageInfo {
                id: "msg_collapse".to_owned(),
                session_id: "ses_collapse".to_owned(),
                role: "assistant".to_owned(),
                ..MessageInfo::default()
            },
            parts: vec![
                Part {
                    id: "reasoning_collapse".to_owned(),
                    kind: "reasoning".to_owned(),
                    text: Some("private reasoning".to_owned()),
                    ..Part::default()
                },
                Part {
                    id: "tool_collapse".to_owned(),
                    kind: "tool".to_owned(),
                    tool: Some("bash".to_owned()),
                    ..Part::default()
                },
            ],
        }]);

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('b'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))));
        assert_eq!(app.transcript.collapsed_parts.len(), 2);
        assert_eq!(
            app.notifications.active(),
            Some("Collapsed reasoning and tool blocks")
        );

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('b'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))));
        assert!(app.transcript.collapsed_parts.is_empty());
        assert_eq!(
            app.notifications.active(),
            Some("Expanded reasoning and tool blocks")
        );
    }

    #[test]
    fn subtask_and_prompt_options_are_sent_and_restored_as_draft_state() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_prompt_controls".to_owned(),
            ..Session::default()
        });
        app.catalog.agents.push(AgentInfo {
            name: "explore".to_owned(),
            description: "Inspect files".to_owned(),
            mode: "subagent".to_owned(),
            ..AgentInfo::default()
        });

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('t'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))));
        assert!(matches!(app.overlay, Some(OverlayState::Subtask { .. })));
        for character in "Inspect the diff".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            app.prompt.subtasks.as_slice(),
            [PromptPart::Subtask { agent, prompt, .. }]
                if agent == "explore" && prompt == "Inspect the diff"
        ));

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('o'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('n'),
            KeyModifiers::NONE,
        ))));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('f'),
            KeyModifiers::NONE,
        ))));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('s'),
            KeyModifiers::NONE,
        ))));
        for character in "Use concise findings".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        app.prompt.composer.set_text("Review the result");

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::Submit { request, .. })]
                if request.no_reply == Some(true)
                    && request.system.as_deref() == Some("Use concise findings")
                    && matches!(
                        &request.output_format,
                        Some(PromptOutputFormat::JsonSchema { schema, retry_count: None })
                            if schema.get("type").and_then(Value::as_str) == Some("object")
                    )
                    && matches!(&request.parts[1], PromptPart::Subtask { agent, .. } if agent == "explore")
        ));
        assert!(app.prompt.subtasks.is_empty());

        app.update(AppMsg::Api(Box::new(ApiResult::Submitted {
            session: None,
            result: Err("offline".to_owned()),
        })));
        assert_eq!(app.prompt.subtasks.len(), 1);
        assert_eq!(app.prompt.composer.text(), "Review the result");
    }

    #[test]
    fn tool_overrides_cycle_custom_names_and_are_sent_with_prompt() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_tool_overrides".to_owned(),
            ..Session::default()
        });

        let names = crate::dialog::tool_override_names(&app.prompt.options.tool_overrides);
        let bash = names
            .iter()
            .position(|name| name == "bash")
            .expect("built-in bash tool is listed");
        app.overlay = Some(OverlayState::PromptTools { selected: bash });
        for _ in 0..3 {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE,
            ))));
        }
        assert!(!app.prompt.options.tool_overrides.contains_key("bash"));

        let custom = crate::dialog::tool_override_names(&app.prompt.options.tool_overrides).len();
        app.overlay = Some(OverlayState::PromptTools { selected: custom });
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            app.overlay,
            Some(OverlayState::PromptToolName { .. })
        ));
        for character in "mcp_search".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            app.prompt.options.tool_overrides.get("mcp_search"),
            Some(&true)
        );
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ))));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ))));

        app.prompt.composer.set_text("Use the search tool");
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::Submit { request, .. })]
                if request.tools.as_ref().is_some_and(|tools| {
                    tools.len() == 1 && tools.get("mcp_search") == Some(&true)
                })
        ));
    }

    #[test]
    fn variant_slash_flow_selects_a_variant_for_the_next_prompt() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_1".to_owned(),
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
                    provider_id: "provider_1".to_owned(),
                    variants: HashMap::from([("fast".to_owned(), json!({}))]),
                    ..ModelInfo::default()
                },
            )]),
        });

        for character in "/variant".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(app.overlay, Some(OverlayState::Variant { .. })));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        ))));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert_eq!(
            app.catalog
                .selected_model
                .as_ref()
                .and_then(|model| model.variant.as_deref()),
            Some("fast")
        );
    }

    #[test]
    fn slash_overlay_escape_clears_the_command_draft() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.prompt.composer.set_text("/");
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('m'),
            KeyModifiers::NONE,
        ))));
        assert!(app.overlay.is_some());

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ))));
        assert!(app.overlay.is_none());
        assert!(app.prompt.composer.is_empty());
    }

    #[test]
    fn shift_enter_inserts_a_newline_and_tracks_the_cursor_line() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.prompt.composer.set_text("你好");

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::SHIFT,
        ))));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))));

        assert_eq!(app.prompt.composer.text(), "你好\nx");
        assert_eq!(app.prompt.composer.cursor(), (1, 1));
    }

    #[test]
    fn empty_prompt_navigation_controls_transcript_scroll() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.transcript.scroll.observe(100, 20);
        assert_eq!(app.transcript.scroll.offset(), 80);
        assert!(app.transcript.scroll.is_following());

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::PageUp,
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.transcript.scroll.offset(), 61);
        assert!(!app.transcript.scroll.is_following());

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::End,
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.transcript.scroll.offset(), 80);
        assert!(app.transcript.scroll.is_following());

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Home,
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.transcript.scroll.offset(), 0);
        assert!(!app.transcript.scroll.is_following());
    }

    #[test]
    fn mouse_wheel_routes_main_and_sidebar_scroll_independently() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.sidebar_area = Some(Rect {
            x: 80,
            y: 0,
            width: 20,
            height: 20,
        });
        app.transcript.scroll.observe(100, 10);
        app.sidebar_scroll.observe(200, 10);
        // The transcript follows its tail; the sidebar opens at its first line.
        assert_eq!(app.transcript.scroll.offset(), 90);
        assert_eq!(app.sidebar_scroll.offset(), 0);

        // A wheel event inside the sidebar rect moves only the sidebar.
        app.update(AppMsg::Terminal(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 90,
            row: 5,
            modifiers: KeyModifiers::NONE,
        })));
        assert_eq!(app.transcript.scroll.offset(), 90);
        assert_eq!(app.sidebar_scroll.offset(), 3);

        // A wheel event outside it moves only the transcript.
        app.update(AppMsg::Terminal(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 10,
            row: 3,
            modifiers: KeyModifiers::NONE,
        })));
        assert_eq!(app.transcript.scroll.offset(), 87);
        assert_eq!(app.sidebar_scroll.offset(), 3);

        // A row below the sidebar rect is not a sidebar hit.
        app.update(AppMsg::Terminal(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 90,
            row: 40,
            modifiers: KeyModifiers::NONE,
        })));
        assert_eq!(app.sidebar_scroll.offset(), 3);
        assert_eq!(app.transcript.scroll.offset(), 90);
    }

    #[test]
    fn sidebar_wheel_scrolls_down_across_repeated_renders() {
        // Regression: a tail-following sidebar was pulled back to its last line by
        // every `observe` call during render, so downward wheel input never moved.
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_sidebar_repeat".to_owned(),
            title: "Sidebar repeat render".to_owned(),
            ..Session::default()
        });
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .expect("test draw should succeed");

        let area = app.sidebar_area.expect("sidebar should be visible");
        assert!(
            app.sidebar_scroll.max_offset() > 0,
            "sidebar content should exceed its viewport for this test to mean anything"
        );
        assert_eq!(app.sidebar_scroll.offset(), 0);

        app.update(AppMsg::Terminal(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: area.x.saturating_add(1),
            row: area.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        })));
        let after_wheel = app.sidebar_scroll.offset();
        assert!(
            after_wheel > 0,
            "downward wheel input should move the sidebar"
        );

        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .expect("second test draw should succeed");
        assert_eq!(
            app.sidebar_scroll.offset(),
            after_wheel,
            "re-rendering should not reset the sidebar scroll position"
        );
    }

    #[test]
    fn mouse_wheel_uses_sidebar_area_after_a_real_render() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_sidebar_mouse".to_owned(),
            title: "Sidebar mouse test".to_owned(),
            ..Session::default()
        });
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .expect("test draw should succeed");

        let area = app.sidebar_area.expect("sidebar should be visible");
        app.sidebar_scroll
            .observe(200, area.height.saturating_sub(2));
        let wheel = |kind| {
            AppMsg::Terminal(Event::Mouse(MouseEvent {
                kind,
                column: area.x.saturating_add(1),
                row: area.y.saturating_add(1),
                modifiers: KeyModifiers::NONE,
            }))
        };

        app.update(wheel(MouseEventKind::ScrollDown));
        let before = app.sidebar_scroll.offset();
        assert!(before > 0);

        app.update(wheel(MouseEventKind::ScrollUp));
        assert!(app.sidebar_scroll.offset() < before);
        assert_eq!(app.sidebar_area, Some(area));
    }

    /// Drives a real render so selection tests see the pane rects the UI actually
    /// produces rather than ones a test invented.
    fn app_with_rendered_session(text: &str) -> (App, ratatui::layout::Rect) {
        use crate::model::{MessageInfo, MessageWithParts, Part};

        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_selection".to_owned(),
            title: "Selection test".to_owned(),
            ..Session::default()
        });
        app.transcript.replace(vec![MessageWithParts {
            info: MessageInfo {
                id: "msg_selection".to_owned(),
                role: "assistant".to_owned(),
                ..MessageInfo::default()
            },
            parts: vec![Part {
                id: "prt_selection".to_owned(),
                kind: "text".to_owned(),
                text: Some(text.to_owned()),
                ..Part::default()
            }],
        }]);
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .expect("test draw should succeed");
        let sidebar = app.sidebar_area.expect("sidebar should be visible");
        (app, sidebar)
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> AppMsg {
        AppMsg::Terminal(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }))
    }

    // At 80x24 with the sidebar visible, a rendered session lays out as:
    // rows 0-2 header, row 3 transcript top border, rows 4-15 transcript content
    // (row 5 the role marker, row 6 the message text), row 16 bottom border,
    // row 17 prompt top border, rows 18-20 prompt content, row 21 prompt bottom
    // border, row 22 footer. The transcript's inner columns are 1..=57 and the
    // sidebar starts at column 59. These constants come from a printed frame, not
    // from arithmetic on the constraints.
    const TRANSCRIPT_TEXT_ROW: u16 = 6;
    const PROMPT_TEXT_ROW: u16 = 18;
    const PANE_LEFT_COLUMN: u16 = 1;

    #[test]
    fn dragging_over_the_transcript_copies_the_selected_text() {
        let (mut app, _) = app_with_rendered_session("selectable transcript text");

        // The message text is indented by two columns, so "selectable" starts at 3.
        app.update(mouse(
            MouseEventKind::Down(MouseButton::Left),
            3,
            TRANSCRIPT_TEXT_ROW,
        ));
        app.update(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            12,
            TRANSCRIPT_TEXT_ROW,
        ));
        let effects = app.update(mouse(
            MouseEventKind::Up(MouseButton::Left),
            12,
            TRANSCRIPT_TEXT_ROW,
        ));

        let copied = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::CopyToClipboard(text) => Some(text.clone()),
                _ => None,
            })
            .expect("releasing after a drag should copy");
        assert_eq!(copied, "selectable");
    }

    #[test]
    fn dragging_over_the_sidebar_selects_and_copies_nothing() {
        let (mut app, sidebar) = app_with_rendered_session("transcript text");

        app.update(mouse(
            MouseEventKind::Down(MouseButton::Left),
            sidebar.x + 2,
            sidebar.y + 2,
        ));
        app.update(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            sidebar.right() - 2,
            sidebar.y + 4,
        ));
        let effects = app.update(mouse(
            MouseEventKind::Up(MouseButton::Left),
            sidebar.right() - 2,
            sidebar.y + 4,
        ));

        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::CopyToClipboard(_))),
            "the sidebar is not selectable, so nothing should be copied"
        );
        assert!(!app.selection.has_selection());
        assert!(app.selection.highlight_spans().is_empty());
    }

    #[test]
    fn a_drag_from_the_transcript_into_the_sidebar_stays_in_the_transcript() {
        let (mut app, sidebar) = app_with_rendered_session("transcript text worth copying");

        app.update(mouse(
            MouseEventKind::Down(MouseButton::Left),
            PANE_LEFT_COLUMN,
            TRANSCRIPT_TEXT_ROW,
        ));
        app.update(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            sidebar.right() - 1,
            sidebar.y + 2,
        ));

        for span in app.selection.highlight_spans() {
            assert!(
                span.end_column <= sidebar.x,
                "the highlight reached into the sidebar: {span:?}"
            );
        }
    }

    #[test]
    fn dragging_over_the_prompt_copies_its_text() {
        let (mut app, _) = app_with_rendered_session("transcript text");
        app.prompt.composer.set_text("prompt draft text");
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .expect("test draw should succeed");

        // The prompt's text starts at the pane's left edge, unlike the transcript's
        // indented message body.
        app.update(mouse(
            MouseEventKind::Down(MouseButton::Left),
            PANE_LEFT_COLUMN,
            PROMPT_TEXT_ROW,
        ));
        app.update(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            PANE_LEFT_COLUMN + 5,
            PROMPT_TEXT_ROW,
        ));
        let effects = app.update(mouse(
            MouseEventKind::Up(MouseButton::Left),
            PANE_LEFT_COLUMN + 5,
            PROMPT_TEXT_ROW,
        ));

        let copied = effects.iter().find_map(|effect| match effect {
            Effect::CopyToClipboard(text) => Some(text.clone()),
            _ => None,
        });
        assert_eq!(copied.as_deref(), Some("prompt"));
    }

    #[test]
    fn a_click_without_a_drag_copies_nothing() {
        let (mut app, _) = app_with_rendered_session("transcript text");

        app.update(mouse(
            MouseEventKind::Down(MouseButton::Left),
            4,
            TRANSCRIPT_TEXT_ROW,
        ));
        let effects = app.update(mouse(
            MouseEventKind::Up(MouseButton::Left),
            4,
            TRANSCRIPT_TEXT_ROW,
        ));

        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::CopyToClipboard(_))),
            "a plain click is not a selection"
        );
    }

    #[test]
    fn the_wheel_does_not_scroll_while_a_selection_is_being_dragged() {
        let (mut app, _) = app_with_rendered_session("transcript text");
        app.transcript.scroll.observe(200, 10);
        app.update(mouse(MouseEventKind::ScrollDown, 4, TRANSCRIPT_TEXT_ROW));
        let scrolled = app.transcript.scroll.offset();
        assert!(scrolled > 0, "the wheel should scroll when not dragging");

        app.update(mouse(
            MouseEventKind::Down(MouseButton::Left),
            4,
            TRANSCRIPT_TEXT_ROW,
        ));
        app.update(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            10,
            TRANSCRIPT_TEXT_ROW,
        ));
        app.update(mouse(MouseEventKind::ScrollDown, 4, TRANSCRIPT_TEXT_ROW));

        assert_eq!(
            app.transcript.scroll.offset(),
            scrolled,
            "scrolling mid-drag would move the text out from under the anchor"
        );
    }

    #[test]
    fn typing_dismisses_a_selection_without_consuming_the_key() {
        let (mut app, _) = app_with_rendered_session("transcript text");
        app.update(mouse(
            MouseEventKind::Down(MouseButton::Left),
            3,
            TRANSCRIPT_TEXT_ROW,
        ));
        app.update(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            10,
            TRANSCRIPT_TEXT_ROW,
        ));
        app.update(mouse(
            MouseEventKind::Up(MouseButton::Left),
            10,
            TRANSCRIPT_TEXT_ROW,
        ));
        assert!(app.selection.has_selection());

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))));
        assert!(!app.selection.has_selection());
        // The keypress still reached the composer rather than being eaten by the
        // dismissal.
        assert_eq!(app.prompt.composer.text(), "x");
    }

    #[test]
    fn the_selection_highlight_repaints_the_selected_cells() {
        let (mut app, _) = app_with_rendered_session("highlighted transcript text");
        app.update(mouse(
            MouseEventKind::Down(MouseButton::Left),
            3,
            TRANSCRIPT_TEXT_ROW,
        ));
        app.update(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            10,
            TRANSCRIPT_TEXT_ROW,
        ));

        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .expect("test draw should succeed");
        let buffer = terminal.backend().buffer();

        let selected = buffer
            .cell((4, TRANSCRIPT_TEXT_ROW))
            .expect("the cell should be inside the buffer");
        assert_eq!(selected.bg, app.theme.selection_background);
        // A row outside the selection keeps its own background.
        let untouched = buffer
            .cell((4, TRANSCRIPT_TEXT_ROW + 1))
            .expect("the cell should be inside the buffer");
        assert_ne!(untouched.bg, app.theme.selection_background);
    }

    #[test]
    fn ignores_status_events_for_another_session() {
        let mut app = app();
        app.session.current_session = Some(Session {
            id: "ses_current".to_owned(),
            ..Session::default()
        });
        let event = crate::event::ServerEvent::from_json(json!({
            "type": "session.status",
            "properties": {
                "sessionID": "ses_other",
                "status": { "type": "working" }
            }
        }))
        .expect("status event should parse");

        app.handle_server_event(event);

        assert!(!app.runtime.working);
    }

    #[test]
    fn typed_transcript_events_update_and_remove_current_session_only() {
        let mut app = app();
        app.session.current_session = Some(Session {
            id: "ses_current".to_owned(),
            ..Session::default()
        });

        let message = crate::event::ServerEvent::from_json(json!({
            "type": "message.updated",
            "properties": {
                "sessionID": "ses_current",
                "info": {
                    "id": "msg_current",
                    "sessionID": "ses_current",
                    "role": "assistant",
                    "time": { "created": 1 }
                }
            }
        }))
        .expect("message event should parse");
        app.handle_server_event(message);

        let part = crate::event::ServerEvent::from_json(json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_current",
                "part": {
                    "id": "prt_current",
                    "sessionID": "ses_current",
                    "messageID": "msg_current",
                    "type": "text",
                    "text": "hello"
                },
                "time": 1
            }
        }))
        .expect("part event should parse");
        app.handle_server_event(part);

        let delta = crate::event::ServerEvent::from_json(json!({
            "type": "message.part.delta",
            "properties": {
                "sessionID": "ses_current",
                "messageID": "msg_current",
                "partID": "prt_current",
                "field": "text",
                "delta": " world"
            }
        }))
        .expect("delta event should parse");
        app.handle_server_event(delta);

        assert_eq!(app.transcript.len(), 1);
        assert_eq!(
            app.transcript.get("msg_current").unwrap().parts[0]
                .text
                .as_deref(),
            Some("hello world")
        );

        for event in [
            json!({
                "type": "session.next.compaction.started",
                "properties": {
                    "timestamp": 2,
                    "sessionID": "ses_current",
                    "messageID": "cmp_current",
                    "reason": "auto"
                }
            }),
            json!({
                "type": "session.next.compaction.delta",
                "properties": {
                    "timestamp": 3,
                    "sessionID": "ses_current",
                    "messageID": "cmp_current",
                    "text": "draft"
                }
            }),
            json!({
                "type": "session.next.compaction.ended",
                "properties": {
                    "timestamp": 4,
                    "sessionID": "ses_current",
                    "messageID": "cmp_current",
                    "reason": "auto",
                    "text": "Summary",
                    "recent": "Recent context"
                }
            }),
        ] {
            app.handle_server_event(
                crate::event::ServerEvent::from_json(event).expect("compaction event should parse"),
            );
        }
        let compaction = app
            .transcript
            .get("cmp_current")
            .expect("compaction should be projected");
        assert_eq!(compaction.parts[0].kind, "compaction");
        assert_eq!(compaction.parts[0].text.as_deref(), Some("Summary"));
        assert_eq!(
            compaction.parts[0]
                .state
                .as_ref()
                .and_then(|state| state.get("recent"))
                .and_then(Value::as_str),
            Some("Recent context")
        );
        assert!(!app.runtime.working);

        for event in [
            json!({
                "type": "message.updated",
                "properties": {
                    "sessionID": "ses_other",
                    "info": {
                        "id": "msg_other",
                        "sessionID": "ses_other",
                        "role": "assistant"
                    }
                }
            }),
            json!({
                "type": "message.part.updated",
                "properties": {
                    "sessionID": "ses_other",
                    "part": {
                        "id": "prt_other",
                        "sessionID": "ses_other",
                        "messageID": "msg_other",
                        "type": "text",
                        "text": "other"
                    },
                    "time": 1
                }
            }),
            json!({
                "type": "message.part.delta",
                "properties": {
                    "sessionID": "ses_other",
                    "messageID": "msg_current",
                    "partID": "prt_current",
                    "field": "text",
                    "delta": " polluted"
                }
            }),
            json!({
                "type": "message.part.removed",
                "properties": {
                    "sessionID": "ses_other",
                    "messageID": "msg_current",
                    "partID": "prt_current"
                }
            }),
        ] {
            app.handle_server_event(
                crate::event::ServerEvent::from_json(event).expect("scoped event should parse"),
            );
        }

        assert_eq!(app.transcript.len(), 2);
        assert_eq!(app.transcript.get("msg_current").unwrap().parts.len(), 1);
        assert_eq!(
            app.transcript.get("msg_current").unwrap().parts[0]
                .text
                .as_deref(),
            Some("hello world")
        );

        let remove_part = crate::event::ServerEvent::from_json(json!({
            "type": "message.part.removed",
            "properties": {
                "sessionID": "ses_current",
                "messageID": "msg_current",
                "partID": "prt_current"
            }
        }))
        .expect("part removal event should parse");
        app.handle_server_event(remove_part);
        assert!(app.transcript.get("msg_current").unwrap().parts.is_empty());

        let remove_message = crate::event::ServerEvent::from_json(json!({
            "type": "message.removed",
            "properties": {
                "sessionID": "ses_current",
                "messageID": "msg_current"
            }
        }))
        .expect("message removal event should parse");
        app.handle_server_event(remove_message);
        assert_eq!(app.transcript.len(), 1);
        assert!(app.transcript.get("cmp_current").is_some());
    }

    #[test]
    fn control_session_events_update_runtime_state_and_transcript() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_control".to_owned(),
            ..Session::default()
        });

        for value in [
            json!({
                "type": "session.next.agent.switched",
                "properties": {
                    "timestamp": 10,
                    "sessionID": "ses_control",
                    "messageID": "evt_agent",
                    "agent": "build"
                }
            }),
            json!({
                "type": "session.next.model.switched",
                "properties": {
                    "timestamp": 11,
                    "sessionID": "ses_control",
                    "messageID": "evt_model",
                    "model": { "id": "model_1", "providerID": "provider_1" }
                }
            }),
            json!({
                "type": "session.next.prompted",
                "properties": {
                    "timestamp": 12,
                    "sessionID": "ses_control",
                    "messageID": "msg_user",
                    "prompt": { "text": "Review this" },
                    "delivery": "steer"
                }
            }),
            json!({
                "type": "session.next.context.updated",
                "properties": {
                    "timestamp": 13,
                    "sessionID": "ses_control",
                    "messageID": "msg_context",
                    "text": "Context updated"
                }
            }),
            json!({
                "type": "session.next.synthetic",
                "properties": {
                    "timestamp": 14,
                    "sessionID": "ses_control",
                    "messageID": "msg_synthetic",
                    "text": "Synthetic note"
                }
            }),
            json!({
                "type": "session.next.prompt.admitted",
                "properties": {
                    "timestamp": 15,
                    "sessionID": "ses_control",
                    "messageID": "msg_user",
                    "prompt": { "text": "Review this" },
                    "delivery": "steer"
                }
            }),
            json!({
                "type": "session.next.retried",
                "properties": {
                    "timestamp": 16,
                    "sessionID": "ses_control",
                    "attempt": 2,
                    "error": {
                        "message": "temporary failure",
                        "isRetryable": true
                    }
                }
            }),
        ] {
            app.handle_server_event(
                crate::event::ServerEvent::from_json(value).expect("control event should parse"),
            );
        }

        let moved = crate::event::ServerEvent::from_json(json!({
            "type": "session.next.moved",
            "properties": {
                "timestamp": 17,
                "sessionID": "ses_control",
                "location": { "directory": "E:/workspace", "workspaceID": "ws_1" }
            }
        }))
        .expect("move event should parse");
        app.handle_server_event(moved);

        let staged = crate::event::ServerEvent::from_json(json!({
            "type": "session.next.revert.staged",
            "properties": {
                "timestamp": 18,
                "sessionID": "ses_control",
                "revert": {
                    "messageID": "msg_user",
                    "files": [{
                        "path": "README.md",
                        "status": "modified",
                        "additions": 1,
                        "deletions": 0,
                        "patch": "@@"
                    }]
                }
            }
        }))
        .expect("revert event should parse");
        app.handle_server_event(staged);

        assert_eq!(
            app.session
                .current_session
                .as_ref()
                .and_then(|session| session.agent.as_deref()),
            Some("build")
        );
        assert_eq!(
            app.session
                .current_session
                .as_ref()
                .and_then(|session| session.model.as_ref())
                .map(|model| model.id.as_str()),
            Some("model_1")
        );
        assert_eq!(
            app.session
                .current_session
                .as_ref()
                .and_then(|session| session.directory.as_deref()),
            Some("E:/workspace")
        );
        assert_eq!(
            app.session.current_session.as_ref().unwrap().time.updated,
            18
        );
        assert_eq!(
            app.transcript.get("msg_user").unwrap().parts[0]
                .text
                .as_deref(),
            Some("Review this")
        );
        assert_eq!(
            app.transcript.get("msg_context").unwrap().info.role,
            "system"
        );
        assert_eq!(
            app.transcript.get("msg_synthetic").unwrap().info.role,
            "synthetic"
        );
        assert!(app.integrations.revert_state.is_some());
        assert!(app.runtime.working);

        let cleared = crate::event::ServerEvent::from_json(json!({
            "type": "session.next.revert.cleared",
            "properties": { "timestamp": 19, "sessionID": "ses_control" }
        }))
        .expect("revert clear event should parse");
        app.handle_server_event(cleared);
        assert!(app.integrations.revert_state.is_none());
    }

    #[test]
    fn live_events_project_text_tools_and_shells_into_the_transcript() {
        let mut app = app();
        app.session.current_session = Some(Session {
            id: "ses_current".to_owned(),
            ..Session::default()
        });

        for value in [
            json!({
                "type": "session.next.step.started",
                "properties": {
                    "timestamp": 1,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "agent": "build",
                    "model": { "id": "model_1", "providerID": "provider_1" },
                    "snapshot": "snap_start"
                }
            }),
            json!({
                "type": "session.next.text.started",
                "properties": {
                    "timestamp": 2,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "textID": "text_1"
                }
            }),
            json!({
                "type": "session.next.text.delta",
                "properties": {
                    "timestamp": 3,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "textID": "text_1",
                    "delta": "hello"
                }
            }),
            json!({
                "type": "session.next.text.ended",
                "properties": {
                    "timestamp": 4,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "textID": "text_1",
                    "text": "hello world"
                }
            }),
            json!({
                "type": "session.next.reasoning.started",
                "properties": {
                    "timestamp": 5,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "reasoningID": "reason_1"
                }
            }),
            json!({
                "type": "session.next.reasoning.delta",
                "properties": {
                    "timestamp": 6,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "reasoningID": "reason_1",
                    "delta": "checking"
                }
            }),
            json!({
                "type": "session.next.reasoning.ended",
                "properties": {
                    "timestamp": 7,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "reasoningID": "reason_1",
                    "text": "checking the workspace"
                }
            }),
            json!({
                "type": "session.next.tool.input.started",
                "properties": {
                    "timestamp": 8,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "callID": "call_tool",
                    "name": "bash"
                }
            }),
            json!({
                "type": "session.next.tool.input.delta",
                "properties": {
                    "timestamp": 9,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "callID": "call_tool",
                    "delta": "{\"command\":\"pwd\"}"
                }
            }),
            json!({
                "type": "session.next.tool.input.ended",
                "properties": {
                    "timestamp": 10,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "callID": "call_tool",
                    "text": "{\"command\":\"pwd\"}"
                }
            }),
            json!({
                "type": "session.next.tool.called",
                "properties": {
                    "timestamp": 11,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "callID": "call_tool",
                    "tool": "bash",
                    "input": { "command": "pwd" },
                    "provider": { "executed": true }
                }
            }),
            json!({
                "type": "session.next.tool.progress",
                "properties": {
                    "timestamp": 12,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "callID": "call_tool",
                    "structured": { "exitCode": 0 },
                    "content": [{ "type": "text", "text": "workspace" }]
                }
            }),
            json!({
                "type": "session.next.tool.success",
                "properties": {
                    "timestamp": 13,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "callID": "call_tool",
                    "structured": { "exitCode": 0 },
                    "content": [{ "type": "text", "text": "workspace" }],
                    "outputPaths": [],
                    "provider": { "executed": true }
                }
            }),
            json!({
                "type": "session.next.shell.started",
                "properties": {
                    "timestamp": 14,
                    "sessionID": "ses_current",
                    "messageID": "msg_shell",
                    "callID": "shell_1",
                    "command": "pwd"
                }
            }),
            json!({
                "type": "session.next.shell.ended",
                "properties": {
                    "timestamp": 15,
                    "sessionID": "ses_current",
                    "callID": "shell_1",
                    "output": "E:/project"
                }
            }),
        ] {
            app.handle_server_event(
                crate::event::ServerEvent::from_json(value).expect("live event should parse"),
            );
        }

        let effects = app.handle_server_event(
            crate::event::ServerEvent::from_json(json!({
                "type": "session.next.step.ended",
                "properties": {
                    "timestamp": 16,
                    "sessionID": "ses_current",
                    "assistantMessageID": "msg_assistant",
                    "finish": "stop",
                    "cost": 0.25,
                    "tokens": {
                        "input": 10,
                        "output": 20,
                        "reasoning": 1,
                        "cache": { "read": 4, "write": 0 }
                    }
                }
            }))
            .expect("step end should parse"),
        );

        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::RefreshCurrent(id))] if id == "ses_current"
        ));
        assert!(!app.runtime.working);
        assert_eq!(app.runtime.response.input_tokens(), 10);
        assert_eq!(app.runtime.response.output_tokens(), 20);
        assert!(app.runtime.response.elapsed() >= std::time::Duration::ZERO);

        let assistant = app
            .transcript
            .get("msg_assistant")
            .expect("assistant projection should exist");
        assert_eq!(assistant.info.model_id, "model_1");
        assert_eq!(assistant.info.finish.as_deref(), Some("stop"));
        assert_eq!(assistant.info.tokens.output, 20);
        assert_eq!(
            assistant.info.snapshot.as_ref().unwrap()["start"],
            "snap_start"
        );
        assert_eq!(
            assistant
                .parts
                .iter()
                .find(|part| part.id == "text_1")
                .and_then(|part| part.text.as_deref()),
            Some("hello world")
        );
        assert_eq!(
            assistant
                .parts
                .iter()
                .find(|part| part.id == "reason_1")
                .and_then(|part| part.text.as_deref()),
            Some("checking the workspace")
        );
        assert_eq!(
            assistant
                .parts
                .iter()
                .find(|part| part.id == "call_tool")
                .and_then(|part| part.state.as_ref())
                .and_then(|state| state.get("status"))
                .and_then(Value::as_str),
            Some("completed")
        );

        let shell = app
            .transcript
            .get("msg_shell")
            .expect("shell projection should exist");
        assert_eq!(shell.info.role, "shell");
        assert_eq!(shell.parts[0].command.as_deref(), Some("pwd"));
        assert_eq!(shell.parts[0].text.as_deref(), Some("E:/project"));

        app.handle_server_event(
            crate::event::ServerEvent::from_json(json!({
                "type": "session.next.text.delta",
                "properties": {
                    "timestamp": 17,
                    "sessionID": "ses_other",
                    "assistantMessageID": "msg_other",
                    "textID": "text_other",
                    "delta": "ignored"
                }
            }))
            .expect("other session event should parse"),
        );
        assert!(app.transcript.get("msg_other").is_none());
    }

    #[test]
    fn ignores_a_late_result_for_an_older_session_selection() {
        let mut app = app();
        app.session.sessions = vec![
            Session {
                id: "ses_old".to_owned(),
                ..Session::default()
            },
            Session {
                id: "ses_new".to_owned(),
                ..Session::default()
            },
        ];
        app.initial_effects(Some("ses_new"));

        app.update(AppMsg::Api(Box::new(ApiResult::OpenedSession(Ok(
            SessionSnapshot {
                session: Session {
                    id: "ses_old".to_owned(),
                    ..Session::default()
                },
                messages: Vec::new(),
            },
        )))));

        assert_eq!(app.session.screen, Screen::Home);
        assert!(app.session.current_session.is_none());
    }

    #[test]
    fn permission_request_is_visible_and_allow_once_is_an_effect() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_current".to_owned(),
            ..Session::default()
        });
        let event = crate::event::ServerEvent::from_json(json!({
            "type": "permission.asked",
            "properties": {
                "id": "per_1",
                "sessionID": "ses_current",
                "permission": "bash",
                "patterns": ["git status"],
                "metadata": {},
                "always": []
            }
        }))
        .expect("permission event should parse");
        app.handle_server_event(event);

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
        ))));

        assert!(app.current_permission().is_some());
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ReplyPermission {
                request_id,
                reply: PermissionReply::Once,
                message: None
            })] if request_id == "per_1"
        ));
    }

    #[test]
    fn question_options_can_be_selected_and_submitted() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_current".to_owned(),
            ..Session::default()
        });
        let event = crate::event::ServerEvent::from_json(json!({
            "type": "question.asked",
            "properties": {
                "id": "que_1",
                "sessionID": "ses_current",
                "questions": [{
                    "question": "Which path?",
                    "header": "Path",
                    "options": [
                        {"label": "A", "description": "First"},
                        {"label": "B", "description": "Second"}
                    ]
                }]
            }
        }))
        .expect("question event should parse");
        app.handle_server_event(event);
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('2'),
            KeyModifiers::NONE,
        ))));
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ReplyQuestion { request_id, answers })]
                if request_id == "que_1" && answers == &vec![vec!["B".to_owned()]]
        ));
    }

    #[test]
    fn rename_session_uses_background_api_effect_and_updates_the_list() {
        let mut app = app();
        app.session.sessions.push(Session {
            id: "ses_rename".to_owned(),
            title: "Old title".to_owned(),
            ..Session::default()
        });

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::F(2),
            KeyModifiers::NONE,
        ))));
        app.overlay = Some(OverlayState::RenameSession {
            value: "New title".to_owned(),
        });
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::RenameSession { session_id, title })]
                if session_id == "ses_rename" && title == "New title"
        ));

        app.update(AppMsg::Api(Box::new(ApiResult::RenamedSession(Ok(
            Session {
                id: "ses_rename".to_owned(),
                title: "New title".to_owned(),
                ..Session::default()
            },
        )))));
        assert_eq!(app.session.sessions[0].title, "New title");
    }

    #[test]
    fn session_sharing_exposes_the_link_and_can_unshare_from_the_overlay() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_share".to_owned(),
            title: "Share me".to_owned(),
            ..Session::default()
        });
        app.session
            .sessions
            .push(app.session.current_session.clone().expect("session"));
        app.overlay = Some(OverlayState::CommandPalette {
            query: "Share Session".to_owned(),
            selected: 0,
        });

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ShareSession(session_id))] if session_id == "ses_share"
        ));

        app.update(AppMsg::Api(Box::new(ApiResult::SharedSession(Ok(
            Session {
                id: "ses_share".to_owned(),
                share: Some(SessionShare {
                    url: "https://share.example/ses_share".to_owned(),
                }),
                ..Session::default()
            },
        )))));
        assert_eq!(
            app.overlay,
            Some(OverlayState::SessionShare {
                url: "https://share.example/ses_share".to_owned(),
            })
        );
        assert_eq!(
            app.session
                .current_session
                .as_ref()
                .and_then(Session::share_url),
            Some("https://share.example/ses_share")
        );

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::UnshareSession(session_id))] if session_id == "ses_share"
        ));

        app.update(AppMsg::Api(Box::new(ApiResult::UnsharedSession(Ok(
            Session {
                id: "ses_share".to_owned(),
                ..Session::default()
            },
        )))));
        assert!(app.overlay.is_none());
        assert!(
            app.session
                .current_session
                .as_ref()
                .and_then(Session::share_url)
                .is_none()
        );
    }

    #[test]
    fn archive_workflow_removes_active_sessions_and_restores_archived_sessions() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_archive".to_owned(),
            title: "Archive me".to_owned(),
            ..Session::default()
        });
        app.session
            .sessions
            .push(app.session.current_session.clone().expect("session"));
        app.overlay = Some(OverlayState::CommandPalette {
            query: "Archive Session".to_owned(),
            selected: 0,
        });

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            app.overlay,
            Some(OverlayState::ArchiveSession {
                session_id: "ses_archive".to_owned(),
                restore: false,
            })
        );

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ArchiveSession { session_id, archived })]
                if session_id == "ses_archive" && *archived
        ));

        let effects = app.update(AppMsg::Api(Box::new(ApiResult::ArchivedSession {
            archived: true,
            result: Ok(Session {
                id: "ses_archive".to_owned(),
                time: SessionTime {
                    archived: Some(42),
                    ..SessionTime::default()
                },
                ..Session::default()
            }),
        })));
        assert_eq!(app.session.screen, Screen::Home);
        assert!(app.session.current_session.is_none());
        assert!(app.session.sessions.is_empty());
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ListSessions)]
        ));

        app.session.show_archived = true;
        app.session.sessions.push(Session {
            id: "ses_archive".to_owned(),
            time: SessionTime {
                archived: Some(42),
                ..SessionTime::default()
            },
            ..Session::default()
        });
        app.overlay = Some(OverlayState::CommandPalette {
            query: "Restore Session".to_owned(),
            selected: 0,
        });
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            app.overlay,
            Some(OverlayState::ArchiveSession {
                session_id: "ses_archive".to_owned(),
                restore: true,
            })
        );
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ArchiveSession { session_id, archived })]
                if session_id == "ses_archive" && !*archived
        ));

        let effects = app.update(AppMsg::Api(Box::new(ApiResult::ArchivedSession {
            archived: false,
            result: Ok(Session {
                id: "ses_archive".to_owned(),
                ..Session::default()
            }),
        })));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ListArchivedSessions)]
        ));
        assert!(app.session.sessions.is_empty());
    }

    #[test]
    fn move_session_opens_an_editable_dialog_and_updates_location_after_success() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_move".to_owned(),
            title: "Move me".to_owned(),
            directory: Some("E:/source".to_owned()),
            ..Session::default()
        });
        app.session
            .sessions
            .push(app.session.current_session.clone().expect("session"));
        app.overlay = Some(OverlayState::CommandPalette {
            query: "Move Session".to_owned(),
            selected: 0,
        });

        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            app.overlay,
            Some(OverlayState::MoveSession {
                session_id: "ses_move".to_owned(),
                destination: String::new(),
                move_changes: false,
            })
        );
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            app.overlay,
            Some(OverlayState::MoveSession {
                ref destination,
                move_changes: false,
                ..
            }) if destination.is_empty()
        ));

        app.update(AppMsg::Terminal(Event::Paste("E:/target".to_owned())));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Tab,
            KeyModifiers::NONE,
        ))));
        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::MoveSession {
                session_id,
                destination,
                move_changes,
            })] if session_id == "ses_move"
                && destination == "E:/target"
                && *move_changes
        ));

        let effects = app.update(AppMsg::Api(Box::new(ApiResult::MovedSession {
            session_id: "ses_move".to_owned(),
            destination: "E:/target".to_owned(),
            result: Ok(()),
        })));
        assert_eq!(
            app.session
                .current_session
                .as_ref()
                .and_then(Session::directory),
            Some("E:/target")
        );
        assert_eq!(
            app.session.sessions[0].directory.as_deref(),
            Some("E:/target")
        );
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::ListSessions)]
        ));
    }

    #[test]
    fn delete_session_requires_confirmation_and_returns_home_after_success() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_delete".to_owned(),
            title: "Remove me".to_owned(),
            ..Session::default()
        });
        app.session
            .sessions
            .push(app.session.current_session.clone().expect("session"));
        app.open_delete_session();

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::DeleteSession(session_id))] if session_id == "ses_delete"
        ));

        app.update(AppMsg::Api(Box::new(ApiResult::DeletedSession {
            session_id: "ses_delete".to_owned(),
            result: Ok(()),
        })));
        assert_eq!(app.session.screen, Screen::Home);
        assert!(app.session.current_session.is_none());
        assert!(app.session.sessions.is_empty());
    }

    #[test]
    fn mention_overlay_replaces_a_file_token_at_the_unicode_cursor() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_mentions".to_owned(),
            ..Session::default()
        });
        app.catalog.workspace_files.push(WorkspaceFile {
            path: "src/main.rs".to_owned(),
            is_directory: false,
        });

        for character in "检查 @src".chars() {
            app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
        }
        assert!(matches!(
            app.overlay,
            Some(OverlayState::Mention { ref query, .. }) if query == "src"
        ));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert_eq!(app.prompt.composer.text(), "检查 @src/main.rs ");
        assert!(app.overlay.is_none());
    }

    #[test]
    fn mention_search_is_async_and_ignores_results_for_an_older_query() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_server_mentions".to_owned(),
            ..Session::default()
        });

        for character in "@src".chars() {
            let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            ))));
            if character == 'c' {
                assert!(matches!(
                    effects.as_slice(),
                    [Effect::Api(ApiRequest::SearchWorkspaceFiles { query })] if query == "src"
                ));
            }
        }

        app.update(AppMsg::Api(Box::new(ApiResult::SearchedFiles {
            query: "sr".to_owned(),
            result: Ok(vec![WorkspaceFile {
                path: "src/old.rs".to_owned(),
                is_directory: false,
            }]),
        })));
        assert!(app.catalog.server_workspace_files.is_empty());

        app.update(AppMsg::Api(Box::new(ApiResult::SearchedFiles {
            query: "src".to_owned(),
            result: Ok(vec![WorkspaceFile {
                path: "src/main.rs".to_owned(),
                is_directory: false,
            }]),
        })));
        app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));

        assert_eq!(app.prompt.composer.text(), "@src/main.rs ");
    }

    #[test]
    fn ctrl_e_returns_a_local_markdown_export_effect() {
        let mut app = app();
        app.session.screen = Screen::Session;
        app.session.current_session = Some(Session {
            id: "ses_export".to_owned(),
            title: "Export me".to_owned(),
            ..Session::default()
        });

        let effects = app.update(AppMsg::Terminal(Event::Key(KeyEvent::new(
            KeyCode::Char('e'),
            KeyModifiers::CONTROL,
        ))));
        assert!(matches!(
            effects.as_slice(),
            [Effect::ExportSession { session_id, title, content }]
                if session_id == "ses_export"
                    && title == "Export me"
                    && content.contains("# Export me")
        ));
    }
}
