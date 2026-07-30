use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{AgoraState, IDENTITY_VALUE_HARD_CAP, SelfIdentity};

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

/// Apply a fully-formed [`SelfIdentity`] to the global [`AgoraState`].
///
/// The shared core behind BOTH the `Agora_set_identity` tool and the bridge
/// `SetIdentity` command, so both mutate identity through one validated path.
/// Every value is word-capped at [`IDENTITY_VALUE_HARD_CAP`]; any overflow is
/// rejected and the whole set is left untouched (all-or-nothing — a partial
/// identity is worse than the prior one). On success the record is written and
/// the Agora panel is refreshed. Returns the human-readable rejection message
/// on overflow.
///
/// # Errors
///
/// Returns `Err` listing the offending keys when any value exceeds the cap.
pub fn set_identity(state: &mut State, next: SelfIdentity) -> Result<(), String> {
    let too_long: Vec<String> = next
        .pairs()
        .into_iter()
        .filter_map(|(key, val)| {
            let count = word_count(val);
            (count > IDENTITY_VALUE_HARD_CAP).then(|| format!("{key} ({count} words)"))
        })
        .collect();

    if !too_long.is_empty() {
        return Err(format!(
            "These values exceed {IDENTITY_VALUE_HARD_CAP} words — shorten them: {}",
            too_long.join(", ")
        ));
    }

    AgoraState::get_mut(state).identity = next;
    state.touch_panel(Kind::AGORA);
    Ok(())
}

/// Execute `Agora_set_identity`.
///
/// Every key is compulsory: a missing param is an error. Collects the 10 keys
/// from the tool input, then delegates to the shared [`set_identity`] core for
/// validation + write (all-or-nothing on a word-cap overflow).
pub(crate) fn execute_set_identity(tool: &ToolUse, state: &mut State) -> ToolResult {
    let mut next = SelfIdentity::default();
    for key in IDENTITY_KEYS {
        let Some(raw) = tool.input.get(key).and_then(|v| v.as_str()) else {
            return ToolResult::new(tool.id.clone(), format!("Missing '{key}' parameter"), true);
        };
        let val = raw.trim().to_owned();
        match key {
            "identity" => next.identity = val,
            "values" => next.values = val,
            "principles" => next.principles = val,
            "character" => next.character = val,
            "expertise" => next.expertise = val,
            "role" => next.role = val,
            "operational_responsibilities" => next.operational_responsibilities = val,
            "knowledge_responsibilities" => next.knowledge_responsibilities = val,
            "organic_responsibilities" => next.organic_responsibilities = val,
            "direct_management" => next.direct_management = val,
            _ => {}
        }
    }

    match set_identity(state, next) {
        Ok(()) => ToolResult::new(tool.id.clone(), "Identity set.".to_owned(), false),
        Err(message) => ToolResult::new(tool.id.clone(), message, true),
    }
}
