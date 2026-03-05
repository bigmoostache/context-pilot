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
        return "a".to_string();
    };
    let is_upper = first_char.is_ascii_uppercase();
    let base = if is_upper { b'A' } else { b'a' };

    // Convert to number (a=0, b=1, ..., z=25, aa=26, ab=27, ...)
    let mut num: usize = 0;
    for c in &chars {
        num = num.saturating_mul(26).saturating_add(c.to_ascii_lowercase() as usize).saturating_sub(b'a' as usize);
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
        result.insert(0, base.saturating_add(rem.to_u8()) as char);
    }
    result
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

    // Check for EMPTY list items (just the prefix with nothing after)
    // Unordered: exactly "- " or "* "
    if trimmed == "- " || trimmed == "* " {
        return Some(ListAction::RemoveItem);
    }

    // Ordered (numeric or alphabetic): exactly "X. " with nothing after
    if let Some(dot_pos) = trimmed.find(". ") {
        let marker = trimmed.get(..dot_pos).unwrap_or("");
        let after = trimmed.get(dot_pos.saturating_add(2)..).unwrap_or("");
        if after.is_empty() {
            // Check if it's a valid marker (numeric or alphabetic)
            let is_numeric = marker.chars().all(|c| c.is_ascii_digit());
            let is_alpha = marker.len() == 1
                && marker.chars().all(|c| c.is_ascii_alphabetic())
                && (marker.chars().all(|c| c.is_ascii_lowercase()) || marker.chars().all(|c| c.is_ascii_uppercase()));
            if is_numeric || is_alpha {
                return Some(ListAction::RemoveItem);
            }
        }
    }

    // Check for NON-EMPTY list items - continue the list
    // Unordered list: "- text" or "* text"
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        let prefix = trimmed.get(..2).unwrap_or("");
        let indent = current_line.len().saturating_sub(trimmed.len());
        return Some(ListAction::Continue(format!("\n{}{}", " ".repeat(indent), prefix)));
    }

    // Ordered list: "1. text", "a. text", "A. text", etc.
    if let Some(dot_pos) = trimmed.find(". ") {
        let marker = trimmed.get(..dot_pos).unwrap_or("");
        let indent = current_line.len().saturating_sub(trimmed.len());

        // Numeric: 1, 2, 3, ...
        if marker.chars().all(|c| c.is_ascii_digit())
            && let Ok(num) = marker.parse::<usize>()
        {
            return Some(ListAction::Continue(format!("\n{}{}. ", " ".repeat(indent), num.saturating_add(1))));
        }

        // Alphabetic: a, b, c, ... or A, B, C, ... (single char only)
        if marker.len() == 1 && marker.chars().all(|c| c.is_ascii_alphabetic()) {
            let all_lower = marker.chars().all(|c| c.is_ascii_lowercase());
            let all_upper = marker.chars().all(|c| c.is_ascii_uppercase());
            if all_lower || all_upper {
                let next = next_alpha_marker(marker);
                return Some(ListAction::Continue(format!("\n{}{}. ", " ".repeat(indent), next)));
            }
        }
    }

    None // Not a list line, send the message
}
