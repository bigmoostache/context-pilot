/// Global middleware: validates that every tool call includes `intent` and `verb` params.
/// Returns a warning string to append to the tool result if metadata is missing or invalid.
/// Tools execute normally regardless — this is purely advisory feedback to the LLM.
pub(crate) fn validate_tool_metadata(params: &serde_json::Value) -> Option<String> {
    let intent = params.get("intent").and_then(serde_json::Value::as_str);
    let verb = params.get("verb").and_then(serde_json::Value::as_str);

    let mut issues = Vec::new();

    match intent {
        None => issues.push("missing 'intent'"),
        Some(s) if s.trim().is_empty() => issues.push("'intent' is empty"),
        Some(s) if s.split_whitespace().count() > 10 => issues.push("'intent' exceeds 10 words"),
        Some(_) => {}
    }
    match verb {
        None => issues.push("missing 'verb'"),
        Some(s) if s.trim().is_empty() => issues.push("'verb' is empty"),
        Some(s) if s.split_whitespace().count() != 1 => issues.push("'verb' must be exactly 1 word"),
        Some(_) => {}
    }

    if issues.is_empty() {
        return None;
    }
    Some(format!(
        "\n\n⚠️ Tool metadata: {}. Include \"intent\" (<10 words, why) and \"verb\" (single -ING word) with every tool call.",
        issues.join(", ")
    ))
}
