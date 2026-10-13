//! Agent persistence: slim metadata in `config.json` + legacy message files.
//!
//! Agent *metadata* lives in `config.json` ([`AgentMeta`]). Messages belong
//! to the agent's human group (`groups/<id>.json`, see `cp_groups::persist`).
//! The pre-groups `.context-pilot/agents/<id>.json` files are only read once,
//! by the boot migration that seeds each agent's human group
//! ([`read_legacy_messages`]); they are left on disk as a backup.

use std::path::PathBuf;

use cp_base::config::constants::STORE_DIR;
use serde::Serialize;

use super::{Agent, AgentMessage, AgentOrigin};

/// Directory (under the store dir) of the pre-groups agent message files.
pub const AGENTS_DIR: &str = "agents";

/// Path of an agent's pre-groups message file.
#[must_use]
pub fn agent_file(id: &str) -> PathBuf {
    PathBuf::from(STORE_DIR).join(AGENTS_DIR).join(format!("{id}.json"))
}

/// Borrowing view of an [`Agent`]: what `config.json` stores.
#[derive(Debug, Serialize)]
pub struct AgentMeta<'agent> {
    /// See [`Agent::id`].
    id: &'agent str,
    /// See [`Agent::name`].
    name: &'agent str,
    /// See [`Agent::human_group_id`].
    #[serde(skip_serializing_if = "str::is_empty")]
    human_group_id: &'agent str,
    /// See [`Agent::created_at`].
    created_at: u64,
    /// See [`Agent::archived`].
    archived: bool,
    /// See [`Agent::paused`].
    paused: bool,
    /// See [`Agent::origin`].
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<&'agent AgentOrigin>,
    /// See [`Agent::parent_agent_id`].
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_agent_id: Option<&'agent str>,
    /// See [`Agent::description`].
    #[serde(skip_serializing_if = "str::is_empty")]
    description: &'agent str,
}

impl<'agent> From<&'agent Agent> for AgentMeta<'agent> {
    fn from(t: &'agent Agent) -> Self {
        Self {
            id: &t.id,
            name: &t.name,

            human_group_id: &t.human_group_id,
            created_at: t.created_at,
            archived: t.archived,
            paused: t.paused,
            origin: t.origin.as_ref(),
            parent_agent_id: t.parent_agent_id.as_deref(),
            description: &t.description,
        }
    }
}

/// Messages of an agent's pre-groups file (empty when absent or unreadable).
#[must_use]
pub fn read_legacy_messages(id: &str) -> Vec<AgentMessage> {
    std::fs::read(agent_file(id))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|v| {
            let m = v.get("messages")?;
            serde_json::from_value(m.clone()).ok()
        })
        .unwrap_or_default()
}
