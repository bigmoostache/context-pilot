//! Compile-time YAML validation tests (extracted from `mod.rs` for the 500-line cap).
//!
//! Loaded via `#[path = "tests.rs"] mod tests;` in `extras/mod.rs`, so `super`
//! is the `extras` module and `super::super` is the `config` root (where the
//! `PROMPTS`/`THEMES`/… statics live). No inner `mod tests { .. }` wrapper —
//! that would double-nest and break the imports.

use super::super::*;

/// Force-initialize every `LazyLock` static to validate that all
/// compile-time-embedded YAML files deserialize without error.
///
/// This makes `invariant_panic` provably unreachable at runtime:
/// if a schema mismatch exists, this test catches it before deployment.
#[test]
fn all_embedded_yaml_parses_successfully() {
    // Each dereference forces LazyLock init — schema errors surface here.
    let _prompts = &*PROMPTS;
    let _library = &*LIBRARY;
    let _ui = &*UI;
    let _themes = &*THEMES;
    let _injections = &*INJECTIONS;
    let _reverie = &*REVERIE;
}

/// Verify the default theme exists in the themes map.
#[test]
fn default_theme_exists() {
    assert!(THEMES.themes.contains_key(DEFAULT_THEME), "default theme '{DEFAULT_THEME}' missing from themes.yaml");
}

/// Verify all theme IDs in `THEME_ORDER` exist in the loaded themes.
#[test]
fn all_theme_order_ids_exist() {
    for id in THEME_ORDER {
        assert!(THEMES.themes.contains_key(*id), "theme order ID '{id}' missing from themes.yaml");
    }
}
