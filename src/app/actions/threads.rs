//! Thread action handlers — dispatches `Thread*` action variants.
//!
//! Extracted from `mod.rs` to keep the central dispatch under the 500-line limit.

use cp_mod_threads::types::{FocusState, ThreadsState};

use crate::state::State;

use super::{Action, ActionResult};

/// Dispatch a no-data `Thread*` action variant to its handler.
///
/// Called from the central `apply_action` match for all `Thread*` variants
/// except `ThreadQuestionChar` (which carries data).
pub(super) fn dispatch(state: &mut State, action: &Action) -> ActionResult {
    match *action {
        Action::ThreadSelectNext => select_next(state),
        Action::ThreadSelectPrev => select_prev(state),
        Action::ThreadCreateStart => create_start(state),
        Action::ThreadCreateCancel => create_cancel(state),
        Action::ThreadArchiveStart => archive_start(state),
        Action::ThreadArchiveConfirm => archive_confirm(state),
        Action::ThreadArchiveCancel => archive_cancel(state),
        Action::ThreadQuestionUp => question_up(state),
        Action::ThreadQuestionDown => question_down(state),
        Action::ThreadQuestionLeft => question_left(state),
        Action::ThreadQuestionRight => question_right(state),
        Action::ThreadQuestionToggle => question_toggle(state),
        Action::ThreadQuestionEnter => question_enter(state),
        Action::ThreadQuestionDismiss => question_dismiss(state),
        Action::ThreadQuestionBackspace => question_backspace(state),
        // Exhaustive: all non-Thread variants — dispatch is only called for Thread* actions
        // from the central match in mod.rs, so these arms are unreachable.
        Action::InputChar(_) | Action::InsertText(_) | Action::PasteText(_)
        | Action::InputBackspace | Action::InputDelete | Action::InputSubmit
        | Action::CursorWordLeft | Action::CursorWordRight | Action::DeleteWordLeft
        | Action::RemoveListItem | Action::CursorHome | Action::CursorEnd
        | Action::CursorLeft | Action::CursorRight | Action::CursorLeftSelect
        | Action::CursorRightSelect | Action::CursorWordLeftSelect
        | Action::CursorWordRightSelect | Action::CursorHomeSelect
        | Action::CursorEndSelect | Action::SelectAll | Action::HistoryPrev
        | Action::HistoryNext | Action::CopyPanelContent | Action::ClearConversation
        | Action::NewContext | Action::SelectNextContext | Action::SelectPrevContext
        | Action::AppendChars(_) | Action::StreamDone { .. } | Action::StreamError(_)
        | Action::ScrollUp(_) | Action::ScrollDown(_) | Action::StopStreaming
        | Action::TmuxSendKeys { .. } | Action::TogglePerfMonitor
        | Action::ToggleConfigView | Action::ToggleIndexOverlay | Action::CopyIndexOverlay
        | Action::ConfigSelectProvider(_) | Action::ConfigSelectAnthropicModel(_)
        | Action::ConfigSelectGrokModel(_) | Action::ConfigSelectGroqModel(_)
        | Action::ConfigSelectDeepSeekModel(_) | Action::ConfigSelectMiniMaxModel(_)
        | Action::ConfigSelectClaudeCodeV2Model(_) | Action::ConfigSelectNextBar
        | Action::ConfigSelectPrevBar | Action::ConfigIncreaseSelectedBar
        | Action::ConfigDecreaseSelectedBar | Action::ConfigNextTheme
        | Action::ConfigPrevTheme | Action::ConfigToggleAutoContinue
        | Action::ConfigThinkThresholdUp | Action::ConfigThinkThresholdDown
        | Action::ConfigSelectSecondaryProvider(_)
        | Action::ConfigSelectSecondaryAnthropicModel(_)
        | Action::ConfigSelectSecondaryGrokModel(_)
        | Action::ConfigSelectSecondaryGroqModel(_)
        | Action::ConfigSelectSecondaryDeepSeekModel(_)
        | Action::ConfigSelectSecondaryMiniMaxModel(_)
        | Action::ConfigSelectSecondaryClaudeCodeV2Model(_)
        | Action::ConfigToggleReverie | Action::ConfigToggleSecondaryMode
        | Action::PageDynamicNext | Action::PageDynamicPrev | Action::CycleViewMode
        | Action::ThreadQuestionChar(_) | Action::OpenCommandPalette
        | Action::ResetSessionCosts | Action::SelectContextById(_) | Action::None
            => ActionResult::Nothing,
    }
}

