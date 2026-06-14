//! Thread message area rendering — right pane of the Threads view.
//!
//! Handles message display, input area, question form overlay, and
//! conversion helpers. All content goes through the IR pipeline.

use ratatui::Frame;
use ratatui::prelude::{Constraint, Direction, Layout, Rect, Style};
use ratatui::widgets::{Block as RBlock, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use cp_render::{Block as IrBlock, Semantic, Span as S};

use crate::modules::conversation::render_blocks::{MessageBlockOpts, render_message_blocks};
use crate::modules::conversation::render_input_blocks::{InputBlockCtx, render_input_blocks};
use crate::state::{Message, MsgKind, MsgStatus, State};
use crate::ui::{ir, theme};
use cp_base::cast::Safe as _;
use cp_mod_threads::types::{FocusState, ThreadAuthor, ThreadStatus, ThreadsState};

/// Render the right-pane message area with input box for the selected thread.
///
/// Messages and input render through the IR pipeline (same `render_message_blocks`
/// and `render_input_blocks` as the main conversation). Border title uses
/// `semantic_to_style` for color mapping.
pub(super) fn render_message_area_with_input(frame: &mut Frame<'_>, state: &State, selected: usize, area: Rect) {
    let ts = ThreadsState::get(state);
    let Some(thread) = ts.threads.get(selected) else {
        return;
    };

    // Title: thread name + status — colors via semantic mapping
    let focus = FocusState::get(state);
    let is_focused = focus.focused_thread_id.as_deref() == Some(thread.id.as_str());
    let (status_label, status_sem) = if is_focused {
        (" [FOCUSED]", Semantic::Accent)
    } else {
        match thread.status {
            ThreadStatus::MyTurn => (" [MY_TURN]", Semantic::Warning),
            ThreadStatus::TheirTurn => (" [THEIR_TURN]", Semantic::Success),
        }
    };

    let title = ratatui::text::Line::from(vec![
        ratatui::text::Span::styled(format!(" {} ", thread.name), ir::semantic_to_style(Semantic::Default)),
        ratatui::text::Span::styled(status_label, ir::semantic_to_style(status_sem)),
        ratatui::text::Span::raw(" "),
    ]);

    let border = RBlock::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(ir::semantic_to_style(Semantic::Border))
        .title(title)
        .style(Style::default().bg(theme::bg_surface()));

    let inner = border.inner(area);
    frame.render_widget(border, area);

    // Calculate input area height based on input content (capped at 50% of area)
    let input_height = calculate_input_height(state, inner.width, inner.height);
    let messages_height = inner.height.saturating_sub(input_height);

    if messages_height == 0 {
        return;
    }

    // Split inner area: messages on top, input at bottom
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(messages_height), Constraint::Length(input_height)])
        .split(inner);

    let (Some(&msg_area), Some(&input_area)) = (layout.first(), layout.get(1)) else {
        return;
    };

    render_thread_messages(frame, state, thread, msg_area);
    render_thread_input(frame, state, input_area);

    // Question form overlay — rendered OVER the input area if active
    if let Some(question_form_ir) = build_thread_question_form_ir(state) {
        let form_height = crate::ui::help::input::calculate_question_form_height(&question_form_ir);
        let form_y = inner.y.saturating_add(inner.height.saturating_sub(form_height));
        let form_area = Rect { x: inner.x, y: form_y, width: inner.width, height: form_height.min(inner.height) };
        crate::ui::help::input::render_question_form(frame, &question_form_ir, form_area);
    }
}

/// Render thread messages using the conversation IR renderer.
///
/// Converts `ThreadMessage` → `Message`, feeds to `render_message_blocks()`
/// (same IR path as the main conversation), converts via `blocks_to_lines()`.
fn render_thread_messages(frame: &mut Frame<'_>, state: &State, thread: &cp_mod_threads::types::Thread, area: Rect) {
    if thread.messages.is_empty() {
        let ir_blocks =
            vec![IrBlock::Line(vec![S::muted("No messages yet. Type below to start the conversation.".to_owned())])];
        let lines = ir::blocks_to_lines(&ir_blocks);
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, area);
        return;
    }

    let opts = MessageBlockOpts { viewport_width: area.width, is_streaming: false, dev_mode: false };

    // Convert ThreadMessages → Messages → IR blocks → ratatui Lines
    let mut all_blocks: Vec<cp_render::Block> = Vec::new();
    for msg in &thread.messages {
        let conv_msg = thread_message_to_message(msg);
        let msg_blocks = render_message_blocks(&conv_msg, &opts);
        all_blocks.extend(msg_blocks);
    }

    let lines = ir::blocks_to_lines(&all_blocks);

    // Scroll: use global scroll_offset; pin to bottom when user hasn't scrolled
    let content_height = lines.len();
    let viewport_height = area.height.to_usize();
    let max_scroll = content_height.saturating_sub(viewport_height);
    let scroll_offset = if state.flags.stream.user_scrolled {
        // User manually scrolled — respect their position, clamped
        (state.scroll_offset.to_usize()).min(max_scroll)
    } else {
        // Auto-scroll to bottom
        max_scroll
    };

    let paragraph = Paragraph::new(lines).scroll((scroll_offset.to_u16(), 0));
    frame.render_widget(paragraph, area);

    // Scrollbar — colors via semantic mapping
    if content_height > viewport_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(ir::semantic_to_style(Semantic::Border))
            .thumb_style(ir::semantic_to_style(Semantic::AccentDim));
        let mut scrollbar_state = ScrollbarState::new(max_scroll).position(scroll_offset);
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Render the input area at the bottom of the thread message area.
///
/// Separator line and input content both go through the IR pipeline.
fn render_thread_input(frame: &mut Frame<'_>, state: &State, area: Rect) {
    // Separator line via IR (border-colored, dimmed)
    let sep_area = Rect { height: 1, ..area };
    let sep_blocks = vec![IrBlock::Line(vec![S::styled("─".repeat(area.width.into()), Semantic::Border).dim()])];
    let sep_lines = ir::blocks_to_lines(&sep_blocks);
    let sep = Paragraph::new(sep_lines);
    frame.render_widget(sep, sep_area);

    // Input content below separator — via IR pipeline
    let input_area = Rect { y: area.y.saturating_add(1), height: area.height.saturating_sub(1), ..area };

    let command_ids: Vec<String> = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Command)
        .iter()
        .map(|p| p.id.clone())
        .collect();

    let ctx = InputBlockCtx {
        command_ids: &command_ids,
        paste_buffers: &state.paste_buffers,
        paste_buffer_labels: &state.paste_buffer_labels,
        viewport_width: input_area.width,
    };

    let input_blocks = render_input_blocks(&state.input, state.input_cursor, state.input_selection_anchor, &ctx);

    let lines = ir::blocks_to_lines(&input_blocks);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, input_area);
}

