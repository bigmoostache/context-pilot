/// Actions for list continuation behavior
use cp_base::cast::Safe as _;

/// Describes what action to take when Enter is pressed on a list item.
pub(super) enum ListAction {
    /// Insert list continuation (e.g., "\n- " or "\n2. ")
    Continue(String),
    /// Remove empty list item but keep the newline
    RemoveItem,
}

/// Increment alphabetical list marker: a->b, z->aa, A->B, Z->AA
fn next_alpha_marker(marker: &str) -> String {
    let chars: Vec<char> = marker.chars().collect();
    let Some(&first_char) = chars.first() else {
        return "a".to_owned();
    };
    let is_upper = first_char.is_ascii_uppercase();
    let base = if is_upper { b'A' } else { b'a' };

    // Convert to number (a=0, b=1, ..., z=25, aa=26, ab=27, ...)
    let mut num: usize = 0;
    for c in &chars {
        num = num
            .saturating_mul(26)
            .saturating_add(usize::try_from(u32::from(c.to_ascii_lowercase())).unwrap_or(0))
            .saturating_sub(usize::from(b'a'));
    }
    num = num.saturating_add(1); // Increment

    // Convert back to letters using base-26 encoding
    alpha_from_number(num, base)
}

/// Convert a number to a base-26 alphabetical string.
///
/// Uses `std::iter::successors` to decompose via repeated divmod,
/// avoiding raw `%` and `/` operators.
fn alpha_from_number(num: usize, base: u8) -> String {
    // Bijective base-26: 0=a, 25=z, 26=aa, 27=ab, ...
    // Each iteration peels off the least-significant "digit".
    let mut result = String::new();
    for n in std::iter::successors(Some(num), |&n| {
        let next = n.checked_div(26)?.checked_sub(1)?;
        Some(next)
    }) {
        let rem = n.checked_rem(26).unwrap_or(0);
        result.insert(0, char::from(base.saturating_add(rem.to_u8())));
    }
    result
}

/// Detect an EMPTY list item (just the prefix, nothing after) that Enter should
/// remove: `- `, `* `, or an ordered `X. ` with a valid numeric/alpha marker.
fn detect_empty_list_item(trimmed: &str) -> Option<ListAction> {
    if trimmed == "- " || trimmed == "* " {
        return Some(ListAction::RemoveItem);
    }
    let dot_pos = trimmed.find(". ")?;
    let marker = trimmed.get(..dot_pos).unwrap_or("");
    let after = trimmed.get(dot_pos.saturating_add(2)..).unwrap_or("");
    if !after.is_empty() {
        return None;
    }
    let is_numeric = marker.chars().all(|c| c.is_ascii_digit());
    let is_alpha = marker.len() == 1
        && marker.chars().all(|c| c.is_ascii_alphabetic())
        && (marker.chars().all(|c| c.is_ascii_lowercase()) || marker.chars().all(|c| c.is_ascii_uppercase()));
    (is_numeric || is_alpha).then_some(ListAction::RemoveItem)
}

/// Detect a NON-EMPTY list item and build its continuation: unordered (`- `/`* `),
/// ordered numeric (`1. `), or ordered single-char alpha (`a. `/`A. `).
fn detect_list_continuation(trimmed: &str, current_line: &str) -> Option<ListAction> {
    let indent = current_line.len().saturating_sub(trimmed.len());

    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        let prefix = trimmed.get(..2).unwrap_or("");
        return Some(ListAction::Continue(format!("\n{}{}", " ".repeat(indent), prefix)));
    }

    let dot_pos = trimmed.find(". ")?;
    let marker = trimmed.get(..dot_pos).unwrap_or("");

    // Numeric: 1, 2, 3, …
    if marker.chars().all(|c| c.is_ascii_digit())
        && let Ok(num) = marker.parse::<usize>()
    {
        return Some(ListAction::Continue(format!("\n{}{}. ", " ".repeat(indent), num.saturating_add(1))));
    }

    // Alphabetic single-char: a, b, … or A, B, …
    if marker.len() == 1 && marker.chars().all(|c| c.is_ascii_alphabetic()) {
        let all_lower = marker.chars().all(|c| c.is_ascii_lowercase());
        let all_upper = marker.chars().all(|c| c.is_ascii_uppercase());
        if all_lower || all_upper {
            let next = next_alpha_marker(marker);
            return Some(ListAction::Continue(format!("\n{}{}. ", " ".repeat(indent), next)));
        }
    }
    None
}

/// Detect list context and return appropriate action
/// - On non-empty list item: continue the list
/// - On empty list item (just "- " or "1. "): remove it, keep newline
/// - On empty line or non-list: None (send message)
pub(super) fn detect_list_action(input: &str) -> Option<ListAction> {
    // Get the current line - handle trailing newline specially
    // (lines() doesn't return empty trailing lines)
    let current_line = if input.ends_with('\n') {
        "" // Cursor is on a new empty line
    } else {
        input.lines().last().unwrap_or("")
    };
    let trimmed = current_line.trim_start();

    // Completely empty line - send the message
    if trimmed.is_empty() {
        return None;
    }
    if let Some(action) = detect_empty_list_item(trimmed) {
        return Some(action);
    }
    detect_list_continuation(trimmed, current_line)
}
