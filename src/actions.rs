use regex::Regex;

use crate::constants::{SCROLL_ACCEL_INCREMENT, SCROLL_ACCEL_MAX};
use crate::persistence::{delete_message, log_error, save_message};
use crate::state::{estimate_tokens, ContextElement, ContextType, Message, MessageStatus, MessageType, State};

/// Remove LLM's mistaken ID prefixes like "[A84]: " from responses
pub fn clean_llm_id_prefix(content: &str) -> String {
    // First trim leading whitespace
    let trimmed = content.trim_start();

    // Pattern: one or more [A##]: or [A###]: at the start, with optional whitespace between
    let re = Regex::new(r"^(\[A\d+\]:\s*)+").unwrap();
    let cleaned = re.replace(trimmed, "").to_string();

    // Also clean any [Axx]: that appears at the start of lines (multiline responses)
    let re_multiline = Regex::new(r"(?m)^\[A\d+\]:\s*").unwrap();
    let result = re_multiline.replace_all(&cleaned, "").to_string();

    // Strip leading/trailing whitespace and newlines after cleaning
    result.trim().to_string()
}

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
    InsertText(String),
    InputBackspace,
    InputDelete,
    InputSubmit,
    CursorWordLeft,
    CursorWordRight,
    DeleteWordLeft,
    RemoveListItem,  // Remove empty list item, keep newline
    CursorHome,
    CursorEnd,
    ClearConversation,
    NewContext,
    SelectNextContext,
    SelectPrevContext,
    AppendChars(String),
    StreamDone { _input_tokens: usize, output_tokens: usize },
    StreamError(String),
    ScrollUp(f32),
    ScrollDown(f32),
    StopStreaming,
    TmuxSendKeys { pane_id: String, keys: String },
    TogglePerfMonitor,
    ToggleConfigView,
    ConfigSelectProvider(crate::llms::LlmProvider),
    ConfigSelectAnthropicModel(crate::llms::AnthropicModel),
    ConfigSelectGrokModel(crate::llms::GrokModel),
    ConfigSelectGroqModel(crate::llms::GroqModel),
    ConfigSelectNextBar,
    ConfigSelectPrevBar,
    ConfigIncreaseSelectedBar,
    ConfigDecreaseSelectedBar,
    ConfigNextTheme,
    ConfigPrevTheme,
    OpenCommandPalette,
    SelectContextById(String),
    None,
}