/// Convert a `ThreadMessage` to a `Message` for the conversation IR renderer.
///
/// If the message has embedded questions, appends a formatted markdown
/// representation so the questions are visible in the thread history.
fn thread_message_to_message(msg: &cp_mod_threads::types::ThreadMessage) -> Message {
    let role = match msg.author {
        ThreadAuthor::User => "user",
        ThreadAuthor::Assistant => "assistant",
    };
    let mut content = msg.content.clone().unwrap_or_default();

    // Append formatted questions if present
    if let Some(ref json) = msg.question
        && let Some(formatted) = format_questions_markdown(json)
    {
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str(&formatted);
    }

    Message {
        id: String::new(),
        uid: None,
        role: role.to_owned(),
        content,
        msg_type: MsgKind::TextMessage,
        status: MsgStatus::Full,
        tool_uses: vec![],
        tool_results: vec![],
        input_tokens: 0,
        content_token_count: 0,
        timestamp_ms: msg.timestamp,
    }
}

/// Format question JSON as readable markdown for thread message display.
///
/// Returns `None` if the JSON is malformed or empty.
fn format_questions_markdown(json: &serde_json::Value) -> Option<String> {
    let arr = json.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut lines = vec!["📋 **Questions:**".to_owned(), String::new()];
    for (i, q) in arr.iter().enumerate() {
        let header = q.get("header").and_then(serde_json::Value::as_str).unwrap_or("?");
        let text = q.get("question").and_then(serde_json::Value::as_str).unwrap_or("");
        let multi = q.get("multiSelect").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let tag = if multi { " *(multi-select)*" } else { "" };
        lines.push(format!("**{}. {}**{} — {}", i.saturating_add(1), header, tag, text));
        if let Some(options) = q.get("options").and_then(serde_json::Value::as_array) {
            for opt in options {
                let label = opt.get("label").and_then(serde_json::Value::as_str).unwrap_or("?");
                let desc = opt.get("description").and_then(serde_json::Value::as_str).unwrap_or("");
                lines.push(format!("  - **{label}**: {desc}"));
            }
        }
        lines.push("  - *Other (free text)*".to_owned());
        lines.push(String::new());
    }
    Some(lines.join("\n"))
}

/// Calculate input area height based on current input content.
///
/// Caps at 50% of the available height so messages remain visible.
fn calculate_input_height(state: &State, width: u16, available_height: u16) -> u16 {
    let max_input = available_height.saturating_div(2).max(3);
    if state.input.is_empty() {
        // Separator (1) + one line for empty input prompt
        return 3;
    }
    let line_count = state.input.lines().count().max(1);
    // Account for wrapping
    let wrap_width = (width as usize).saturating_sub(10).max(20);
    let wrapped_lines: usize =
        state.input.lines().map(|l| if l.is_empty() { 1 } else { l.len().div_ceil(wrap_width).max(1) }).sum();
    let total = wrapped_lines.max(line_count);
    // Separator (1) + content + hint line (1), capped at 50% of available height
    (total.saturating_add(3)).min(max_input.into()).to_u16()
}

/// Build a [`QuestionForm`] IR snapshot from the active thread question form.
///
/// Returns `None` if no active question form exists.
fn build_thread_question_form_ir(state: &State) -> Option<cp_render::conversation::QuestionForm> {
    let focus = FocusState::get(state);
    let form = focus.active_question.as_ref()?;

    let questions = form
        .questions
        .iter()
        .map(|q| cp_render::conversation::Question {
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
            cursor: q.cursor,
            selected: q.selected.clone(),
            typing_other: q.typing_other,
            other_text: q.other_text.clone(),
        })
        .collect();

    Some(cp_render::conversation::QuestionForm { questions, focused_index: form.focused_index })
}
