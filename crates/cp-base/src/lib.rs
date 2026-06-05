//! Foundation crate for Context Pilot: shared types, traits, config, state, and panel/tool abstractions.
//!
//! All module crates depend on `cp-base` for common infrastructure.

/// Safe numeric casting helpers (saturating `as` replacements).
pub mod cast;

/// YAML config loader: prompts, library, themes, injections, constants.
pub mod config;
/// Flame graph telemetry — zero overhead when disabled.
///
/// Set `CP_FLAMEGRAPH=1` to enable. Writes folded-stack samples to
/// `.context-pilot/logs/flame-folded.txt`. Render with:
///
/// ```sh
/// inferno-flamegraph < .context-pilot/logs/flame-folded.txt > flame.svg
/// ```
///
/// ## How it works
///
/// A thread-local span stack tracks the current call path. Each [`flame::Guard`]
/// pushes a span name on creation and pops + emits a sample on drop. Self-time
/// (total minus children) is emitted to avoid double-counting in nested spans.
///
/// ## Usage
///
/// ```ignore
/// let _fg = cp_base::flame!("render");
/// // ... work happens here ...
/// // guard drops at end of scope → sample emitted
/// ```
///
/// For dynamic names (tools, callbacks):
/// ```ignore
/// let _fg = cp_base::flame!(&format!("tool_{}", tool.name));
/// ```
pub mod flame {
    use std::cell::RefCell;
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;

    use crate::cast::Safe as _;

    /// Whether flame telemetry is enabled. Cached after first check.
    static ENABLED: OnceLock<bool> = OnceLock::new();

    /// Buffered writer for the folded-stack output file.
    static WRITER: OnceLock<Mutex<std::io::BufWriter<std::fs::File>>> = OnceLock::new();

    thread_local! {
        /// Per-thread span stack: `(span_name, accumulated_children_µs)`.
        ///
        /// Each entry tracks its own children's total time so the guard can
        /// compute self-time on drop: `self_us = total_us - children_us`.
        static SPAN_STACK: RefCell<Vec<(String, u64)>> = const { RefCell::new(Vec::new()) };
    }

    /// Check if flame graph telemetry is enabled (`CP_FLAMEGRAPH=1`).
    ///
    /// Result is cached after the first call — zero overhead on subsequent checks.
    #[inline]
    pub fn is_enabled() -> bool {
        *ENABLED.get_or_init(|| std::env::var("CP_FLAMEGRAPH").is_ok_and(|v| v == "1" || v == "true"))
    }

    /// Initialize the flame writer. Call once at startup.
    ///
    /// Creates `.context-pilot/logs/flame-folded.txt` and opens a buffered writer.
    /// No-ops silently if telemetry is disabled or the file cannot be created.
    pub fn init() {
        if !is_enabled() {
            return;
        }
        let dir = ".context-pilot/logs";
        let _mkdir = std::fs::create_dir_all(dir);
        let Ok(file) = std::fs::File::create(format!("{dir}/flame-folded.txt")) else {
            return;
        };
        let _writer = WRITER.get_or_init(|| Mutex::new(std::io::BufWriter::new(file)));
    }

    /// Flush the writer buffer to disk. Call on shutdown.
    pub fn flush() {
        if let Some(w) = WRITER.get()
            && let Ok(mut guard) = w.lock()
        {
            let _r = std::io::Write::flush(&mut *guard);
        }
    }

    /// Write a single folded-stack sample: `"span1;span2;span3 duration_µs\n"`.
    fn write_sample(folded: &str, us: u64) {
        if let Some(w) = WRITER.get()
            && let Ok(mut guard) = w.lock()
        {
            use std::io::Write as _;
            let _r = writeln!(guard, "{folded} {us}");
        }
    }

    /// RAII guard for a flame graph span.
    ///
    /// Created by [`Guard::new`] (or the [`flame!`](crate::flame!) macro). Pushes a span
    /// name onto the thread-local stack on creation. On drop, computes self-time
    /// (total minus children), emits a folded-stack sample, and pops the stack.
    ///
    /// Returns `None` from [`new`](Self::new) when telemetry is disabled,
    /// so the guard is zero-cost in production.
    #[derive(Debug)]
    pub struct Guard {
        /// Wall-clock start time for this span.
        start: Instant,
    }

