//! Thread action handlers — dispatches `Thread*` action variants.
//!
//! Extracted from `mod.rs` to keep the central dispatch under the 500-line limit.

use cp_mod_threads::types::{FocusState, ThreadsState};

use crate::state::State;

use super::{Action, ActionResult};

/// Dispatch a no-data `Thread*` action variant to its handler.
///
/// Called from the central `apply_action` match for all `Thread*` variants
/// except `ThreadQuestionChar` (which carries data). Uses equality checks
/// rather than an exhaustive `match` so the ~60 non-thread variants need not be
/// enumerated as a wildcard-free no-op arm.
pub(super) fn dispatch(state: &mut State, action: &Action) -> ActionResult {
    if let Some(result) = dispatch_selection(state, action) {
        return result;
    }
    dispatch_archive(state, action)
}

/// Handle the selection/creation `Thread*` variants (next/prev/create-start/
/// create-cancel). Returns `None` when `action` is not one of them, so
/// [`dispatch`] can fall through to the archive handlers.
fn dispatch_selection(state: &mut State, action: &Action) -> Option<ActionResult> {
    if matches!(action, Action::ThreadSelectNext) {
        return Some(select_next(state));
    }
    if matches!(action, Action::ThreadSelectPrev) {
        return Some(select_prev(state));
    }
    if matches!(action, Action::ThreadCreateStart) {
        return Some(create_start(state));
    }
    if matches!(action, Action::ThreadCreateCancel) {
        return Some(create_cancel(state));
    }
    None
}

/// Handle the archive/restore `Thread*` variants (archive-start/confirm/cancel,
/// toggle-archived-view). Any non-thread variant no-ops (caller pre-filters).
fn dispatch_archive(state: &mut State, action: &Action) -> ActionResult {
    if matches!(action, Action::ThreadArchiveStart) {
        return archive_start(state);
    }
    if matches!(action, Action::ThreadArchiveConfirm) {
        return archive_confirm(state);
    }
    if matches!(action, Action::ThreadArchiveCancel) {
        return archive_cancel(state);
    }
    if matches!(action, Action::ThreadToggleArchivedView) {
        return toggle_archived_view(state);
    }
    // Non-thread variants never reach here (caller pre-filters); no-op fallback.
    ActionResult::Nothing
}

/// Navigate to the next thread (or wrap to first).
fn select_next(state: &mut State) -> ActionResult {
    let viewing_archived = FocusState::get(state).viewing_archived;
    let visible_count = ThreadsState::get(state).visible_indices(viewing_archived).len();
    // Active view has a trailing virtual "+ New Thread" entry; archived view does not.
    let total = if viewing_archived { visible_count } else { visible_count.saturating_add(1) };
    let focus = FocusState::get_mut(state);
    focus.selected_thread_idx = if focus.selected_thread_idx >= total.saturating_sub(1) {
        0
    } else {
        focus.selected_thread_idx.saturating_add(1)
    };
    if focus.selected_thread_idx < visible_count {
        FocusState::mark_selected_read(state);
    }
    state.scroll_offset = 0.0;
    state.flags.stream.user_scrolled = false;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Navigate to the previous thread (or wrap to last).
fn select_prev(state: &mut State) -> ActionResult {
    let viewing_archived = FocusState::get(state).viewing_archived;
    let visible_count = ThreadsState::get(state).visible_indices(viewing_archived).len();
    let total = if viewing_archived { visible_count } else { visible_count.saturating_add(1) };
    let focus = FocusState::get_mut(state);
    focus.selected_thread_idx = if focus.selected_thread_idx == 0 {
        total.saturating_sub(1)
    } else {
        focus.selected_thread_idx.saturating_sub(1)
    };
    if focus.selected_thread_idx < visible_count {
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
///
/// Guards on the *visible* list for the current view (active or archived),
/// so the prompt only appears when there is actually a thread to act on.
fn archive_start(state: &mut State) -> ActionResult {
    let viewing_archived = FocusState::get(state).viewing_archived;
    let has_visible = !ThreadsState::get(state).visible_indices(viewing_archived).is_empty();
    if has_visible {
        FocusState::get_mut(state).confirming_archive = true;
        state.flags.ui.dirty = true;
    }
    ActionResult::Nothing
}

/// Toggle between the active and archived thread lists (Ctrl+U).
///
/// Resets the selection to the top of the newly-shown list and clears any
/// pending archive confirmation, so the two views never share stale state.
fn toggle_archived_view(state: &mut State) -> ActionResult {
    let focus = FocusState::get_mut(state);
    focus.viewing_archived = !focus.viewing_archived;
    focus.selected_thread_idx = 0;
    focus.confirming_archive = false;
    state.scroll_offset = 0.0;
    state.flags.stream.user_scrolled = false;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Confirm thread archive (active view) or restore (archived view).
///
/// `selected_thread_idx` is a position into the *visible* slice
/// ([`ThreadsState::visible_indices`]) — the active threads in the normal
/// view, the archived ones in the archived view. We resolve it to a real
/// storage index, then **soft-delete** (set `archived = true`) or **restore**
/// (`archived = false`) instead of removing the thread, so it is retained in
/// state and the web frontend can still display it.
///
/// Cleans up all `FocusState` references to a thread being archived:
/// focused ID, last-read count, `MY_TURN` notification debounce.
fn archive_confirm(state: &mut State) -> ActionResult {
    let focus = FocusState::get(state);
    let viewing_archived = focus.viewing_archived;
    let selected_pos = focus.selected_thread_idx;
    FocusState::get_mut(state).confirming_archive = false;

    // Resolve the visible position to a real storage index.
    let visible = ThreadsState::get(state).visible_indices(viewing_archived);
    let Some(&real_idx) = visible.get(selected_pos) else {
        state.flags.ui.dirty = true;
        return ActionResult::Nothing;
    };

    // Toggle the archived flag (archive in active view, restore in archived view).
    let toggled_id = {
        let ts = ThreadsState::get_mut(state);
        ts.threads.get_mut(real_idx).map(|t| {
            t.archived = !viewing_archived;
            t.id.clone()
        })
    };

    // Clamp selection to the new visible-list length for the current view.
    let new_visible_len = ThreadsState::get(state).visible_indices(viewing_archived).len();
    let focus_after = FocusState::get_mut(state);
    if focus_after.selected_thread_idx >= new_visible_len {
        focus_after.selected_thread_idx = new_visible_len.saturating_sub(1);
    }

    // Clean up focus references to a thread leaving the active list (archive only).
    if !viewing_archived && let Some(aid) = toggled_id {
        if focus_after.focused_thread_id.as_deref() == Some(&aid) {
            focus_after.focused_thread_id = None;
            focus_after.dangling_remaining = 0i32;
            focus_after.escalation_level = 0;
        }
        let _prev = focus_after.last_read_count.remove(&aid);
        if focus_after.notified_my_turn_id.as_deref() == Some(&aid) {
            focus_after.notified_my_turn_id = None;
        }
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