pub enum ActionResult {
    Nothing,
    StartStream,
    StopStream,
    StartApiCheck,
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
        Action::InsertText(text) => {
            state.input.insert_str(state.input_cursor, &text);
            state.input_cursor += text.len();
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
        Action::DeleteWordLeft => {
            if state.input_cursor > 0 {
                let before = &state.input[..state.input_cursor];
                // Find word start (same logic as CursorWordLeft)
                let trimmed = before.trim_end();
                let word_start = if trimmed.is_empty() {
                    0
                } else {
                    trimmed.rfind(|c: char| c.is_whitespace())
                        .map(|i| i + 1)
                        .unwrap_or(0)
                };
                // Delete from word_start to cursor
                state.input = format!("{}{}", &state.input[..word_start], &state.input[state.input_cursor..]);
                state.input_cursor = word_start;
            }
            ActionResult::Nothing
        }
        Action::RemoveListItem => {
            // Remove the current line's content (empty list prefix) but keep the newline
            // Input: "- item\n- " -> "- item\n"
            if state.input_cursor > 0 {
                let before = &state.input[..state.input_cursor];
                // Find the last newline
                let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                // Delete from line_start to cursor
                state.input = format!("{}{}", &state.input[..line_start], &state.input[state.input_cursor..]);
                state.input_cursor = line_start;
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

            let content = std::mem::take(&mut state.input);
            state.input_cursor = 0;
            let user_token_estimate = estimate_tokens(&content);

            // Assign user display ID and UID
            let user_id = format!("U{}", state.next_user_id);
            let user_uid = format!("UID_{}_U", state.global_next_uid);
            state.next_user_id += 1;
            state.global_next_uid += 1;

            let user_msg = Message {
                id: user_id,
                uid: Some(user_uid),
                role: "user".to_string(),
                message_type: MessageType::TextMessage,
                content,
                content_token_count: user_token_estimate,
                tl_dr: None,
                tl_dr_token_count: 0,
                status: MessageStatus::Full,
                tool_uses: Vec::new(),
                tool_results: Vec::new(),
                input_tokens: 0,
                timestamp_ms: crate::core::panels::now_ms(),
            };
            save_message(&user_msg);

            // Add user message tokens to Conversation context and update timestamp
            if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
                ctx.token_count += user_token_estimate;
                ctx.last_refresh_ms = crate::core::panels::now_ms();
            }

            // During streaming: insert BEFORE the streaming assistant message
            // Otherwise: append normally
            if state.is_streaming {
                // Insert before the last message (the streaming assistant message)
                let insert_pos = state.messages.len().saturating_sub(1);
                state.messages.insert(insert_pos, user_msg);
                return ActionResult::SaveMessage(state.messages[insert_pos].uid.clone().unwrap_or_else(|| state.messages[insert_pos].id.clone()));
            }

            state.messages.push(user_msg);

            // Create assistant message and start streaming
            let assistant_id = format!("A{}", state.next_assistant_id);
            let assistant_uid = format!("UID_{}_A", state.global_next_uid);
            state.next_assistant_id += 1;
            state.global_next_uid += 1;

            let assistant_msg = Message {
                id: assistant_id,
                uid: Some(assistant_uid),
                role: "assistant".to_string(),
                message_type: MessageType::TextMessage,
                content: String::new(),
                content_token_count: 0,
                tl_dr: None,
                tl_dr_token_count: 0,
                status: MessageStatus::Full,
                tool_uses: Vec::new(),
                tool_results: Vec::new(),
                input_tokens: 0,
                timestamp_ms: crate::core::panels::now_ms(),
            };
            state.messages.push(assistant_msg);

            state.is_streaming = true;
            state.streaming_estimated_tokens = 0;
            ActionResult::StartStream
        }
        Action::ClearConversation => {
            for msg in &state.messages {
                // Delete by UID if available, otherwise by id
                let file_id = msg.uid.as_ref().unwrap_or(&msg.id);
                delete_message(file_id);
            }
            state.messages.clear();
            state.input.clear();
            // Reset token count for Conversation context and update timestamp
            if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
                ctx.token_count = 0;
                ctx.last_refresh_ms = crate::core::panels::now_ms();
            }
            ActionResult::Save
        }
        Action::NewContext => {
            let context_id = state.next_available_context_id();
            state.context.push(ContextElement {
                id: context_id,
                uid: None, // UIDs for dynamic panels assigned separately
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
                last_refresh_ms: crate::core::panels::now_ms(),
                tmux_last_lines_hash: None,
                current_page: 0,
                total_pages: 1,
                full_token_count: 0,
            });
            ActionResult::Save
        }
        Action::SelectNextContext => {
            if !state.context.is_empty() {
                // Sort indices by P-number to match visual order in sidebar
                let mut sorted_indices: Vec<usize> = (0..state.context.len()).collect();
                sorted_indices.sort_by(|&a, &b| {
                    let id_a = state.context[a].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
                    let id_b = state.context[b].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
                    id_a.cmp(&id_b)
                });

                // Find current position in sorted order
                let current_pos = sorted_indices.iter().position(|&i| i == state.selected_context).unwrap_or(0);
                let next_pos = (current_pos + 1) % sorted_indices.len();
                state.selected_context = sorted_indices[next_pos];
                state.scroll_offset = 0.0;
                state.user_scrolled = false;
            }
            ActionResult::Nothing
        }
        Action::SelectPrevContext => {
            if !state.context.is_empty() {
                // Sort indices by P-number to match visual order in sidebar
                let mut sorted_indices: Vec<usize> = (0..state.context.len()).collect();
                sorted_indices.sort_by(|&a, &b| {
                    let id_a = state.context[a].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
                    let id_b = state.context[b].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
                    id_a.cmp(&id_b)
                });

                // Find current position in sorted order
                let current_pos = sorted_indices.iter().position(|&i| i == state.selected_context).unwrap_or(0);
                let prev_pos = if current_pos == 0 {
                    sorted_indices.len() - 1
                } else {
                    current_pos - 1
                };
                state.selected_context = sorted_indices[prev_pos];
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
                        if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
                            ctx.token_count += added;
                        }
                        state.streaming_estimated_tokens = new_estimate;
                    }
                }
            }
            ActionResult::Nothing
        }
        Action::StreamDone { _input_tokens, output_tokens } => {
            state.is_streaming = false;

            // Correct the estimated tokens with actual output tokens on Conversation context and update timestamp
            if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
                // Remove our estimate, add actual
                ctx.token_count = ctx.token_count
                    .saturating_sub(state.streaming_estimated_tokens)
                    .saturating_add(output_tokens);
                ctx.last_refresh_ms = crate::core::panels::now_ms();
            }
            state.streaming_estimated_tokens = 0;

            // Store actual token count on message and clean up LLM prefixes
            if let Some(msg) = state.messages.last_mut() {
                if msg.role == "assistant" {
                    // Remove any [A##]: prefixes the LLM mistakenly added
                    msg.content = clean_llm_id_prefix(&msg.content);
                    msg.content_token_count = output_tokens;
                    msg.input_tokens = _input_tokens;
                    let id = msg.id.clone();
                    return ActionResult::SaveMessage(id);
                }
            }
            ActionResult::Save
        }
        Action::StreamError(e) => {
            state.is_streaming = false;

            // Remove estimated tokens on error from Conversation context
            if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
                ctx.token_count = ctx.token_count.saturating_sub(state.streaming_estimated_tokens);
            }
            state.streaming_estimated_tokens = 0;

            // Log error to file
            let error_file = log_error(&e);

            if let Some(msg) = state.messages.last_mut() {
                if msg.role == "assistant" {
                    msg.content = format!("[Error occurred. See details in {}]", error_file);
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
            // Don't clamp here - render will clamp to actual max_scroll for current panel
            state.scroll_offset += accel_amount;
            state.scroll_accel = (state.scroll_accel + SCROLL_ACCEL_INCREMENT).min(SCROLL_ACCEL_MAX);
            ActionResult::Nothing
        }
        Action::StopStreaming => {
            if state.is_streaming {
                state.is_streaming = false;
                // Remove estimated tokens on cancel from Conversation context
                if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
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
        Action::TmuxSendKeys { pane_id, keys } => {
            // Send keys to tmux pane
            use std::process::Command;
            let _ = Command::new("tmux")
                .args(["send-keys", "-t", &pane_id, &keys])
                .output();

            // Update last_keys on the context
            if let Some(ctx) = state.context.iter_mut()
                .find(|c| c.tmux_pane_id.as_ref() == Some(&pane_id))
            {
                ctx.tmux_last_keys = Some(keys);
                // Mark cache as deprecated to refresh the pane content
                ctx.cache_deprecated = true;
            }
            ActionResult::Nothing
        }
        Action::TogglePerfMonitor => {
            state.perf_enabled = crate::perf::PERF.toggle();
            state.dirty = true;
            ActionResult::Nothing
        }
        Action::ToggleConfigView => {
            state.config_view = !state.config_view;
            state.dirty = true;
            ActionResult::Nothing
        }
        Action::ConfigSelectProvider(provider) => {
            state.llm_provider = provider;
            state.api_check_in_progress = true;
            state.api_check_result = None;
            state.dirty = true;
            ActionResult::StartApiCheck
        }
        Action::ConfigSelectAnthropicModel(model) => {
            state.anthropic_model = model;
            state.api_check_in_progress = true;
            state.api_check_result = None;
            state.dirty = true;
            ActionResult::StartApiCheck
        }
        Action::ConfigSelectGrokModel(model) => {
            state.grok_model = model;
            state.api_check_in_progress = true;
            state.api_check_result = None;
            state.dirty = true;
            ActionResult::StartApiCheck
        }
        Action::ConfigSelectGroqModel(model) => {
            state.groq_model = model;
            state.api_check_in_progress = true;
            state.api_check_result = None;
            state.dirty = true;
            ActionResult::StartApiCheck
        }
        Action::ConfigSelectNextBar => {
            state.config_selected_bar = (state.config_selected_bar + 1) % 3;
            state.dirty = true;
            ActionResult::Nothing
        }
        Action::ConfigSelectPrevBar => {
            state.config_selected_bar = if state.config_selected_bar == 0 { 2 } else { state.config_selected_bar - 1 };
            state.dirty = true;
            ActionResult::Nothing
        }
        Action::ConfigIncreaseSelectedBar => {
            match state.config_selected_bar {
                0 => {
                    // Context budget
                    let max_budget = state.model_context_window();
                    let step = max_budget / 20; // 5% steps
                    let current = state.context_budget.unwrap_or(max_budget);
                    state.context_budget = Some((current + step).min(max_budget));
                }
                1 => {
                    // Cleaning threshold
                    state.cleaning_threshold = (state.cleaning_threshold + 0.05).min(0.95);
                }
                2 => {
                    // Target proportion
                    state.cleaning_target_proportion = (state.cleaning_target_proportion + 0.05).min(0.95);
                }
                _ => {}
            }
            state.dirty = true;
            ActionResult::Save
        }
        Action::ConfigDecreaseSelectedBar => {
            match state.config_selected_bar {
                0 => {
                    // Context budget
                    let max_budget = state.model_context_window();
                    let step = max_budget / 20; // 5% steps
                    let min_budget = max_budget / 10; // Minimum 10% of context
                    let current = state.context_budget.unwrap_or(max_budget);
                    state.context_budget = Some((current.saturating_sub(step)).max(min_budget));
                }
                1 => {
                    // Cleaning threshold
                    state.cleaning_threshold = (state.cleaning_threshold - 0.05).max(0.30);
                }
                2 => {
                    // Target proportion
                    state.cleaning_target_proportion = (state.cleaning_target_proportion - 0.05).max(0.30);
                }
                _ => {}
            }
            state.dirty = true;
            ActionResult::Save
        }
        Action::ConfigNextTheme => {
            use crate::config::THEME_ORDER;
            let current_idx = THEME_ORDER.iter().position(|&t| t == state.active_theme).unwrap_or(0);
            let next_idx = (current_idx + 1) % THEME_ORDER.len();
            state.active_theme = THEME_ORDER[next_idx].to_string();
            crate::config::set_active_theme(&state.active_theme);
            state.dirty = true;
            ActionResult::Save
        }
        Action::ConfigPrevTheme => {
            use crate::config::THEME_ORDER;
            let current_idx = THEME_ORDER.iter().position(|&t| t == state.active_theme).unwrap_or(0);
            let prev_idx = if current_idx == 0 { THEME_ORDER.len() - 1 } else { current_idx - 1 };
            state.active_theme = THEME_ORDER[prev_idx].to_string();
            crate::config::set_active_theme(&state.active_theme);
            state.dirty = true;
            ActionResult::Save
        }
        Action::OpenCommandPalette => {
            // Handled in app.rs directly
            ActionResult::Nothing
        }
        Action::SelectContextById(id) => {
            // Find context by ID and select it
            if let Some(idx) = state.context.iter().position(|c| c.id == id) {
                state.selected_context = idx;
                state.scroll_offset = 0.0;
                state.user_scrolled = false;
                state.dirty = true;
            }
            ActionResult::Nothing
        }
        Action::None => ActionResult::Nothing,
    }
}
