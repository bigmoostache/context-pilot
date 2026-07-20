use std::sync::LazyLock;

use regex::Regex;

use crate::state::{Kind, State};

use super::ActionResult;
use super::config;

/// Switch to a target panel, saving the outgoing panel's scroll state and restoring
/// the incoming panel's scroll state. This preserves scroll position across TAB switches.
pub(crate) fn switch_to_panel(state: &mut State, target_index: usize) {
    // Save outgoing panel's scroll state
    if let Some(outgoing) = state.context.get_mut(state.selected_context) {
        outgoing.scroll_state.offset = state.scroll_offset;
        outgoing.scroll_state.user_scrolled = state.flags.stream.user_scrolled;
    }
    // Switch to target
    state.selected_context = target_index;
    // Restore incoming panel's scroll state
    if let Some(incoming) = state.context.get(state.selected_context) {
        state.scroll_offset = incoming.scroll_state.offset;
        state.flags.stream.user_scrolled = incoming.scroll_state.user_scrolled;
    } else {
        state.scroll_offset = 0.0;
        state.flags.stream.user_scrolled = false;
    }
}

/// Regex to match LLM ID prefixes like `[A84]: ` at the start of a string.
static RE_ID_PREFIX: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^(\[A\d+\]:\s*)+").ok());
/// Regex to match LLM ID prefixes on any line in multiline text.
static RE_ID_MULTILINE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?m)^\[A\d+\]:\s*").ok());

/// Remove LLM's mistaken ID prefixes like "[A84]: " from responses.
pub(crate) fn clean_llm_id_prefix(content: &str) -> String {
    // First trim leading whitespace
    let trimmed = content.trim_start();

    let cleaned = RE_ID_PREFIX.as_ref().map_or_else(|| trimmed.to_owned(), |re| re.replace(trimmed, "").to_string());

    let result =
        RE_ID_MULTILINE.as_ref().map_or_else(|| cleaned.clone(), |re| re.replace_all(&cleaned, "").to_string());

    // Strip leading/trailing whitespace and newlines after cleaning
    result.trim().to_owned()
}

/// Parse context selection patterns like p1, p-1, `p_1`, P1, P-1, `P_1`.
/// Returns the context ID (e.g., "P1", "P28") if matched.
pub(crate) fn parse_context_pattern(raw: &str) -> Option<String> {
    let input = raw.trim();
    if input.is_empty() {
        return None;
    }

    let input_lower = input.to_lowercase();

    // Must start with 'p'
    if !input_lower.starts_with('p') {
        return None;
    }

    // Get the rest after 'p'
    let rest = input_lower.get(1..).unwrap_or("");

    // Skip optional separator (- or _)
    let num_str = if rest.starts_with('-') || rest.starts_with('_') { rest.get(1..).unwrap_or("") } else { rest };

    // Parse the number and return the canonical ID format
    num_str.parse::<usize>().ok().map(|n| format!("P{n}"))
}

/// Find context index by ID.
pub(crate) fn find_context_by_id(state: &State, id: &str) -> Option<usize> {
    state.context.iter().position(|c| c.id == id)
}

/// If cursor is inside a paste sentinel (\x00{idx}\x00), eject it to after the sentinel.
pub(crate) fn eject_cursor_from_sentinel(input: &str, cursor: usize) -> usize {
    let bytes = input.as_bytes();
    if cursor == 0 || cursor >= bytes.len() {
        return cursor;
    }
    // Scan backwards from cursor to see if we hit \x00 before any non-digit
    let mut scan = cursor;
    while scan > 0 {
        let Some(&b) = bytes.get(scan.saturating_sub(1)) else { break };
        if b == 0 {
            // Found opening \x00 — we're inside a sentinel. Find the closing \x00.
            let mut end = cursor;
            while let Some(&eb) = bytes.get(end) {
                if eb == 0 {
                    break;
                }
                end = end.saturating_add(1);
            }
            if let Some(&eb) = bytes.get(end)
                && eb == 0
            {
                return end.saturating_add(1); // after closing \x00
            }
            return cursor;
        } else if b.is_ascii_digit() {
            scan = scan.saturating_sub(1);
        } else {
            break; // Not inside a sentinel
        }
    }
    cursor
}

