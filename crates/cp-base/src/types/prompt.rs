use serde::{Deserialize, Serialize};

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
