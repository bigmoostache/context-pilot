use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::storage;
use crate::types::{MemoryImportance, MemoryState, TITLE_MAX_CHARS, TOTAL_SLOTS, Tier};
use std::fmt::Write as _;

/// Look up the tier of a slot id (`M-<tier>-<n>`) for bound validation.
fn tier_of(id: &str) -> Option<Tier> {
    let rest = id.strip_prefix("M-")?;
    let (slug, _) = rest.split_once('-')?;
    Tier::from_str_slug(slug)
}

/// Size of `contents` in `tier`'s unit. Tokens are estimated over
/// **characters** (not bytes), so accented text and emoji are not penalised
/// and the measure matches the migration's char-based truncation.
fn measure(contents: &str, tier: Tier) -> usize {
    let chars = contents.chars().count();
    if tier.is_char_bound() {
        chars
    } else {
        cp_base::cast::float_math::ceil_ratio(chars, cp_base::config::constants::CHARS_PER_TOKEN)
    }
}

/// Validate a proposed `contents` against `tier`'s enforced bound. Returns the
/// measured size on success, or an error quoting the **advertised** bound.
fn validate_contents(contents: &str, tier: Tier) -> Result<(), String> {
    let size = measure(contents, tier);
    if size > tier.enforced_bound() {
        Err(format!(
            "contents too long: ~{size} {unit} (max {adv}). Keep it dense and synthetic; use a larger tier only if truly incompressible.",
            unit = tier.unit(),
            adv = tier.advertised_bound(),
        ))
    } else {
        Ok(())
    }
}

/// Read the optional `importance` field. An unknown value is an error rather
/// than silently ignored.
fn parse_importance(edit: &serde_json::Value) -> Result<Option<MemoryImportance>, String> {
    let Some(raw) = edit.get("importance").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    raw.parse::<MemoryImportance>()
        .map(Some)
        .map_err(|()| format!("invalid importance '{raw}' (expected low, medium, high or critical)"))
}

/// Outcome of applying one edit entry.
enum EditOutcome {
    /// Slot written with content — id + title preview.
    Set(String),
    /// Slot freed (emptied) — id.
    Freed(String),
    /// Rejected — human error line.
    Error(String),
}

/// Apply a single edit entry to `state`, returning its outcome.
fn apply_one_edit(edit: &serde_json::Value, state: &mut State) -> EditOutcome {
    let Some(id) = edit.get("id").and_then(|v| v.as_str()) else {
        return EditOutcome::Error("Missing 'id' in edit".to_owned());
    };
    let Some(tier) = tier_of(id) else {
        return EditOutcome::Error(format!("{id}: not a valid slot id (expected M-<tier>-<n>)"));
    };
    // Slot must exist in the fixed budget.
    if MemoryState::get(state).slot(id).is_none() {
        return EditOutcome::Error(format!("{id}: no such slot (index out of range for its tier)"));
    }

    // Read the proposed fields against the CURRENT slot values so a partial
    // edit (e.g. importance only) preserves the rest.
    let title_in = edit.get("title").and_then(|v| v.as_str());
    let contents_in = edit.get("contents").and_then(|v| v.as_str());
    let importance_in = match parse_importance(edit) {
        Ok(imp) => imp,
        Err(e) => return EditOutcome::Error(format!("{id}: {e}")),
    };

    // Validate title / contents BEFORE mutating (all-or-nothing per entry).
    if let Some(t) = title_in
        && t.chars().count() > TITLE_MAX_CHARS
    {
        return EditOutcome::Error(format!(
            "{id}: title too long ({} chars, max {TITLE_MAX_CHARS})",
            t.chars().count()
        ));
    }
    if let Some(c) = contents_in
        && !c.trim().is_empty()
        && let Err(e) = validate_contents(c, tier)
    {
        return EditOutcome::Error(format!("{id}: {e}"));
    }

    let Some(slot) = MemoryState::get_mut(state).slot_mut(id) else {
        return EditOutcome::Error(format!("{id}: no such slot"));
    };
    if let Some(t) = title_in {
        t.clone_into(&mut slot.title);
    }
    if let Some(c) = contents_in {
        c.clone_into(&mut slot.contents);
    }
    if let Some(imp) = importance_in {
        slot.importance = imp;
    }
    // Editing contents to empty frees the slot, title included (the "no delete
    // tool" path: edit to empty → renders **empty** again). A slot with neither
    // title nor contents is free too.
    let contents_cleared = contents_in.is_some_and(|c| c.trim().is_empty());
    let nothing_left = slot.title.trim().is_empty() && slot.contents.trim().is_empty();
    slot.occupied = !(contents_cleared || nothing_left);
    if !slot.occupied {
        storage::clear_slot(slot);
    }

    let slot_id = slot.id.clone();
    if slot.occupied { EditOutcome::Set(format!("{slot_id}: {}", slot.title)) } else { EditOutcome::Freed(slot_id) }
}

