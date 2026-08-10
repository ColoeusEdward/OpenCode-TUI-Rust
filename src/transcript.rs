use crate::model::{MessageInfo, MessageTime, MessageWithParts, ModelRef, Part, TokenUsage};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::Write;

#[derive(Debug, Clone, Default)]
pub struct TranscriptStore {
    messages: Vec<MessageWithParts>,
    message_index: HashMap<String, usize>,
    part_index: HashMap<(String, String), usize>,
    call_index: HashMap<(String, String), (usize, usize)>,
    shell_index: HashMap<String, (usize, usize)>,
}

impl TranscriptStore {
    pub fn replace(&mut self, mut messages: Vec<MessageWithParts>) {
        messages.sort_by(message_order);
        self.messages = messages;
        self.rebuild_indices();
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.message_index.clear();
        self.part_index.clear();
        self.call_index.clear();
        self.shell_index.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, MessageWithParts> {
        self.messages.iter()
    }

    pub fn export_markdown(&self) -> String {
        let mut output = String::new();
        for message in &self.messages {
            let _ = writeln!(output, "## {} `{}`\n", message.info.role, message.info.id);
            for part in &message.parts {
                match part.kind.as_str() {
                    "text" => append_export_block(&mut output, part.text.as_deref().unwrap_or("")),
                    "reasoning" => {
                        let _ = writeln!(output, "<details>\n<summary>Reasoning</summary>\n");
                        append_export_block(&mut output, part.text.as_deref().unwrap_or(""));
                        output.push_str("</details>\n\n");
                    }
                    "tool" => {
                        let name = part.tool.as_deref().unwrap_or("tool");
                        let status = part
                            .state
                            .as_ref()
                            .and_then(|state| state.get("status"))
                            .and_then(Value::as_str)
                            .unwrap_or("unknown");
                        let _ = writeln!(output, "### Tool `{name}` [{status}]\n");
                        if let Some(state) = &part.state {
                            let rendered = serde_json::to_string_pretty(state)
                                .unwrap_or_else(|_| state.to_string());
                            output.push_str("```json\n");
                            output.push_str(&rendered);
                            output.push_str("\n```\n\n");
                        }
                    }
                    "compaction" => {
                        let reason = part
                            .state
                            .as_ref()
                            .and_then(|state| state.get("reason"))
                            .and_then(Value::as_str)
                            .unwrap_or("unknown");
                        let _ = writeln!(output, "### Context compacted ({reason})\n");
                        append_export_block(&mut output, part.text.as_deref().unwrap_or(""));
                        if let Some(recent) = part
                            .state
                            .as_ref()
                            .and_then(|state| state.get("recent"))
                            .and_then(Value::as_str)
                            .filter(|recent| !recent.is_empty())
                        {
                            let _ = writeln!(output, "Recent context: {recent}\n");
                        }
                    }
                    "shell" => {
                        let command = part.command.as_deref().unwrap_or("shell");
                        let _ = writeln!(output, "### Shell `$ {command}`\n");
                        append_export_block(&mut output, part.text.as_deref().unwrap_or(""));
                    }
                    _ => {
                        append_export_block(&mut output, part.text.as_deref().unwrap_or(""));
                    }
                }
            }
        }
        output
    }

    pub fn append_event_message(
        &mut self,
        session_id: &str,
        message_id: &str,
        role: &str,
        text: &str,
        created: i64,
        state: Option<Value>,
    ) {
        self.ensure_message(MessageInfo {
            id: message_id.to_owned(),
            session_id: session_id.to_owned(),
            role: role.to_owned(),
            time: MessageTime {
                created,
                ..MessageTime::default()
            },
            ..MessageInfo::default()
        });
        self.upsert_part(Part {
            id: format!("{message_id}:event"),
            session_id: session_id.to_owned(),
            message_id: message_id.to_owned(),
            kind: "text".to_owned(),
            text: Some(text.to_owned()),
            state,
            ..Part::default()
        });
    }

    #[cfg(test)]
    pub fn get(&self, message_id: &str) -> Option<&MessageWithParts> {
        self.message_index
            .get(message_id)
            .and_then(|index| self.messages.get(*index))
    }

    pub fn upsert_message_info(&mut self, info: MessageInfo) {
        if let Some(index) = self.message_index.get(&info.id).copied() {
            self.messages[index].info = info;
        } else {
            self.messages.push(MessageWithParts {
                info,
                parts: Vec::new(),
            });
        }
        self.messages.sort_by(message_order);
        self.rebuild_indices();
    }

    pub fn start_assistant(
        &mut self,
        session_id: &str,
        message_id: &str,
        agent: &str,
        model: &ModelRef,
        snapshot: Option<&str>,
        created: i64,
    ) {
        self.ensure_assistant_message(session_id, message_id, created);
        if let Some(index) = self.message_index.get(message_id).copied() {
            let info = &mut self.messages[index].info;
            info.agent = agent.to_owned();
            info.provider_id = model.provider_id.clone();
            info.model_id = model.id.clone();
            if let Some(snapshot) = snapshot {
                info.snapshot = Some(json!({ "start": snapshot }));
            }
        }
    }

    pub fn start_text(&mut self, session_id: &str, message_id: &str, part_id: &str, created: i64) {
        self.ensure_assistant_message(session_id, message_id, created);
        self.upsert_text_part(session_id, message_id, part_id, "text");
    }

    pub fn append_text_delta(
        &mut self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
        created: i64,
        delta: &str,
    ) -> bool {
        if !self.has_part(message_id, part_id) {
            self.start_text(session_id, message_id, part_id, created);
        }
        self.apply_part_delta(message_id, part_id, "text", delta)
    }

    pub fn finish_text(
        &mut self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
        created: i64,
        text: &str,
    ) -> bool {
        if !self.has_part(message_id, part_id) {
            self.start_text(session_id, message_id, part_id, created);
        }
        let Some((message_index, part_index)) = self.part_location(message_id, part_id) else {
            return false;
        };
        self.messages[message_index].parts[part_index].text = Some(text.to_owned());
        true
    }

    pub fn start_reasoning(
        &mut self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
        created: i64,
    ) {
        self.ensure_assistant_message(session_id, message_id, created);
        self.upsert_text_part(session_id, message_id, part_id, "reasoning");
    }

    pub fn append_reasoning_delta(
        &mut self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
        created: i64,
        delta: &str,
    ) -> bool {
        if !self.has_part(message_id, part_id) {
            self.start_reasoning(session_id, message_id, part_id, created);
        }
        self.apply_part_delta(message_id, part_id, "text", delta)
    }

    pub fn finish_reasoning(
        &mut self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
        created: i64,
        text: &str,
    ) -> bool {
        if !self.has_part(message_id, part_id) {
            self.start_reasoning(session_id, message_id, part_id, created);
        }
        let Some((message_index, part_index)) = self.part_location(message_id, part_id) else {
            return false;
        };
        self.messages[message_index].parts[part_index].text = Some(text.to_owned());
        true
    }

    pub fn start_compaction(
        &mut self,
        session_id: &str,
        message_id: &str,
        reason: &str,
        created: i64,
    ) {
        self.ensure_message(MessageInfo {
            id: message_id.to_owned(),
            session_id: session_id.to_owned(),
            role: "system".to_owned(),
            time: MessageTime {
                created,
                ..MessageTime::default()
            },
            ..MessageInfo::default()
        });
        self.upsert_part(Part {
            id: message_id.to_owned(),
            session_id: session_id.to_owned(),
            message_id: message_id.to_owned(),
            kind: "compaction".to_owned(),
            text: Some(String::new()),
            state: Some(json!({ "status": "running", "reason": reason })),
            ..Part::default()
        });
    }

    pub fn append_compaction_delta(
        &mut self,
        message_id: &str,
        timestamp: i64,
        delta: &str,
    ) -> bool {
        let Some((message_index, part_index)) = self.part_location(message_id, message_id) else {
            return false;
        };
        let part = &mut self.messages[message_index].parts[part_index];
        part.text.get_or_insert_with(String::new).push_str(delta);
        if let Some(state) = part.state.as_mut() {
            state["updatedAt"] = json!(timestamp);
        }
        true
    }

    pub fn finish_compaction(
        &mut self,
        session_id: &str,
        message_id: &str,
        reason: &str,
        text: &str,
        recent: &str,
        completed: i64,
    ) -> bool {
        if self.part_location(message_id, message_id).is_none() {
            self.start_compaction(session_id, message_id, reason, completed);
        }
        let Some((message_index, part_index)) = self.part_location(message_id, message_id) else {
            return false;
        };
        let message = &mut self.messages[message_index];
        message.info.time.completed = Some(completed);
        let part = &mut message.parts[part_index];
        part.text = Some(text.to_owned());
        part.state = Some(json!({
            "status": "completed",
            "reason": reason,
            "recent": recent,
            "updatedAt": completed,
        }));
        true
    }

    pub fn start_tool(
        &mut self,
        session_id: &str,
        message_id: &str,
        call_id: &str,
        name: &str,
        created: i64,
    ) {
        self.ensure_assistant_message(session_id, message_id, created);
        if self.call_location(message_id, call_id).is_none() {
            self.upsert_part(Part {
                id: call_id.to_owned(),
                session_id: session_id.to_owned(),
                message_id: message_id.to_owned(),
                kind: "tool".to_owned(),
                tool: Some(name.to_owned()),
                call_id: Some(call_id.to_owned()),
                state: Some(json!({ "status": "pending", "input": "" })),
                ..Part::default()
            });
        }
    }

    pub fn tool_state(&self, message_id: &str, call_id: &str) -> Option<&Value> {
        let (message_index, part_index) = self.call_location(message_id, call_id)?;
        self.messages[message_index].parts[part_index]
            .state
            .as_ref()
    }

    pub fn set_tool_state(&mut self, message_id: &str, call_id: &str, state: Value) -> bool {
        let Some((message_index, part_index)) = self.call_location(message_id, call_id) else {
            return false;
        };
        self.messages[message_index].parts[part_index].state = Some(state);
        true
    }

    pub fn append_tool_input(
        &mut self,
        message_id: &str,
        call_id: &str,
        timestamp: i64,
        delta: &str,
    ) -> bool {
        let Some((message_index, part_index)) = self.call_location(message_id, call_id) else {
            return false;
        };
        let Some(state) = self.messages[message_index].parts[part_index]
            .state
            .as_mut()
        else {
            return false;
        };
        let Some(input) = state.get("input").and_then(Value::as_str) else {
            return false;
        };
        let mut input = input.to_owned();
        input.push_str(delta);
        state["input"] = Value::String(input);
        state["updatedAt"] = json!(timestamp);
        true
    }

    pub fn set_tool_input(
        &mut self,
        message_id: &str,
        call_id: &str,
        timestamp: i64,
        input: &str,
    ) -> bool {
        let Some((message_index, part_index)) = self.call_location(message_id, call_id) else {
            return false;
        };
        let Some(state) = self.messages[message_index].parts[part_index]
            .state
            .as_mut()
        else {
            return false;
        };
        state["input"] = Value::String(input.to_owned());
        state["updatedAt"] = json!(timestamp);
        true
    }

    pub fn start_shell(
        &mut self,
        session_id: &str,
        message_id: &str,
        call_id: &str,
        command: &str,
        created: i64,
    ) {
        if self.shell_index.contains_key(call_id) {
            return;
        }
        self.ensure_message(MessageInfo {
            id: message_id.to_owned(),
            session_id: session_id.to_owned(),
            role: "shell".to_owned(),
            time: MessageTime {
                created,
                ..MessageTime::default()
            },
            ..MessageInfo::default()
        });
        self.upsert_part(Part {
            id: call_id.to_owned(),
            session_id: session_id.to_owned(),
            message_id: message_id.to_owned(),
            kind: "shell".to_owned(),
            call_id: Some(call_id.to_owned()),
            command: Some(command.to_owned()),
            text: Some(String::new()),
            state: Some(json!({ "status": "running" })),
            ..Part::default()
        });
    }

    pub fn finish_shell(&mut self, call_id: &str, output: &str, completed: i64) -> bool {
        let Some((message_index, part_index)) = self.shell_index.get(call_id).copied() else {
            return false;
        };
        let message = &mut self.messages[message_index];
        message.info.time.completed = Some(completed);
        let part = &mut message.parts[part_index];
        part.text = Some(output.to_owned());
        part.state = Some(json!({ "status": "completed" }));
        true
    }

    pub fn finish_assistant(
        &mut self,
        message_id: &str,
        finish: &str,
        cost: f64,
        tokens: TokenUsage,
        snapshot: Option<Value>,
        completed: i64,
    ) -> bool {
        let Some(index) = self.message_index.get(message_id).copied() else {
            return false;
        };
        let info = &mut self.messages[index].info;
        info.finish = Some(finish.to_owned());
        info.cost = cost;
        info.tokens = tokens;
        if let Some(snapshot) = snapshot {
            let start = info
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.get("start"))
                .cloned();
            info.snapshot = Some(json!({
                "start": start,
                "end": snapshot.get("end").cloned().unwrap_or(Value::Null),
                "files": snapshot.get("files").cloned().unwrap_or_else(|| json!([])),
            }));
        }
        info.time.completed = Some(completed);
        true
    }

