//! Action handling split into domain-focused modules.
//!
//! - `helpers` — Utility functions (`clean_llm_id_prefix`, `parse_context_pattern`, `find_context_by_id`)
//! - `input` — Input submission and conversation clearing
//! - `streaming` — Stream append/done/error handling
//! - `config` — Configuration bar and theme controls
//! - `cursor` — Cursor movement, text editing, and command expansion
//! - `history` — Prompt history navigation and panel clipboard copy
//! - `threads` — Thread action handlers (`Thread*` variants)
//!
//! [`apply_action`] itself is a single flat `match` over the closed [`Action`]
//! enum — the dispatch twin of a flat aggregate initializer. Every arm delegates
//! to a one-line handler (here or in a sibling module), so the body is a straight
//! variant→handler table.
//!
//! ## Why one flat match (and the lone length allowance)
//!
//! `Action` is an exhaustive ~70-variant enum. Splitting the dispatch across
//! helper functions is impossible under the line cap without a catch-all arm:
//! Rust requires each `match` to cover every variant, and a `_` / bare-binding
//! catch-all trips `wildcard_enum_match_arm` (forbid). A by-value match also
//! avoids `pattern_type_mismatch` (payload fields bind owned, not by-ref). The
//! only residual is length — intrinsic to a 70-action enum — so `apply_action`
//! carries a single `clippy::too_many_lines` allowance, exactly like the flat
//! `State::default` initializer.

/// Configuration bar and theme controls.
pub(crate) mod config;
/// Cursor movement, text editing, and command expansion.
mod cursor;
/// Utility functions for action handling.
pub(crate) mod helpers;
/// Prompt history navigation and panel clipboard copy.
mod history;
/// Input submission and conversation clearing.
pub(crate) mod input;
/// Stream append/done/error handling.
pub(crate) mod streaming;
/// Thread action handlers (Thread* variants).
mod threads;

// Re-export helpers for external use
pub(crate) use helpers::{clean_llm_id_prefix, find_context_by_id, parse_context_pattern, switch_to_panel};

// Re-export Action/ActionResult from cp-base (shared with module crates)
pub(crate) use cp_base::state::actions::{Action, ActionResult};

use crate::infra::constants::{SCROLL_ACCEL_INCREMENT, SCROLL_ACCEL_MAX};
use crate::state::{Kind, State, StreamPhase};
use cp_base::cast::float_math;

// ── Multi-line leaf handlers (kept out of the match so each arm stays 1 line) ─

/// Stop an in-progress stream: mark idle, roll back the streaming token
/// estimate, and append a `[Stopped]` marker to the last assistant message.
fn handle_stop_streaming(state: &mut State) -> ActionResult {
    if !state.flags.stream.phase.is_streaming() {
        return ActionResult::Nothing;
    }
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
}

/// Zero every session, per-stream, and per-tick cost/token accumulator and clear
/// the guard-rail + cache-engine snapshots — the full effect of `ResetSessionCosts`.
fn reset_session_costs(state: &mut State) {
    state.cache_hit_tokens = 0;
    state.cache_miss_tokens = 0;
    state.total_output_tokens = 0;
    state.uncached_input_tokens = 0;
    state.cost_hit_usd = 0.0f64;
    state.cost_miss_usd = 0.0f64;
    state.cost_output_usd = 0.0f64;
    state.stream_cost_hit_usd = 0.0f64;
    state.stream_cost_miss_usd = 0.0f64;
    state.stream_cost_output_usd = 0.0f64;
    state.tick_cost_hit_usd = 0.0f64;
    state.tick_cost_miss_usd = 0.0f64;
    state.tick_cost_output_usd = 0.0f64;
    state.guard_rail_blocked = None;
    state.cache_engine_json = None;
}

/// Copy the Ctrl+I index overlay's plain-text form to the clipboard via `pbcopy`.
fn handle_copy_index_overlay(state: &mut State) {
    let ir = crate::ui::search_overlay::build_search_index_overlay(state);
    let text = crate::ui::search_overlay::text::build_overlay_text(&ir);
    if let Ok(mut child) = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write as _;
            let _r = stdin.write_all(text.as_bytes());
        }
        let _r = child.wait();
    }
    state.flags.overlays.copied_flash_ms = crate::app::panels::now_ms();
    state.flags.ui.dirty = true;
}

