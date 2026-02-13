//! Action handling split into domain-focused modules.
//!
//! - `helpers` — Utility functions (clean_llm_id_prefix, parse_context_pattern, find_context_by_id)
//! - `input` — Input submission and conversation clearing
//! - `streaming` — Stream append/done/error handling
//! - `config` — Configuration bar and theme controls

pub mod helpers;
pub mod input;
pub mod streaming;
pub mod config;

// Re-export helpers for external use
pub use helpers::{clean_llm_id_prefix, parse_context_pattern, find_context_by_id};

use crate::constants::{SCROLL_ACCEL_INCREMENT, SCROLL_ACCEL_MAX};
use crate::state::{ContextElement, ContextType, State};

#[derive(Debug, Clone)]
pub enum Action {
    InputChar(char),
    InsertText(String),
    PasteText(String),
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
    StreamDone { _input_tokens: usize, output_tokens: usize, cache_hit_tokens: usize, cache_miss_tokens: usize, stop_reason: Option<String> },
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
    ConfigSelectDeepSeekModel(crate::llms::DeepSeekModel),
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
        Action::PasteText(text) => {
            // Store in paste buffers and insert sentinel marker at cursor
            let idx = state.paste_buffers.len();
            state.paste_buffers.push(text);
            let sentinel = format!("\x00{}\x00", idx);
            state.input.insert_str(state.input_cursor, &sentinel);
            state.input_cursor += sentinel.len();
            ActionResult::Nothing
        }
        Action::InputBackspace => {
            if state.input_cursor > 0 {
                // Check if we're at the end of a paste sentinel (\x00{idx}\x00)
                // The closing \x00 is at cursor-1
                let bytes = state.input.as_bytes();
                if bytes[state.input_cursor - 1] == 0 {
                    // Find the opening \x00 by scanning backwards past the index digits
                    let mut scan = state.input_cursor - 2; // skip closing \x00
                    while scan > 0 && bytes[scan] != 0 {
                        scan -= 1;
                    }
                    if bytes[scan] == 0 {
                        // Remove the entire sentinel from scan..cursor
                        state.input = format!(
                            "{}{}",
                            &state.input[..scan],
                            &state.input[state.input_cursor..]
                        );
                        state.input_cursor = scan;
                    }
                } else if state.input_cursor >= 2 && bytes[state.input_cursor - 1].is_ascii_digit() {
                    // Check if cursor is inside a sentinel (between \x00 and closing \x00)
                    // Scan backwards to see if we hit \x00 before any non-digit
                    let mut scan = state.input_cursor - 1;
                    while scan > 0 && bytes[scan].is_ascii_digit() {
                        scan -= 1;
                    }
                    if bytes[scan] == 0 {
                        // We're inside a sentinel — find the closing \x00
                        let mut end = state.input_cursor;
                        while end < bytes.len() && bytes[end] != 0 {
                            end += 1;
                        }
                        if end < bytes.len() && bytes[end] == 0 {
                            end += 1; // include closing \x00
                        }
                        state.input = format!(
                            "{}{}",
                            &state.input[..scan],
                            &state.input[end..]
                        );
                        state.input_cursor = scan;
                    } else {
                        // Not a sentinel — normal backspace
                        let prev = state.input[..state.input_cursor]
                            .char_indices()
                            .last()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        state.input.remove(prev);
                        state.input_cursor = prev;
                    }
                } else {
                    // Normal backspace — remove one character
                    let prev = state.input[..state.input_cursor]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    state.input.remove(prev);
                    state.input_cursor = prev;
                }
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
                let trimmed = before.trim_end();
                let word_start = if trimmed.is_empty() {
                    0
                } else {
                    trimmed.rfind(|c: char| c.is_whitespace())
                        .map(|i| i + 1)
                        .unwrap_or(0)
                };
                state.input = format!("{}{}", &state.input[..word_start], &state.input[state.input_cursor..]);
                state.input_cursor = word_start;
            }
            ActionResult::Nothing
        }
        Action::RemoveListItem => {
            if state.input_cursor > 0 {
                let before = &state.input[..state.input_cursor];
                let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                state.input = format!("{}{}", &state.input[..line_start], &state.input[state.input_cursor..]);
                state.input_cursor = line_start;
            }
            ActionResult::Nothing
        }
        Action::CursorHome => {
            let before_cursor = &state.input[..state.input_cursor];
            state.input_cursor = before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
            ActionResult::Nothing
        }
        Action::CursorEnd => {
            let after_cursor = &state.input[state.input_cursor..];
            state.input_cursor += after_cursor.find('\n').unwrap_or(after_cursor.len());
            ActionResult::Nothing
        }