    pub fn fail_assistant(
        &mut self,
        message_id: &str,
        error_kind: &str,
        error_message: &str,
        completed: i64,
    ) -> bool {
        let Some(index) = self.message_index.get(message_id).copied() else {
            return false;
        };
        let info = &mut self.messages[index].info;
        info.finish = Some("error".to_owned());
        info.error = Some(json!({ "type": error_kind, "message": error_message }));
        info.time.completed = Some(completed);
        true
    }

    pub fn remove_message(&mut self, message_id: &str) -> bool {
        let Some(index) = self.message_index.get(message_id).copied() else {
            return false;
        };
        self.messages.remove(index);
        self.rebuild_indices();
        true
    }

    pub fn upsert_part(&mut self, part: Part) {
        let message_id = part.message_id.clone();
        if let Some(message_index) = self.message_index.get(&message_id).copied() {
            let part_key = (message_id.clone(), part.id.clone());
            if let Some(part_index) = self.part_index.get(&part_key).copied() {
                let previous = self.messages[message_index].parts[part_index].clone();
                self.messages[message_index].parts[part_index] = part;
                self.unindex_part(&message_id, &previous);
                let current = self.messages[message_index].parts[part_index].clone();
                self.index_part(&message_id, message_index, part_index, &current);
            } else {
                let part_index = self.messages[message_index].parts.len();
                self.messages[message_index].parts.push(part);
                self.part_index.insert(part_key, part_index);
                let current = self.messages[message_index].parts[part_index].clone();
                self.index_part(&message_id, message_index, part_index, &current);
            }
            return;
        }

        self.messages.push(MessageWithParts {
            info: MessageInfo {
                id: message_id,
                session_id: part.session_id.clone(),
                role: "assistant".to_owned(),
                ..MessageInfo::default()
            },
            parts: vec![part],
        });
        self.messages.sort_by(message_order);
        self.rebuild_indices();
    }

