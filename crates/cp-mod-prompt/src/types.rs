use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

/// Discriminator for the three kinds of prompt library entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptType {
    /// System prompt defining the AI's identity and behavior.
    Agent,
    /// Knowledge/instruction block loaded as a context panel.
    Skill,
    /// Inline replacement triggered by `/command-name` in the input field.
    Command,
}

impl std::fmt::Display for PromptType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Agent => write!(f, "agent"),
            Self::Skill => write!(f, "skill"),
            Self::Command => write!(f, "command"),
        }
    }
}

/// A prompt library entry (agent, skill, or command).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptItem {
    /// Unique identifier (e.g., "pirate-coder", "brave-goggles").
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Short description shown in the library table.
    pub description: String,
    /// Full content body (system prompt, skill instructions, or command expansion).
    pub content: String,
    /// Which kind of prompt this is.
    pub prompt_type: PromptType,
    /// Whether this is a built-in (non-deletable) entry.
    pub is_builtin: bool,
}

/// Runtime state for the prompt library (agents, skills, commands).
#[derive(Debug)]
pub struct PromptState {
    /// All known agents (built-in + user-created).
    pub agents: Vec<PromptItem>,
    /// Currently active agent ID (None = default).
    pub active_agent_id: Option<String>,
    /// All known skills (built-in + user-created).
    pub skills: Vec<PromptItem>,
    /// IDs of skills currently loaded as context panels.
    pub loaded_skill_ids: Vec<String>,
    /// All known commands (built-in + user-created).
    pub commands: Vec<PromptItem>,
    /// ID of the prompt currently open in the Library editor (for editing).
    /// Max one at a time. `Edit_prompt` requires this to be set.
    pub open_prompt_id: Option<String>,
}

impl Default for PromptState {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptState {
    /// Create an empty prompt state with no entries loaded.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            agents: vec![],
            active_agent_id: None,
            skills: vec![],
            loaded_skill_ids: vec![],
            commands: vec![],
            open_prompt_id: None,
        }
    }
    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }
    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }
}
