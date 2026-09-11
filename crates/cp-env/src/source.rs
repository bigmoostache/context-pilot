//! Where raw values come from.
//!
//! The resolver never touches `std::env` directly: it reads through a
//! [`Source`], so the process environment and an in-memory map are
//! interchangeable. That is what makes every validation rule testable without
//! mutating the environment (`unsafe_code` is forbidden workspace-wide, and
//! `std::env::set_var` is unsafe on edition 2024).

use std::collections::BTreeMap;

/// A read-only view of an environment.
pub trait Source {
    /// The raw value of `name`, if set. A value that is not valid UTF-8 comes
    /// back lossily converted so validation can still name it in its report.
    fn get(&self, name: &str) -> Option<String>;

    /// Every variable name present, in no particular order.
    fn names(&self) -> Vec<String>;
}

/// The real process environment. This is the only place in the workspace
/// that reads `std::env::var`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnv;

impl Source for ProcessEnv {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
    }

    fn names(&self) -> Vec<String> {
        std::env::vars_os().map(|(key, _value)| key.to_string_lossy().into_owned()).collect()
    }
}

impl Source for BTreeMap<String, String> {
    fn get(&self, name: &str) -> Option<String> {
        Self::get(self, name).cloned()
    }

    fn names(&self) -> Vec<String> {
        self.keys().cloned().collect()
    }
}
