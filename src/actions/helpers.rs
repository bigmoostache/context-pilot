use std::sync::LazyLock;

use regex::Regex;

use crate::state::State;

static RE_ID_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\[A\d+\]:\s*)+").expect("invalid RE_ID_PREFIX regex"));
static RE_ID_MULTILINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\[A\d+\]:\s*").expect("invalid RE_ID_MULTILINE regex"));

/// Remove LLM's mistaken ID prefixes like "[A84]: " from responses
pub fn clean_llm_id_prefix(content: &str) -> String {
    // First trim leading whitespace
    let trimmed = content.trim_start();

    // Pattern: one or more [A##]: or [A###]: at the start, with optional whitespace between
    let cleaned = RE_ID_PREFIX.replace(trimmed, "").to_string();

    // Also clean any [Axx]: that appears at the start of lines (multiline responses)
    let result = RE_ID_MULTILINE.replace_all(&cleaned, "").to_string();

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
