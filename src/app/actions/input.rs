use crate::state::persistence::message::record_prompt_history;
use crate::state::persistence::{delete_message, save_message};
use crate::state::{Kind, Message, State, estimate_tokens};
use cp_mod_prompt::types::PromptItem;
use cp_mod_spine::types::{NotificationType, SpineState};
use cp_mod_threads::types::{FocusState, ThreadAuthor, ThreadMessage, ThreadStatus, ThreadsState};

use super::ActionResult;
use super::helpers::{find_context_by_id, parse_context_pattern};
use crate::modules::all_modules;

/// Handle `InputSubmit` action — context switching, message creation, stream start.
pub(crate) fn handle_input_submit(state: &mut State) -> ActionResult {
    if state.input.is_empty() {
        return ActionResult::Nothing;
    }

    // Context switching is always allowed, even during streaming
    if let Some(id) = parse_context_pattern(&state.input)
        && let Some(index) = find_context_by_id(state, &id)
    {
        super::switch_to_panel(state, index);
        state.input.clear();
        state.input_cursor = 0;
        return ActionResult::Nothing;
    }

    // Threads view: route input to the selected thread instead of conversation
    if state.view_mode == cp_base::state::data::config::ViewMode::Threads {
        return handle_thread_input_submit(state);
    }

    let commands = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Command);
    let content = replace_commands(&state.input, &commands);
    // Expand paste sentinels: replace \x00{idx}\x00 with actual paste buffer content
    let content = expand_paste_sentinels(&content, &state.paste_buffers);
    state.input.clear();
    state.input_cursor = 0;
    state.input_selection_anchor = None;
    state.paste_buffers.clear();
    state.paste_buffer_labels.clear();
    let user_token_estimate = estimate_tokens(&content);

    // Assign user display ID and UID
    let user_id = format!("U{}", state.next_user_id);
    let user_global_uid = format!("UID_{}_U", state.global_next_uid);
    state.next_user_id = state.next_user_id.saturating_add(1);
    state.global_next_uid = state.global_next_uid.saturating_add(1);

    // Capture info for notification before moving user_msg
    let user_id_str = user_id.clone();
    let content_preview = if content.len() > 80 {
        format!("{}...", content.get(..content.floor_char_boundary(80)).unwrap_or(""))
    } else {
        content.clone()
    };

    // Record to persistent prompt history (append-only, survives clears)
    record_prompt_history(&content);

    let user_msg = Message::new_user(user_id, user_global_uid, content, user_token_estimate);
    save_message(&user_msg);

    // Add user message tokens to Conversation context and update timestamp
    if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type.as_str() == Kind::CONVERSATION) {
        ctx.token_count = ctx.token_count.saturating_add(user_token_estimate);
        ctx.last_refresh_ms = crate::app::panels::now_ms();
    }

    // Create a UserMessage notification — spine will detect this and start streaming
    // This works both during streaming (missed-message scenario) and when idle
    create_user_notification(state, &user_id_str, &content_preview);

    // Notify all modules that the user sent a message
    for module in all_modules() {
        module.on_user_message(state);
    }

    // During streaming: insert BEFORE the streaming assistant message
    // The notification will be picked up when the current stream ends
    if state.flags.stream.phase.is_streaming() {
        let insert_pos = state.messages.len().saturating_sub(1);
        state.messages.insert(insert_pos, user_msg);
        return ActionResult::Save;
    }

    state.messages.push(user_msg);

    // Reset per-stream and per-tick token counters for new user-initiated stream
    state.stream_cache_hit_tokens = 0;
    state.stream_cache_miss_tokens = 0;
    state.stream_output_tokens = 0;
    state.stream_uncached_input_tokens = 0;
    state.stream_cost_hit_usd = 0.0;
    state.stream_cost_miss_usd = 0.0;
    state.stream_cost_output_usd = 0.0;
    state.tick_cache_hit_tokens = 0;
    state.tick_cache_miss_tokens = 0;
    state.tick_output_tokens = 0;
    state.tick_uncached_input_tokens = 0;
    state.tick_cost_hit_usd = 0.0;
    state.tick_cost_miss_usd = 0.0;
    state.tick_cost_output_usd = 0.0;

    // Return Save — the spine check in handle_action will detect the unprocessed
    // notification and start streaming synchronously for responsive feel.
    ActionResult::Save
}

