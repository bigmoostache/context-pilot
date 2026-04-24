//! Conversation IR builder — assembles [`Conversation`] from application state.
//!
//! Extracts the data logic from `modules::conversation::panel` into a pure
//! function returning IR types. No ratatui, no Frame, no caching — just
//! state → data transformation. Caching lives in the adapter layer (Phase 5).

use cp_render::conversation::{
    Autocomplete, AutocompleteEntry, Conversation, HistorySection, InputArea, Message as IrMessage, Overlay,
    QuestionForm, StreamingTool, ToolResultPreview, ToolUsePreview,
};
use cp_render::{Block, Semantic};

use crate::state::{Kind, MsgKind, MsgStatus, State, ToolResultRecord, ToolUseRecord};

/// Build the conversation region from application state.
#[must_use]
pub(crate) fn build_conversation(state: &State) -> Conversation {
    let history_sections = build_history_sections(state);
    let messages = build_messages(state);
    let streaming_tools = build_streaming_tools(state);
    let input = build_input(state);

    Conversation { history_sections, messages, streaming_tools, input }
}

// ── History sections ─────────────────────────────────────────────────

/// Build history sections from `ConversationHistory` context elements.
fn build_history_sections(state: &State) -> Vec<HistorySection> {
    let mut history_panels: Vec<_> =
        state.context.iter().filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY).collect();
    history_panels.sort_by_key(|c| c.last_refresh_ms);

    history_panels
        .iter()
        .map(|ctx| {
            let messages = ctx
                .history_messages
                .as_ref()
                .map(|msgs| msgs.iter().filter(|m| m.status != MsgStatus::Deleted).map(msg_to_ir).collect())
                .unwrap_or_default();

            HistorySection { label: ctx.name.clone(), expanded: true, messages }
        })
        .collect()
}

// ── Messages ─────────────────────────────────────────────────────────

/// Build the visible message list from current conversation.
fn build_messages(state: &State) -> Vec<IrMessage> {
    let last_msg_id = state.messages.last().map(|m| m.id.clone());

    state
        .messages
        .iter()
        .filter(|msg| {
            if msg.status == MsgStatus::Deleted {
                return false;
            }
            // Skip empty text messages (unless currently streaming)
            let is_last = last_msg_id.as_ref() == Some(&msg.id);
            let is_streaming = state.flags.stream.phase.is_streaming() && is_last && msg.role == "assistant";
            if msg.msg_type == MsgKind::TextMessage && msg.content.trim().is_empty() && !is_streaming {
                return false;
            }
            true
        })
        .map(msg_to_ir)
        .collect()
}

/// Convert a single application Message to an IR Message.
fn msg_to_ir(msg: &crate::state::Message) -> IrMessage {
    let content = build_message_content(msg);
    let tool_uses = msg.tool_uses.iter().map(tool_use_to_ir).collect();
    let tool_results = msg.tool_results.iter().map(tool_result_to_ir).collect();

    IrMessage { role: msg.role.clone(), content, tool_uses, tool_results }
}

/// Build content blocks for a message based on its type.
fn build_message_content(msg: &crate::state::Message) -> Vec<Block> {
    match msg.msg_type {
        MsgKind::TextMessage => {
            if msg.content.is_empty() {
                Vec::new()
            } else {
                // Each line becomes a Block::Line. Markdown rendering
                // is deferred to the adapter layer (Phase 5).
                msg.content.lines().map(|line| Block::text(line.to_owned())).collect()
            }
        }
        MsgKind::ToolCall => {
            // Tool calls are represented via tool_uses, content is usually empty
            if msg.content.is_empty() { Vec::new() } else { vec![Block::text(msg.content.clone())] }
        }
        MsgKind::ToolResult => {
            // Tool results are represented via tool_results, content is usually empty
            if msg.content.is_empty() { Vec::new() } else { vec![Block::text(msg.content.clone())] }
        }
    }
}

/// Convert a [`ToolUseRecord`] to an IR [`ToolUsePreview`].
fn tool_use_to_ir(tu: &ToolUseRecord) -> ToolUsePreview {
    // Build a short summary from input parameters
    let summary: String =
        tu.input.as_object().map(|obj| obj.keys().take(3).cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default();

    ToolUsePreview { tool_name: tu.name.clone(), summary, semantic: Semantic::Success }
}

/// Convert a [`ToolResultRecord`] to an IR [`ToolResultPreview`].
fn tool_result_to_ir(tr: &ToolResultRecord) -> ToolResultPreview {
    // Prefer display (user-facing) over content (LLM-facing) for the UI
    let source = tr.display.as_deref().unwrap_or(&tr.content);

    // Truncate content for summary
    let summary = if source.len() > 80 {
        let boundary = source.floor_char_boundary(77);
        format!("{}...", source.get(..boundary).unwrap_or(""))
    } else {
        source.to_string()
    };

    ToolResultPreview { tool_name: tr.tool_name.clone(), summary, success: !tr.is_error }
}

// ── Streaming tools ──────────────────────────────────────────────────

/// Build streaming tool previews from state.
fn build_streaming_tools(state: &State) -> Vec<StreamingTool> {
    state
        .streaming_tool
        .as_ref()
        .map(|st| vec![StreamingTool { tool_name: st.name.clone(), partial_input: st.input_so_far.clone() }])
        .unwrap_or_default()
}

// ── Input area ───────────────────────────────────────────────────────

/// Build the input area from state.
fn build_input(state: &State) -> InputArea {
    InputArea {
        text: state.input.clone(),
        cursor: state.input_cursor,
        placeholder: "Type a message…".into(),
        focused: !state.flags.stream.phase.is_streaming(),
    }
}

// ── Overlays ─────────────────────────────────────────────────────────

/// Build overlay stack from state (question form, autocomplete).
#[must_use]
pub(crate) fn build_overlays(state: &State) -> Vec<Overlay> {
    let mut overlays = Vec::new();

    // Question form overlay
    if let Some(form) = state.get_ext::<cp_base::ui::question_form::PendingForm>()
        && !form.resolved
    {
        overlays.push(Overlay::QuestionForm(build_question_form(form)));
    }

    // Autocomplete overlay
    if let Some(ac) = state.get_ext::<cp_base::state::autocomplete::Suggestions>()
        && ac.active
    {
        overlays.push(Overlay::Autocomplete(build_autocomplete(ac)));
    }

    overlays
}

/// Build question form from pending form state.
fn build_question_form(form: &cp_base::ui::question_form::PendingForm) -> QuestionForm {
    let questions = form
        .questions
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let answer = form.answers.get(i);
            cp_render::conversation::Question {
                header: q.header.clone(),
                text: q.text.clone(),
                options: q
                    .options
                    .iter()
                    .map(|o| cp_render::conversation::QuestionOption {
                        label: o.label.clone(),
                        description: o.description.clone(),
                    })
                    .collect(),
                multi_select: q.multi_select,
                selected: answer.map(|a| a.selected.clone()).unwrap_or_default(),
                other_text: answer.map(|a| a.other_text.clone()).unwrap_or_default(),
            }
        })
        .collect();

    QuestionForm { questions, focused_index: form.current_question }
}

/// Build autocomplete from suggestions state.
fn build_autocomplete(ac: &cp_base::state::autocomplete::Suggestions) -> Autocomplete {
    let entries = ac
        .visible_matches()
        .iter()
        .map(|e| AutocompleteEntry {
            label: e.name.clone(),
            is_dir: e.is_dir,
            icon: if e.is_dir { "📁".into() } else { "📄".into() },
        })
        .collect();

    Autocomplete { query: ac.query.clone(), entries, selected_index: ac.selected }
}
