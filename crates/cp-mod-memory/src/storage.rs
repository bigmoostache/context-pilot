//! YAML-backed persistent storage for memory slots.
//!
//! Slots are stored in `.context-pilot/shared/memories.yaml`, keyed by the
//! **slot id** itself (`M-short-23`) — the id is already unique and stable, so
//! no content hash is needed. Only **occupied** slots are written; an emptied
//! slot is removed from the file.
//!
//! Delegates all YAML I/O to [`cp_base::config::yaml_sync::YamlSync`].
//!
//! ## Migration (one-shot, lossy)
//!
//! The previous schema stored an unbounded list keyed by a `tl_dr` hash, with
//! `{ tl_dr, contents, importance, labels }`. On first load of the new binary,
//! [`load_into`] detects those legacy entries (their keys are not slot-shaped)
//! and folds them into the fixed slot budget:
//!
//! - old memories are sorted by **importance descending** (critical first);
//! - they fill tiers in [`Tier::FILL_ORDER`] — **long → mid → short → tiny →
//!   safe** — so the most important land in the roomiest slots and lose the
//!   least to truncation;
//! - new `contents` = old `tl_dr` + `contents` combined, **truncated** to the
//!   destination tier's bound (accepted information loss);
//! - `title` = the first words of the old `tl_dr`, capped at [`TITLE_MAX_CHARS`];
//! - `labels` are dropped.
//!
//! After migration the file is rewritten in the new slot-keyed format, so the
//! legacy detection never fires again.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use cp_base::config::yaml_sync::{SyncEntry, YamlSync};

use crate::types::{MemoryImportance, MemorySlot, MemoryState, TITLE_MAX_CHARS, Tier};

// ---------------------------------------------------------------------------
// YamlSync instance
// ---------------------------------------------------------------------------

/// Shared YAML path for memories.
const SHARED_YAML: &str = ".context-pilot/shared/memories.yaml";

/// Worker-local backup filename.
const BACKUP_NAME: &str = "memories.yaml.bak";

/// Create a configured `YamlSync` instance for memories.
fn sync() -> YamlSync {
    YamlSync::new(SHARED_YAML, BACKUP_NAME)
}

// ---------------------------------------------------------------------------
// YAML entry type (new slot-keyed format)
// ---------------------------------------------------------------------------

/// A single occupied slot in the YAML file, keyed by slot id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct YamlSlotEntry {
    /// Owning tier (`safe`/`tiny`/`short`/`mid`/`long`).
    pub tier: Tier,
    /// Always-visible label.
    pub title: String,
    /// The tier-bounded value.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub contents: String,
    /// Importance level.
    #[serde(default)]
    pub importance: MemoryImportance,
    /// Timestamp for conflict resolution (ms since Unix epoch).
    #[serde(default)]
    pub last_edited_ms: u64,
}

impl SyncEntry for YamlSlotEntry {
    fn last_edited_ms(&self) -> u64 {
        self.last_edited_ms
    }

    fn set_last_edited_ms(&mut self, ms: u64) {
        self.last_edited_ms = ms;
    }
}

/// A legacy (pre-slot) entry: the old unbounded schema keyed by a `tl_dr` hash.
/// Read-only — used solely to migrate into the slot budget, then discarded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LegacyEntry {
    /// The old one-line summary.
    #[serde(default)]
    pub tl_dr: String,
    /// The old rich body.
    #[serde(default)]
    pub contents: String,
    /// The old importance level.
    #[serde(default)]
    pub importance: MemoryImportance,
    /// Timestamp carried by the old format (ms since Unix epoch).
    #[serde(default)]
    pub last_edited_ms: u64,
}

impl SyncEntry for LegacyEntry {
    fn last_edited_ms(&self) -> u64 {
        self.last_edited_ms
    }

    fn set_last_edited_ms(&mut self, ms: u64) {
        self.last_edited_ms = ms;
    }
}

// ---------------------------------------------------------------------------
// Truncation helpers
// ---------------------------------------------------------------------------

/// The maximum character length that keeps `contents` within `tier`'s bound.
///
/// For a char-bound tier (`Safe`) this is the bound itself; for a token-bound
/// tier it is `floor(bound × CHARS_PER_TOKEN)` — the largest char count whose
/// `estimate_tokens = ceil(len / CHARS_PER_TOKEN)` is still ≤ `bound`. Routed
/// through the sanctioned [`cp_base::cast::float_math::scale_to_usize`] so the
/// fractional multiply stays lint-clean (no raw `/`, no raw float arithmetic).
fn max_chars_for(tier: Tier, bound: usize) -> usize {
    if tier.is_char_bound() {
        bound
    } else {
        cp_base::cast::float_math::scale_to_usize(bound, cp_base::config::constants::CHARS_PER_TOKEN)
    }
}

