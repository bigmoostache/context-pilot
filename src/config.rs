//! YAML configuration loader for prompts, icons, and UI strings.
use lazy_static::lazy_static;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::sync::RwLock;

// ============================================================================
// Prompts Configuration
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct PromptsConfig {
    pub seeds: Vec<SeedEntry>,
    pub default_seed_id: String,
    pub tldr_prompt: String,
    pub tldr_min_tokens: usize,
    pub panel: PanelPrompts,
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

#[derive(Debug, Deserialize, Clone)]
pub struct ContextIcons {
    pub system: String,
    pub conversation: String,
    pub tree: String,
    pub todo: String,
    pub memory: String,
    pub overview: String,
    pub file: String,
    pub glob: String,
    pub grep: String,
    pub tmux: String,
    pub git: String,
    pub scratchpad: String,
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

fn load_yaml<T: for<'de> Deserialize<'de>>(path: &str) -> T {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
    serde_yaml::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path, e))
}

// ============================================================================
// Global Configuration (Lazy Static)
// ============================================================================

lazy_static! {
    pub static ref PROMPTS: PromptsConfig = load_yaml("yamls/prompts.yaml");
    pub static ref UI: UiConfig = load_yaml("yamls/ui.yaml");
    pub static ref THEMES: ThemesConfig = load_yaml("yamls/themes.yaml");
}

/// Get a theme by ID, falling back to default if not found
pub fn get_theme(theme_id: &str) -> &'static Theme {
    THEMES.themes.get(theme_id)
        .or_else(|| THEMES.themes.get(DEFAULT_THEME))
        .expect("Default theme must exist")
}

// ============================================================================
// Active Theme (Global State)
// ============================================================================

lazy_static! {
    /// Global active theme ID - updated when state changes
    static ref ACTIVE_THEME: RwLock<String> = RwLock::new(DEFAULT_THEME.to_string());
}

/// Set the active theme ID (call when state is loaded or theme changes)
pub fn set_active_theme(theme_id: &str) {
    if let Ok(mut theme) = ACTIVE_THEME.write() {
        *theme = theme_id.to_string();
    }
}

/// Get the currently active theme
pub fn active_theme() -> &'static Theme {
    let theme_id = ACTIVE_THEME.read()
        .map(|t| t.clone())
        .unwrap_or_else(|_| DEFAULT_THEME.to_string());
    get_theme(&theme_id)
}

// ============================================================================
// Icon Helper
// ============================================================================

/// Return icon with trailing space for visual separation.
/// All icons should be single-width Unicode symbols.
pub fn normalize_icon(icon: &str) -> String {
    format!("{} ", icon)
}