/// Insert a typed character at the cursor, replacing any active selection,
/// then trigger `@`-autocomplete or `/command` expansion when warranted.
fn handle_input_char(state: &mut State, ch: char) {
    let _r = cursor::delete_selection(state);
    state.input.insert(state.input_cursor, ch);
    state.input_cursor = state.input_cursor.saturating_add(ch.len_utf8());

    // '@' at input start or after whitespace opens directory autocomplete.
    if ch == '@' {
        let anchor_pos = state.input_cursor.saturating_sub(1);
        let should_trigger = anchor_pos == 0
            || state
                .input
                .as_bytes()
                .get(anchor_pos.saturating_sub(1))
                .is_some_and(|&b| b == b' ' || b == b'\n' || b == b'\t');
        if should_trigger {
            let filter = cp_mod_tree::types::TreeState::get(state).filter.clone();
            let entries = cp_mod_tree::tools::list_dir_entries(&filter, "", "");
            if let Some(ac) = state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() {
                ac.activate(anchor_pos);
                ac.set_matches(entries);
            }
        }
    }

    // A trailing space/newline may complete a /command token.
    if (ch == ' ' || ch == '\n')
        && !cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Command).is_empty()
    {
        cursor::handle_command_expansion(state);
    }
}

/// Insert literal text at the cursor, replacing any active selection.
fn handle_insert_text(state: &mut State, text: &str) {
    let _r = cursor::delete_selection(state);
    state.input.insert_str(state.input_cursor, text);
    state.input_cursor = state.input_cursor.saturating_add(text.len());
}

/// Stash a pasted blob in a paste buffer and insert a `\x00{idx}\x00` sentinel
/// at the cursor (expanded to the real text at submit time).
fn handle_paste_text(state: &mut State, text: String) {
    let _r = cursor::delete_selection(state);
    let idx = state.paste_buffers.len();
    state.paste_buffers.push(text);
    state.paste_buffer_labels.push(None);
    let sentinel = format!("\x00{idx}\x00");
    state.input.insert_str(state.input_cursor, &sentinel);
    state.input_cursor = state.input_cursor.saturating_add(sentinel.len());
}

/// Delete the selection if any, else the character to the right of the cursor.
fn handle_input_delete(state: &mut State) {
    if !cursor::delete_selection(state) && state.input_cursor < state.input.len() {
        let _r = state.input.remove(state.input_cursor);
    }
}

/// Scroll the conversation up (`up = true`) or down, applying + growing the
/// scroll-acceleration factor. Sets `user_scrolled` when scrolling up.
const fn handle_scroll(state: &mut State, amount: f32, up: bool) {
    let accel = float_math::mul_f32(amount, state.scroll_accel);
    if up {
        state.scroll_offset = float_math::sub_f32(state.scroll_offset, accel).max(0.0);
        state.flags.stream.user_scrolled = true;
    } else {
        state.scroll_offset = float_math::add_f32(state.scroll_offset, accel);
    }
    state.scroll_accel = float_math::add_f32(state.scroll_accel, SCROLL_ACCEL_INCREMENT).min(SCROLL_ACCEL_MAX);
}

/// Send `keys` to the tmux `pane_id` and record them on the matching context.
fn handle_tmux_send_keys(state: &mut State, pane_id: &str, keys: &str) {
    let _r = std::process::Command::new("tmux").args(["send-keys", "-t", pane_id, keys]).output();
    if let Some(ctx) = state.context.iter_mut().find(|c| c.get_meta_str("tmux_pane_id") == Some(pane_id)) {
        ctx.set_meta("tmux_last_keys", &keys.to_owned());
        ctx.cache_deprecated = true;
    }
}

/// Record the trimmed input into prompt history (resetting nav), then delegate
/// to the input module's submit handler.
fn handle_input_submit_action(state: &mut State) -> ActionResult {
    history::ensure_history_nav(state);
    let trimmed = state.input.trim_end().to_owned();
    let nav = state.ext_mut::<history::PromptHistoryNav>();
    if !trimmed.is_empty() {
        nav.push(trimmed);
    }
    nav.reset_nav();
    input::handle_input_submit(state)
}

/// Switch to the panel whose id equals `id`, if one exists.
fn handle_select_context_by_id(state: &mut State, id: &str) {
    if let Some(idx) = state.context.iter().position(|c| c.id == id) {
        switch_to_panel(state, idx);
        state.flags.ui.dirty = true;
    }
}

/// Toggle the perf monitor overlay and mark the UI dirty.
fn toggle_perf_monitor(state: &mut State) {
    state.flags.ui.perf_enabled = crate::ui::perf::PERF.toggle();
    state.flags.ui.dirty = true;
}