    pub fn apply_part_delta(
        &mut self,
        message_id: &str,
        part_id: &str,
        field: &str,
        delta: &str,
    ) -> bool {
        if field != "text" {
            return false;
        }
        let Some(message_index) = self.message_index.get(message_id).copied() else {
            return false;
        };
        let Some(part_index) = self
            .part_index
            .get(&(message_id.to_owned(), part_id.to_owned()))
            .copied()
        else {
            return false;
        };
        self.messages[message_index].parts[part_index]
            .text
            .get_or_insert_with(String::new)
            .push_str(delta);
        true
    }

    pub fn remove_part(&mut self, message_id: &str, part_id: &str) -> bool {
        let Some(message_index) = self.message_index.get(message_id).copied() else {
            return false;
        };
        let Some(part_index) = self
            .part_index
            .get(&(message_id.to_owned(), part_id.to_owned()))
            .copied()
        else {
            return false;
        };
        self.messages[message_index].parts.remove(part_index);
        self.rebuild_indices();
        true
    }

    fn rebuild_indices(&mut self) {
        self.message_index.clear();
        self.part_index.clear();
        self.call_index.clear();
        self.shell_index.clear();
        for (message_index, message) in self.messages.iter().enumerate() {
            self.message_index
                .insert(message.info.id.clone(), message_index);
            for (part_index, part) in message.parts.iter().enumerate() {
                self.part_index
                    .insert((message.info.id.clone(), part.id.clone()), part_index);
                if let Some(call_id) = part.call_id.as_deref() {
                    self.call_index.insert(
                        (message.info.id.clone(), call_id.to_owned()),
                        (message_index, part_index),
                    );
                    if part.kind == "shell" {
                        self.shell_index
                            .insert(call_id.to_owned(), (message_index, part_index));
                    }
                }
            }
        }
    }

