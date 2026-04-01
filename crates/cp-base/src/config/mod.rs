//! YAML configuration loader for prompts, icons, and UI strings.
use std::sync::LazyLock;

use serde::Deserialize;
use std::collections::HashMap;

// ============================================================================
// Prompts Configuration
// ============================================================================

/// Prompt templates used when assembling context panels for LLM calls.
/// Loaded from `yamls/prompts.yaml`.
#[derive(Debug, Deserialize)]
pub struct Prompts {
    /// Templates for panel header/footer/timestamp formatting.
    pub panel: PanelPrompts,
    /// Message injected when context crosses the cleaning threshold.
    #[serde(default)]
    pub context_threshold_notification: String,
}

/// Seed data for the prompt library: built-in agents, skills, and commands.
/// Loaded from `yamls/library.yaml`.
#[derive(Debug, Deserialize)]
pub struct Library {
    /// ID of the agent used when none is explicitly selected.
    pub default_agent_id: String,
    /// Built-in agent definitions (system prompts).
    pub agents: Vec<SeedEntry>,
    /// Built-in skill definitions (loadable context panels).
    #[serde(default)]
    pub skills: Vec<SeedEntry>,
    /// Built-in command definitions (`/command` inline expansions).
    #[serde(default)]
    pub commands: Vec<SeedEntry>,
}

/// A single built-in prompt library entry (agent, skill, or command).
#[derive(Debug, Deserialize, Clone)]
pub struct SeedEntry {
    /// Unique identifier (e.g., `"default"`, `"brave-goggles"`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// One-line summary shown in the library table.
    pub description: String,
    /// Full prompt/content body.
    pub content: String,
}

/// Format strings for rendering context panels in the LLM prompt.
/// Each panel is wrapped with a header, timestamp, and footer.
#[derive(Debug, Deserialize)]
pub struct PanelPrompts {
    /// Panel opening line (contains `{id}`, `{type}`, `{name}` placeholders).
    pub header: String,
    /// Timestamp line appended after header (`{timestamp}` placeholder).
    pub timestamp: String,
    /// Fallback when a panel has no known timestamp.
    pub timestamp_unknown: String,
    /// Panel closing line.
    pub footer: String,
    /// Assistant acknowledgment injected after the footer.
    pub footer_ack: String,
}

// ============================================================================
// Injections Configuration (LLM-facing behavioral text)
// ============================================================================

/// LLM-facing behavioral text injected at runtime — not UI strings.
/// Loaded from `yamls/injections.yaml`.
#[derive(Debug, Deserialize)]
pub struct Injections {
    /// Synthetic messages for the spine auto-continuation engine.
    pub spine: SpineInjections,
    /// Warning banners shown inside callback/prompt editor panels.
    pub editor_warnings: EditorWarnings,
    /// Tool-result messages warning about dedicated tool usage.
    pub console_guardrails: ConsoleGuardrails,
    /// Behavioral redirects (e.g., "use X tool instead").
    pub redirects: RedirectInjections,
    /// Provider-specific injected text (cleaner mode, seed re-injection).
    pub providers: ProviderInjections,
}

/// Synthetic user/assistant messages injected by the spine engine.
#[derive(Debug, Deserialize)]
pub struct SpineInjections {
    /// Injected when auto-continuation fires (tells LLM to keep going).
    pub auto_continuation: String,
    /// Injected when the user types during an active stream.
    pub user_message_during_stream: String,
    /// Injected after a TUI reload completes.
    pub reload_complete: String,
    /// The "continue" synthetic message content.
    #[serde(rename = "continue")]
    pub continue_msg: String,
}

/// Warning banners rendered inside editor panels to prevent the LLM
/// from treating edited content as instructions.
#[derive(Debug, Deserialize)]
pub struct EditorWarnings {
    /// Warnings for the callback script editor.
    pub callback: EditorWarningSet,
    /// Warnings for the prompt (agent/skill/command) editor.
    pub prompt: PromptEditorWarningSet,
}

