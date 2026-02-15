/// Actions for list continuation behavior
pub(super) enum ListAction {
    Continue(String), // Insert list continuation (e.g., "\n- " or "\n2. ")
    RemoveItem,       // Remove empty list item but keep the newline
}

/// Increment alphabetical list marker: a->b, z->aa, A->B, Z->AA
fn next_alpha_marker(marker: &str) -> String {
    let chars: Vec<char> = marker.chars().collect();
    let is_upper = chars[0].is_ascii_uppercase();
    let base = if is_upper { b'A' } else { b'a' };

    // Convert to number (a=0, b=1, ..., z=25, aa=26, ab=27, ...)
    let mut num: usize = 0;
    for c in &chars {
        num = num * 26 + (c.to_ascii_lowercase() as usize - b'a' as usize);
    }
    num += 1; // Increment

    // Convert back to letters
    let mut result = String::new();
    let mut n = num;
    loop {
        result.insert(0, (base + (n % 26) as u8) as char);
        n /= 26;
        if n == 0 {
            break;
        }
        n -= 1; // Adjust for 1-based (a=1, not a=0 for multi-char)
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
        let marker = &trimmed[..dot_pos];
        let after = &trimmed[dot_pos + 2..];
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
        let prefix = &trimmed[..2];
        let indent = current_line.len() - trimmed.len();
        return Some(ListAction::Continue(format!("\n{}{}", " ".repeat(indent), prefix)));
    }

    // Ordered list: "1. text", "a. text", "A. text", etc.
    if let Some(dot_pos) = trimmed.find(". ") {
        let marker = &trimmed[..dot_pos];
        let indent = current_line.len() - trimmed.len();

        // Numeric: 1, 2, 3, ...
        if marker.chars().all(|c| c.is_ascii_digit())
            && let Ok(num) = marker.parse::<usize>()
        {
            return Some(ListAction::Continue(format!("\n{}{}. ", " ".repeat(indent), num + 1)));
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
