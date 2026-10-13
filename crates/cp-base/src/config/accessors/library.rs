//! Accessors for the seed prompt library (systems, skills, commands).

use crate::config::LIBRARY;

/// Default system ID (used when none is selected).
#[must_use]
pub fn default_system_id() -> &'static str {
    &LIBRARY.default_system_id
}
/// Content body of the default system.
#[must_use]
pub fn default_system_content() -> &'static str {
    let id = &LIBRARY.default_system_id;
    LIBRARY.systems.iter().find(|a| a.id == *id).map_or("", |a| a.content.as_str())
}
/// All built-in system definitions.
#[must_use]
pub fn systems() -> &'static [crate::config::SeedEntry] {
    &LIBRARY.systems
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
