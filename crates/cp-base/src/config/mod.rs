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
pub struct PromptsConfig {
    /// Templates for panel header/footer/timestamp formatting.
    pub panel: PanelPrompts,
    /// Message injected when context crosses the cleaning threshold.
    #[serde(default)]
    pub context_threshold_notification: String,
}

/// Seed data for the prompt library: built-in agents, skills, and commands.
/// Loaded from `yamls/library.yaml`.
#[derive(Debug, Deserialize)]
pub struct LibraryConfig {
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
    /// Format for a single message line inside the footer.
    pub footer_msg_line: String,
    /// Header introducing the recent-messages section in footer.
    pub footer_msg_header: String,
    /// Assistant acknowledgment injected after the footer.
    pub footer_ack: String,
}

// ============================================================================
// Injections Configuration (LLM-facing behavioral text)
// ============================================================================

/// LLM-facing behavioral text injected at runtime — not UI strings.
/// Loaded from `yamls/injections.yaml`.
#[derive(Debug, Deserialize)]
pub struct InjectionsConfig {
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
pub struct ReverieConfig {
    /// System prompt given to reverie sub-agents.
    pub system_prompt: String,
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
pub struct UiConfig {
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
    /// Label for context management tools (Close_panel, etc.).
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
/// Keys match module icon_ids (e.g., "tree", "todo", "git").
#[derive(Debug, Deserialize, Clone)]
#[serde(transparent)]
pub struct ContextIcons(pub HashMap<String, String>);

impl ContextIcons {
    /// Look up an icon by key (e.g., "tree", "git").
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
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
pub struct ThemesConfig {
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

/// Deserialize a YAML string into `T`, panicking with a descriptive message on failure.
fn parse_yaml<T: for<'de> Deserialize<'de>>(name: &str, content: &str) -> T {
    serde_yaml::from_str(content).unwrap_or_else(|e| panic!("Failed to parse {}: {}", name, e))
}

// ============================================================================
// Global Configuration (Lazy Static — embedded at compile time)
// ============================================================================

/// Compile-time constants: API endpoints, token limits, UI layout values, persistence paths.
pub mod constants;
/// LLM provider/model type definitions and capabilities.
pub mod llm_types;

/// Prompt templates — panel header/footer/timestamp formatting.
pub static PROMPTS: LazyLock<PromptsConfig> =
    LazyLock::new(|| parse_yaml("prompts.yaml", include_str!("../../../../yamls/prompts.yaml")));
/// Seed library — built-in agents, skills, and commands.
pub static LIBRARY: LazyLock<LibraryConfig> =
    LazyLock::new(|| parse_yaml("library.yaml", include_str!("../../../../yamls/library.yaml")));
/// UI strings — tool category labels.
pub static UI: LazyLock<UiConfig> = LazyLock::new(|| parse_yaml("ui.yaml", include_str!("../../../../yamls/ui.yaml")));
/// Theme definitions — icons and color palettes.
pub static THEMES: LazyLock<ThemesConfig> =
    LazyLock::new(|| parse_yaml("themes.yaml", include_str!("../../../../yamls/themes.yaml")));
/// LLM-facing injections — spine messages, editor warnings, guardrails.
pub static INJECTIONS: LazyLock<InjectionsConfig> =
    LazyLock::new(|| parse_yaml("injections.yaml", include_str!("../../../../yamls/injections.yaml")));
/// Reverie sub-agent configuration — system prompt, tool restrictions, errors.
pub static REVERIE: LazyLock<ReverieConfig> =
    LazyLock::new(|| parse_yaml("reverie.yaml", include_str!("../../../../yamls/reverie.yaml")));

/// Get a theme by ID, falling back to default if not found
pub fn get_theme(theme_id: &str) -> &'static Theme {
    THEMES.themes.get(theme_id).or_else(|| THEMES.themes.get(DEFAULT_THEME)).expect("Default theme must exist")
}

// ============================================================================
// Active Theme (Global State — cached atomic pointer for zero-cost access)
// ============================================================================

use std::sync::atomic::{AtomicPtr, Ordering};

/// Cached pointer to the active theme. Updated by set_active_theme().
/// Points into the static THEMES LazyLock, so the reference is always valid.
static CACHED_THEME: AtomicPtr<Theme> = AtomicPtr::new(std::ptr::null_mut());

/// Set the active theme ID (call when state is loaded or theme changes)
pub fn set_active_theme(theme_id: &str) {
    let theme: &'static Theme = get_theme(theme_id);
    CACHED_THEME.store(std::ptr::from_ref(theme).cast_mut(), Ordering::Release);
}

/// Get the currently active theme (single atomic load — no locking, no allocation)
#[expect(unsafe_code, reason = "atomic pointer deref from static LazyLock — always valid")]
pub fn active_theme() -> &'static Theme {
    let ptr = CACHED_THEME.load(Ordering::Acquire);
    if !ptr.is_null() {
        // SAFETY: ptr was set from a &'static Theme reference stored in LazyLock THEMES.
        // The Theme data is never mutated or freed after initialization.
        unsafe { &*ptr }
    } else {
        // First call before set_active_theme — initialize from default
        let theme = get_theme(DEFAULT_THEME);
        CACHED_THEME.store(std::ptr::from_ref(theme).cast_mut(), Ordering::Release);
        theme
    }
}

// ============================================================================
// Icon Helper
// ============================================================================

/// Return icon with trailing space for visual separation.
/// All icons are expected to be single-width Unicode symbols; the space
/// ensures consistent 2-cell alignment in the TUI.
pub fn normalize_icon(icon: &str) -> String {
    format!("{} ", icon)
}

// =============================================================================
// THEME COLORS (loaded from active theme in yamls/themes.yaml)
// =============================================================================

/// Theme color accessors — each returns a `ratatui::style::Color::Rgb`
/// from the currently active theme. Zero-cost after first call (atomic pointer).
pub mod theme {
    use crate::config::active_theme;
    use ratatui::style::Color;

