//! Foundation crate for Context Pilot: shared types, traits, config, state, and panel/tool abstractions.
//!
//! All module crates depend on `cp-base` for common infrastructure.

/// Safe numeric casting helpers (saturating `as` replacements).
pub mod cast;
/// YAML config loader: prompts, library, themes, injections, constants.
pub mod config;

/// Module trait: tools, panels, lifecycle hooks for pluggable functionality.
pub mod modules;
/// Panel trait and caching infrastructure for context elements.
pub mod panels;
/// State types: runtime State, `config::Shared`, `WorkerState`, Messages, Actions.
pub mod state;
/// Tool definition types and YAML-driven builder.
pub mod tools;
/// Shared UI helpers: table rendering, text cells, question forms.
pub mod ui;

#[cfg(test)]
mod tests {
    //! Compile-time YAML validation: every embedded YAML file is deserialized
    //! into its typed struct. If a schema drifts from the Rust types, these
    //! tests catch it before the binary is ever produced.

    use super::config::{Injections, Library, Prompts, Reverie, Themes, Ui};
    use super::tools::ToolTexts;

    /// Validate all 6 config YAML files by forcing `LazyLock` initialization.
    #[test]
    fn config_yaml_deserialization() {
        // Each access forces the LazyLock to parse — panics if YAML is malformed.
        let _ = &*super::config::PROMPTS;
        let _ = &*super::config::LIBRARY;
        let _ = &*super::config::UI;
        let _ = &*super::config::THEMES;
        let _ = &*super::config::INJECTIONS;
        let _ = &*super::config::REVERIE;
    }

    /// Validate every tool YAML file parses into `ToolTexts`.
    #[test]
    fn tool_yaml_deserialization() {
        let yamls: Vec<(&str, &str)> = vec![
            ("brave", include_str!("../../../yamls/tools/brave.yaml")),
            ("callback", include_str!("../../../yamls/tools/callback.yaml")),
            ("console", include_str!("../../../yamls/tools/console.yaml")),
            ("core", include_str!("../../../yamls/tools/core.yaml")),
            ("files", include_str!("../../../yamls/tools/files.yaml")),
            ("firecrawl", include_str!("../../../yamls/tools/firecrawl.yaml")),
            ("git", include_str!("../../../yamls/tools/git.yaml")),
            ("github", include_str!("../../../yamls/tools/github.yaml")),
            ("logs", include_str!("../../../yamls/tools/logs.yaml")),
            ("memory", include_str!("../../../yamls/tools/memory.yaml")),
            ("preset", include_str!("../../../yamls/tools/preset.yaml")),
            ("prompt", include_str!("../../../yamls/tools/prompt.yaml")),
            ("questions", include_str!("../../../yamls/tools/questions.yaml")),
            ("queue", include_str!("../../../yamls/tools/queue.yaml")),
            ("reverie", include_str!("../../../yamls/tools/reverie.yaml")),
            ("scratchpad", include_str!("../../../yamls/tools/scratchpad.yaml")),
            ("spine", include_str!("../../../yamls/tools/spine.yaml")),
            ("todo", include_str!("../../../yamls/tools/todo.yaml")),
            ("tree", include_str!("../../../yamls/tools/tree.yaml")),
            ("typst", include_str!("../../../yamls/tools/typst.yaml")),
        ];
        for (name, content) in &yamls {
            // Panics with a clear message if schema doesn't match ToolTexts
            drop(
                serde_yaml::from_str::<ToolTexts>(content)
                    .unwrap_or_else(|e| panic!("yamls/tools/{name}.yaml failed to parse: {e}")),
            );
        }
    }

    /// Validate config YAML files parse into their specific types directly
    /// (not via `LazyLock` — catches type mismatches even if statics change).
    #[test]
    fn config_yaml_direct_parse() {
        drop(
            serde_yaml::from_str::<Prompts>(include_str!("../../../yamls/prompts.yaml"))
                .expect("prompts.yaml schema mismatch"),
        );
        drop(
            serde_yaml::from_str::<Library>(include_str!("../../../yamls/library.yaml"))
                .expect("library.yaml schema mismatch"),
        );
        drop(serde_yaml::from_str::<Ui>(include_str!("../../../yamls/ui.yaml")).expect("ui.yaml schema mismatch"));
        drop(
            serde_yaml::from_str::<Themes>(include_str!("../../../yamls/themes.yaml"))
                .expect("themes.yaml schema mismatch"),
        );
        drop(
            serde_yaml::from_str::<Injections>(include_str!("../../../yamls/injections.yaml"))
                .expect("injections.yaml schema mismatch"),
        );
        drop(
            serde_yaml::from_str::<Reverie>(include_str!("../../../yamls/reverie.yaml"))
                .expect("reverie.yaml schema mismatch"),
        );
    }
}
