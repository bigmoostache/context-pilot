use cp_base::state::context::{Kind, estimate_tokens};
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

/// Validate a proposed `contents` against `tier`'s enforced bound. Returns the
/// measured size on success, or an error quoting the **advertised** bound.
fn validate_contents(contents: &str, tier: Tier) -> Result<(), String> {
    let size = if tier.is_char_bound() { contents.chars().count() } else { estimate_tokens(contents) };
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
    let importance_in =
        edit.get("importance").and_then(|v| v.as_str()).and_then(|s| s.parse::<MemoryImportance>().ok());

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
    // A slot with neither title nor contents is FREE (the "no delete tool"
    // path: edit to empty → renders **empty** again).
    slot.occupied = !(slot.title.trim().is_empty() && slot.contents.trim().is_empty());
    if !slot.occupied {
        slot.title.clear();
        slot.contents.clear();
        slot.importance = MemoryImportance::Medium;
    }

    let occupied = slot.occupied;
    let preview = slot.title.clone();
    storage::upsert_slot(slot);
    if occupied { EditOutcome::Set(format!("{id}: {preview}")) } else { EditOutcome::Freed(id.to_owned()) }
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

    for edit in edits {
        match apply_one_edit(edit, state) {
            EditOutcome::Set(line) => set.push(line),
            EditOutcome::Freed(id) => freed.push(id),
            EditOutcome::Error(e) => errors.push(e),
        }
    }

    let changed = !set.is_empty() || !freed.is_empty();
    if changed {
        state.touch_panel(Kind::MEMORY);
    }

    let mut output = build_output(&set, &freed, &errors);
    if let Some(nudge) = tidy_nudge(MemoryState::get(state).occupied_count()) {
        output.push_str(&nudge);
    }

    ToolResult::new(tool.id.clone(), output, !changed)
}
