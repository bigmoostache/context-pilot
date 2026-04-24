//! Action handling split into domain-focused modules.
//!
//! - `helpers` — Utility functions (`clean_llm_id_prefix`, `parse_context_pattern`, `find_context_by_id`)
//! - `input` — Input submission and conversation clearing
//! - `streaming` — Stream append/done/error handling
//! - `config` — Configuration bar and theme controls
//! - `cursor` — Cursor movement, text editing, and command expansion

/// Configuration bar and theme controls.
pub(crate) mod config;
/// Cursor movement, text editing, and command expansion.
mod cursor;
/// Utility functions for action handling.
pub(crate) mod helpers;
/// Input submission and conversation clearing.
pub(crate) mod input;
/// Stream append/done/error handling.
pub(crate) mod streaming;

// Re-export helpers for external use
pub(crate) use helpers::{clean_llm_id_prefix, find_context_by_id, parse_context_pattern};

use crate::infra::constants::{SCROLL_ACCEL_INCREMENT, SCROLL_ACCEL_MAX};
use crate::state::{Entry, Kind, State, StreamPhase};
use cp_mod_prompt::types::PromptState;

// Re-export Action/ActionResult from cp-base (shared with module crates)
pub(crate) use cp_base::state::actions::{Action, ActionResult};

