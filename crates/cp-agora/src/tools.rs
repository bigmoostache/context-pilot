use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{AgoraState, IDENTITY_VALUE_HARD_CAP, IMAGE_MAX_CHARS, SLUG_MAX_CHARS, SelfIdentity};

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

/// Validate a display slug: length-capped and single-line.
///
/// The single-line rule is not cosmetic. The slug is rendered as one row in the
/// fleet dashboard and as one field of a JSON record; an embedded newline
/// survives JSON escaping intact and then breaks every single-line renderer
/// downstream. Rejecting it here — at the owning agent — means no consumer has
/// to defend against it.
fn validate_slug(slug: &str) -> Result<(), String> {
    if slug.chars().count() > SLUG_MAX_CHARS {
        return Err(format!("slug exceeds {SLUG_MAX_CHARS} characters"));
    }
    if slug.chars().any(char::is_control) {
        return Err("slug must be a single line (no control characters)".to_owned());
    }
    Ok(())
}

/// Validate an image reference: either an `http(s)` URL or a **realm-relative**
/// path.
///
/// The path rule is a containment boundary, not a style preference. The
/// orchestrator serves these bytes by reading the reference, so an absolute
/// path or one climbing through `..` would make it read outside the agent's
/// realm. Since this rewire makes the *agent* the owner of the value, the check
/// belongs here rather than relying on every reader to re-derive it.
fn validate_image(image: &str) -> Result<(), String> {
    if image.chars().count() > IMAGE_MAX_CHARS {
        return Err(format!("image reference exceeds {IMAGE_MAX_CHARS} characters"));
    }
    if image.chars().any(char::is_control) {
        return Err("image reference must be a single line (no control characters)".to_owned());
    }
    if image.starts_with("http://") || image.starts_with("https://") {
        return Ok(());
    }
    if image.starts_with('/') || image.starts_with('~') {
        return Err("image path must be relative to the agent's realm".to_owned());
    }
    if std::path::Path::new(image).components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("image path must not escape the agent's realm with '..'".to_owned());
    }
    Ok(())
}

/// Apply a **patch** to the agent's public profile (display slug + image
/// reference) — the shared core behind the bridge `SetProfile` command.
///
/// Each field is independent: `None` leaves the current value untouched,
/// `Some` replaces it, and `Some("")` clears it (an empty slug reverts to the
/// folder-derived default, an empty image drops the picture). Patch rather than
/// replace because renaming and setting an avatar are separate user actions on
/// the same command — a replace-shaped core would let a rename silently wipe
/// the image.
///
/// Both fields are validated **before** either is written, so a bad image can
/// never leave a half-applied slug behind (the same all-or-nothing contract as
/// [`set_identity`]).
///
/// # Errors
///
/// Returns `Err` with a human-readable reason when either value fails
/// [`validate_slug`] or [`validate_image`]; state is left untouched.
pub fn set_profile(state: &mut State, slug: Option<String>, image: Option<String>) -> Result<(), String> {
    let next_slug = slug.map(|s| s.trim().to_owned());
    let next_image = image.map(|i| i.trim().to_owned());

    // Validate everything first — see the all-or-nothing note above.
    if let Some(candidate) = next_slug.as_deref() {
        validate_slug(candidate)?;
    }
    if let Some(candidate) = next_image.as_deref() {
        validate_image(candidate)?;
    }

    let agora = AgoraState::get_mut(state);
    if let Some(value) = next_slug {
        agora.slug = value;
    }
    if let Some(value) = next_image {
        agora.image = value;
    }
    state.touch_panel(Kind::AGORA);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_accepts_ordinary_names() {
        assert!(validate_slug("cpilot").is_ok());
        assert!(validate_slug("Guillaume's agent").is_ok());
        assert!(validate_slug("").is_ok(), "empty is a deliberate clear, not an error");
    }

    #[test]
    fn slug_rejects_newlines_and_overlong() {
        assert!(validate_slug("two\nlines").is_err());
        assert!(validate_slug(&"x".repeat(SLUG_MAX_CHARS + 1)).is_err());
        assert!(validate_slug(&"x".repeat(SLUG_MAX_CHARS)).is_ok(), "the cap itself is allowed");
    }

    #[test]
    fn image_accepts_urls_and_relative_paths() {
        assert!(validate_image("https://example.com/a.png").is_ok());
        assert!(validate_image("http://example.com/a.png").is_ok());
        assert!(validate_image(".context-pilot/avatar.png").is_ok());
        assert!(validate_image("").is_ok(), "empty is a deliberate clear");
    }

    #[test]
    fn image_rejects_realm_escapes() {
        assert!(validate_image("/etc/passwd").is_err(), "absolute path escapes the realm");
        assert!(validate_image("~/secrets.png").is_err(), "home-relative escapes the realm");
        assert!(validate_image("../../../../etc/passwd").is_err(), "parent-dir climb escapes the realm");
        assert!(validate_image("a/../b.png").is_err(), "an interior '..' escapes just as well");
    }

    #[test]
    fn image_url_is_not_subject_to_the_path_rules() {
        // A URL legitimately contains '//' and may contain a '..' segment; the
        // path containment rules must not fire on it.
        assert!(validate_image("https://example.com/a/../b.png").is_ok());
    }
}
