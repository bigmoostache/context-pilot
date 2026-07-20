//! Accessors for the seed prompt library (agents, skills, commands).

use crate::config::LIBRARY;

/// Default agent ID (used when none is selected).
#[must_use]
pub fn default_agent_id() -> &'static str {
    &LIBRARY.default_agent_id
}
/// Content body of the default agent.
#[must_use]
pub fn default_agent_content() -> &'static str {
    let id = &LIBRARY.default_agent_id;
    LIBRARY.agents.iter().find(|a| a.id == *id).map_or("", |a| a.content.as_str())
}
/// All built-in agent definitions.
#[must_use]
pub fn agents() -> &'static [crate::config::SeedEntry] {
    &LIBRARY.agents
}
/// All built-in skill definitions.
#[must_use]
pub fn skills() -> &'static [crate::config::SeedEntry] {
    &LIBRARY.skills
}
/// All built-in command definitions.
#[must_use]
pub fn commands() -> &'static [crate::config::SeedEntry] {
    &LIBRARY.commands
}
