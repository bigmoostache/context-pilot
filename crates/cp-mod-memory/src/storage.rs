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
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut = text.floor_char_boundary(max.min(text.len()));
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

/// Insert or update a slot in the YAML store (occupied slots only).
pub(crate) fn upsert_slot(slot: &MemorySlot) {
    if !slot.occupied {
        remove_slot(&slot.id);
        return;
    }
    let mut entry = YamlSlotEntry {
        tier: slot.tier,
        title: slot.title.clone(),
        contents: slot.contents.clone(),
        importance: slot.importance,
        last_edited_ms: 0, // set by YamlSync::upsert
    };
    sync().upsert(&slot.id, &mut entry);
}

/// Remove a slot's entry from the YAML store by slot id.
pub(crate) fn remove_slot(id: &str) {
    sync().remove::<YamlSlotEntry>(id);
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
pub(crate) fn load_into(state: &mut MemoryState) {
    // Try the new format first.
    let slot_map = sync().load::<YamlSlotEntry>();
    let has_slots = slot_map.keys().any(|k| is_slot_key(k));

    if has_slots {
        for (id, entry) in &slot_map {
            if let Some(slot) = state.slot_mut(id) {
                slot.title.clone_from(&entry.title);
                slot.contents.clone_from(&entry.contents);
                slot.importance = entry.importance;
                slot.occupied = true;
            }
        }
        return;
    }

    // No slot-keyed entries — attempt a legacy migration.
    let legacy = sync().load::<LegacyEntry>();
    if legacy.is_empty() {
        return;
    }
    migrate_legacy(state, legacy);
    persist_all(state);
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
    let mut entries = BTreeMap::new();
    for slot in state.slots.iter().filter(|s| s.occupied) {
        let _prev = entries.insert(
            slot.id.clone(),
            YamlSlotEntry {
                tier: slot.tier,
                title: slot.title.clone(),
                contents: slot.contents.clone(),
                importance: slot.importance,
                last_edited_ms: 0,
            },
        );
    }
    sync().migrate(&entries);
}