        // === Delegated to submodules ===
        Action::InputSubmit => input::handle_input_submit(state),
        Action::ClearConversation => input::handle_clear_conversation(state),

        Action::NewContext => {
            let context_id = state.next_available_context_id();
            state.context.push(ContextElement {
                id: context_id,
                uid: None,
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
                result_command: None,
                result_command_hash: None,
                skill_prompt_id: None,
                cached_content: None,
                history_messages: None,
                cache_deprecated: false,
                cache_in_flight: false,
                last_refresh_ms: crate::core::panels::now_ms(),
                last_polled_ms: 0,
                content_hash: None,
                tmux_last_lines_hash: None,
                current_page: 0,
                total_pages: 1,
                full_token_count: 0,
            });
            ActionResult::Save
        }
        Action::SelectNextContext => {
            if !state.context.is_empty() {
                let mut sorted_indices: Vec<usize> = (0..state.context.len()).collect();
                sorted_indices.sort_by(|&a, &b| {
                    let id_a = state.context[a].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
                    let id_b = state.context[b].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
                    id_a.cmp(&id_b)
                });
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
                let mut sorted_indices: Vec<usize> = (0..state.context.len()).collect();
                sorted_indices.sort_by(|&a, &b| {
                    let id_a = state.context[a].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
                    let id_b = state.context[b].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
                    id_a.cmp(&id_b)
                });
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

        // === Streaming (delegated) ===
        Action::AppendChars(text) => streaming::handle_append_chars(state, &text),
        Action::StreamDone { _input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, ref stop_reason } => {
            streaming::handle_stream_done(state, _input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason)
        }
        Action::StreamError(e) => streaming::handle_stream_error(state, &e),

        Action::ScrollUp(amount) => {
            let accel_amount = amount * state.scroll_accel;
            state.scroll_offset = (state.scroll_offset - accel_amount).max(0.0);
            state.user_scrolled = true;
            state.scroll_accel = (state.scroll_accel + SCROLL_ACCEL_INCREMENT).min(SCROLL_ACCEL_MAX);
            ActionResult::Nothing
        }
        Action::ScrollDown(amount) => {
            let accel_amount = amount * state.scroll_accel;
            state.scroll_offset += accel_amount;
            state.scroll_accel = (state.scroll_accel + SCROLL_ACCEL_INCREMENT).min(SCROLL_ACCEL_MAX);
            ActionResult::Nothing
        }
        Action::StopStreaming => {
            if state.is_streaming {
                state.is_streaming = false;
                if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
                    ctx.token_count = ctx.token_count.saturating_sub(state.streaming_estimated_tokens);
                }
                state.streaming_estimated_tokens = 0;
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
            use std::process::Command;
            let _ = Command::new("tmux")
                .args(["send-keys", "-t", &pane_id, &keys])
                .output();
            if let Some(ctx) = state.context.iter_mut()
                .find(|c| c.tmux_pane_id.as_ref() == Some(&pane_id))
            {
                ctx.tmux_last_keys = Some(keys);
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
        Action::ConfigSelectDeepSeekModel(model) => {
            state.deepseek_model = model;
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

        // === Config bar/theme (delegated) ===
        Action::ConfigIncreaseSelectedBar => config::handle_config_increase_bar(state),
        Action::ConfigDecreaseSelectedBar => config::handle_config_decrease_bar(state),
        Action::ConfigNextTheme => config::handle_config_next_theme(state),
        Action::ConfigPrevTheme => config::handle_config_prev_theme(state),

        Action::OpenCommandPalette => {
            // Handled in app.rs directly
            ActionResult::Nothing
        }
        Action::SelectContextById(id) => {
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