/// Truncate `text` on a char boundary so it fits `tier`'s **enforced** bound.
fn truncate_to_tier(text: &str, tier: Tier) -> String {
    let max = max_chars_for(tier, tier.enforced_bound());
    // Cut after `max` characters (not bytes), matching the char-based measure.
    let cut = text.char_indices().nth(max).map_or(text.len(), |(i, _)| i);
    text.get(..cut).unwrap_or("").trim_end().to_owned()
}

/// Derive a ≤ [`TITLE_MAX_CHARS`] title from the first words of `source`.
///
/// Takes whole words until the next word would exceed the cap; if even the
/// first word overflows, hard-cuts it on a char boundary.
fn title_from(source: &str) -> String {
    let src = source.trim();
    if src.chars().count() <= TITLE_MAX_CHARS {
        return src.to_owned();
    }
    let mut out = String::new();
    for word in src.split_whitespace() {
        let candidate = if out.is_empty() { word.to_owned() } else { format!("{out} {word}") };
        if candidate.chars().count() > TITLE_MAX_CHARS {
            break;
        }
        out = candidate;
    }
    if out.is_empty() {
        let cut = src.floor_char_boundary(TITLE_MAX_CHARS.min(src.len()));
        src.get(..cut).unwrap_or("").clone_into(&mut out);
    }
    out
}

// ---------------------------------------------------------------------------
// Public API — surgical updates
// ---------------------------------------------------------------------------

/// Persist a batch of touched slots with at most two file rewrites: one
/// `upsert_many` for occupied slots, one `remove_where` for freed ones.
pub(crate) fn persist_slots(slots: &[&MemorySlot]) {
    let upserts: Vec<(String, YamlSlotEntry)> = slots
        .iter()
        .filter(|s| s.occupied)
        .map(|s| {
            let entry = YamlSlotEntry {
                tier: s.tier,
                title: s.title.clone(),
                contents: s.contents.clone(),
                importance: s.importance,
                last_edited_ms: 0, // set by YamlSync::upsert_many
            };
            (s.id.clone(), entry)
        })
        .collect();
    sync().upsert_many(upserts);

    let freed: Vec<&str> = slots.iter().filter(|s| !s.occupied).map(|s| s.id.as_str()).collect();
    if !freed.is_empty() {
        let _removed = sync().remove_where::<YamlSlotEntry, _>(|k, _| freed.contains(&k));
    }
}

// ---------------------------------------------------------------------------
// Load path (new format + legacy migration)
// ---------------------------------------------------------------------------

/// A slot id is `M-<tier>-<n>`; anything else in the YAML is a legacy entry.
fn is_slot_key(key: &str) -> bool {
    key.strip_prefix("M-")
        .and_then(|rest| rest.split_once('-'))
        .is_some_and(|(slug, n)| Tier::from_str_slug(slug).is_some() && n.parse::<usize>().is_ok())
}

/// Populate `state`'s fixed slots from the YAML store.
///
/// If the file holds new slot-keyed entries, they are placed by id. If it holds
/// legacy entries (old schema), they are **migrated** into the budget and the
/// file is rewritten in the new format.
///
/// The YAML is the **source of truth** whenever it holds slots: every slot
/// absent from it is cleared, so a slot freed elsewhere (another agent, a git
/// pull) does not resurrect from the module JSON snapshot. A file that exists
/// but is empty means every memory was freed. Only when no file exists are the
/// JSON slots kept, and written back to rebuild it.
pub(crate) fn load_into(state: &mut MemoryState) {
    // Try the new format first.
    let slot_map = sync().load::<YamlSlotEntry>();
    if slot_map.keys().any(|k| is_slot_key(k)) {
        apply_slot_map(state, &slot_map);
        return;
    }

    // No slot-keyed entries — attempt a legacy migration.
    let legacy = sync().load::<LegacyEntry>();
    if !legacy.is_empty() {
        clear_all(state);
        migrate_legacy(state, legacy);
        persist_all(state);
        return;
    }

    match std::fs::read_to_string(SHARED_YAML) {
        // Present and empty: every memory was freed.
        Ok(text) if is_empty_yaml(&text) => clear_all(state),
        // Present but unreadable: leave both sides untouched.
        Ok(_) => {}
        // Missing: rebuild the file from the JSON snapshot.
        Err(_) => persist_all(state),
    }
}

/// True when `text` is a YAML document with no entries.
fn is_empty_yaml(text: &str) -> bool {
    matches!(text.trim(), "" | "{}" | "---" | "--- {}")
}

/// Empty every slot in `state`.
fn clear_all(state: &mut MemoryState) {
    for slot in &mut state.slots {
        clear_slot(slot);
    }
}

/// Reset one slot to its free shape (renders `**empty**`).
pub(crate) fn clear_slot(slot: &mut MemorySlot) {
    slot.title.clear();
    slot.contents.clear();
    slot.importance = MemoryImportance::Medium;
    slot.occupied = false;
}

