//! Action handling split into domain-focused modules.
//!
//! - `helpers` — Utility functions (clean_llm_id_prefix, parse_context_pattern, find_context_by_id)
//! - `input` — Input submission and conversation clearing
//! - `streaming` — Stream append/done/error handling
//! - `config` — Configuration bar and theme controls

pub mod config;
pub mod helpers;
pub mod input;
pub mod streaming;

// Re-export helpers for external use
pub use helpers::{clean_llm_id_prefix, find_context_by_id, parse_context_pattern};

use crate::constants::{SCROLL_ACCEL_INCREMENT, SCROLL_ACCEL_MAX};
use crate::state::{ContextElement, ContextType, State};

/// If cursor is inside a paste sentinel (\x00{idx}\x00), eject it to after the sentinel.
fn eject_cursor_from_sentinel(input: &str, cursor: usize) -> usize {
    let bytes = input.as_bytes();
    if cursor == 0 || cursor >= bytes.len() {
        return cursor;
    }
    // Scan backwards from cursor to see if we hit \x00 before any non-digit
    let mut scan = cursor;
    while scan > 0 {
        let b = bytes[scan - 1];
        if b == 0 {
            // Found opening \x00 — we're inside a sentinel. Find the closing \x00.
            let mut end = cursor;
            while end < bytes.len() && bytes[end] != 0 {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == 0 {
                return end + 1; // after closing \x00
            }
            return cursor;
        } else if b.is_ascii_digit() {
            scan -= 1;
        } else {
            break; // Not inside a sentinel
        }
    }
    cursor
}

// Re-export Action/ActionResult from cp-base (shared with module crates)
pub use cp_base::actions::{Action, ActionResult};

pub fn apply_action(state: &mut State, action: Action) -> ActionResult {
    // Reset scroll acceleration on non-scroll actions
    if !matches!(action, Action::ScrollUp(_) | Action::ScrollDown(_)) {
        state.scroll_accel = 1.0;
    }

    match action {
        Action::InputChar(c) => {
            state.input.insert(state.input_cursor, c);
            state.input_cursor += c.len_utf8();

            // After typing a space or newline, check if preceding text is a /command
            if (c == ' ' || c == '\n') && !state.commands.is_empty() {
                // Find start of current "word" — scan back past the space we just inserted
                let before_space = state.input_cursor - 1; // position of the space
                let bytes = state.input.as_bytes();
                let mut word_start = before_space;
                // Scan backwards to find word boundary (newline, space, or sentinel \x00)
                while word_start > 0 {
                    let prev_byte = bytes[word_start - 1];
                    if prev_byte == b'\n' || prev_byte == b' ' || prev_byte == 0 {
                        break;
                    }
                    word_start -= 1;
                }
                let word = &state.input[word_start..before_space];
                if let Some(cmd_name) = word.strip_prefix('/')
                    && let Some(cmd) = state.commands.iter().find(|c| c.id == cmd_name)
                {
                    let content = cmd.content.clone();
                    let label = cmd_name.to_string();
                    let idx = state.paste_buffers.len();
                    state.paste_buffers.push(content);
                    state.paste_buffer_labels.push(Some(label.clone()));
                    let sentinel = format!("\x00{}\x00", idx);
                    // Replace /command<space> with sentinel
                    state.input =
                        format!("{}{}\n{}", &state.input[..word_start], sentinel, &state.input[state.input_cursor..],);
                    state.input_cursor = word_start + sentinel.len() + 1;
                }
            }

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
            state.paste_buffer_labels.push(None);
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
                        state.input = format!("{}{}", &state.input[..scan], &state.input[state.input_cursor..]);
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
                        state.input = format!("{}{}", &state.input[..scan], &state.input[end..]);
                        state.input_cursor = scan;
                    } else {
                        // Not a sentinel — normal backspace
                        let prev = state.input[..state.input_cursor].char_indices().last().map(|(i, _)| i).unwrap_or(0);
                        state.input.remove(prev);
                        state.input_cursor = prev;
                    }
                } else {
                    // Normal backspace — remove one character
                    let prev = state.input[..state.input_cursor].char_indices().last().map(|(i, _)| i).unwrap_or(0);
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
                    let word_start = trimmed.rfind(|c: char| c.is_whitespace()).map(|i| i + 1).unwrap_or(0);
                    state.input_cursor = word_start;
                }
                state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
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
                state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
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
                    trimmed.rfind(|c: char| c.is_whitespace()).map(|i| i + 1).unwrap_or(0)
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
            state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
            ActionResult::Nothing
        }
        Action::CursorEnd => {
            let after_cursor = &state.input[state.input_cursor..];
            state.input_cursor += after_cursor.find('\n').unwrap_or(after_cursor.len());
            state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
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
                context_type: ContextType::new(ContextType::CONVERSATION),
                name: format!("Conv {}", state.context.len()),
                token_count: 0,
                file_path: None,
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
                skill_prompt_id: None,
                cached_content: None,
                history_messages: None,
                cache_deprecated: false,
                cache_in_flight: false,
                last_refresh_ms: crate::core::panels::now_ms(),
                content_hash: None,
                source_hash: None,
                current_page: 0,
                total_pages: 1,
                full_token_count: 0,
                panel_cache_hit: false,
                panel_total_cost: 0.0,
            });
            ActionResult::Save
        }
        Action::SelectNextContext => {
            if !state.context.is_empty() {
                let mut sorted_indices: Vec<usize> = (0..state.context.len()).collect();
                sorted_indices.sort_by(|&a, &b| {
                    let id_a = state.context[a]
                        .id
                        .strip_prefix('P')
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(usize::MAX);
                    let id_b = state.context[b]
                        .id
                        .strip_prefix('P')
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(usize::MAX);
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
                    let id_a = state.context[a]
                        .id
                        .strip_prefix('P')
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(usize::MAX);
                    let id_b = state.context[b]
                        .id
                        .strip_prefix('P')
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(usize::MAX);
                    id_a.cmp(&id_b)
                });
                let current_pos = sorted_indices.iter().position(|&i| i == state.selected_context).unwrap_or(0);
                let prev_pos = if current_pos == 0 { sorted_indices.len() - 1 } else { current_pos - 1 };
                state.selected_context = sorted_indices[prev_pos];
                state.scroll_offset = 0.0;
                state.user_scrolled = false;
            }
            ActionResult::Nothing
        }

        // === Streaming (delegated) ===
        Action::AppendChars(text) => streaming::handle_append_chars(state, &text),
        Action::StreamDone { _input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, ref stop_reason } => {
            streaming::handle_stream_done(
                state,
                _input_tokens,
                output_tokens,
                cache_hit_tokens,
                cache_miss_tokens,
                stop_reason,
            )
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
                if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::CONVERSATION) {
                    ctx.token_count = ctx.token_count.saturating_sub(state.streaming_estimated_tokens);
                }
                state.streaming_estimated_tokens = 0;
                if let Some(msg) = state.messages.last_mut()
                    && msg.role == "assistant"
                    && !msg.content.is_empty()
                {
                    msg.content.push_str("\n[Stopped]");
                }
                ActionResult::StopStream
            } else {
                ActionResult::Nothing
            }
        }
        Action::TmuxSendKeys { pane_id, keys } => {
            use std::process::Command;
            let _ = Command::new("tmux").args(["send-keys", "-t", &pane_id, &keys]).output();
            if let Some(ctx) = state.context.iter_mut().find(|c| c.tmux_pane_id.as_ref() == Some(&pane_id)) {
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
