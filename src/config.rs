//! YAML configuration loader for prompts, icons, and UI strings.
use std::sync::LazyLock;

use serde::Deserialize;
use std::collections::HashMap;


// ============================================================================
// Prompts Configuration
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct PromptsConfig {
    pub tldr_prompt: String,
    pub tldr_min_tokens: usize,
    pub panel: PanelPrompts,
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
    pub library: String,
    pub skill: String,
    #[serde(default = "default_spine_icon")]
    pub spine: String,
}

fn default_spine_icon() -> String { "⚡".to_string() }

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
    serde_yaml::from_str(content)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", name, e))
}

// ============================================================================
// Global Configuration (Lazy Static — embedded at compile time)
// ============================================================================

pub static PROMPTS: LazyLock<PromptsConfig> = LazyLock::new(|| parse_yaml("prompts.yaml", include_str!("../yamls/prompts.yaml")));
pub static LIBRARY: LazyLock<LibraryConfig> = LazyLock::new(|| parse_yaml("library.yaml", include_str!("../yamls/library.yaml")));
pub static UI: LazyLock<UiConfig> = LazyLock::new(|| parse_yaml("ui.yaml", include_str!("../yamls/ui.yaml")));
pub static THEMES: LazyLock<ThemesConfig> = LazyLock::new(|| parse_yaml("themes.yaml", include_str!("../yamls/themes.yaml")));

/// Get a theme by ID, falling back to default if not found
pub fn get_theme(theme_id: &str) -> &'static Theme {
    THEMES.themes.get(theme_id)
        .or_else(|| THEMES.themes.get(DEFAULT_THEME))
        .expect("Default theme must exist")
}

// ============================================================================
// Active Theme (Global State — thread-safe cached theme ID)
// ============================================================================

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Cached theme ID stored as an index into THEME_ORDER
/// 0 = uninitialized, 1 = first theme (dnd), etc.
static CACHED_THEME_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Fallback storage for custom theme IDs not in THEME_ORDER
static CUSTOM_THEME_ID: OnceLock<String> = OnceLock::new();

/// Set the active theme ID (call when state is loaded or theme changes)
/// Thread-safe without unsafe code
pub fn set_active_theme(theme_id: &str) {
    // Try to find theme in THEME_ORDER for fast indexing
    if let Some(idx) = THEME_ORDER.iter().position(|&id| id == theme_id) {
        CACHED_THEME_INDEX.store(idx + 1, Ordering::Release);
    } else {
        // Custom theme not in THEME_ORDER - store in fallback
        let _ = CUSTOM_THEME_ID.set(theme_id.to_string());
        CACHED_THEME_INDEX.store(usize::MAX, Ordering::Release);
    }
}

/// Get the currently active theme (lock-free for standard themes)
pub fn active_theme() -> &'static Theme {
    let idx = CACHED_THEME_INDEX.load(Ordering::Acquire);
    
    if idx == 0 {
        // Not initialized yet - use default
        get_theme(DEFAULT_THEME)
    } else if idx == usize::MAX {
        // Custom theme
        if let Some(theme_id) = CUSTOM_THEME_ID.get() {
            get_theme(theme_id)
        } else {
            get_theme(DEFAULT_THEME)
        }
    } else {
        // Standard theme from THEME_ORDER (idx is 1-based)
        // Debug assertion helps catch bugs during development
        // In release builds, we have unwrap_or_else fallback below for safety
        debug_assert!(idx > 0 && idx <= THEME_ORDER.len(), 
            "Invalid theme index: {}. THEME_ORDER has {} entries.", idx, THEME_ORDER.len());
        
        let theme_id = THEME_ORDER.get(idx - 1).unwrap_or_else(|| {
            // This should never happen if set_active_theme is used correctly
            // Fall back to default theme to prevent panic in release builds
            &DEFAULT_THEME
        });
        get_theme(theme_id)
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

