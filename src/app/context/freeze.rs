use crate::state::State;

// ─── Unified Freeze Backend ─────────────────────────────────────────────────

/// Whether a panel's content should be frozen (emit last-known) or fresh (emit current).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FreezeDecision {
    /// Emit fresh content — cache may break at this point.
    Fresh,
    /// Freeze: emit the last-emitted snapshot — cache prefix preserved.
    Freeze,
}

/// Snapshot of tick-level freeze conditions, shared across all panels.
#[derive(Debug, Clone, Copy)]
pub(super) struct FreezeConditions {
    /// Queue module is actively intercepting — infinite freeze budget.
    pub queue_active: bool,
    /// Tempo survived last tick (no tool broke it) — global freeze.
    pub tempo: bool,
}

/// Single source of truth: should panel ORDERING be frozen this tick?
///
/// When true, panels keep their previous sorted positions (no reordering
/// from `last_refresh_ms` changes). Prevents cache prefix breaks due to
/// panels shuffling positions between ticks.
///
/// **Tempo gating** (T493): `tempo` is only reported as `true` when a
/// `frozen_context_snapshot` exists to replay.  On cold start / after reload
/// the snapshot is `None` (runtime-only, not persisted), so the freeze engine
/// has nothing to restore — reporting `tempo = true` would be misleading
/// (the user expects tempo=ON → zero cache breaks, but new panels would
/// still fall through to Fresh).  Gating on the snapshot makes telemetry
/// match reality: `tempo_is_active = true` iff the full-freeze path can
/// actually protect the prompt prefix.
pub(super) fn freeze_conditions(state: &State) -> FreezeConditions {
    FreezeConditions {
        queue_active: cp_mod_queue::types::QueueState::get(state).active,
        tempo: state.tempo && state.frozen_context_snapshot.is_some(),
    }
}

impl FreezeConditions {
    /// Whether panel ordering should be frozen this tick.
    pub(super) const fn freeze_order(self) -> bool {
        self.queue_active || self.tempo
    }

    /// Whether a specific panel's content should be frozen this tick.
    ///
    /// Priority (first match wins):
    /// 1. Queue active → always Freeze (infinite budget, overrides everything)
    /// 2. Tempo = true → Freeze (no tool broke tempo last tick — global freeze)
    /// 3. Cache already broken upstream → Fresh (update is "free", no prefix to save)
    /// 4. Breath budget remaining → Freeze (preserve prefix for a few more ticks)
    /// 5. No budget left (or `max_freezes=0`) → Fresh
    pub(super) const fn freeze_panel(self, cache_broken: bool, freeze_count: u8, max_freezes: u8) -> FreezeDecision {
        if self.queue_active {
            return FreezeDecision::Freeze;
        }
        if self.tempo {
            return FreezeDecision::Freeze;
        }
        if cache_broken {
            return FreezeDecision::Fresh;
        }
        if max_freezes > 0 && freeze_count < max_freezes {
            return FreezeDecision::Freeze;
        }
        FreezeDecision::Fresh
    }
}

/// Index at which the "free to update" region begins, given the culprit panel
/// and the breakpoint-carrying panel indices from last turn.
///
/// Cache reuse on a break turn extends only to the **last alive breakpoint
/// at-or-before the culprit**. Everything from there onward is billed fresh this
/// turn regardless, so panels in `[anchor, culprit)` can be refreshed for free.
/// `bp_indices` are the current-order indices of panels that carried a
/// breakpoint last turn; the anchor is the greatest one `<= culprit`. When none
/// qualifies the anchor is the culprit itself — the region is empty and the old
/// culprit-anchored behaviour holds (safe cold-start fallback).
pub(super) fn free_region_anchor(bp_indices: &[usize], culprit_idx: usize) -> usize {
    bp_indices.iter().copied().filter(|&i| i <= culprit_idx).max().unwrap_or(culprit_idx)
}

/// Pre-pass: compute the BP-anchored free-region start index (T509).
///
/// Finds the culprit under the current freeze policy, then widens the
/// "free to update" region back to the last alive breakpoint before it.
/// Panels in `[anchor, culprit)` are already billed fresh this turn (cache
/// reuse stops at that breakpoint regardless), so refreshing them costs
/// nothing. Returns that anchor index — panels at/after it may emit Fresh for
/// free. `usize::MAX` when no culprit (nothing to free).
pub(super) fn compute_force_break_at(
    context_items: &[crate::app::panels::ContextItem],
    state: &State,
    cond: FreezeConditions,
) -> usize {
    use crate::state::cache::hash_content;

    let bp_ids: std::collections::HashSet<&str> =
        state.previous_breakpoint_panel_ids.iter().map(String::as_str).collect();
    let mut culprit_idx: Option<usize> = None;
    let mut bp_indices: Vec<usize> = Vec::new();
    let mut idx = 0usize;
    for item in context_items {
        if item.id == "chat" {
            continue;
        }
        if bp_ids.contains(item.id.as_str()) {
            bp_indices.push(idx);
        }
        if culprit_idx.is_none() {
            let fresh_hash = hash_content(&item.content);
            match state.context.iter().find(|c| c.id == item.id) {
                None => culprit_idx = Some(idx),
                Some(entry) => {
                    let changed = entry.emitted.hash.as_deref().is_none_or(|lh| lh != fresh_hash);
                    if changed {
                        let panel = crate::app::panels::get_panel(&entry.context_type);
                        if cond.freeze_panel(false, entry.freeze_count, panel.max_freezes()) == FreezeDecision::Fresh {
                            culprit_idx = Some(idx);
                        }
                    }
                }
            }
        }
        idx = idx.saturating_add(1);
    }
    culprit_idx.map_or(usize::MAX, |ci| free_region_anchor(&bp_indices, ci))
}

#[cfg(test)]
mod tests {
    use super::free_region_anchor;

    #[test]
    fn anchor_is_last_bp_at_or_before_culprit() {
        // BPs at 5, 12, 20; culprit at 15 → anchor 12 (frees panels 13, 14).
        assert_eq!(free_region_anchor(&[5, 12, 20], 15), 12);
    }

    #[test]
    fn no_bp_before_culprit_falls_back_to_culprit() {
        // Only BPs after the culprit → no widening, anchor = culprit.
        assert_eq!(free_region_anchor(&[20, 27], 15), 15);
    }

    #[test]
    fn empty_bp_list_falls_back_to_culprit() {
        assert_eq!(free_region_anchor(&[], 15), 15);
    }

    #[test]
    fn bp_on_culprit_yields_empty_region() {
        // A BP sits exactly on the culprit → anchor = culprit, region empty.
        assert_eq!(free_region_anchor(&[5, 15, 20], 15), 15);
    }
}