    impl Guard {
        /// Create a new flame span. Returns `None` if telemetry is disabled.
        ///
        /// The `name` is pushed onto the thread-local span stack and will appear
        /// as a frame in the resulting flame graph.
        #[inline]
        #[must_use]
        pub fn new(name: &str) -> Option<Self> {
            if !is_enabled() {
                return None;
            }
            SPAN_STACK.with(|s| s.borrow_mut().push((name.to_string(), 0)));
            Some(Self { start: Instant::now() })
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let total_us = self.start.elapsed().as_micros().to_u64();

            SPAN_STACK.with(|s| {
                let mut stack = s.borrow_mut();

                // Self-time = total minus accumulated children time.
                let children_us = stack.last().map_or(0, |(_, c)| *c);
                let self_us = total_us.saturating_sub(children_us);

                // Build the folded stack path: "parent;child;grandchild".
                let folded: String = stack.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(";");

                // Only emit if there's meaningful self-time (>0 µs).
                if self_us > 0 {
                    write_sample(&folded, self_us);
                }

                // Pop this span from the stack.
                drop(stack.pop());

                // Propagate total time to the parent's children accumulator.
                if let Some((_, parent_children)) = stack.last_mut() {
                    *parent_children = parent_children.saturating_add(total_us);
                }
            });
        }
    }
}

/// Create a flame graph span guard for the current scope.
///
/// Returns `Option<Guard>` — `None` when telemetry is disabled (zero overhead).
/// The span is automatically closed when the guard drops.
///
/// # Examples
///
/// ```ignore
/// let _fg = cp_base::flame!("render");
/// let _fg = cp_base::flame!(&format!("tool_{}", name));
/// ```
#[macro_export]
macro_rules! flame {
    ($name:expr) => {
        $crate::flame::Guard::new($name)
    };
}

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
    #[expect(clippy::panic, reason = "test assertions use panic for tool YAML validation")]
    fn tool_yaml_deserialization() {
        let yamls: Vec<(&str, &str)> = vec![
            ("brave", include_str!("../../../yamls/tools/brave.yaml")),
            ("callback", include_str!("../../../yamls/tools/callback.yaml")),
            ("console", include_str!("../../../yamls/tools/console.yaml")),
            ("core", include_str!("../../../yamls/tools/core.yaml")),
            ("entities", include_str!("../../../yamls/tools/entities.yaml")),
            ("files", include_str!("../../../yamls/tools/files.yaml")),
            ("firecrawl", include_str!("../../../yamls/tools/firecrawl.yaml")),
            ("git", include_str!("../../../yamls/tools/git.yaml")),
            ("github", include_str!("../../../yamls/tools/github.yaml")),
            ("logs", include_str!("../../../yamls/tools/logs.yaml")),
            ("memory", include_str!("../../../yamls/tools/memory.yaml")),
            ("ocr", include_str!("../../../yamls/tools/ocr.yaml")),
            ("prompt", include_str!("../../../yamls/tools/prompt.yaml")),
            ("questions", include_str!("../../../yamls/tools/questions.yaml")),
            ("queue", include_str!("../../../yamls/tools/queue.yaml")),
            ("reverie", include_str!("../../../yamls/tools/reverie.yaml")),
            ("scratchpad", include_str!("../../../yamls/tools/scratchpad.yaml")),
            ("spine", include_str!("../../../yamls/tools/spine.yaml")),
            ("todo", include_str!("../../../yamls/tools/todo.yaml")),
            ("tree", include_str!("../../../yamls/tools/tree.yaml")),
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
    #[expect(clippy::panic, reason = "test assertions use panic for config YAML validation")]
    fn config_yaml_direct_parse() {
        drop(
            serde_yaml::from_str::<Prompts>(include_str!("../../../yamls/prompts.yaml"))
                .unwrap_or_else(|e| panic!("prompts.yaml schema mismatch: {e}")),
        );
        drop(
            serde_yaml::from_str::<Library>(include_str!("../../../yamls/library.yaml"))
                .unwrap_or_else(|e| panic!("library.yaml schema mismatch: {e}")),
        );
        drop(
            serde_yaml::from_str::<Ui>(include_str!("../../../yamls/ui.yaml"))
                .unwrap_or_else(|e| panic!("ui.yaml schema mismatch: {e}")),
        );
        drop(
            serde_yaml::from_str::<Themes>(include_str!("../../../yamls/themes.yaml"))
                .unwrap_or_else(|e| panic!("themes.yaml schema mismatch: {e}")),
        );
        drop(
            serde_yaml::from_str::<Injections>(include_str!("../../../yamls/injections.yaml"))
                .unwrap_or_else(|e| panic!("injections.yaml schema mismatch: {e}")),
        );
        drop(
            serde_yaml::from_str::<Reverie>(include_str!("../../../yamls/reverie.yaml"))
                .unwrap_or_else(|e| panic!("reverie.yaml schema mismatch: {e}")),
        );
    }
}
