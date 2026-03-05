/// Best-effort extraction of key-value pairs from potentially incomplete JSON.
///
/// Handles the common case of `{"key": "value", "key2": ...}` even when the
/// closing brace or last value is missing (still streaming).
pub(super) fn extract_json_fields(partial: &str) -> Vec<(String, String)> {
    // Try full parse first — if the JSON is complete, use serde
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(partial) {
        return map
            .into_iter()
            .map(|(k, v)| {
                let val = match v {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Null
                    | serde_json::Value::Bool(_)
                    | serde_json::Value::Number(_)
                    | serde_json::Value::Array(_)
                    | serde_json::Value::Object(_) => v.to_string(),
                };
                (k, val)
            })
            .collect();
    }

    // Incomplete JSON — hand-parse key-value pairs
    let mut fields = Vec::new();
    let mut chars = partial.char_indices().peekable();

    // Skip opening brace
    while let Some(&(_, c)) = chars.peek() {
        if c == '{' {
            let _ = chars.next();
            break;
        }
        let _ = chars.next();
    }

    loop {
        // Skip whitespace and commas
        while let Some(&(_, c)) = chars.peek() {
            if c == ' ' || c == '\n' || c == '\r' || c == '\t' || c == ',' {
                let _ = chars.next();
            } else {
                break;
            }
        }

        // Try to read a key
        let Some(key) = read_json_string(&mut chars) else { break };

        // Skip colon
        while let Some(&(_, c)) = chars.peek() {
            if c == ':' || c == ' ' {
                let _ = chars.next();
            } else {
                break;
            }
        }

        // Read value (may be incomplete)
        let val = read_json_value(&mut chars, partial);
        fields.push((key, val));
    }

    fields
}

/// Read a JSON string literal, returning the unescaped content.
fn read_json_string(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> Option<String> {
    // Expect opening quote
    if chars.peek().map(|&(_, c)| c) != Some('"') {
        return None;
    }
    let _ = chars.next(); // consume opening quote

    let mut s = String::new();
    let mut escaped = false;
    for (_, c) in chars.by_ref() {
        if escaped {
            s.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(s);
        } else {
            s.push(c);
        }
    }
    // Unterminated string — return what we have
    if s.is_empty() { None } else { Some(s) }
}

/// Read a JSON value (string, number, bool, array, object) from the stream.
/// For incomplete values, returns whatever was consumed.
fn read_json_value(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>, full: &str) -> String {
    match chars.peek().map(|&(_, c)| c) {
        Some('"') => read_json_string(chars).unwrap_or_default(),
        Some(c) if c == '{' || c == '[' => {
            // Capture from current position to end (may be incomplete)
            let start = chars.peek().map_or(full.len(), |&(idx, _)| idx);
            // Consume remaining chars for this nested structure
            let open = c;
            let close = if c == '{' { '}' } else { ']' };
            let mut depth: i32 = 0;
            let mut end = full.len();
            for (byte_idx, nested_ch) in chars.by_ref() {
                if nested_ch == open {
                    depth = depth.saturating_add(1);
                } else if nested_ch == close {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = byte_idx.saturating_add(nested_ch.len_utf8());
                        break;
                    }
                }
            }
            full.get(start..end).unwrap_or("").to_string()
        }
        Some(_) => {
            // Number, bool, null — read until delimiter
            let mut val = String::new();
            while let Some(&(_, c)) = chars.peek() {
                if c == ',' || c == '}' || c == ']' || c == '\n' {
                    break;
                }
                val.push(c);
                let _ = chars.next();
            }
            val.trim().to_string()
        }
        None => String::new(),
    }
}