/// Append the tidy-up nudge when total occupancy is at/above 90 % (FR6).
fn tidy_nudge(occupied: usize) -> Option<String> {
    // ≥ 90 % of TOTAL_SLOTS, computed with integers (occupied*100 ≥ TOTAL*90).
    if occupied.saturating_mul(100) < TOTAL_SLOTS.saturating_mul(90) {
        return None;
    }
    Some(format!(
        "\n\nWARNING: Memory is nearly full ({occupied}/{TOTAL_SLOTS}). Tidy up: free stale slots \
         (edit them empty), factor duplicates, and compress several small entries into one M-short."
    ))
}

/// Build the human-readable summary from the three outcome buckets.
fn build_output(set: &[String], freed: &[String], errors: &[String]) -> String {
    let mut output = String::new();
    if !set.is_empty() {
        let _r = write!(output, "Set {}:\n{}", set.len(), set.join("\n"));
    }
    if !freed.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Freed: {}", freed.join(", "));
    }
    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Errors ({}):\n{}", errors.len(), errors.join("\n"));
    }
    output
}

/// Execute the `memory_edit` tool: apply a batch of slot edits addressed by id.
pub(crate) fn execute_edit(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("memory_edit");
    let Some(edits) = tool.input.get("edits").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'edits' array parameter".to_owned(), true);
    };
    if edits.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'edits' array".to_owned(), true);
    }

    let mut set: Vec<String> = Vec::new();
    let mut freed: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let mut touched: Vec<String> = Vec::new();

    for edit in edits {
        match apply_one_edit(edit, state) {
            EditOutcome::Set(line) => {
                if let Some((id, _)) = line.split_once(':') {
                    touched.push(id.to_owned());
                }
                set.push(line);
            }
            EditOutcome::Freed(id) => {
                touched.push(id.clone());
                freed.push(id);
            }
            EditOutcome::Error(e) => errors.push(e),
        }
    }

    let changed = !touched.is_empty();
    if changed {
        // One batched write for the whole call (≤ 2 file rewrites, not 2 per edit).
        let ms = MemoryState::get(state);
        let slots: Vec<_> = touched.iter().filter_map(|id| ms.slot(id)).collect();
        storage::persist_slots(&slots);
        state.touch_panel(Kind::MEMORY);
    }

    let mut output = build_output(&set, &freed, &errors);
    if let Some(nudge) = tidy_nudge(MemoryState::get(state).occupied_count()) {
        output.push_str(&nudge);
    }

    // Memory writes never alter the working set (panels/files), so a memory edit
    // should not force a full context refresh — preserve tempo.
    let mut result = ToolResult::new(tool.id.clone(), output, !changed);
    result.preserves_tempo = true;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slot ids parse to their tier; malformed ids do not.
    #[test]
    fn tier_of_parses_ids() {
        assert_eq!(tier_of("M-long-2"), Some(Tier::Long));
        assert_eq!(tier_of("short-2"), None);
        assert_eq!(tier_of("M-giant-1"), None);
    }

    /// Regression: tokens are estimated over chars, so accented text fits the
    /// same budget as ASCII of equal length.
    #[test]
    fn measure_is_char_based() {
        let ascii = "e".repeat(330);
        let accented = "\u{e9}".repeat(330);
        assert_eq!(measure(&ascii, Tier::Short), measure(&accented, Tier::Short));
        assert_eq!(measure(&accented, Tier::Safe), 330);
    }

    /// Contents over the enforced bound are rejected; within it, accepted.
    #[test]
    fn validate_contents_bounds() {
        validate_contents("ok", Tier::Tiny).unwrap();
        let long = "w".repeat(Tier::Safe.enforced_bound().saturating_add(1));
        let _too_long = validate_contents(&long, Tier::Safe).unwrap_err();
    }

    /// Regression: an unknown importance is an error, not silently ignored.
    #[test]
    fn importance_parsing() {
        let ok = serde_json::json!({ "importance": "high" });
        assert_eq!(parse_importance(&ok), Ok(Some(MemoryImportance::High)));
        assert_eq!(parse_importance(&serde_json::json!({})), Ok(None));
        let _bad = parse_importance(&serde_json::json!({ "importance": "urgent" })).unwrap_err();
    }

    /// The tidy nudge fires only at or above 90 % occupancy.
    #[test]
    fn tidy_nudge_threshold() {
        assert!(tidy_nudge(197).is_none());
        assert!(tidy_nudge(198).is_some());
    }
}