    fn ensure_message(&mut self, info: MessageInfo) {
        if self.message_index.contains_key(&info.id) {
            return;
        }
        self.messages.push(MessageWithParts {
            info,
            parts: Vec::new(),
        });
        self.messages.sort_by(message_order);
        self.rebuild_indices();
    }

    fn ensure_assistant_message(&mut self, session_id: &str, message_id: &str, created: i64) {
        self.ensure_message(MessageInfo {
            id: message_id.to_owned(),
            session_id: session_id.to_owned(),
            role: "assistant".to_owned(),
            time: MessageTime {
                created,
                ..MessageTime::default()
            },
            ..MessageInfo::default()
        });
    }

    fn upsert_text_part(&mut self, session_id: &str, message_id: &str, part_id: &str, kind: &str) {
        self.upsert_part(Part {
            id: part_id.to_owned(),
            session_id: session_id.to_owned(),
            message_id: message_id.to_owned(),
            kind: kind.to_owned(),
            text: Some(String::new()),
            ..Part::default()
        });
    }

    fn has_part(&self, message_id: &str, part_id: &str) -> bool {
        self.part_index
            .contains_key(&(message_id.to_owned(), part_id.to_owned()))
    }

    fn part_location(&self, message_id: &str, part_id: &str) -> Option<(usize, usize)> {
        let message_index = self.message_index.get(message_id).copied()?;
        let part_index = self
            .part_index
            .get(&(message_id.to_owned(), part_id.to_owned()))
            .copied()?;
        Some((message_index, part_index))
    }

