use std::sync::LazyLock;

use regex::Regex;

use crate::state::State;

/// Regex to match LLM ID prefixes like `[A84]: ` at the start of a string.
static RE_ID_PREFIX: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^(\[A\d+\]:\s*)+").ok());
/// Regex to match LLM ID prefixes on any line in multiline text.
static RE_ID_MULTILINE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?m)^\[A\d+\]:\s*").ok());

/// Remove LLM's mistaken ID prefixes like "[A84]: " from responses.
pub(crate) fn clean_llm_id_prefix(content: &str) -> String {
    // First trim leading whitespace
    let trimmed = content.trim_start();

    let cleaned = RE_ID_PREFIX.as_ref().map_or_else(|| trimmed.to_string(), |re| re.replace(trimmed, "").to_string());

    let result =
        RE_ID_MULTILINE.as_ref().map_or_else(|| cleaned.clone(), |re| re.replace_all(&cleaned, "").to_string());

    // Strip leading/trailing whitespace and newlines after cleaning
    result.trim().to_string()
}

/// Parse context selection patterns like p1, p-1, `p_1`, P1, P-1, `P_1`.
/// Returns the context ID (e.g., "P1", "P28") if matched.
pub(crate) fn parse_context_pattern(input: &str) -> Option<String> {
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
