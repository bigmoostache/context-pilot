//! Library content accessors loaded from YAML configuration.

use crate::infra::config::LIBRARY;

/// Returns the default system content from the library configuration.
pub(crate) fn default_system_content() -> &'static str {
    let id = &LIBRARY.default_system_id;
    LIBRARY.systems.iter().find(|a| a.id == *id).map_or("", |a| a.content.as_str())
}