    fn call_location(&self, message_id: &str, call_id: &str) -> Option<(usize, usize)> {
        self.call_index
            .get(&(message_id.to_owned(), call_id.to_owned()))
            .copied()
            .or_else(|| self.part_location(message_id, call_id))
    }

    fn index_part(
        &mut self,
        message_id: &str,
        message_index: usize,
        part_index: usize,
        part: &Part,
    ) {
        if let Some(call_id) = part.call_id.as_deref() {
            self.call_index.insert(
                (message_id.to_owned(), call_id.to_owned()),
                (message_index, part_index),
            );
            if part.kind == "shell" {
                self.shell_index
                    .insert(call_id.to_owned(), (message_index, part_index));
            }
        }
    }

    fn unindex_part(&mut self, message_id: &str, part: &Part) {
        if let Some(call_id) = part.call_id.as_deref() {
            self.call_index
                .remove(&(message_id.to_owned(), call_id.to_owned()));
            if part.kind == "shell" {
                self.shell_index.remove(call_id);
            }
        }
    }
}

fn append_export_block(output: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }
    output.push_str(value.trim_end());
    output.push_str("\n\n");
}

fn message_order(left: &MessageWithParts, right: &MessageWithParts) -> std::cmp::Ordering {
    left.sort_key()
        .cmp(&right.sort_key())
        .then_with(|| left.info.id.cmp(&right.info.id))
}

#[cfg(test)]
mod tests {
    use super::TranscriptStore;
    use crate::model::{MessageInfo, MessageTime, MessageWithParts, Part};

    fn message(id: &str, created: i64) -> MessageWithParts {
        MessageWithParts {
            info: MessageInfo {
                id: id.to_owned(),
                session_id: "ses_1".to_owned(),
                role: "assistant".to_owned(),
                time: MessageTime {
                    created,
                    ..MessageTime::default()
                },
                ..MessageInfo::default()
            },
            parts: Vec::new(),
        }
    }

    fn text_part(id: &str, message_id: &str, text: &str) -> Part {
        Part {
            id: id.to_owned(),
            session_id: "ses_1".to_owned(),
            message_id: message_id.to_owned(),
            kind: "text".to_owned(),
            text: Some(text.to_owned()),
            ..Part::default()
        }
    }