/// Navigate to the next thread (or wrap to first).
fn select_next(state: &mut State) -> ActionResult {
    let thread_count = ThreadsState::get(state).threads.len();
    let total = thread_count.saturating_add(1);
    let focus = FocusState::get_mut(state);
    focus.selected_thread_idx = if focus.selected_thread_idx >= total.saturating_sub(1) {
        0
    } else {
        focus.selected_thread_idx.saturating_add(1)
    };
    if focus.selected_thread_idx < thread_count {
        FocusState::mark_selected_read(state);
    }
    state.scroll_offset = 0.0;
    state.flags.stream.user_scrolled = false;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Navigate to the previous thread (or wrap to last).
fn select_prev(state: &mut State) -> ActionResult {
    let thread_count = ThreadsState::get(state).threads.len();
    let total = thread_count.saturating_add(1);
    let focus = FocusState::get_mut(state);
    focus.selected_thread_idx = if focus.selected_thread_idx == 0 {
        total.saturating_sub(1)
    } else {
        focus.selected_thread_idx.saturating_sub(1)
    };
    if focus.selected_thread_idx < thread_count {
        FocusState::mark_selected_read(state);
    }
    state.scroll_offset = 0.0;
    state.flags.stream.user_scrolled = false;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Enter thread creation mode — switches input to naming.
fn create_start(state: &mut State) -> ActionResult {
    let focus = FocusState::get_mut(state);
    focus.creating_thread = true;
    state.input.clear();
    state.input_cursor = 0;
    state.input_selection_anchor = None;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Cancel thread creation without creating.
fn create_cancel(state: &mut State) -> ActionResult {
    let focus = FocusState::get_mut(state);
    focus.creating_thread = false;
    state.input.clear();
    state.input_cursor = 0;
    state.input_selection_anchor = None;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Start thread archive — show confirmation prompt.
fn archive_start(state: &mut State) -> ActionResult {
    let has_threads = !ThreadsState::get(state).threads.is_empty();
    if has_threads {
        FocusState::get_mut(state).confirming_archive = true;
        state.flags.ui.dirty = true;
    }
    ActionResult::Nothing
}

/// Confirm thread archive — remove the selected thread.
fn archive_confirm(state: &mut State) -> ActionResult {
    let focus = FocusState::get_mut(state);
    focus.confirming_archive = false;
    let selected_idx = focus.selected_thread_idx;

    let threads_state = ThreadsState::get_mut(state);
    if selected_idx < threads_state.threads.len() {
        let _removed = threads_state.threads.remove(selected_idx);
    }
    let len = ThreadsState::get(state).threads.len();
    let focus_after = FocusState::get_mut(state);
    if len == 0 {
        focus_after.selected_thread_idx = 0;
    } else if focus_after.selected_thread_idx >= len {
        focus_after.selected_thread_idx = len.saturating_sub(1);
    }
    state.flags.ui.dirty = true;
    ActionResult::Save
}

/// Cancel thread archive — dismiss confirmation.
fn archive_cancel(state: &mut State) -> ActionResult {
    FocusState::get_mut(state).confirming_archive = false;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Move cursor up in the active question form.
fn question_up(state: &mut State) -> ActionResult {
    if let Some(aq) = FocusState::get_mut(state).active_question.as_mut() {
        aq.cursor_up();
    }
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Move cursor down in the active question form.
fn question_down(state: &mut State) -> ActionResult {
    if let Some(aq) = FocusState::get_mut(state).active_question.as_mut() {
        aq.cursor_down();
    }
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Navigate to previous question in multi-question form.
fn question_left(state: &mut State) -> ActionResult {
    if let Some(aq) = FocusState::get_mut(state).active_question.as_mut() {
        aq.prev_question();
    }
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Navigate to next question in multi-question form.
fn question_right(state: &mut State) -> ActionResult {
    if let Some(aq) = FocusState::get_mut(state).active_question.as_mut() {
        aq.next_question();
    }
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Toggle selection on the current option.
fn question_toggle(state: &mut State) -> ActionResult {
    if let Some(aq) = FocusState::get_mut(state).active_question.as_mut() {
        aq.toggle_selection();
    }
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Handle Enter in question form — select + advance, or submit on final question.
fn question_enter(state: &mut State) -> ActionResult {
    let should_submit = {
        let focus = FocusState::get_mut(state);
        focus.active_question.as_mut().is_some_and(|aq| {
            aq.handle_enter();
            aq.is_last_question() && aq.all_answered()
        })
    };
    if should_submit {
        let (thread_id, yaml) = {
            let focus = FocusState::get_mut(state);
            let aq = focus.active_question.take();
            aq.map_or_else(
                || (String::new(), String::new()),
                |form| (form.thread_id.clone(), form.format_answers_yaml()),
            )
        };
        if !thread_id.is_empty() {
            let ts = ThreadsState::get_mut(state);
            if let Some(thread) = ts.threads.iter_mut().find(|t| t.id == thread_id) {
                thread.messages.push(cp_mod_threads::types::ThreadMessage {
                    author: cp_mod_threads::types::ThreadAuthor::User,
                    content: Some(yaml),
                    file_path: None,
                    question: None,
                    timestamp: crate::app::panels::now_ms(),
                    acknowledged: false,
                });
                thread.status = cp_mod_threads::types::ThreadStatus::MyTurn;
            }
            let _id = cp_mod_spine::types::SpineState::create_notification(
                state,
                cp_mod_spine::types::NotificationType::Custom,
                "Thread Question Answered".into(),
                format!(
                    "User answered questions in thread \"{thread_id}\". \
                     Use Read(thread_id=\"{thread_id}\") to see the answers."
                ),
            );
        }
    }
    state.flags.ui.dirty = true;
    ActionResult::Save
}

/// Dismiss the active question form without answering.
fn question_dismiss(state: &mut State) -> ActionResult {
    FocusState::get_mut(state).active_question = None;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Type a character into the question form's "Other" text field.
pub(super) fn question_char(state: &mut State, c: char) -> ActionResult {
    if let Some(aq) = FocusState::get_mut(state).active_question.as_mut() {
        aq.type_char(c);
    }
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Delete last character from the "Other" text field.
fn question_backspace(state: &mut State) -> ActionResult {
    if let Some(aq) = FocusState::get_mut(state).active_question.as_mut() {
        aq.backspace();
    }
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}
