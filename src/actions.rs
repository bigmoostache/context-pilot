use crate::constants::{SCROLL_ACCEL_INCREMENT, SCROLL_ACCEL_MAX};
use crate::persistence::{delete_message, save_message};
use crate::state::{estimate_tokens, ContextElement, ContextType, Message, MessageStatus, MessageType, State};

/// Parse context selection patterns like p1, p-1, p_1, P1, P-1, P_1
/// Returns the context ID (e.g., "P1", "P28") if matched
pub fn parse_context_pattern(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let input_lower = input.to_lowercase();

    // Must start with 'p'
    if !input_lower.starts_with('p') {
        return None;
    }

    // Get the rest after 'p'
    let rest = &input_lower[1..];

    // Skip optional separator (- or _)
    let num_str = if rest.starts_with('-') || rest.starts_with('_') {
        &rest[1..]
    } else {
        rest
    };

    // Parse the number and return the canonical ID format
    num_str.parse::<usize>().ok().map(|n| format!("P{}", n))
}

/// Find context index by ID
pub fn find_context_by_id(state: &State, id: &str) -> Option<usize> {
    state.context.iter().position(|c| c.id == id)
}

#[derive(Debug, Clone)]
pub enum Action {
    InputChar(char),
    InputBackspace,
    InputDelete,
    InputSubmit,
    CursorWordLeft,
    CursorWordRight,
    CursorHome,
    CursorEnd,
    ClearConversation,
    NewContext,
    SelectContext(usize),
    SelectNextContext,
    SelectPrevContext,
    AppendChars(String),
    StreamDone { _input_tokens: usize, output_tokens: usize },
    StreamError(String),
    ScrollUp(f32),
    ScrollDown(f32),
    ToggleCopyMode,
    StopStreaming,
    StartContextCleaning,
    None,
}

pub enum ActionResult {
    Nothing,
    StartStream,
    StopStream,
    StartCleaning,
    Save,
    SaveMessage(String),
}