    #[test]
    fn replace_orders_messages_and_updates_by_id() {
        let mut store = TranscriptStore::default();
        store.replace(vec![message("msg_2", 2), message("msg_1", 1)]);

        assert_eq!(
            store
                .iter()
                .map(|message| message.info.id.as_str())
                .collect::<Vec<_>>(),
            vec!["msg_1", "msg_2"]
        );

        store.upsert_message_info(MessageInfo {
            id: "msg_2".to_owned(),
            session_id: "ses_1".to_owned(),
            role: "assistant".to_owned(),
            time: MessageTime {
                created: 0,
                ..MessageTime::default()
            },
            ..MessageInfo::default()
        });
        assert_eq!(
            store.iter().next().map(|message| message.info.id.as_str()),
            Some("msg_2")
        );
    }

    #[test]
    fn part_index_supports_update_delta_and_remove() {
        let mut store = TranscriptStore::default();
        store.upsert_message_info(message("msg_1", 1).info);
        store.upsert_part(text_part("prt_1", "msg_1", "hello"));

        assert!(store.apply_part_delta("msg_1", "prt_1", "text", " world"));
        assert_eq!(
            store.get("msg_1").unwrap().parts[0].text.as_deref(),
            Some("hello world")
        );

        store.upsert_part(text_part("prt_1", "msg_1", "replacement"));
        assert_eq!(
            store.get("msg_1").unwrap().parts[0].text.as_deref(),
            Some("replacement")
        );
        assert!(store.remove_part("msg_1", "prt_1"));
        assert!(store.get("msg_1").unwrap().parts.is_empty());
    }

    #[test]
    fn an_orphan_part_creates_a_message_that_can_later_be_hydrated() {
        let mut store = TranscriptStore::default();
        store.upsert_part(text_part("prt_1", "msg_1", "streamed"));

        assert_eq!(store.len(), 1);
        assert_eq!(store.get("msg_1").unwrap().info.role, "assistant");
        store.upsert_message_info(message("msg_1", 4).info);
        assert_eq!(store.get("msg_1").unwrap().info.time.created, 4);
        assert_eq!(
            store.get("msg_1").unwrap().parts[0].text.as_deref(),
            Some("streamed")
        );
    }

    #[test]
    fn exports_text_tool_and_shell_parts_as_markdown() {
        let mut store = TranscriptStore::default();
        store.replace(vec![MessageWithParts {
            info: message("msg_1", 1).info,
            parts: vec![
                text_part("text_1", "msg_1", "hello"),
                Part {
                    id: "tool_1".to_owned(),
                    message_id: "msg_1".to_owned(),
                    kind: "tool".to_owned(),
                    tool: Some("bash".to_owned()),
                    state: Some(serde_json::json!({
                        "status": "completed",
                        "result": "ok"
                    })),
                    ..Part::default()
                },
                Part {
                    id: "shell_1".to_owned(),
                    message_id: "msg_1".to_owned(),
                    kind: "shell".to_owned(),
                    command: Some("pwd".to_owned()),
                    text: Some("E:/project".to_owned()),
                    ..Part::default()
                },
            ],
        }]);

        let markdown = store.export_markdown();
        assert!(markdown.contains("hello"));
        assert!(markdown.contains("Tool `bash` [completed]"));
        assert!(markdown.contains("E:/project"));
    }

    #[test]
    fn compaction_lifecycle_keeps_summary_and_recent_context() {
        let mut store = TranscriptStore::default();
        store.start_compaction("ses_1", "cmp_1", "auto", 10);
        assert!(store.append_compaction_delta("cmp_1", 11, "draft"));
        assert!(store.finish_compaction("ses_1", "cmp_1", "auto", "Summary", "Recent context", 12));

        let message = store.get("cmp_1").expect("compaction message should exist");
        assert_eq!(message.info.role, "system");
        assert_eq!(message.parts[0].kind, "compaction");
        assert_eq!(message.parts[0].text.as_deref(), Some("Summary"));
        assert_eq!(
            message.parts[0]
                .state
                .as_ref()
                .and_then(|state| state.get("recent"))
                .and_then(serde_json::Value::as_str),
            Some("Recent context")
        );

        let markdown = store.export_markdown();
        assert!(markdown.contains("Context compacted (auto)"));
        assert!(markdown.contains("Recent context: Recent context"));
    }
}