    /// Convert an `[r, g, b]` triple to a ratatui RGB color.
    fn rgb(c: [u8; 3]) -> Color {
        Color::Rgb(c[0], c[1], c[2])
    }

    /// Primary accent color.
    pub fn accent() -> Color {
        rgb(active_theme().colors.accent)
    }
    /// Dimmed accent for inactive highlights.
    pub fn accent_dim() -> Color {
        rgb(active_theme().colors.accent_dim)
    }
    /// Success indicator color.
    pub fn success() -> Color {
        rgb(active_theme().colors.success)
    }
    /// Warning indicator color.
    pub fn warning() -> Color {
        rgb(active_theme().colors.warning)
    }
    /// Error indicator color.
    pub fn error() -> Color {
        rgb(active_theme().colors.error)
    }
    /// Primary text color.
    pub fn text() -> Color {
        rgb(active_theme().colors.text)
    }
    /// Secondary text color (labels, metadata).
    pub fn text_secondary() -> Color {
        rgb(active_theme().colors.text_secondary)
    }
    /// Muted text color (hints, disabled).
    pub fn text_muted() -> Color {
        rgb(active_theme().colors.text_muted)
    }
    /// Base background color.
    pub fn bg_base() -> Color {
        rgb(active_theme().colors.bg_base)
    }
    /// Elevated surface background (panels).
    pub fn bg_surface() -> Color {
        rgb(active_theme().colors.bg_surface)
    }
    /// Highest-elevation background (popups, overlays).
    pub fn bg_elevated() -> Color {
        rgb(active_theme().colors.bg_elevated)
    }
    /// Primary border color.
    pub fn border() -> Color {
        rgb(active_theme().colors.border)
    }
    /// Subtle border color (dividers).
    pub fn border_muted() -> Color {
        rgb(active_theme().colors.border_muted)
    }
    /// User message accent color.
    pub fn user() -> Color {
        rgb(active_theme().colors.user)
    }
    /// Assistant message accent color.
    pub fn assistant() -> Color {
        rgb(active_theme().colors.assistant)
    }
}

// =============================================================================
// UI CHARACTERS
// =============================================================================

/// Unicode box-drawing and indicator characters for TUI rendering.
pub mod chars {
    /// Horizontal line segment (─).
    pub const HORIZONTAL: &str = "─";
    /// Full-width block (█).
    pub const BLOCK_FULL: &str = "█";
    /// Light shade block (░).
    pub const BLOCK_LIGHT: &str = "░";
    /// Filled circle (●).
    pub const DOT: &str = "●";
    /// Right-pointing triangle (▸).
    pub const ARROW_RIGHT: &str = "▸";
    /// Up arrow (↑).
    pub const ARROW_UP: &str = "↑";
    /// Down arrow (↓).
    pub const ARROW_DOWN: &str = "↓";
    /// Cross mark (✗).
    pub const CROSS: &str = "✗";
}

// =============================================================================
// ICONS / EMOJIS (loaded from active theme in yamls/themes.yaml)
// All icons are normalized to 2 display cells width for consistent alignment
// =============================================================================

pub mod icons {
    //! Message and context icons from the active theme.
    use crate::config::{active_theme, normalize_icon};

