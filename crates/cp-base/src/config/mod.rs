//! YAML configuration loader for prompts, icons, and UI strings.
use std::sync::LazyLock;

use serde::Deserialize;
use std::collections::HashMap;

// ============================================================================
// Prompts Configuration
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct PromptsConfig {
    pub panel: PanelPrompts,
    #[serde(default)]
    pub context_threshold_notification: String,
}

#[derive(Debug, Deserialize)]
pub struct LibraryConfig {
    pub default_agent_id: String,
    pub agents: Vec<SeedEntry>,
    #[serde(default)]
    pub skills: Vec<SeedEntry>,
    #[serde(default)]
    pub commands: Vec<SeedEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeedEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct PanelPrompts {
    pub header: String,
    pub timestamp: String,
    pub timestamp_unknown: String,
    pub footer: String,
    pub footer_msg_line: String,
    pub footer_msg_header: String,
    pub footer_ack: String,
}

// ============================================================================
// Injections Configuration (LLM-facing behavioral text)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct InjectionsConfig {
    pub spine: SpineInjections,
    pub editor_warnings: EditorWarnings,
    pub console_guardrails: ConsoleGuardrails,
    pub redirects: RedirectInjections,
    pub providers: ProviderInjections,
}

#[derive(Debug, Deserialize)]
pub struct SpineInjections {
    pub auto_continuation: String,
    pub user_message_during_stream: String,
    pub reload_complete: String,
    #[serde(rename = "continue")]
    pub continue_msg: String,
}

#[derive(Debug, Deserialize)]
pub struct EditorWarnings {
    pub callback: EditorWarningSet,
    pub prompt: PromptEditorWarningSet,
}

#[derive(Debug, Deserialize)]
pub struct EditorWarningSet {
    pub banner: String,
    pub no_execute: String,
    pub close_hint: String,
}

#[derive(Debug, Deserialize)]
pub struct PromptEditorWarningSet {
    pub banner: String,
    pub no_follow: String,
    pub load_hint: String,
    pub close_hint: String,
}

#[derive(Debug, Deserialize)]
pub struct ConsoleGuardrails {
    pub git: String,
    pub gh: String,
    pub typst: String,
}

#[derive(Debug, Deserialize)]
pub struct RedirectInjections {
    pub conversation_history_close: String,
}

#[derive(Debug, Deserialize)]
pub struct ProviderInjections {
    pub cleaner_mode: String,
    pub seed_reinjection_header: String,
    pub seed_reinjection_ack: String,
    pub gpt_oss_suffix: String,
}

// ============================================================================
// Reverie Configuration (sub-agent prompts and behavioral text)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ReverieConfig {
    pub system_prompt: String,
    pub kickoff_message: String,
    pub tool_restrictions: ReverieToolRestrictions,
    pub report_nudge: String,
    pub errors: ReverieErrors,
}

#[derive(Debug, Deserialize)]
pub struct ReverieToolRestrictions {
    pub header: String,
    pub footer: String,
    pub report_instructions: String,
}

#[derive(Debug, Deserialize)]
pub struct ReverieErrors {
    pub tool_not_available: String,
    pub queue_not_empty: String,
    pub reverie_disabled: String,
    pub already_running: String,
}

// ============================================================================
// UI Configuration
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct UiConfig {
    pub tool_categories: ToolCategories,
}

#[derive(Debug, Deserialize)]
pub struct ToolCategories {
    pub file: String,
    pub tree: String,
    pub console: String,
    pub context: String,
    pub todo: String,
    pub memory: String,
    pub git: String,
    pub scratchpad: String,
}

// ============================================================================
// Theme Configuration
// ============================================================================

