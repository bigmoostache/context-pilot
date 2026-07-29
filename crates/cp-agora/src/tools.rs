use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{AgoraState, IDENTITY_VALUE_HARD_CAP};

/// The 10 identity keys, in canonical order. Mirrors the `Identity` fields and
/// the `Agora_set_identity` tool parameters 1:1.
const IDENTITY_KEYS: [&str; 10] = [
    "identity",
    "values",
    "principles",
    "character",
    "expertise",
    "role",
    "operational_responsibilities",
    "knowledge_responsibilities",
    "organic_responsibilities",
    "direct_management",
];

/// Count whitespace-delimited words in a value.
fn word_count(value: &str) -> usize {
    value.split_whitespace().count()
}

/// Execute `Agora_set_identity`.
///
/// Every key is compulsory: a missing param is an error. Every value is
/// word-capped at [`IDENTITY_VALUE_HARD_CAP`]; any overflow is rejected and the
/// whole set is left untouched (all-or-nothing — a partial identity is worse
/// than the prior one). On success the 10 fields are written into the global
/// [`AgoraState`] and the Agora panel is refreshed.
pub(crate) fn execute_set_identity(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Collect + validate every key BEFORE mutating any state (all-or-nothing).
    let mut values: Vec<(&'static str, String)> = Vec::with_capacity(IDENTITY_KEYS.len());
    let mut too_long: Vec<String> = Vec::new();

    for key in IDENTITY_KEYS {
        let Some(raw) = tool.input.get(key).and_then(|v| v.as_str()) else {
            return ToolResult::new(tool.id.clone(), format!("Missing '{key}' parameter"), true);
        };
        let trimmed = raw.trim();
        let count = word_count(trimmed);
        if count > IDENTITY_VALUE_HARD_CAP {
            too_long.push(format!("{key} ({count} words)"));
        }
        values.push((key, trimmed.to_owned()));
    }

    if !too_long.is_empty() {
        return ToolResult::new(
            tool.id.clone(),
            format!("These values exceed {IDENTITY_VALUE_HARD_CAP} words — shorten them: {}", too_long.join(", ")),
            true,
        );
    }

    // All valid — write into global state. `values` is in IDENTITY_KEYS order.
    let ag = AgoraState::get_mut(state);
    let id = &mut ag.identity;
    for (key, val) in values {
        match key {
            "identity" => id.identity = val,
            "values" => id.values = val,
            "principles" => id.principles = val,
            "character" => id.character = val,
            "expertise" => id.expertise = val,
            "role" => id.role = val,
            "operational_responsibilities" => id.operational_responsibilities = val,
            "knowledge_responsibilities" => id.knowledge_responsibilities = val,
            "organic_responsibilities" => id.organic_responsibilities = val,
            "direct_management" => id.direct_management = val,
            _ => {}
        }
    }

    state.touch_panel(Kind::AGORA);

    ToolResult::new(tool.id.clone(), "Identity set.".to_owned(), false)
}