/// Handle `ClearConversation` action.
pub(crate) fn handle_clear_conversation(state: &mut State) -> ActionResult {
    for msg in &state.messages {
        // Delete by UID if available, otherwise by id
        let file_id = msg.uid.as_ref().unwrap_or(&msg.id);
        delete_message(file_id);
    }
    state.messages.clear();
    state.input.clear();
    state.input_selection_anchor = None;
    // Reset token count for Conversation context and update timestamp
    if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type.as_str() == Kind::CONVERSATION) {
        ctx.token_count = 0;
        ctx.last_refresh_ms = crate::app::panels::now_ms();
    }
    ActionResult::Save
}

/// Create a `UserMessage` notification in the spine system.
/// This is the primary trigger for starting a stream — the spine engine
/// will detect the unprocessed notification and launch streaming.
fn create_user_notification(state: &mut State, user_id: &str, content_preview: &str) {
    let _r = SpineState::create_notification(
        state,
        NotificationType::UserMessage,
        user_id.to_string(),
        content_preview.to_string(),
    );
}

/// Handle input submission when `ViewMode::Threads` is active.
///
/// Routes the user's text to the currently selected thread as a
/// `ThreadMessage(User)`, sets the thread to `MyTurn`, and creates
/// a spine notification so the AI picks up the new message.
///
/// When `creating_thread` is active, creates a new thread with the
/// input text as the thread name instead.
fn handle_thread_input_submit(state: &mut State) -> ActionResult {
    // `selected_thread_idx` indexes into the VISIBLE (view-filtered) thread
    // slice, not the raw storage vec — resolve it through `visible_indices`
    // for the current view (T9 archive refactor). The virtual "+ New Thread"
    // entry lives only in the active (non-archived) view, positioned just past
    // the last visible thread; comparing against the raw thread count would
    // mis-route the New-Thread row as a message into another thread whenever
    // any thread is archived.
    let (selected_idx, viewing_archived) = {
        let focus = FocusState::get(state);
        (focus.selected_thread_idx, focus.viewing_archived)
    };
    let visible = ThreadsState::get(state).visible_indices(viewing_archived);
    if !viewing_archived && selected_idx >= visible.len() {
        return handle_thread_create(state);
    }
    // Map the visible-list position back to a real storage index.
    let Some(&real_idx) = visible.get(selected_idx) else {
        return ActionResult::Nothing;
    };

    let commands = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Command);
    let content = replace_commands(&state.input, &commands);
    let content = expand_paste_sentinels(&content, &state.paste_buffers);

    // Clear input state
    state.input.clear();
    state.input_cursor = 0;
    state.input_selection_anchor = None;
    state.paste_buffers.clear();
    state.paste_buffer_labels.clear();

    // Record to persistent prompt history
    record_prompt_history(&content);

    // Find the selected thread (real storage index resolved above)
    let threads_state = ThreadsState::get_mut(state);

    let Some(thread) = threads_state.threads.get_mut(real_idx) else {
        return ActionResult::Nothing;
    };

    // Create a user message in the thread
    let msg = ThreadMessage {
        author: ThreadAuthor::User,
        content: Some(content),
        file_path: None,
        timestamp: crate::app::panels::now_ms(),
        acknowledged: false,
        auto: false,
    };
    thread.messages.push(msg);
    thread.status = ThreadStatus::MyTurn;

    // NO instant spine notification here — the idle MY_TURN detection
    // (`check_my_turn_threads`) fires when the agent finishes its current
    // work, avoiding mid-task distraction.  Auto-continuation picks it up.

    // Notify all modules
    for module in all_modules() {
        module.on_user_message(state);
    }

    ActionResult::Save
}

