//! Why an agent row carries the ⚠ — the human-readable reason shipped to the
//! web sidebar (red square) and composer banner alongside the light.

use cp_fleet::AgentExecState;
use cp_mod_agents::types::AgentStatus;
use cp_mod_agents::view_state::FleetExecMirror;

use crate::state::State;

/// The reason `agent` carries the ⚠, or `None` when it is fine.
///
/// This MIRRORS the TUI's `leading_slot` warning EXACTLY (user decision): the
/// TUI shows ⚠ for every non-paused agent whose turn it is and that is not
/// currently streaming — the "LLM owes a reply but isn't producing one" state.
/// So this returns `Some` for that whole set, picking the most specific text it
/// can:
/// - [`Errored`](AgentExecState::Errored) — repeated failures, the agent is stuck;
/// - an abnormal last stop reason (refusal, `max_tokens`, …) when one is known;
/// - otherwise a generic "owes a reply" message (a normal/absent stop reason,
///   e.g. a plain between-turns pause the idle watcher has not yet nudged).
///
/// A `Streaming` agent (stream/tools in flight) and a `THEIR_TURN` agent are
/// fine, so both return `None` — matching the TUI, which shows the spinner or
/// nothing for them.
pub(crate) fn agent_warning(state: &State, agent: &cp_mod_agents::types::Agent) -> Option<String> {
    if agent.paused || agent.archived {
        return None;
    }
    // Turn first: the TUI's ⚠ needs `MyTurn`. `Errored` is sticky in the fleet
    // registry (only a user message clears it), so checking it first kept
    // agents whose turn is with the human red on the web.
    if cp_mod_agents::types::turn::status(state, &agent.id) != AgentStatus::MyTurn {
        return None;
    }
    let exec = FleetExecMirror::get(state).exec_state_of(&agent.id);
    if exec == AgentExecState::Errored {
        return Some(
            "Stuck: the last LLM calls kept failing. Send a message to retry, or switch the model.".to_owned(),
        );
    }
    if exec == AgentExecState::Streaming {
        return None;
    }
    // From here the TUI shows ⚠ (MyTurn, nothing in flight). Pick the most
    // specific reason available; fall back to the generic "owes a reply" text
    // for a normal/absent stop reason so the web square still mirrors the ⚠.
    let generic =
        || "Waiting on the model: it owes a reply but produced none. Send a message, or switch the model.".to_owned();
    let Some(reason) = state.agent_store.get(&agent.id).and_then(|rt| rt.last_stop_reason.as_deref()) else {
        return Some(generic());
    };
    let text = match reason {
        "end_turn" | "tool_use" => generic(),
        "refusal" => {
            "Stopped: the model refused to continue (safety guardrail). Rephrase, or switch the model.".to_owned()
        }
        "max_tokens" => "Stopped: the reply hit the max output tokens. Send a message to continue.".to_owned(),
        other => format!("Stopped abnormally (reason: {other}). Send a message to retry, or switch the model."),
    };
    Some(text)
}