    /// User message icon (e.g., "⚔ ").
    pub fn msg_user() -> String {
        normalize_icon(&active_theme().messages.user)
    }
    /// Assistant message icon (e.g., "🐉 ").
    pub fn msg_assistant() -> String {
        normalize_icon(&active_theme().messages.assistant)
    }
    /// Tool-call message icon.
    pub fn msg_tool_call() -> String {
        normalize_icon(&active_theme().messages.tool_call)
    }
    /// Tool-result message icon.
    pub fn msg_tool_result() -> String {
        normalize_icon(&active_theme().messages.tool_result)
    }
    /// Error message icon.
    pub fn msg_error() -> String {
        normalize_icon(&active_theme().messages.error)
    }
    /// Status icon for messages included in full.
    pub fn status_full() -> String {
        normalize_icon(&active_theme().status.full)
    }
    /// Status icon for deleted/detached messages.
    pub fn status_deleted() -> String {
        normalize_icon(&active_theme().status.deleted)
    }
    /// Todo icon for pending items.
    pub fn todo_pending() -> String {
        normalize_icon(&active_theme().todo.pending)
    }
    /// Todo icon for in-progress items.
    pub fn todo_in_progress() -> String {
        normalize_icon(&active_theme().todo.in_progress)
    }
    /// Todo icon for completed items.
    pub fn todo_done() -> String {
        normalize_icon(&active_theme().todo.done)
    }
}

// =============================================================================
// PROMPTS (loaded from yamls/prompts.yaml via config module)
// =============================================================================

pub mod library {
    //! Accessors for the seed prompt library (agents, skills, commands).
    use crate::config::LIBRARY;

    /// Default agent ID (used when none is selected).
    pub fn default_agent_id() -> &'static str {
        &LIBRARY.default_agent_id
    }
    /// Content body of the default agent.
    pub fn default_agent_content() -> &'static str {
        let id = &LIBRARY.default_agent_id;
        LIBRARY.agents.iter().find(|a| a.id == *id).map(|a| a.content.as_str()).unwrap_or("")
    }
    /// All built-in agent definitions.
    pub fn agents() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.agents
    }
    /// All built-in skill definitions.
    pub fn skills() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.skills
    }
    /// All built-in command definitions.
    pub fn commands() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.commands
    }
}

pub mod prompts {
    //! Accessors for prompt templates (panel header/footer formatting).
    use crate::config::PROMPTS;

    /// Panel opening header template (`{id}`, `{type}`, `{name}` placeholders).
    pub fn panel_header() -> &'static str {
        &PROMPTS.panel.header
    }
    /// Panel timestamp template (`{timestamp}` placeholder).
    pub fn panel_timestamp() -> &'static str {
        &PROMPTS.panel.timestamp
    }
    /// Fallback when panel has no known timestamp.
    pub fn panel_timestamp_unknown() -> &'static str {
        &PROMPTS.panel.timestamp_unknown
    }
    /// Panel closing footer template.
    pub fn panel_footer() -> &'static str {
        &PROMPTS.panel.footer
    }
    /// Format for a message line inside footer.
    pub fn panel_footer_msg_line() -> &'static str {
        &PROMPTS.panel.footer_msg_line
    }
    /// Header for recent-messages section in footer.
    pub fn panel_footer_msg_header() -> &'static str {
        &PROMPTS.panel.footer_msg_header
    }
    /// Assistant ack injected after footer.
    pub fn panel_footer_ack() -> &'static str {
        &PROMPTS.panel.footer_ack
    }
}