/// Warning lines for the callback editor panel.
#[derive(Debug, Deserialize)]
pub struct EditorWarningSet {
    /// Top banner identifying this as an editor view.
    pub banner: String,
    /// Reminder not to execute the script content.
    pub no_execute: String,
    /// Hint about how to close the editor.
    pub close_hint: String,
}

/// Warning lines for the prompt library editor panel.
#[derive(Debug, Deserialize)]
pub struct PromptEditorWarningSet {
    /// Top banner identifying this as a prompt editor.
    pub banner: String,
    /// Reminder not to follow the prompt's instructions.
    pub no_follow: String,
    /// Hint about loading the prompt.
    pub load_hint: String,
    /// Hint about closing the editor.
    pub close_hint: String,
}

/// Messages appended to tool results when a console command
/// should have used a dedicated tool (git, gh, typst).
#[derive(Debug, Deserialize)]
pub struct ConsoleGuardrails {
    /// Shown when `git` is run via console instead of `git_execute`.
    pub git: String,
    /// Shown when `gh` is run via console instead of `gh_execute`.
    pub gh: String,
    /// Shown when `typst` is run via console instead of `typst_execute`.
    pub typst: String,
}

/// Behavioral redirects injected to steer the LLM toward correct tools.
#[derive(Debug, Deserialize)]
pub struct RedirectInjections {
    /// Tells the LLM to use `Close_conversation_history` instead of `Close_panel`.
    pub conversation_history_close: String,
}

/// Provider-specific text injected during prompt assembly.
#[derive(Debug, Deserialize)]
pub struct ProviderInjections {
    /// System suffix appended in cleaner/reverie mode.
    pub cleaner_mode: String,
    /// Header for the seed re-injection block.
    pub seed_reinjection_header: String,
    /// Assistant acknowledgment after seed re-injection.
    pub seed_reinjection_ack: String,
    /// System suffix for GPT-OSS compatible providers.
    pub gpt_oss_suffix: String,
}

// ============================================================================
// Reverie Configuration (sub-agent prompts and behavioral text)
// ============================================================================

/// Configuration for reverie sub-agents (background context optimizer, cartographer).
/// Loaded from `yamls/reverie.yaml`.
#[derive(Debug, Deserialize)]
pub struct Reverie {
    /// First user message that kicks off the reverie session.
    pub kickoff_message: String,
    /// Tool restriction header/footer and Report tool instructions.
    pub tool_restrictions: ReverieToolRestrictions,
    /// Nudge appended when the reverie nears its tool cap.
    pub report_nudge: String,
    /// Error messages for reverie-specific failure modes.
    pub errors: ReverieErrors,
}

/// Text blocks injected to constrain which tools a reverie agent can use.
#[derive(Debug, Deserialize)]
pub struct ReverieToolRestrictions {
    /// Prefix before the allowed-tool list.
    pub header: String,
    /// Suffix after the allowed-tool list.
    pub footer: String,
    /// Instructions describing the Report tool's purpose and format.
    pub report_instructions: String,
}

/// Error messages returned when reverie operations fail.
#[derive(Debug, Deserialize)]
pub struct ReverieErrors {
    /// Returned when a reverie tries to call a forbidden tool.
    pub tool_not_available: String,
    /// Returned when Report is called with unflushed queue items.
    pub queue_not_empty: String,
    /// Returned when reverie is disabled in config.
    pub reverie_disabled: String,
    /// Returned when a reverie of the same agent type is already running.
    pub already_running: String,
}

// ============================================================================
// UI Configuration
// ============================================================================

/// UI configuration — display strings, category labels.
/// Loaded from `yamls/ui.yaml`.
#[derive(Debug, Deserialize)]
pub struct Ui {
    /// Display names for tool category groupings in the tools panel.
    pub tool_categories: ToolCategories,
}

/// Human-readable category labels shown in the tools overview panel.
/// Each field is the display string for that tool group.
#[derive(Debug, Deserialize)]
pub struct ToolCategories {
    /// Label for file manipulation tools (Open, Edit, Write).
    pub file: String,
    /// Label for directory tree tools.
    pub tree: String,
    /// Label for console/process tools.
    pub console: String,
    /// Label for context management tools (`Close_panel`, etc.).
    pub context: String,
    /// Label for todo/task tools.
    pub todo: String,
    /// Label for memory tools.
    pub memory: String,
    /// Label for git tools.
    pub git: String,
    /// Label for scratchpad tools.
    pub scratchpad: String,
}

