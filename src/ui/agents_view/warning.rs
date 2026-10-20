//! Why an agent row carries the ⚠ — the human-readable reason shipped to the
//! web sidebar (red square) and composer banner alongside the light.

use cp_fleet::AgentExecState;
use cp_mod_agents::types::AgentStatus;
use cp_mod_agents::view_state::FleetExecMirror;

use crate::state::State;

/// The reason `agent` is stalled owing a turn, or `None` when it is fine.
///
/// Only *concrete* stalls are reported: an agent parked as
/// [`Errored`](AgentExecState::Errored), or a `MyTurn` agent with nothing in
/// flight whose last LLM turn ended abnormally (refusal, `max_tokens`, content
/// filter…). A plain idle `MyTurn` agent after a normal stop is transient (the
/// idle watcher nudges it) and a never-run agent has no stop reason yet, so
/// neither is flagged — that keeps the web square from flashing red every turn.
pub(crate) fn agent_warning(state: &State, agent: &cp_mod_agents::types::Agent) -> Option<String> {
    if agent.paused || agent.archived {
        return None;
    }
    let exec = FleetExecMirror::get(state).exec_state_of(&agent.id);
    if exec == AgentExecState::Errored {
        return Some(
            "Stuck: the last LLM calls kept failing. Send a message to retry, or switch the model.".to_owned(),
        );
    }
    if exec == AgentExecState::Streaming || cp_mod_agents::types::turn::status(state, &agent.id) != AgentStatus::MyTurn
    {
        return None;
    }
    let reason = state.agent_store.get(&agent.id)?.last_stop_reason.as_deref()?;
    let text = match reason {
        "end_turn" | "tool_use" => return None,
        "refusal" => {
            "Stopped: the model refused to continue (safety guardrail). Rephrase, or switch the model.".to_owned()
        }
        "max_tokens" => "Stopped: the reply hit the max output tokens. Send a message to continue.".to_owned(),
        other => format!("Stopped abnormally (reason: {other}). Send a message to retry, or switch the model."),
    };
    Some(text)
}
