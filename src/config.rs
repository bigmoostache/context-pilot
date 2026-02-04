//! YAML configuration loader for prompts, icons, and UI strings.
#![allow(dead_code)]

use lazy_static::lazy_static;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

// ============================================================================
// Prompts Configuration
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct PromptsConfig {
    pub default_seed: DefaultSeed,
    pub tldr_prompt: String,
    pub tldr_min_tokens: usize,
    pub panel: PanelPrompts,
}

#[derive(Debug, Deserialize)]
pub struct DefaultSeed {
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
// Icons Configuration
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct IconsConfig {
    pub messages: MessageIcons,
    pub context: ContextIcons,
    pub status: StatusIcons,
    pub todo: TodoIcons,
}

#[derive(Debug, Deserialize)]
pub struct MessageIcons {
    pub user: String,
    pub assistant: String,
    pub tool_call: String,
    pub tool_result: String,
    pub error: String,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
pub struct StatusIcons {
    pub full: String,
    pub summarized: String,
    pub deleted: String,
}

#[derive(Debug, Deserialize)]
pub struct TodoIcons {
    pub pending: String,
    pub in_progress: String,
    pub done: String,
}

// ============================================================================
// UI Configuration
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct UiConfig {
    pub panels: PanelNames,
    pub tool_categories: ToolCategories,
    pub status_bar: StatusBarLabels,
    pub sidebar: SidebarLabels,
    pub commands: HashMap<String, CommandConfig>,
    pub panel_titles: PanelTitles,
    pub labels: CommonLabels,
}

#[derive(Debug, Deserialize)]
pub struct PanelNames {
    pub system: String,
    pub conversation: String,
    pub tree: String,
    pub todo: String,
    pub memory: String,
    pub overview: String,
    pub git: String,
    pub scratchpad: String,
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

#[derive(Debug, Deserialize)]
pub struct StatusBarLabels {
    pub streaming: String,
    pub loading_files: String,
    pub summarizing: String,
    pub loading: String,
    pub ready: String,
}

#[derive(Debug, Deserialize)]
pub struct SidebarLabels {
    pub context_header: String,
    pub page_indicator: String,
    pub help: SidebarHelp,
}

#[derive(Debug, Deserialize)]
pub struct SidebarHelp {
    pub tab: String,
    pub arrows: String,
    pub ctrl_p: String,
    pub ctrl_q: String,
}

#[derive(Debug, Deserialize)]
pub struct CommandConfig {
    pub label: String,
    pub description: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PanelTitles {
    pub file: String,
    pub conversation: String,
    pub conversation_streaming: String,
    pub git: String,
    pub glob: String,
    pub grep: String,
    pub memory: String,
    pub overview: String,
    pub scratchpad: String,
    pub system: String,
    pub tmux: String,
    pub todo: String,
    pub tree: String,
}

#[derive(Debug, Deserialize)]
pub struct CommonLabels {
    pub loading: String,
    pub no_content: String,
    pub no_memories: String,
    pub not_git_repo: String,
    pub branch: String,
    pub branches: String,
    pub working_tree_clean: String,
}

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
    pub static ref ICONS: IconsConfig = load_yaml("yamls/icons.yaml");
    pub static ref UI: UiConfig = load_yaml("yamls/ui.yaml");
}

// ============================================================================
// Convenience Accessors
// ============================================================================

/// Get icon for a context type
pub fn context_icon(context_type: &str) -> &'static str {
    match context_type {
        "system" => &ICONS.context.system,
        "conversation" => &ICONS.context.conversation,
        "tree" => &ICONS.context.tree,
        "todo" => &ICONS.context.todo,
        "memory" => &ICONS.context.memory,
        "overview" => &ICONS.context.overview,
        "file" => &ICONS.context.file,
        "glob" => &ICONS.context.glob,
        "grep" => &ICONS.context.grep,
        "tmux" => &ICONS.context.tmux,
        "git" => &ICONS.context.git,
        "scratchpad" => &ICONS.context.scratchpad,
        _ => "?",
    }
}

/// Get default panel name for a context type
pub fn panel_name(context_type: &str) -> &'static str {
    match context_type {
        "system" => &UI.panels.system,
        "conversation" => &UI.panels.conversation,
        "tree" => &UI.panels.tree,
        "todo" => &UI.panels.todo,
        "memory" => &UI.panels.memory,
        "overview" => &UI.panels.overview,
        "git" => &UI.panels.git,
        "scratchpad" => &UI.panels.scratchpad,
        _ => "Unknown",
    }
}