// ============================================================================
// Theme Configuration
// ============================================================================

/// Icons displayed next to messages in the conversation panel.
#[derive(Debug, Deserialize, Clone)]
pub struct MessageIcons {
    /// Icon for user messages.
    pub user: String,
    /// Icon for assistant messages.
    pub assistant: String,
    /// Icon for tool call entries.
    pub tool_call: String,
    /// Icon for tool result entries.
    pub tool_result: String,
    /// Icon for error messages.
    pub error: String,
}

/// Context panel icons — a string-keyed map loaded from theme YAML.
/// Keys match module `icon_ids` (e.g., "tree", "todo", "git").
#[derive(Debug, Deserialize, Clone)]
#[serde(transparent)]
pub struct ContextIcons(pub HashMap<String, String>);

impl ContextIcons {
    /// Look up an icon by key (e.g., "tree", "git").
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
}

/// Icons indicating message lifecycle status (full, summarized, deleted).
#[derive(Debug, Deserialize, Clone)]
pub struct StatusIcons {
    /// Shown for messages included in full.
    pub full: String,
    /// Shown for summarized/compressed messages.
    pub summarized: String,
    /// Shown for deleted/detached messages.
    pub deleted: String,
}

/// Icons for todo item statuses.
#[derive(Debug, Deserialize, Clone)]
pub struct TodoIcons {
    /// Not yet started.
    pub pending: String,
    /// Currently being worked on.
    pub in_progress: String,
    /// Completed.
    pub done: String,
}

/// All available themes, keyed by theme ID.
/// Loaded from `yamls/themes.yaml`.
#[derive(Debug, Deserialize, Clone)]
pub struct Themes {
    /// Map of theme ID → theme definition.
    pub themes: HashMap<String, Theme>,
}

/// A complete visual theme: icons, colors, and metadata.
#[derive(Debug, Deserialize, Clone)]
pub struct Theme {
    /// Human-readable theme name.
    pub name: String,
    /// One-line theme description.
    pub description: String,
    /// Icons for conversation messages.
    pub messages: MessageIcons,
    /// Icons for context panel types.
    pub context: ContextIcons,
    /// Icons for message lifecycle status.
    pub status: StatusIcons,
    /// Icons for todo item statuses.
    pub todo: TodoIcons,
    /// Color palette for this theme.
    pub colors: ThemeColors,
}

/// RGB color as `[r, g, b]` array.
pub type RgbColor = [u8; 3];

/// Color palette for a theme — all values are RGB triples.
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct ThemeColors {
    /// Primary accent (selections, active elements).
    pub accent: RgbColor,
    /// Dimmed accent (inactive highlights).
    pub accent_dim: RgbColor,
    /// Success indicators (passed tests, completed items).
    pub success: RgbColor,
    /// Warning indicators (approaching limits).
    pub warning: RgbColor,
    /// Error indicators (failures, blocked items).
    pub error: RgbColor,
    /// Primary text color.
    pub text: RgbColor,
    /// Secondary text (labels, metadata).
    pub text_secondary: RgbColor,
    /// Muted text (hints, disabled items).
    pub text_muted: RgbColor,
    /// Base background.
    pub bg_base: RgbColor,
    /// Elevated surface (panels, cards).
    pub bg_surface: RgbColor,
    /// Highest elevation (popups, overlays).
    pub bg_elevated: RgbColor,
    /// Primary border color.
    pub border: RgbColor,
    /// Subtle border (dividers).
    pub border_muted: RgbColor,
    /// User message accent.
    pub user: RgbColor,
    /// Assistant message accent.
    pub assistant: RgbColor,
}

/// Default theme ID used when none is configured or the configured one is missing.
pub const DEFAULT_THEME: &str = "dnd";

