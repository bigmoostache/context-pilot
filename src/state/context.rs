use std::collections::HashSet;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::constants::{CHARS_PER_TOKEN, icons};

/// Pre-computed set of fixed context types (avoids heap allocations on every call)
static FIXED_TYPES: LazyLock<HashSet<ContextType>> = LazyLock::new(|| {
    let mut set = HashSet::new();
    for module in crate::modules::all_modules() {
        for ct in module.fixed_panel_types() {
            set.insert(ct);
        }
    }
    set
});

/// Pre-computed set of context types that need cache (avoids heap allocations on every call)
static CACHE_TYPES: LazyLock<HashSet<ContextType>> = LazyLock::new(|| {
    let mut set = HashSet::new();
    for module in crate::modules::all_modules() {
        for ct in module.fixed_panel_types().into_iter().chain(module.dynamic_panel_types()) {
            if let Some(panel) = module.create_panel(ct)
                && panel.needs_cache()
            {
                set.insert(ct);
            }
        }
    }
    set
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextType {
    System,
    Conversation,
    File,
    Tree,
    Glob,
    Grep,
    Tmux,
    Todo,
    Memory,
    Overview,
    Git,
    GitResult,
    GithubResult,
    Scratchpad,
    Library,
    Skill,
    ConversationHistory,
    Spine,
    Logs,
}

impl ContextType {
    /// Returns true if this is a fixed/system context type (pre-computed, zero allocation)
    pub fn is_fixed(&self) -> bool {
        FIXED_TYPES.contains(self)
    }

    /// Get icon for this context type (normalized to 2 cells)
    pub fn icon(&self) -> String {
        match self {
            ContextType::System => icons::ctx_system(),
            ContextType::Conversation => icons::ctx_conversation(),
            ContextType::File => icons::ctx_file(),
            ContextType::Tree => icons::ctx_tree(),
            ContextType::Glob => icons::ctx_glob(),
            ContextType::Grep => icons::ctx_grep(),
            ContextType::Tmux => icons::ctx_tmux(),
            ContextType::Todo => icons::ctx_todo(),
            ContextType::Memory => icons::ctx_memory(),
            ContextType::Overview => icons::ctx_overview(),
            ContextType::Git => icons::ctx_git(),
            ContextType::GitResult => icons::ctx_git(),
            ContextType::GithubResult => icons::ctx_git(),
            ContextType::Scratchpad => icons::ctx_scratchpad(),
            ContextType::Library => icons::ctx_library(),
            ContextType::Skill => icons::ctx_skill(),
            ContextType::ConversationHistory => icons::ctx_conversation(),
            ContextType::Spine => icons::ctx_spine(),
            ContextType::Logs => icons::ctx_memory(), // Reuse memory icon for logs
        }
    }

    /// Returns true if this context type uses cached_content from background loading.
    /// Pre-computed from Panel trait's needs_cache() method (zero allocation).
    pub fn needs_cache(&self) -> bool {
        CACHE_TYPES.contains(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextElement {
    /// Display ID (e.g., P1, P2, ... for UI/LLM)
    pub id: String,
    /// UID for dynamic panels (None for fixed P1-P7, Some for P8+)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    pub context_type: ContextType,
    pub name: String,
    pub token_count: usize,
    /// File path (for File context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Glob pattern (for Glob context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_pattern: Option<String>,
    /// Glob search path (for Glob context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_path: Option<String>,
    /// Grep regex pattern (for Grep context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_pattern: Option<String>,
    /// Grep search path (for Grep context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_path: Option<String>,
    /// Grep file filter pattern (for Grep context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_file_pattern: Option<String>,
    /// Tmux pane ID (for Tmux context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_pane_id: Option<String>,
    /// Number of lines to capture from tmux pane
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_lines: Option<usize>,
    /// Last keys sent to this tmux pane
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_last_keys: Option<String>,
    /// Description of what this tmux pane is for
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_description: Option<String>,
    /// Command string for GitResult/GithubResult panels
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_command: Option<String>,
    /// Skill prompt ID (links to PromptItem.id for Skill panels)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_prompt_id: Option<String>,

    // === Caching fields (not persisted) ===
    /// Cached content for LLM context and UI rendering
    #[serde(skip)]
    pub cached_content: Option<String>,
    /// Frozen Message objects for ConversationHistory panels (UI rendering)
    #[serde(skip)]
    pub history_messages: Option<Vec<super::Message>>,
    /// Cache is deprecated - source data changed, needs regeneration
    #[serde(skip)]
    pub cache_deprecated: bool,
    /// A cache request is already in-flight for this element (prevents duplicate spawning)
    #[serde(skip)]
    pub cache_in_flight: bool,
    /// Last time this element was refreshed (content actually changed — for display "refreshed X ago")
    #[serde(skip)]
    pub last_refresh_ms: u64,
    /// Hash of cached content (for change detection to avoid unnecessary timestamp bumps)
    #[serde(skip)]
    pub content_hash: Option<String>,
    /// Source data hash for background-thread early-exit optimization (not persisted)
    #[serde(skip)]
    pub source_hash: Option<String>,
    /// Current page (0-indexed) for LLM context pagination
    #[serde(skip)]
    pub current_page: usize,
    /// Total pages for LLM context pagination
    #[serde(skip)]
    pub total_pages: usize,
    /// Full content token count (before pagination). token_count reflects current page.
    #[serde(skip)]
    pub full_token_count: usize,
    /// Whether this panel was a cache hit on the last LLM tick (prefix match)
    #[serde(skip)]
    pub panel_cache_hit: bool,
    /// Accumulated cost of this panel across all ticks ($USD). Never resets.
    #[serde(skip)]
    pub panel_total_cost: f64,
}

/// Estimate tokens from text (uses CHARS_PER_TOKEN constant)
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() as f32 / CHARS_PER_TOKEN).ceil() as usize
}

/// Compute total pages for a given token count using PANEL_PAGE_TOKENS
pub fn compute_total_pages(token_count: usize) -> usize {
    let max = crate::constants::PANEL_PAGE_TOKENS;
    if token_count <= max { 1 } else { token_count.div_ceil(max) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{CHARS_PER_TOKEN, PANEL_PAGE_TOKENS};

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_short_text() {
        let text = "hello world";
        let expected = (text.len() as f32 / CHARS_PER_TOKEN).ceil() as usize;
        assert_eq!(estimate_tokens(text), expected);
    }

    #[test]
    fn estimate_tokens_single_char() {
        // 1 char / 3.3 = 0.303... → ceil = 1
        assert_eq!(estimate_tokens("a"), 1);
    }

    #[test]
    fn compute_total_pages_zero() {
        assert_eq!(compute_total_pages(0), 1);
    }

    #[test]
    fn compute_total_pages_at_threshold() {
        assert_eq!(compute_total_pages(PANEL_PAGE_TOKENS), 1);
    }

    #[test]
    fn compute_total_pages_above_threshold() {
        assert_eq!(compute_total_pages(PANEL_PAGE_TOKENS + 1), 2);
    }

    #[test]
    fn compute_total_pages_double() {
        assert_eq!(compute_total_pages(PANEL_PAGE_TOKENS * 2), 2);
    }

    #[test]
    fn compute_total_pages_double_plus_one() {
        assert_eq!(compute_total_pages(PANEL_PAGE_TOKENS * 2 + 1), 3);
    }
}
