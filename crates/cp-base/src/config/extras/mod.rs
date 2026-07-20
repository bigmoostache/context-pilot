//! Config extras grouped to keep the `config/` directory under the 8-entry cap.
//!
//! Holds the standalone behavioral injection types and the compile-time YAML
//! validation test module (both extracted from `config/mod.rs`).

pub mod behavioral;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