/// Bump the think-reminder threshold up/down, clamped so it never exceeds `-1`
/// on the way up, and mark the UI dirty.
fn think_threshold(state: &mut State, up: bool) {
    let ts = state.ext_mut::<crate::modules::questions::ThinkState>();
    ts.reminder_threshold =
        if up { ts.reminder_threshold.saturating_add(1).min(-1i32) } else { ts.reminder_threshold.saturating_sub(1) };
    state.flags.ui.dirty = true;
}

/// Cycle to the next view mode, resetting scroll so the new view starts clean.
const fn cycle_view_mode(state: &mut State) {
    state.view_mode = state.view_mode.next();
    state.scroll_offset = 0.0;
    state.flags.stream.user_scrolled = false;
    state.flags.ui.dirty = true;
}

// ── Entry point ──────────────────────────────────────────────────────────────

/// Dispatch an `Action` to its handler, returning the resulting [`ActionResult`].
///
/// A single flat `match` over every `Action` variant; each arm is a one-line
/// delegation. See the module docs for why this is one exhaustive match with a
/// lone `clippy::too_many_lines` allowance rather than split sub-dispatchers.
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive dispatch over ~70 Action variants; splitting requires either a forbidden wildcard catch-all (wildcard_enum_match_arm) or a duplicated giant or-pattern, both worse than one flat variant→handler table — the dispatch twin of the flat State::default initializer"
)]
pub(crate) fn apply_action(state: &mut State, action: Action) -> ActionResult {
    // Reset scroll acceleration on any non-scroll action.
    if !matches!(action, Action::ScrollUp(_) | Action::ScrollDown(_)) {
        state.scroll_accel = 1.0;
    }

    match action {
        // ── Cursor / text-edit / history (side-effect only → Nothing) ────────
        Action::InputBackspace => cursor::handle_input_backspace(state),
        Action::InputDelete => handle_input_delete(state),
        Action::DeleteWordLeft => cursor::handle_delete_word_left(state),
        Action::RemoveListItem => cursor::handle_remove_list_item(state),
        Action::CursorWordLeft => cursor::handle_cursor_word_left(state),
        Action::CursorWordRight => cursor::handle_cursor_word_right(state),
        Action::CursorHome => cursor::handle_cursor_home(state),
        Action::CursorEnd => cursor::handle_cursor_end(state),
        Action::CursorLeft => cursor::handle_cursor_left(state),
        Action::CursorRight => cursor::handle_cursor_right(state),
        Action::CursorLeftSelect => cursor::handle_cursor_left_select(state),
        Action::CursorRightSelect => cursor::handle_cursor_right_select(state),
        Action::CursorWordLeftSelect => cursor::handle_cursor_word_left_select(state),
        Action::CursorWordRightSelect => cursor::handle_cursor_word_right_select(state),
        Action::CursorHomeSelect => cursor::handle_cursor_home_select(state),
        Action::CursorEndSelect => cursor::handle_cursor_end_select(state),
        Action::SelectAll => cursor::handle_select_all(state),
        Action::HistoryPrev => history::handle_history_prev(state),
        Action::HistoryNext => history::handle_history_next(state),
        Action::CopyPanelContent => history::handle_copy_panel_content(state),

        // ── Text insertion (payload) ─────────────────────────────────────────
        Action::InputChar(ch) => {
            return {
                handle_input_char(state, ch);
                ActionResult::Nothing
            };
        }
        Action::InsertText(text) => {
            return {
                handle_insert_text(state, &text);
                ActionResult::Nothing
            };
        }
        Action::PasteText(text) => {
            return {
                handle_paste_text(state, text);
                ActionResult::Nothing
            };
        }

        // ── Streaming / scroll / tmux (payload) ──────────────────────────────
        Action::AppendChars(text) => return streaming::handle_append_chars(state, &text),
        Action::StreamDone { input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason } => {
            let event = streaming::StreamDoneEvent {
                input_tokens,
                output_tokens,
                cache_hit: cache_hit_tokens,
                cache_miss: cache_miss_tokens,
                stop_reason: stop_reason.as_deref(),
            };
            return streaming::handle_stream_done(state, &event);
        }
        Action::StreamError(e) => return streaming::handle_stream_error(state, &e),
        Action::ScrollUp(amount) => handle_scroll(state, amount, true),
        Action::ScrollDown(amount) => handle_scroll(state, amount, false),
        Action::StopStreaming => return handle_stop_streaming(state),
        Action::TmuxSendKeys { pane_id, keys } => handle_tmux_send_keys(state, &pane_id, &keys),

        // ── Context navigation ───────────────────────────────────────────────
        Action::NewContext => return helpers::create_new_context(state),
        Action::SelectNextContext => helpers::select_context(state, true),
        Action::SelectPrevContext => helpers::select_context(state, false),
        Action::PageDynamicNext => helpers::page_dynamic(state, true),
        Action::PageDynamicPrev => helpers::page_dynamic(state, false),
        Action::SelectContextById(id) => handle_select_context_by_id(state, &id),

        // ── Config / toggles / theme ─────────────────────────────────────────
        Action::TogglePerfMonitor => toggle_perf_monitor(state),
        Action::ToggleConfigView => {
            state.flags.config.config_view = !state.flags.config.config_view;
            state.flags.ui.dirty = true;
        }
        Action::ToggleIndexOverlay => {
            state.flags.overlays.index_status = !state.flags.overlays.index_status;
            state.flags.ui.dirty = true;
        }
        Action::CopyIndexOverlay => handle_copy_index_overlay(state),
        Action::ConfigToggleReverie => {
            state.flags.config.reverie_enabled = !state.flags.config.reverie_enabled;
            state.flags.ui.dirty = true;
            return ActionResult::Save;
        }
        Action::ConfigSelectNextBar => {
            state.config_selected_bar = config::next_bar(state.config_selected_bar);
            state.flags.ui.dirty = true;
        }
        Action::ConfigSelectPrevBar => {
            state.config_selected_bar = config::prev_bar(state.config_selected_bar);
            state.flags.ui.dirty = true;
        }
        Action::ConfigIncreaseSelectedBar => return config::handle_config_increase_bar(state),
        Action::ConfigDecreaseSelectedBar => return config::handle_config_decrease_bar(state),
        Action::ConfigNextTheme => return config::handle_config_next_theme(state),
        Action::ConfigPrevTheme => return config::handle_config_prev_theme(state),
        Action::ConfigToggleAutoContinue => {
            let spine = cp_mod_spine::types::SpineState::get_mut(state);
            spine.config.continue_until_todos_done = !spine.config.continue_until_todos_done;
            state.flags.ui.dirty = true;
            return ActionResult::Save;
        }
        Action::ConfigThinkThresholdUp => {
            think_threshold(state, true);
            return ActionResult::Save;
        }
        Action::ConfigThinkThresholdDown => {
            think_threshold(state, false);
            return ActionResult::Save;
        }

        // ── Provider / model selection (each kicks off an API health check) ──
        Action::ConfigSelectProvider(provider) => {
            state.llm_provider = provider;
            state.flags.lifecycle.api_check_in_progress = true;
            state.api_check_result = None;
            state.flags.ui.dirty = true;
            return ActionResult::StartApiCheck;
        }
        Action::ConfigSelectAnthropicModel(m) => {
            return {
                state.anthropic_model = m;
                config::api_check(state)
            };
        }
        Action::ConfigSelectGrokModel(m) => {
            return {
                state.grok_model = m;
                config::api_check(state)
            };
        }
        Action::ConfigSelectGroqModel(m) => {
            return {
                state.groq_model = m;
                config::api_check(state)
            };
        }
        Action::ConfigSelectDeepSeekModel(m) => {
            return {
                state.deepseek_model = m;
                config::api_check(state)
            };
        }
        Action::ConfigSelectMiniMaxModel(m) => {
            return {
                state.minimax_model = m;
                config::api_check(state)
            };
        }
        Action::ConfigSelectClaudeCodeV2Model(m) => {
            return {
                state.claude_code_v2_model = m;
                config::api_check(state)
            };
        }

        // ── Misc top-level ───────────────────────────────────────────────────
        Action::InputSubmit => return handle_input_submit_action(state),
        Action::ClearConversation => return input::handle_clear_conversation(state),
        Action::ResetSessionCosts => {
            reset_session_costs(state);
            return ActionResult::Save;
        }
        // Handled in app.rs directly; a no-op here.
        Action::OpenCommandPalette | Action::None => {}
        Action::CycleViewMode => cycle_view_mode(state),

        // ── Threads (all no-data variants delegate to the thread dispatcher) ─
        Action::ThreadSelectNext
        | Action::ThreadSelectPrev
        | Action::ThreadCreateStart
        | Action::ThreadCreateCancel
        | Action::ThreadArchiveStart
        | Action::ThreadArchiveConfirm
        | Action::ThreadArchiveCancel
        | Action::ThreadToggleArchivedView => return threads::dispatch(state, &action),
    }
    ActionResult::Nothing
}
