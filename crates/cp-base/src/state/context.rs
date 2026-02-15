use serde::{Deserialize, Serialize};

use crate::constants::{CHARS_PER_TOKEN, icons};

/// A string-backed context type identifier.
///
/// Replaces the former hardcoded enum. Modules define their own context type
/// constants (e.g., `pub const CONTEXT_TYPE: &str = "todo"`) and cp-base
/// provides associated `&str` constants for backwards compatibility.
///
/// Serialized transparently as a plain string (e.g., `"todo"`, `"git_result"`),
/// which is backwards-compatible with the old `#[serde(rename_all = "snake_case")]`
/// enum serialization.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContextType(String);

impl ContextType {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    // === Well-known context type constants ===
    // These match the old enum variant names in snake_case (serde format).
    // Modules also export their own constants; these exist for convenience
    // and will be gradually removed as module-specific code moves out.

    pub const SYSTEM: &str = "system";
    pub const CONVERSATION: &str = "conversation";
    pub const FILE: &str = "file";
    pub const TREE: &str = "tree";
    pub const GLOB: &str = "glob";
    pub const GREP: &str = "grep";
    pub const TMUX: &str = "tmux";
    pub const TODO: &str = "todo";
    pub const MEMORY: &str = "memory";
    pub const OVERVIEW: &str = "overview";
    pub const GIT: &str = "git";
    pub const GIT_RESULT: &str = "git_result";
    pub const GITHUB_RESULT: &str = "github_result";
    pub const SCRATCHPAD: &str = "scratchpad";
    pub const LIBRARY: &str = "library";
    pub const SKILL: &str = "skill";
    pub const CONVERSATION_HISTORY: &str = "conversation_history";
    pub const SPINE: &str = "spine";
    pub const LOGS: &str = "logs";

    /// Returns true if this is a fixed/system context type
    pub fn is_fixed(&self) -> bool {
        matches!(
            self.0.as_str(),
            Self::TODO
                | Self::LIBRARY
                | Self::OVERVIEW
                | Self::TREE
                | Self::MEMORY
                | Self::SPINE
                | Self::LOGS
                | Self::GIT
                | Self::SCRATCHPAD
        )
    }

    /// Get icon for this context type (normalized to 2 cells)
    pub fn icon(&self) -> String {
        match self.0.as_str() {
            Self::SYSTEM => icons::ctx_system(),
            Self::CONVERSATION => icons::ctx_conversation(),
            Self::FILE => icons::ctx_file(),
            Self::TREE => icons::ctx_tree(),
            Self::GLOB => icons::ctx_glob(),
            Self::GREP => icons::ctx_grep(),
            Self::TMUX => icons::ctx_tmux(),
            Self::TODO => icons::ctx_todo(),
            Self::MEMORY => icons::ctx_memory(),
            Self::OVERVIEW => icons::ctx_overview(),
            Self::GIT | Self::GIT_RESULT | Self::GITHUB_RESULT => icons::ctx_git(),
            Self::SCRATCHPAD => icons::ctx_scratchpad(),
            Self::LIBRARY => icons::ctx_library(),
            Self::SKILL => icons::ctx_skill(),
            Self::CONVERSATION_HISTORY => icons::ctx_conversation(),
            Self::SPINE => icons::ctx_spine(),
            Self::LOGS => icons::ctx_memory(),
            _ => icons::ctx_file(), // fallback for unknown types
        }
    }

    /// Returns true if this context type uses cached_content from background loading.
    pub fn needs_cache(&self) -> bool {
        matches!(
            self.0.as_str(),
            Self::FILE | Self::TREE | Self::GLOB | Self::GREP | Self::TMUX | Self::GIT | Self::GIT_RESULT | Self::GITHUB_RESULT
        )
    }
}

impl std::fmt::Display for ContextType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Allow `ctx.context_type == "todo"` comparisons
impl PartialEq<&str> for ContextType {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_str() == *other
    }
}

/// Allow `"todo" == ctx.context_type` comparisons
impl PartialEq<ContextType> for &str {
    fn eq(&self, other: &ContextType) -> bool {
        *self == other.0.as_str()
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

/// Create a default ContextElement for a fixed or dynamic panel.
pub fn make_default_context_element(
    id: &str,
    context_type: ContextType,
    name: &str,
    cache_deprecated: bool,
) -> ContextElement {
    ContextElement {
        id: id.to_string(),
        uid: None,
        context_type,
        name: name.to_string(),
        token_count: 0,
        file_path: None,
        glob_pattern: None,
        glob_path: None,
        grep_pattern: None,
        grep_path: None,
        grep_file_pattern: None,
        tmux_pane_id: None,
        tmux_lines: None,
        tmux_last_keys: None,
        tmux_description: None,
        result_command: None,
        skill_prompt_id: None,
        cached_content: None,
        history_messages: None,
        cache_deprecated,
        cache_in_flight: false,
        last_refresh_ms: crate::panels::now_ms(),
        content_hash: None,
        source_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
        panel_cache_hit: false,
        panel_total_cost: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{CHARS_PER_TOKEN, PANEL_PAGE_TOKENS};

    #[test]
    fn context_type_serde_roundtrip() {
        let ct = ContextType::new("todo");
        let json = serde_json::to_string(&ct).unwrap();
        assert_eq!(json, "\"todo\"");
        let ct2: ContextType = serde_json::from_str(&json).unwrap();
        assert_eq!(ct, ct2);
    }

    #[test]
    fn context_type_eq_str() {
        let ct = ContextType::new("todo");
        assert!(ct == "todo");
        assert!("todo" == ct);
        assert!(ct != "file");
    }

    #[test]
    fn context_type_is_fixed() {
        assert!(ContextType::new(ContextType::TODO).is_fixed());
        assert!(ContextType::new(ContextType::GIT).is_fixed());
        assert!(!ContextType::new(ContextType::FILE).is_fixed());
        assert!(!ContextType::new(ContextType::CONVERSATION).is_fixed());
    }

    #[test]
    fn context_type_needs_cache() {
        assert!(ContextType::new(ContextType::FILE).needs_cache());
        assert!(ContextType::new(ContextType::GIT).needs_cache());
        assert!(!ContextType::new(ContextType::TODO).needs_cache());
        assert!(!ContextType::new(ContextType::CONVERSATION).needs_cache());
    }

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