pub fn apply_action(state: &mut State, action: Action) -> ActionResult {
    // Reset scroll acceleration on non-scroll actions
    if !matches!(action, Action::ScrollUp(_) | Action::ScrollDown(_)) {
        state.scroll_accel = 1.0;
    }

    match action {
        Action::InputChar(c) => {
            state.input.insert(state.input_cursor, c);
            state.input_cursor += c.len_utf8();
            ActionResult::Nothing
        }
        Action::InputBackspace => {
            if state.input_cursor > 0 {
                // Find the previous character boundary
                let prev = state.input[..state.input_cursor]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                state.input.remove(prev);
                state.input_cursor = prev;
            }
            ActionResult::Nothing
        }
        Action::InputDelete => {
            if state.input_cursor < state.input.len() {
                state.input.remove(state.input_cursor);
            }
            ActionResult::Nothing
        }
        Action::CursorWordLeft => {
            if state.input_cursor > 0 {
                let before = &state.input[..state.input_cursor];
                // Skip whitespace, then skip word chars
                let trimmed = before.trim_end();
                if trimmed.is_empty() {
                    state.input_cursor = 0;
                } else {
                    let word_start = trimmed.rfind(|c: char| c.is_whitespace())
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    state.input_cursor = word_start;
                }
            }
            ActionResult::Nothing
        }
        Action::CursorWordRight => {
            if state.input_cursor < state.input.len() {
                let after = &state.input[state.input_cursor..];
                // Skip current word chars, then skip whitespace
                let skip_word = after.find(|c: char| c.is_whitespace()).unwrap_or(after.len());
                let remaining = &after[skip_word..];
                let skip_space = remaining.find(|c: char| !c.is_whitespace()).unwrap_or(remaining.len());
                state.input_cursor += skip_word + skip_space;
            }
            ActionResult::Nothing
        }
        Action::CursorHome => {
            // Move to start of current line
            let before_cursor = &state.input[..state.input_cursor];
            state.input_cursor = before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
            ActionResult::Nothing
        }
        Action::CursorEnd => {
            // Move to end of current line
            let after_cursor = &state.input[state.input_cursor..];
            state.input_cursor += after_cursor.find('\n').unwrap_or(after_cursor.len());
            ActionResult::Nothing
        }
        Action::InputSubmit => {
            if state.input.is_empty() {
                return ActionResult::Nothing;
            }

            // Context switching is always allowed, even during streaming
            if let Some(id) = parse_context_pattern(&state.input) {
                if let Some(index) = find_context_by_id(state, &id) {
                    state.selected_context = index;
                    state.scroll_offset = 0.0;
                    state.user_scrolled = false;
                    state.input.clear();
                    state.input_cursor = 0;
                    return ActionResult::Nothing;
                }
            }

            // Starting a new message is blocked during streaming
            if state.is_streaming {
                return ActionResult::Nothing;
            }

            let content = std::mem::take(&mut state.input);
            state.input_cursor = 0;
            let user_token_estimate = estimate_tokens(&content);

            // Assign IDs
            let user_id = format!("U{}", state.next_user_id);
            state.next_user_id += 1;
            let assistant_id = format!("A{}", state.next_assistant_id);
            state.next_assistant_id += 1;

            let user_msg = Message {
                id: user_id,
                role: "user".to_string(),
                message_type: MessageType::TextMessage,
                content,
                content_token_count: user_token_estimate,
                tl_dr: None,
                tl_dr_token_count: 0,
                status: MessageStatus::Full,
                tool_uses: Vec::new(),
                tool_results: Vec::new(),
            };
            save_message(&user_msg);

            // Add user message tokens to context
            if let Some(ctx) = state.context.get_mut(state.selected_context) {
                ctx.token_count += user_token_estimate;
            }

            state.messages.push(user_msg);

            let assistant_msg = Message {
                id: assistant_id,
                role: "assistant".to_string(),
                message_type: MessageType::TextMessage,
                content: String::new(),
                content_token_count: 0,
                tl_dr: None,
                tl_dr_token_count: 0,
                status: MessageStatus::Full,
                tool_uses: Vec::new(),
                tool_results: Vec::new(),
            };
            state.messages.push(assistant_msg);

            state.is_streaming = true;
            state.streaming_estimated_tokens = 0;
            ActionResult::StartStream
        }
        Action::ClearConversation => {
            for msg in &state.messages {
                delete_message(&msg.id);
            }
            state.messages.clear();
            state.input.clear();
            // Reset token count for current context
            if let Some(ctx) = state.context.get_mut(state.selected_context) {
                ctx.token_count = 0;
            }
            ActionResult::Save
        }
        Action::NewContext => {
            let context_id = format!("P{}", state.next_context_id);
            state.next_context_id += 1;
            state.context.push(ContextElement {
                id: context_id,
                context_type: ContextType::Conversation,
                name: format!("Conv {}", state.context.len()),
                token_count: 0,
                file_path: None,
                file_hash: None,
                glob_pattern: None,
                glob_path: None,
                grep_pattern: None,
                grep_path: None,
                grep_file_pattern: None,
                tmux_pane_id: None,
                tmux_lines: None,
                tmux_last_keys: None,
                tmux_description: None,
                cached_content: None,
                cache_deprecated: false,
                last_refresh_ms: 0,
                tmux_last_lines_hash: None,
            });
            ActionResult::Save
        }
        Action::SelectContext(index) => {
            if index < state.context.len() {
                state.selected_context = index;
                state.scroll_offset = 0.0;
                state.user_scrolled = false;
            }
            ActionResult::Nothing
        }
        Action::SelectNextContext => {
            if !state.context.is_empty() {
                state.selected_context = (state.selected_context + 1) % state.context.len();
                state.scroll_offset = 0.0;
                state.user_scrolled = false;
            }
            ActionResult::Nothing
        }
        Action::SelectPrevContext => {
            if !state.context.is_empty() {
                state.selected_context = if state.selected_context == 0 {
                    state.context.len() - 1
                } else {
                    state.selected_context - 1
                };
                state.scroll_offset = 0.0;
                state.user_scrolled = false;
            }
            ActionResult::Nothing
        }
        Action::AppendChars(text) => {
            if let Some(msg) = state.messages.last_mut() {
                if msg.role == "assistant" {
                    msg.content.push_str(&text);

                    // Update estimated token count during streaming
                    let new_estimate = estimate_tokens(&msg.content);
                    let added = new_estimate.saturating_sub(state.streaming_estimated_tokens);

                    if added > 0 {
                        if let Some(ctx) = state.context.get_mut(state.selected_context) {
                            ctx.token_count += added;
                        }
                        state.streaming_estimated_tokens = new_estimate;
                    }
                }
            }
            ActionResult::Nothing
        }
        Action::StreamDone { _input_tokens: _, output_tokens } => {
            state.is_streaming = false;

            // Correct the estimated tokens with actual output tokens
            if let Some(ctx) = state.context.get_mut(state.selected_context) {
                // Remove our estimate, add actual
                ctx.token_count = ctx.token_count
                    .saturating_sub(state.streaming_estimated_tokens)
                    .saturating_add(output_tokens);
            }
            state.streaming_estimated_tokens = 0;

            // Store actual token count on message
            if let Some(msg) = state.messages.last_mut() {
                if msg.role == "assistant" {
                    msg.content_token_count = output_tokens;
                    let id = msg.id.clone();
                    return ActionResult::SaveMessage(id);
                }
            }
            ActionResult::Save
        }
        Action::StreamError(e) => {
            state.is_streaming = false;

            // Remove estimated tokens on error
            if let Some(ctx) = state.context.get_mut(state.selected_context) {
                ctx.token_count = ctx.token_count.saturating_sub(state.streaming_estimated_tokens);
            }
            state.streaming_estimated_tokens = 0;

            if let Some(msg) = state.messages.last_mut() {
                if msg.role == "assistant" {
                    msg.content = format!("[Error: {}]", e);
                    let id = msg.id.clone();
                    return ActionResult::SaveMessage(id);
                }
            }
            ActionResult::Save
        }
        Action::ScrollUp(amount) => {
            let accel_amount = amount * state.scroll_accel;
            state.scroll_offset = (state.scroll_offset - accel_amount).max(0.0);
            state.user_scrolled = true;
            state.scroll_accel = (state.scroll_accel + SCROLL_ACCEL_INCREMENT).min(SCROLL_ACCEL_MAX);
            ActionResult::Nothing
        }
        Action::ScrollDown(amount) => {
            let accel_amount = amount * state.scroll_accel;
            state.scroll_offset = (state.scroll_offset + accel_amount).min(state.max_scroll);
            state.scroll_accel = (state.scroll_accel + SCROLL_ACCEL_INCREMENT).min(SCROLL_ACCEL_MAX);
            ActionResult::Nothing
        }
        Action::ToggleCopyMode => {
            state.copy_mode = !state.copy_mode;
            ActionResult::Nothing
        }
        Action::StopStreaming => {
            if state.is_streaming {
                state.is_streaming = false;
                // Remove estimated tokens on cancel
                if let Some(ctx) = state.context.get_mut(state.selected_context) {
                    ctx.token_count = ctx.token_count.saturating_sub(state.streaming_estimated_tokens);
                }
                state.streaming_estimated_tokens = 0;
                // Mark partial response
                if let Some(msg) = state.messages.last_mut() {
                    if msg.role == "assistant" && !msg.content.is_empty() {
                        msg.content.push_str("\n[Stopped]");
                    }
                }
                ActionResult::StopStream
            } else {
                ActionResult::Nothing
            }
        }
        Action::StartContextCleaning => {
            // Don't start if already cleaning (streaming is OK - cleaning runs independently)
            if state.is_cleaning_context {
                ActionResult::Nothing
            } else {
                state.is_cleaning_context = true;
                ActionResult::StartCleaning
            }
        }
        Action::None => ActionResult::Nothing,
    }
}