/// Create a new conversation context panel.
pub(super) fn create_new_context(state: &mut State) -> ActionResult {
    let context_id = state.next_available_context_id();
    let name = format!("Conv {}", state.context.len());
    state.context.push(cp_base::state::context::make_default_entry(
        &context_id,
        Kind::new(Kind::CONVERSATION),
        &name,
        false,
    ));
    ActionResult::Save
}

// =============================================================================
// Context panel navigation (moved from navigation.rs)
// =============================================================================

/// Maximum dynamic entries per sidebar page (must match `render_sidebar.rs`).
const DYNAMIC_PAGE_SIZE: usize = 10;

/// Numeric panel-ID sort key (`P12` → 12), `usize::MAX` when unparsable.
fn panel_id_key(state: &State, idx: usize) -> usize {
    state
        .context
        .get(idx)
        .and_then(|el| el.id.strip_prefix('P'))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(usize::MAX)
}

/// Context indices sorted by numeric panel ID (shared ordering).
fn sorted_by_panel_id(state: &State) -> Vec<usize> {
    let mut sorted: Vec<usize> = (0..state.context.len()).collect();
    sorted.sort_by_key(|&a| panel_id_key(state, a));
    sorted
}

/// Page to jump to from a dynamic panel (wraps circularly).
const fn next_dynamic_page(current_page: usize, total_pages: usize, forward: bool) -> usize {
    if forward {
        if current_page >= total_pages.saturating_sub(1) { 0 } else { current_page.saturating_add(1) }
    } else if current_page == 0 {
        total_pages.saturating_sub(1)
    } else {
        current_page.saturating_sub(1)
    }
}

/// Navigate to the next (`forward=true`) or previous (`forward=false`) context panel,
/// sorted by numeric panel ID.
pub(super) fn select_context(state: &mut State, forward: bool) {
    if state.context.is_empty() {
        return;
    }
    let sorted = sorted_by_panel_id(state);
    let cur = sorted.iter().position(|&i| i == state.selected_context).unwrap_or(0);
    let next = if forward {
        config::wrap_next(cur, sorted.len())
    } else if cur == 0 {
        sorted.len().saturating_sub(1)
    } else {
        cur.saturating_sub(1)
    };
    let Some(&selected) = sorted.get(next) else { return };
    switch_to_panel(state, selected);
}

/// Jump to the first dynamic panel on the next or previous page.
///
/// - From a **fixed** panel: forward → last page start, backward → first page start.
/// - From a **dynamic** panel: forward/backward wraps circularly through pages.
pub(super) fn page_dynamic(state: &mut State, forward: bool) {
    if state.context.is_empty() {
        return;
    }
    let sorted = sorted_by_panel_id(state);

    // Collect dynamic panel indices only (preserving sorted order).
    let dynamic_indices: Vec<usize> =
        sorted.iter().filter(|&&i| state.context.get(i).is_some_and(|c| !c.context_type.is_fixed())).copied().collect();

    if dynamic_indices.is_empty() {
        return;
    }

    let total_pages = dynamic_indices.len().div_ceil(DYNAMIC_PAGE_SIZE);

    // Is the currently selected panel dynamic?
    let current_is_dynamic = state.context.get(state.selected_context).is_some_and(|c| !c.context_type.is_fixed());

    let target_page = if current_is_dynamic {
        let pos = dynamic_indices.iter().position(|&i| i == state.selected_context).unwrap_or(0);
        let current_page = pos.checked_div(DYNAMIC_PAGE_SIZE).unwrap_or(0);
        next_dynamic_page(current_page, total_pages, forward)
    } else {
        // From a fixed panel: forward → last page, backward → first page.
        if forward { total_pages.saturating_sub(1) } else { 0 }
    };

    // Jump to the first panel on the target page.
    let target_idx = target_page.saturating_mul(DYNAMIC_PAGE_SIZE);
    if let Some(&selected) = dynamic_indices.get(target_idx) {
        switch_to_panel(state, selected);
    }
}