/// Create a new thread from the input text (used as thread name).
///
/// Called when the virtual "New Thread" entry is selected and the user
/// presses Enter. Generates a unique thread ID, creates an empty
/// `TheirTurn` thread, and selects it.
fn handle_thread_create(state: &mut State) -> ActionResult {
    /// Maximum thread name length to prevent state bloat.
    const MAX_THREAD_NAME_LEN: usize = 200;

    let name = state.input.trim().to_string();

    // Clear input regardless of outcome
    state.input.clear();
    state.input_cursor = 0;
    state.input_selection_anchor = None;

    if name.is_empty() {
        return ActionResult::Nothing;
    }

    // Cap thread name length
    let name = if name.len() > MAX_THREAD_NAME_LEN {
        name.get(..name.floor_char_boundary(MAX_THREAD_NAME_LEN)).unwrap_or(&name).to_string()
    } else {
        name
    };

    // Generate thread ID and create the thread
    let threads_state = ThreadsState::get_mut(state);
    let id = format!("T{}", threads_state.next_id);
    threads_state.next_id = threads_state.next_id.saturating_add(1);

    let thread = cp_mod_threads::types::Thread {
        id,
        name,
        status: ThreadStatus::TheirTurn,
        messages: vec![],
        created_at: crate::app::panels::now_ms(),
        archived: false,
        paused: false,
    };
    threads_state.threads.push(thread);

    // Select the newly created thread — it is the last entry of the active
    // (non-archived) visible slice, so the stored selection is a VISIBLE-list
    // position (consistent with `handle_thread_input_submit`), not a raw index.
    let new_idx = ThreadsState::get(state).visible_indices(false).len().saturating_sub(1);
    FocusState::get_mut(state).selected_thread_idx = new_idx;
    state.flags.ui.dirty = true;

    ActionResult::Save
}

/// Expand paste sentinel markers (\x00{idx}\x00) with actual paste buffer content.
fn expand_paste_sentinels(input: &str, paste_buffers: &[String]) -> String {
    if !input.contains('\x00') {
        return input.to_string();
    }

    let mut result = String::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let Some(&current_byte) = bytes.get(i) else { break };
        if current_byte == 0 {
            // Found sentinel start — find the index and closing \x00
            let start = i;
            i = i.saturating_add(1);
            let idx_start = i;
            while let Some(&b) = bytes.get(i) {
                if b == 0 {
                    break;
                }
                i = i.saturating_add(1);
            }
            if i < bytes.len() {
                // Found closing \x00
                let idx_str = input.get(idx_start..i).unwrap_or("");
                i = i.saturating_add(1); // skip closing \x00

                if let Ok(idx) = idx_str.parse::<usize>() {
                    if let Some(content) = paste_buffers.get(idx) {
                        result.push_str(content);
                    }
                    // If index out of bounds, silently drop the sentinel
                } else {
                    // Invalid index — keep original bytes
                    result.push_str(input.get(start..i).unwrap_or(""));
                }
            } else {
                // No closing \x00 — keep as-is
                result.push_str(input.get(start..).unwrap_or(""));
            }
        } else {
            let Some(ch) = input.get(i..).unwrap_or("").chars().next() else { break };
            result.push(ch);
            i = i.saturating_add(ch.len_utf8());
        }
    }

    result
}

/// Replace /command-name tokens in input with command content.
/// Only replaces at line start (after optional whitespace).
fn replace_commands(input: &str, commands: &[PromptItem]) -> String {
    if commands.is_empty() || !input.contains('/') {
        return input.to_string();
    }

    input
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('/') {
                return line.to_string();
            }
            // Extract the command token after /
            let token = trimmed.get(1..).unwrap_or("");
            let (cmd_id, rest) = token
                .find(|c: char| c.is_whitespace())
                .map_or((token, ""), |pos| (token.get(..pos).unwrap_or(""), token.get(pos..).unwrap_or("")));
            commands
                .iter()
                .find(|cmd| cmd.id == cmd_id)
                .map_or_else(|| line.to_string(), |cmd| format!("{}{}", cmd.content.trim_end(), rest))
        })
        .collect::<Vec<_>>()
        .join("\n")
}
