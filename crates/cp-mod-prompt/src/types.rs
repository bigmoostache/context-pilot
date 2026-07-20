use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

/// Discriminator for the three kinds of prompt library entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::exhaustive_enums,
    reason = "prompt-kind contract: PromptType is a closed Agent/Skill/Command set serde-persisted and constructed cross-crate, matched exhaustively by Display/dir_for; #[non_exhaustive] would forbid that construction"
)]
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
        match *self {
            Self::Agent => write!(f, "agent"),
            Self::Skill => write!(f, "skill"),
            Self::Command => write!(f, "command"),
        }
    }
}

/// A prompt library entry (agent, skill, or command).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
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

/// Runtime state for the prompt library.
/// Prompt content is loaded dynamically from disk — this only tracks
/// active selections and loaded panels.
#[derive(Debug)]
#[non_exhaustive]
pub struct PromptState {
    /// Currently active agent ID (None = default).
    pub active_agent_id: Option<String>,
    /// IDs of skills currently loaded as context panels.
    pub loaded_skill_ids: Vec<String>,
}

impl Default for PromptState {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptState {
    /// Create an empty prompt state.
    #[must_use]
    pub const fn new() -> Self {
        Self { active_agent_id: None, loaded_skill_ids: vec![] }
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
