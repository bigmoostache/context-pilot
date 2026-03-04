use serde::{Deserialize, Serialize};

use cp_base::state::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptType {
    Agent,
    Skill,
    Command,
}

impl std::fmt::Display for PromptType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromptType::Agent => write!(f, "agent"),
            PromptType::Skill => write!(f, "skill"),
            PromptType::Command => write!(f, "command"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub prompt_type: PromptType,
    pub is_builtin: bool,
}

#[derive(Debug)]
pub struct PromptState {
    pub agents: Vec<PromptItem>,
    pub active_agent_id: Option<String>,
    pub skills: Vec<PromptItem>,
    pub loaded_skill_ids: Vec<String>,
    pub commands: Vec<PromptItem>,
    /// ID of the prompt currently open in the Library editor (for editing).
    /// Max one at a time. Edit_prompt requires this to be set.
    pub open_prompt_id: Option<String>,
}

impl Default for PromptState {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptState {
    pub fn new() -> Self {
        Self {
            agents: vec![],
            active_agent_id: None,
            skills: vec![],
            loaded_skill_ids: vec![],
            commands: vec![],
            open_prompt_id: None,
        }
    }
    pub fn get(state: &State) -> &Self {
        state.get_ext::<Self>().expect("PromptState not initialized")
    }
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.get_ext_mut::<Self>().expect("PromptState not initialized")
    }
}
