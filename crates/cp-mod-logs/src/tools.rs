//! Tool implementations: `log_create` and `Close_conversation_history`.

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ToolResult, ToolUse};
use serde_json::Value;

use crate::types::{Importance, LogEntry, LogsState};

/// One validated log, ready to append.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NewLog {
    /// Log text (never blank).
    content: String,
    /// Validated level.
    importance: Importance,
}

impl NewLog {
    /// Build a validated log (content must already be non-blank).
    pub(crate) const fn new(content: String, importance: Importance) -> Self {
        Self { content, importance }
    }
}

/// `log_create` pre-flight: reject the call if any entry is invalid.
pub(crate) fn pre_flight_log_create(tool: &ToolUse) -> Verdict {
    let mut pf = Verdict::new();
    let entries = tool.input.get("entries").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    if let Err(errors) = parse_log_items(entries) {
        pf.errors.extend(errors);
    }
    pf
}

/// Parse one log item: a bare string (legacy form, medium) or
/// `{content, importance?}`. `Ok(None)` means blank content, which is skipped.
pub(crate) fn parse_log_item(item: &Value) -> Result<Option<NewLog>, String> {
    let (content, raw_importance) = if let Some(s) = item.as_str() {
        (s, None)
    } else if item.is_object() {
        (item.get("content").and_then(Value::as_str).unwrap_or(""), item.get("importance").filter(|v| !v.is_null()))
    } else {
        return Err("must be a string or an object with 'content'".to_owned());
    };
    let importance = match raw_importance {
        None => Importance::default(),
        Some(v) => v
            .as_str()
            .and_then(Importance::parse)
            .ok_or_else(|| format!("invalid importance {v} (expected one of: {})", Importance::NAMES.join(", ")))?,
    };
    if content.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(NewLog::new(content.to_owned(), importance)))
}

/// Parse a whole logs array. Any invalid item fails the batch, so a call
/// never half-applies; each error names its 1-based entry.
pub(crate) fn parse_log_items(items: &[Value]) -> Result<Vec<NewLog>, Vec<String>> {
    let mut logs = Vec::new();
    let mut errors = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match parse_log_item(item) {
            Ok(Some(log)) => logs.push(log),
            Ok(None) => {}
            Err(e) => errors.push(format!("entry #{}: {e}", i.saturating_add(1))),
        }
    }
    if errors.is_empty() { Ok(logs) } else { Err(errors) }
}

/// Append a log stamped `at_ms` (ms since epoch; 0 = now).
fn push_log(state: &mut State, log: NewLog, at_ms: u64) {
    let ls = LogsState::get_mut(state);
    let id = format!("L{}", ls.next_log_id);
    ls.next_log_id = ls.next_log_id.saturating_add(1);
    let mut entry =
        if at_ms > 0 { LogEntry::with_timestamp(id, log.content, at_ms) } else { LogEntry::new(id, log.content) };
    log.importance.as_str().clone_into(&mut entry.importance);
    ls.logs.push(entry);
}

/// Execute `log_create`: add one or more timestamped log entries.
pub(crate) fn execute_log_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("log_create");
    let Some(entries) = tool.input.get("entries").and_then(Value::as_array) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'entries' array".to_owned(), true);
    };
    if entries.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'entries' array".to_owned(), true);
    }
    let logs = match parse_log_items(entries) {
        Ok(logs) => logs,
        Err(errors) => {
            return ToolResult::new(tool.id.clone(), format!("Error: no log created. {}", errors.join("; ")), true);
        }
    };
    let count = logs.len();
    for log in logs {
        push_log(state, log, 0);
    }
    let mut result = ToolResult::new(tool.id.clone(), format!("Created {count} log(s)"), false);
    result.preserves_tempo = true;
    result
}

/// Execute `Close_conversation_history`: extract logs and remove one or more panels.
///
/// The tool queue is auto-activated by `pre_flight` (`Verdict::activate_queue`)
/// before the pipeline's intercept check, so this call always arrives here
/// via a queue flush — never executed directly. A panel whose logs fail to
/// parse stays open, so its history is never dropped without its logs.
pub(crate) fn execute_close_conversation_history(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("close_history");
    let Some(panels) = tool.input.get("panels").and_then(Value::as_array) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'panels' array".to_owned(), true);
    };
    if panels.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'panels' array".to_owned(), true);
    }
    let output_parts: Vec<String> = panels.iter().map(|panel_obj| close_one(panel_obj, state)).collect();
    ToolResult::new(tool.id.clone(), output_parts.join("\n"), false)
}

/// Close one history panel after appending its logs; returns the report line.
fn close_one(panel_obj: &Value, state: &mut State) -> String {
    let Some(panel_id) = panel_obj.get("panel_id").and_then(Value::as_str) else {
        return "Skipped entry: missing 'panel_id'".to_owned();
    };
    let Some(panel) = state.agent().context.iter().find(|c| c.id == panel_id) else {
        return format!("Panel '{panel_id}' not found");
    };
    if panel.context_type.as_str() != Kind::CONVERSATION_HISTORY {
        return format!("Panel '{panel_id}' is not a conversation history panel (type: {:?})", panel.context_type);
    }
    // Logs carry the panel's last message time so the Radar ages them right.
    let last_msg_timestamp =
        panel.history_messages.as_ref().and_then(|msgs| msgs.last()).map_or(0, |msg| msg.timestamp_ms);
    let panel_name = panel.name.clone();
    let items = panel_obj.get("logs").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let logs = match parse_log_items(items) {
        Ok(logs) => logs,
        Err(errors) => return format!("Error: {panel_id} left open. {}", errors.join("; ")),
    };
    let log_count = logs.len();
    for log in logs {
        push_log(state, log, last_msg_timestamp);
    }
    state.agent_mut().context.retain(|c| c.id != panel_id);
    format!("Closed {panel_id} ({panel_name}) — {log_count} log(s)")
}
