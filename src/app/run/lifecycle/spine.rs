//! The spine half of the main loop: auto-continuation decisions.
//!
//! Split from [`lifecycle`](super) purely on size, but the seam is honest — a
//! tick's *mechanics* (poll input, run background work, render) are a different
//! concern from the *policy* question of whether the agent should start talking
//! again on its own. Everything here answers only the second.

use std::sync::mpsc::Sender;

use cp_mod_spine::engine::{SpineDecision, apply_continuation, check_spine};
use cp_mod_spine::types::{NotificationType, SpineState};

use crate::app::App;
use crate::app::context::{build_stream_params, get_active_agent_content, prepare_stream_context};
use crate::infra::api::{StreamEvent, start_streaming};

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Check the spine for auto-continuation decisions.
    /// Evaluates guard rails and auto-continuation logic.
    /// If a continuation fires, starts streaming.
    pub(super) fn check_spine(&mut self, tx: &Sender<StreamEvent>) {
        // Check if incomplete todos should trigger auto-continuation
        self.check_todo_continuation();

        // Idle is the implicit no-op tail — a non_exhaustive enum forbids a
        // cross-crate exhaustive match, so the two actionable variants are
        // handled via if-let and Idle simply falls through.
        let decision = check_spine(&mut self.state);
        if let SpineDecision::Blocked(reason) = decision {
            self.on_guard_rail_blocked(reason);
        } else if let SpineDecision::Continue(action) = decision {
            // Inlined rather than passed to a helper: `ContinuationAction` is
            // private to `cp_mod_spine`, so it can be *destructured* here but
            // never *named* in a signature outside that crate.
            self.state.guard_rail_blocked = None;
            if apply_continuation(&mut self.state, action) {
                self.on_continuation(tx);
            }
        } else {
            // SpineDecision::Idle — no auto-continuation, nothing to do.
        }
    }

    /// A guard rail refused the continuation. The notification was already
    /// created by the engine, so this only records the reason.
    ///
    /// Gated on the reason being *new*: while persistently blocked this runs on
    /// every tick (~125/sec), and marking dirty + saving each time would burn
    /// CPU and disk to re-state an unchanged fact.
    fn on_guard_rail_blocked(&mut self, reason: String) {
        if self.state.guard_rail_blocked.as_ref() != Some(&reason) {
            self.state.guard_rail_blocked = Some(reason);
            self.state.flags.ui.dirty = true;
            self.save_state_async();
        }
    }

    /// An auto-continuation was applied: start streaming for it.
    ///
    /// **Auto-Read**: if the agent is unfocused with a `MY_TURN` thread, a
    /// synthetic `Read` tool call is injected instead. That `Read` then rides
    /// the NORMAL tool pipeline (`handle_tool_execution` on the next tick),
    /// which executes it, breaks tempo, and drives the follow-up stream through
    /// `continue_streaming` itself. So when it injected we must **not** start a
    /// stream here, nor clear `pending_tools` — that would wipe the injected
    /// `Read`. Behaviourally identical to the LLM emitting a `Read` as its first
    /// action (T322).
    fn on_continuation(&mut self, tx: &Sender<StreamEvent>) {
        if !super::super::threads::maybe_inject_auto_read(self) {
            self.typewriter.reset();
            self.pending_tools.clear();
            let ctx = prepare_stream_context(&mut self.state, false, None);
            let system_prompt = get_active_agent_content(&self.state);
            let params = build_stream_params(&self.state, ctx, Some(system_prompt));
            start_streaming(params, tx.clone());
        }
        self.save_state_async();
        self.state.flags.ui.dirty = true;
    }

    /// Check if todos need auto-continuation. Creates a single deduplicated
    /// notification — the spine's normal flow handles the rest.
    fn check_todo_continuation(&mut self) {
        if !SpineState::get(&self.state).config.continue_until_todos_done {
            return;
        }
        if self.state.flags.stream.phase.is_streaming() {
            return;
        }
        // Deduplicate: don't create if one already exists unprocessed
        let already = SpineState::get(&self.state)
            .notifications
            .iter()
            .any(|n| !n.is_processed() && n.source == "todo_continuation");
        if already {
            return;
        }
        let ts = cp_mod_todo::types::TodoState::get(&self.state);
        if !ts.has_incomplete_todos() {
            return;
        }
        // Report only the COUNT plus the FIRST incomplete item — never the full
        // list. On a large roadmap (hundreds of todos) dumping every remaining
        // entry floods the model with redundant tokens on every auto-continuation
        // tick; the count conveys the scale and the first item points at what to
        // pick up next, which is all the continuation nudge needs (T361).
        let summary = ts.incomplete_todos_summary();
        let first = summary.first().map_or("", String::as_str);
        let _r = SpineState::create_notification(
            &mut self.state,
            NotificationType::Custom,
            "todo_continuation".to_owned(),
            format!("Non-completed todo items ({}). First: {first}", summary.len()),
        );
    }
}