#[derive(Debug, Deserialize, Clone)]
pub struct MessageIcons {
    pub user: String,
    pub assistant: String,
    pub tool_call: String,
    pub tool_result: String,
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

#[derive(Debug, Deserialize, Clone)]
pub struct StatusIcons {
    pub full: String,
    pub summarized: String,
    pub deleted: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TodoIcons {
    pub pending: String,
    pub in_progress: String,
    pub done: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ThemesConfig {
    pub themes: HashMap<String, Theme>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Theme {
    pub name: String,
    pub description: String,
    pub messages: MessageIcons,
    pub context: ContextIcons,
    pub status: StatusIcons,
    pub todo: TodoIcons,
    pub colors: ThemeColors,
}

/// RGB color as [r, g, b] array
pub type RgbColor = [u8; 3];

#[derive(Debug, Deserialize, Clone)]
pub struct ThemeColors {
    pub accent: RgbColor,
    pub accent_dim: RgbColor,
    pub success: RgbColor,
    pub warning: RgbColor,
    pub error: RgbColor,
    pub text: RgbColor,
    pub text_secondary: RgbColor,
    pub text_muted: RgbColor,
    pub bg_base: RgbColor,
    pub bg_surface: RgbColor,
    pub bg_elevated: RgbColor,
    pub border: RgbColor,
    pub border_muted: RgbColor,
    pub user: RgbColor,
    pub assistant: RgbColor,
}

/// Default theme ID
pub const DEFAULT_THEME: &str = "dnd";

/// Available theme IDs in display order
pub const THEME_ORDER: &[&str] = &["dnd", "modern", "futuristic", "forest", "sea", "space"];

// ============================================================================
// Loading Functions
// ============================================================================

fn parse_yaml<T: for<'de> Deserialize<'de>>(name: &str, content: &str) -> T {
    serde_yaml::from_str(content).unwrap_or_else(|e| panic!("Failed to parse {}: {}", name, e))
}

// ============================================================================
// Global Configuration (Lazy Static — embedded at compile time)
// ============================================================================

pub mod constants;

pub static PROMPTS: LazyLock<PromptsConfig> =
    LazyLock::new(|| parse_yaml("prompts.yaml", include_str!("../../../../yamls/prompts.yaml")));
pub static LIBRARY: LazyLock<LibraryConfig> =
    LazyLock::new(|| parse_yaml("library.yaml", include_str!("../../../../yamls/library.yaml")));
pub static UI: LazyLock<UiConfig> = LazyLock::new(|| parse_yaml("ui.yaml", include_str!("../../../../yamls/ui.yaml")));
pub static THEMES: LazyLock<ThemesConfig> =
    LazyLock::new(|| parse_yaml("themes.yaml", include_str!("../../../../yamls/themes.yaml")));
pub static INJECTIONS: LazyLock<InjectionsConfig> =
    LazyLock::new(|| parse_yaml("injections.yaml", include_str!("../../../../yamls/injections.yaml")));
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
    CACHED_THEME.store(theme as *const Theme as *mut Theme, Ordering::Release);
}

/// Get the currently active theme (single atomic load — no locking, no allocation)
pub fn active_theme() -> &'static Theme {
    let ptr = CACHED_THEME.load(Ordering::Acquire);
    if !ptr.is_null() {
        // SAFETY: ptr was set from a &'static Theme reference stored in LazyLock THEMES.
        // The Theme data is never mutated or freed after initialization.
        unsafe { &*ptr }
    } else {
        // First call before set_active_theme — initialize from default
        let theme = get_theme(DEFAULT_THEME);
        CACHED_THEME.store(theme as *const Theme as *mut Theme, Ordering::Release);
        theme
    }
}

// ============================================================================
// Icon Helper
// ============================================================================

/// Return icon with trailing space for visual separation.
/// All icons should be single-width Unicode symbols.
pub fn normalize_icon(icon: &str) -> String {
    format!("{} ", icon)
}

// =============================================================================
// THEME COLORS (loaded from active theme in yamls/themes.yaml)
// =============================================================================

pub mod theme {
    use crate::config::active_theme;
    use ratatui::style::Color;

    fn rgb(c: [u8; 3]) -> Color {
        Color::Rgb(c[0], c[1], c[2])
    }

