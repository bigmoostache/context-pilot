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