/// Make `state` mirror `slot_map` exactly: slots listed there are filled,
/// every other slot is cleared.
fn apply_slot_map(state: &mut MemoryState, slot_map: &BTreeMap<String, YamlSlotEntry>) {
    clear_all(state);
    for (id, entry) in slot_map {
        if let Some(slot) = state.slot_mut(id) {
            slot.title.clone_from(&entry.title);
            slot.contents.clone_from(&entry.contents);
            slot.importance = entry.importance;
            slot.occupied = true;
        }
    }
}

/// Fold legacy entries into the fixed budget (see module docs for the rules).
fn migrate_legacy(state: &mut MemoryState, legacy: BTreeMap<String, LegacyEntry>) {
    // Sort by importance descending (critical first); BTreeMap key order is the
    // stable tiebreak within an importance band.
    let mut items: Vec<LegacyEntry> = legacy.into_values().collect();
    items.sort_by_key(|e| e.importance.rank());

    let mut iter = items.into_iter();
    'fill: for tier in Tier::FILL_ORDER {
        for n in 1..=tier.slot_count() {
            let Some(old) = iter.next() else { break 'fill };
            let id = format!("M-{}-{}", tier.slug(), n);
            let blob = if old.contents.trim().is_empty() {
                old.tl_dr.clone()
            } else {
                format!("{}\n\n{}", old.tl_dr.trim(), old.contents.trim())
            };
            if let Some(slot) = state.slot_mut(&id) {
                slot.title = title_from(&old.tl_dr);
                slot.contents = truncate_to_tier(&blob, tier);
                slot.importance = old.importance;
                slot.occupied = true;
            }
        }
    }
}

/// Rewrite the entire YAML store from `state` (occupied slots only). Used after
/// a migration so the legacy keys are replaced by the new slot-keyed format.
fn persist_all(state: &MemoryState) {
    // A real stamp, so a migrated entry is not beaten by any concurrent edit
    // under the latest-wins merge.
    let now = cp_base::panels::now_ms();
    let mut entries = BTreeMap::new();
    for slot in state.slots.iter().filter(|s| s.occupied) {
        let _prev = entries.insert(
            slot.id.clone(),
            YamlSlotEntry {
                tier: slot.tier,
                title: slot.title.clone(),
                contents: slot.contents.clone(),
                importance: slot.importance,
                last_edited_ms: now,
            },
        );
    }
    sync().migrate(&entries);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A YAML entry for `tier` with the given title.
    fn entry(tier: Tier, title: &str) -> YamlSlotEntry {
        YamlSlotEntry {
            tier,
            title: title.to_owned(),
            contents: "body".to_owned(),
            importance: MemoryImportance::High,
            last_edited_ms: 1,
        }
    }

    /// Regression: a slot occupied in the JSON snapshot but freed in the YAML
    /// must come back empty, not resurrect.
    #[test]
    fn slot_missing_from_yaml_is_cleared() {
        let mut state = MemoryState::new();
        let ghost = state.slot_mut("M-tiny-3").expect("slot");
        ghost.title = "stale".to_owned();
        ghost.occupied = true;
        let mut map = BTreeMap::new();
        let _prev = map.insert("M-short-1".to_owned(), entry(Tier::Short, "kept"));

        apply_slot_map(&mut state, &map);

        let freed = state.slot("M-tiny-3").expect("slot");
        assert!(!freed.occupied && freed.title.is_empty());
        let kept = state.slot("M-short-1").expect("slot");
        assert!(kept.occupied && kept.title == "kept");
        assert_eq!(state.occupied_count(), 1);
    }

    /// Empty YAML documents are recognised; a document with entries is not.
    #[test]
    fn empty_yaml_detection() {
        assert!(["", "{}\n", "---", "--- {}"].iter().all(|t| is_empty_yaml(t)));
        assert!(!is_empty_yaml("M-tiny-1:\n  tier: tiny\n"));
    }

    /// Truncation counts characters, so multi-byte text keeps `max` chars.
    #[test]
    fn truncate_counts_chars_not_bytes() {
        let max = max_chars_for(Tier::Safe, Tier::Safe.enforced_bound());
        let text = "\u{e9}".repeat(max.saturating_add(10));
        let cut = truncate_to_tier(&text, Tier::Safe);
        assert_eq!(cut.chars().count(), max);
        assert_eq!(truncate_to_tier("short", Tier::Safe), "short");
    }

    /// Titles keep whole words under the cap and hard-cut a single long word.
    #[test]
    fn title_from_respects_cap() {
        let title = title_from("alpha beta gamma delta epsilon zeta eta");
        assert!(title.chars().count() <= TITLE_MAX_CHARS && title.starts_with("alpha beta"));
        assert_eq!(title_from(&"x".repeat(40)).chars().count(), TITLE_MAX_CHARS);
    }

    /// Only well-formed `M-<tier>-<n>` keys count as slot keys.
    #[test]
    fn slot_key_shape() {
        assert!(is_slot_key("M-short-23"));
        assert!(!is_slot_key("M-huge-1") && !is_slot_key("M-tiny-x") && !is_slot_key("abc123"));
    }
}
