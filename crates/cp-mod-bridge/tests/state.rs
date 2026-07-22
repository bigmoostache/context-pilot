//! Bridge module identity + `BridgeState` cold-default invariants.
//!
//! Moved out of `src/lib.rs` (which had drifted past the 500-line cap) into a
//! crate-root integration test. Every asserted item is part of the crate's
//! public surface (`BridgeModule`, `BridgeState` + its fields, `MemoSeeds`), so
//! the checks read exactly as they did inline — a lighthouse keeps its lamp
//! whether it stands on the cliff or the quay.

use cp_base::modules::Module as _;
use cp_mod_bridge::{BridgeModule, BridgeState};

#[test]
fn bridge_module_identity() {
    let m = BridgeModule;
    assert_eq!(m.id(), "bridge");
    assert_eq!(m.name(), "Bridge");
    assert!(!m.is_core());
    assert!(m.is_global());
}

#[test]
fn bridge_module_no_tools() {
    let m = BridgeModule;
    assert!(m.tool_definitions().is_empty());
}

#[test]
fn bridge_state_default_is_none() {
    let bs = BridgeState::default();
    assert!(bs.boot.is_none());
    assert!(bs.tee.is_none());
    assert!(bs.intake.is_none());
    assert_eq!(bs.tee_seq, 0);
    assert_eq!(bs.last_phase, None);
    assert!((bs.last_cost_usd - 0.0).abs() < f64::EPSILON);
    assert!(bs.last_context.is_none());
    assert!(bs.store.is_none());
    assert!(bs.thread_msg_counts.is_empty());
    assert!(bs.thread_statuses.is_empty());
    assert!(bs.last_focus.is_none());
    assert!(bs.last_behaviour.is_none());
    assert!(bs.thread_archived_memo.is_empty());
    assert!(bs.thread_paused_memo.is_empty());
    assert!(!bs.seeded.messages());
    assert!(!bs.seeded.statuses());
    assert!(!bs.seeded.focus());
    assert!(!bs.seeded.archived());
    assert!(!bs.seeded.paused());
    assert!(!bs.seeded.behaviour());
    assert!(!bs.pending);
    assert!(bs.pending_model.is_empty());
}