    pub fn accent() -> Color {
        rgb(active_theme().colors.accent)
    }
    pub fn accent_dim() -> Color {
        rgb(active_theme().colors.accent_dim)
    }
    pub fn success() -> Color {
        rgb(active_theme().colors.success)
    }
    pub fn warning() -> Color {
        rgb(active_theme().colors.warning)
    }
    pub fn error() -> Color {
        rgb(active_theme().colors.error)
    }
    pub fn text() -> Color {
        rgb(active_theme().colors.text)
    }
    pub fn text_secondary() -> Color {
        rgb(active_theme().colors.text_secondary)
    }
    pub fn text_muted() -> Color {
        rgb(active_theme().colors.text_muted)
    }
    pub fn bg_base() -> Color {
        rgb(active_theme().colors.bg_base)
    }
    pub fn bg_surface() -> Color {
        rgb(active_theme().colors.bg_surface)
    }
    pub fn bg_elevated() -> Color {
        rgb(active_theme().colors.bg_elevated)
    }
    pub fn border() -> Color {
        rgb(active_theme().colors.border)
    }
    pub fn border_muted() -> Color {
        rgb(active_theme().colors.border_muted)
    }
    pub fn user() -> Color {
        rgb(active_theme().colors.user)
    }
    pub fn assistant() -> Color {
        rgb(active_theme().colors.assistant)
    }
}

// =============================================================================
// UI CHARACTERS
// =============================================================================

pub mod chars {
    pub const HORIZONTAL: &str = "─";
    pub const BLOCK_FULL: &str = "█";
    pub const BLOCK_LIGHT: &str = "░";
    pub const DOT: &str = "●";
    pub const ARROW_RIGHT: &str = "▸";
    pub const ARROW_UP: &str = "↑";
    pub const ARROW_DOWN: &str = "↓";
    pub const CROSS: &str = "✗";
}

// =============================================================================
// ICONS / EMOJIS (loaded from active theme in yamls/themes.yaml)
// All icons are normalized to 2 display cells width for consistent alignment
// =============================================================================

pub mod icons {
    use crate::config::{active_theme, normalize_icon};

    pub fn msg_user() -> String {
        normalize_icon(&active_theme().messages.user)
    }
    pub fn msg_assistant() -> String {
        normalize_icon(&active_theme().messages.assistant)
    }
    pub fn msg_tool_call() -> String {
        normalize_icon(&active_theme().messages.tool_call)
    }
    pub fn msg_tool_result() -> String {
        normalize_icon(&active_theme().messages.tool_result)
    }
    pub fn msg_error() -> String {
        normalize_icon(&active_theme().messages.error)
    }
    pub fn status_full() -> String {
        normalize_icon(&active_theme().status.full)
    }
    pub fn status_deleted() -> String {
        normalize_icon(&active_theme().status.deleted)
    }
    pub fn todo_pending() -> String {
        normalize_icon(&active_theme().todo.pending)
    }
    pub fn todo_in_progress() -> String {
        normalize_icon(&active_theme().todo.in_progress)
    }
    pub fn todo_done() -> String {
        normalize_icon(&active_theme().todo.done)
    }
}

// =============================================================================
// PROMPTS (loaded from yamls/prompts.yaml via config module)
// =============================================================================

pub mod library {
    use crate::config::LIBRARY;

    pub fn default_agent_id() -> &'static str {
        &LIBRARY.default_agent_id
    }
    pub fn default_agent_content() -> &'static str {
        let id = &LIBRARY.default_agent_id;
        LIBRARY.agents.iter().find(|a| a.id == *id).map(|a| a.content.as_str()).unwrap_or("")
    }
    pub fn agents() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.agents
    }
    pub fn skills() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.skills
    }
    pub fn commands() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.commands
    }
}

pub mod prompts {
    use crate::config::PROMPTS;

    pub fn panel_header() -> &'static str {
        &PROMPTS.panel.header
    }
    pub fn panel_timestamp() -> &'static str {
        &PROMPTS.panel.timestamp
    }
    pub fn panel_timestamp_unknown() -> &'static str {
        &PROMPTS.panel.timestamp_unknown
    }
    pub fn panel_footer() -> &'static str {
        &PROMPTS.panel.footer
    }
    pub fn panel_footer_msg_line() -> &'static str {
        &PROMPTS.panel.footer_msg_line
    }
    pub fn panel_footer_msg_header() -> &'static str {
        &PROMPTS.panel.footer_msg_header
    }
    pub fn panel_footer_ack() -> &'static str {
        &PROMPTS.panel.footer_ack
    }
}
