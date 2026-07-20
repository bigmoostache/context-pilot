//! Library content accessors loaded from YAML configuration.

use crate::infra::config::LIBRARY;

/// Returns the default agent content from the library configuration.
pub(crate) fn default_agent_content() -> &'static str {
    let id = &LIBRARY.default_agent_id;
    LIBRARY.agents.iter().find(|a| a.id == *id).map_or("", |a| a.content.as_str())
}