/// Dispatch an `Action` to the appropriate handler, returning the resulting `ActionResult`.
pub(crate) fn apply_action(state: &mut State, action: Action) -> ActionResult {
    // Reset scroll acceleration on non-scroll actions
    if !matches!(action, Action::ScrollUp(_) | Action::ScrollDown(_)) {
        state.scroll_accel = 1.0;
    }

    match action {
        Action::InputChar(ch) => {
            state.input.insert(state.input_cursor, ch);
            state.input_cursor = state.input_cursor.saturating_add(ch.len_utf8());

            // Check if '@' was typed and should trigger autocomplete
            if ch == '@' {
                let anchor_pos = state.input_cursor.saturating_sub(1); // position of '@'
                // Trigger if at start of input OR preceded by whitespace
                let should_trigger = anchor_pos == 0
                    || state
                        .input
                        .as_bytes()
                        .get(anchor_pos.saturating_sub(1))
                        .is_some_and(|&b| b == b' ' || b == b'\n' || b == b'\t');
                if should_trigger {
                    // Populate entries for root directory
                    let filter = cp_mod_tree::types::TreeState::get(state).filter.clone();
                    let entries = cp_mod_tree::tools::list_dir_entries(&filter, "", "");
                    if let Some(ac) = state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() {
                        ac.activate(anchor_pos);
                        ac.set_matches(entries);
                    }
                }
            }

            // After typing a space or newline, check if preceding text is a /command
            if (ch == ' ' || ch == '\n') && !PromptState::get(state).commands.is_empty() {
                cursor::handle_command_expansion(state);
            }

            ActionResult::Nothing
        }
        Action::InsertText(text) => {
            state.input.insert_str(state.input_cursor, &text);
            state.input_cursor = state.input_cursor.saturating_add(text.len());
            ActionResult::Nothing
        }
        Action::PasteText(text) => {
            // Store in paste buffers and insert sentinel marker at cursor
            let idx = state.paste_buffers.len();
            state.paste_buffers.push(text);
            state.paste_buffer_labels.push(None);
            let sentinel = format!("\x00{idx}\x00");
            state.input.insert_str(state.input_cursor, &sentinel);
            state.input_cursor = state.input_cursor.saturating_add(sentinel.len());
            ActionResult::Nothing
        }
        Action::InputBackspace => {
            cursor::handle_input_backspace(state);
            ActionResult::Nothing
        }
        Action::InputDelete => {
            if state.input_cursor < state.input.len() {
                let _r = state.input.remove(state.input_cursor);
            }
            ActionResult::Nothing
        }
        Action::CursorWordLeft => {
            cursor::handle_cursor_word_left(state);
            ActionResult::Nothing
        }
        Action::CursorWordRight => {
            cursor::handle_cursor_word_right(state);
            ActionResult::Nothing
        }
        Action::DeleteWordLeft => {
            cursor::handle_delete_word_left(state);
            ActionResult::Nothing
        }
        Action::RemoveListItem => {
            cursor::handle_remove_list_item(state);
            ActionResult::Nothing
        }
        Action::CursorHome => {
            cursor::handle_cursor_home(state);
            ActionResult::Nothing
        }
        Action::CursorEnd => {
            cursor::handle_cursor_end(state);
            ActionResult::Nothing
        }

        // === Delegated to submodules ===
        Action::InputSubmit => input::handle_input_submit(state),
        Action::ClearConversation => input::handle_clear_conversation(state),

        Action::NewContext => {
            let context_id = state.next_available_context_id();
            state.context.push(Entry {
                id: context_id,
                uid: None,
                context_type: Kind::new(Kind::CONVERSATION),
                name: format!("Conv {}", state.context.len()),
                token_count: 0,
                metadata: std::collections::HashMap::new(),
                cached_content: None,
                history_messages: None,
                cache_deprecated: false,
                cache_in_flight: false,
                last_refresh_ms: crate::app::panels::now_ms(),
                content_hash: None,
                source_hash: None,
                current_page: 0,
                total_pages: 1,
                full_token_count: 0,
                panel_cache_hit: false,
                panel_total_cost: 0.0,
                freeze_count: 0,
                total_freezes: 0,
                total_cache_misses: 0,
                last_emitted_content: None,
                last_emitted_hash: None,
                last_emitted_context: None,
            });
            ActionResult::Save
        }
        Action::SelectNextContext => {
            select_context(state, true);
            ActionResult::Nothing
        }
        Action::SelectPrevContext => {
            select_context(state, false);
            ActionResult::Nothing
        }

        // === Streaming (delegated) ===
        Action::AppendChars(text) => streaming::handle_append_chars(state, &text),
        Action::StreamDone { input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, ref stop_reason } => {
            let event = streaming::StreamDoneEvent {
                input_tokens,
                output_tokens,
                cache_hit: cache_hit_tokens,
                cache_miss: cache_miss_tokens,
                stop_reason: stop_reason.as_deref(),
            };
            streaming::handle_stream_done(state, &event)
        }
        Action::StreamError(e) => streaming::handle_stream_error(state, &e),

        Action::ScrollUp(amount) => {
            let accel_amount = amount * state.scroll_accel;
            state.scroll_offset = (state.scroll_offset - accel_amount).max(0.0);
            state.flags.stream.user_scrolled = true;
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
            if state.flags.stream.phase.is_streaming() {
                state.flags.stream.phase.transition(StreamPhase::Idle);
                if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type.as_str() == Kind::CONVERSATION) {
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
            let _r = Command::new("tmux").args(["send-keys", "-t", &pane_id, &keys]).output();
            if let Some(ctx) = state.context.iter_mut().find(|c| c.get_meta_str("tmux_pane_id") == Some(&pane_id)) {
                ctx.set_meta("tmux_last_keys", &keys);
                ctx.cache_deprecated = true;
            }
            ActionResult::Nothing
        }
        Action::ResetSessionCosts => {
            state.cache_hit_tokens = 0;
            state.cache_miss_tokens = 0;
            state.total_output_tokens = 0;
            state.guard_rail_blocked = None;
            ActionResult::Save
        }
        Action::TogglePerfMonitor => {
            state.flags.ui.perf_enabled = crate::ui::perf::PERF.toggle();
            state.flags.ui.dirty = true;
            ActionResult::Nothing
        }
        Action::ToggleConfigView => {
            state.flags.config.config_view = !state.flags.config.config_view;
            state.flags.ui.dirty = true;
            ActionResult::Nothing
        }
        Action::ConfigSelectProvider(provider) => {
            state.llm_provider = provider;
            state.flags.lifecycle.api_check_in_progress = true;
            state.api_check_result = None;
            state.flags.ui.dirty = true;
            ActionResult::StartApiCheck
        }
        Action::ConfigSelectAnthropicModel(m) => {
            state.anthropic_model = m;
            config::api_check(state)
        }
        Action::ConfigSelectGrokModel(m) => {
            state.grok_model = m;
            config::api_check(state)
        }
        Action::ConfigSelectGroqModel(m) => {
            state.groq_model = m;
            config::api_check(state)
        }
        Action::ConfigSelectDeepSeekModel(m) => {
            state.deepseek_model = m;
            config::api_check(state)
        }
        Action::ConfigSelectMiniMaxModel(m) => {
            state.minimax_model = m;
            config::api_check(state)
        }
        Action::ConfigSelectNextBar => {
            state.config_selected_bar = config::next_bar(state.config_selected_bar);
            state.flags.ui.dirty = true;
            ActionResult::Nothing
        }
        Action::ConfigSelectPrevBar => {
            state.config_selected_bar = config::prev_bar(state.config_selected_bar);
            state.flags.ui.dirty = true;
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
                state.flags.stream.user_scrolled = false;
                state.flags.ui.dirty = true;
            }
            ActionResult::Nothing
        }
        Action::ConfigToggleAutoContinue => {
            let spine = cp_mod_spine::types::SpineState::get_mut(state);
            spine.config.continue_until_todos_done = !spine.config.continue_until_todos_done;
            state.flags.ui.dirty = true;
            ActionResult::Save
        }
        Action::ConfigSelectSecondaryProvider(provider) => {
            state.secondary_provider = provider;
            state.flags.ui.dirty = true;
            ActionResult::Save
        }
        Action::ConfigSelectSecondaryAnthropicModel(m) => {
            state.secondary_anthropic_model = m;
            state.flags.ui.dirty = true;
            ActionResult::Save
        }
        Action::ConfigSelectSecondaryGrokModel(m) => {
            state.secondary_grok_model = m;
            state.flags.ui.dirty = true;
            ActionResult::Save
        }
        Action::ConfigSelectSecondaryGroqModel(m) => {
            state.secondary_groq_model = m;
            state.flags.ui.dirty = true;
            ActionResult::Save
        }
        Action::ConfigSelectSecondaryDeepSeekModel(m) => {
            state.secondary_deepseek_model = m;
            state.flags.ui.dirty = true;
            ActionResult::Save
        }
        Action::ConfigSelectSecondaryMiniMaxModel(m) => {
            state.secondary_minimax_model = m;
            state.flags.ui.dirty = true;
            ActionResult::Save
        }
        Action::ConfigToggleReverie => {
            state.flags.config.reverie_enabled = !state.flags.config.reverie_enabled;
            state.flags.ui.dirty = true;
            ActionResult::Save
        }
        Action::ConfigToggleSecondaryMode => {
            state.flags.config.config_secondary_mode = !state.flags.config.config_secondary_mode;
            state.flags.ui.dirty = true;
            ActionResult::Nothing
        }
        Action::CycleSidebarMode => {
            state.sidebar_mode = state.sidebar_mode.next();
            state.flags.ui.dirty = true;
            ActionResult::Nothing
        }
        Action::None => ActionResult::Nothing,
    }
}

/// Navigate to the next (`forward=true`) or previous (`forward=false`) context panel,
/// sorted by numeric panel ID.
fn select_context(state: &mut State, forward: bool) {
    if state.context.is_empty() {
        return;
    }
    let mut sorted: Vec<usize> = (0..state.context.len()).collect();
    sorted.sort_by(|&a, &b| {
        let id_a = state
            .context
            .get(a)
            .and_then(|el| el.id.strip_prefix('P'))
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        let id_b = state
            .context
            .get(b)
            .and_then(|el| el.id.strip_prefix('P'))
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        id_a.cmp(&id_b)
    });
    let cur = sorted.iter().position(|&i| i == state.selected_context).unwrap_or(0);
    let next = if forward {
        config::wrap_next(cur, sorted.len())
    } else if cur == 0 {
        sorted.len().saturating_sub(1)
    } else {
        cur.saturating_sub(1)
    };
    let Some(&selected) = sorted.get(next) else { return };
    state.selected_context = selected;
    state.scroll_offset = 0.0;
    state.flags.stream.user_scrolled = false;
}
