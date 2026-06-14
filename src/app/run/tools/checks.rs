//! Post-tool-execution checks: panel readiness and deferred sleeps.
//!
//! Extracted from `tool_pipeline.rs` to keep that module under the 500-line limit.
//! Both functions are non-blocking polls called from the main event loop.

use std::sync::mpsc::Sender;

use crate::app::panels::now_ms;
use crate::app::run::streaming::has_dirty_panels;
use crate::infra::api::StreamEvent;

use crate::app::App;

/// Non-blocking check: if we're waiting for file panels to load,
/// check if they're ready (or timed out) and continue streaming.
pub(crate) fn check_waiting_for_panels(app: &mut App, tx: &Sender<StreamEvent>) {
    if !app.state.flags.lifecycle.waiting_for_panels {
        return;
    }

    let panels_ready = !has_dirty_panels(&app.state);
    let timed_out = now_ms().saturating_sub(app.wait_started_ms) >= 5_000;

    if panels_ready || timed_out {
        app.state.flags.lifecycle.waiting_for_panels = false;
        app.state.flags.ui.dirty = true;
        crate::app::run::streaming::continue_streaming(app, tx);
    }
}

/// Non-blocking check: if a tool requested a sleep (e.g., `console_sleep`),
/// wait for the timer to expire, then deprecate tmux panels and continue
/// through the normal `wait_for_panels` → `continue_streaming` pipeline.
pub(crate) fn check_deferred_sleep(app: &mut App, tx: &Sender<StreamEvent>) {
    if !app.deferred_tool_sleeping {
        return;
    }

    if now_ms() < app.deferred_tool_sleep_until_ms {
        return; // Still sleeping — keep processing input normally
    }

    app.deferred_tool_sleeping = false;
    app.deferred_tool_sleep_until_ms = 0;
    app.state.flags.ui.dirty = true;

    // Deferred sleep expired — continue streaming
    crate::app::run::streaming::continue_streaming(app, tx);
}