/// Theme IDs in the order they cycle through when the user presses the theme key.
pub const THEME_ORDER: &[&str] = &["dnd", "modern", "futuristic", "forest", "sea", "space"];

// ============================================================================
// Loading Functions
// ============================================================================

/// Deserialize a YAML string into `T`.
///
/// # Panics
///
/// Panics via [`invariant_panic`] if the YAML content doesn't match the target type.
#[must_use]
pub fn parse_yaml<T: for<'de> Deserialize<'de>>(name: &str, content: &str) -> T {
    serde_yaml::from_str(content).unwrap_or_else(|e| invariant_panic(&format!("Failed to parse {name}: {e}")))
}

/// Panic for compile-time invariant violations (YAML schemas, module state, theme lookups).
///
/// Centralizes `clippy::panic` suppression — all build-time-embedded config
/// invariant panics route through here, as do module-state initialization checks.
/// **Provably unreachable** at runtime: `tests::all_embedded_yaml_parses_successfully`
/// validates every YAML schema on every `cargo test` run.
///
/// # Panics
///
/// Always panics — that is its purpose.
#[expect(
    clippy::panic,
    reason = "invariant violation is unrecoverable — validated by tests::all_embedded_yaml_parses_successfully"
)]
pub fn invariant_panic(msg: &str) -> ! {
    panic!("{msg}")
}

// ============================================================================
// Global Configuration (Lazy Static — embedded at compile time)
// ============================================================================

/// Compile-time constants: API endpoints, token limits, UI layout values, persistence paths.
pub mod constants;
/// Global API key storage at `~/.config/context-pilot/config.json`.
pub mod global;
/// LLM provider/model type definitions and capabilities.
pub mod llm_types;

/// Prompt templates — panel header/footer/timestamp formatting.
pub static PROMPTS: LazyLock<Prompts> =
    LazyLock::new(|| parse_yaml("prompts.yaml", include_str!("../../../../yamls/prompts.yaml")));
/// Seed library — built-in agents, skills, and commands.
pub static LIBRARY: LazyLock<Library> =
    LazyLock::new(|| parse_yaml("library.yaml", include_str!("../../../../yamls/library.yaml")));
/// UI strings — tool category labels.
pub static UI: LazyLock<Ui> = LazyLock::new(|| parse_yaml("ui.yaml", include_str!("../../../../yamls/ui.yaml")));
/// Theme definitions — icons and color palettes.
pub static THEMES: LazyLock<Themes> =
    LazyLock::new(|| parse_yaml("themes.yaml", include_str!("../../../../yamls/themes.yaml")));
/// LLM-facing injections — spine messages, editor warnings, guardrails.
pub static INJECTIONS: LazyLock<Injections> =
    LazyLock::new(|| parse_yaml("injections.yaml", include_str!("../../../../yamls/injections.yaml")));
/// Reverie sub-agent configuration — system prompt, tool restrictions, errors.
pub static REVERIE: LazyLock<Reverie> =
    LazyLock::new(|| parse_yaml("reverie.yaml", include_str!("../../../../yamls/reverie.yaml")));

/// Get a theme by ID, falling back to default, then to any available theme.
///
/// Returns `None` only if the themes map is completely empty (compile-time bug).
#[must_use]
pub fn get_theme(theme_id: &str) -> Option<&'static Theme> {
    THEMES.themes.get(theme_id).or_else(|| THEMES.themes.get(DEFAULT_THEME)).or_else(|| THEMES.themes.values().next())
}

// ============================================================================
// Icon Helper
// ============================================================================

/// Return icon with trailing space for visual separation.
/// All icons are expected to be single-width Unicode symbols; the space
/// ensures consistent 2-cell alignment in the TUI.
#[must_use]
pub fn normalize_icon(icon: &str) -> String {
    format!("{icon} ")
}

// =============================================================================
// Accessor sub-modules (theme colors, chars, icons, library, prompts)
// =============================================================================

/// Thin accessor modules: theme colors, UI chars, icons, library, prompt templates.
pub mod accessors;

// ============================================================================
// Compile-time YAML validation
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
}
