use std::collections::HashMap;
use std::sync::OnceLock;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::cast::Safe;
use crate::config::accessors::active_theme;
use crate::config::constants::CHARS_PER_TOKEN;
use crate::config::normalize_icon;

// =============================================================================
// Kind Registry — modules register metadata at startup
// =============================================================================

/// Metadata for a context type, provided by the owning module.
#[derive(Debug, Clone, Copy)]
pub struct TypeMeta {
    /// The context type string (e.g., "todo", "`git_result`")
    pub context_type: &'static str,
    /// Key into the theme's context icon `HashMap` (e.g., "todo", "git")
    pub icon_id: &'static str,
    /// Whether this is a fixed/sidebar panel
    pub is_fixed: bool,
    /// Whether this context type uses background cache loading
    pub needs_cache: bool,
    /// Sort order for fixed panels (P1=0, P2=1, ...). None for dynamic panels.
    pub fixed_order: Option<u8>,
    /// UI display name for overview tables (e.g., "todo", "git-result")
    pub display_name: &'static str,
    /// Short name for LLM context (e.g., "wip", "git-cmd")
    pub short_name: &'static str,
    /// Whether the stream should wait for this panel's cache to load after a tool opens it
    pub needs_async_wait: bool,
}

/// Global registry of context type metadata, populated once at startup.
static CONTEXT_TYPE_REGISTRY: OnceLock<Vec<TypeMeta>> = OnceLock::new();

/// Initialize the global context type registry. Called once at startup.
/// Modules provide their metadata via `Module::context_type_metadata()`.
pub fn init_context_type_registry(metadata: Vec<TypeMeta>) {
    _ = CONTEXT_TYPE_REGISTRY.get_or_init(|| metadata);
}

/// Look up metadata for a context type string.
pub fn get_context_type_meta(ct: &str) -> Option<&'static TypeMeta> {
    let registry = CONTEXT_TYPE_REGISTRY.get()?;
    registry.iter().find(|m| m.context_type == ct)
}

/// Return the canonical fixed panel order, derived from the registry.
/// Sorted by `fixed_order` for panels that declare `is_fixed = true`.
pub fn fixed_panel_order() -> Vec<&'static str> {
    let Some(registry) = CONTEXT_TYPE_REGISTRY.get() else { return vec![] };
    let mut fixed: Vec<_> = registry.iter().filter(|m| m.is_fixed && m.fixed_order.is_some()).collect();
    fixed.sort_by_key(|m| m.fixed_order.unwrap_or(0));
    fixed.iter().map(|m| m.context_type).collect()
}

// =============================================================================
// Kind
// =============================================================================

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
pub struct Kind(String);

impl Kind {
    /// Create a new context type from a string ID.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    /// Return the raw string ID.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    // === Well-known context type constants ===
    // These match the old enum variant names in snake_case (serde format).
    // Modules also export their own constants; these exist for convenience
    // and will be gradually removed as module-specific code moves out.

    /// System prompt panel.
    pub const SYSTEM: &str = "system";
    /// Active conversation panel.
    pub const CONVERSATION: &str = "conversation";
    /// Opened file panel.
    pub const FILE: &str = "file";
    /// Directory tree panel.
    pub const TREE: &str = "tree";
    /// Glob search results.
    pub const GLOB: &str = "glob";
    /// Grep search results.
    pub const GREP: &str = "grep";
    /// Tmux pane panel (legacy).
    pub const TMUX: &str = "tmux";
    /// Todo list panel.
    pub const TODO: &str = "todo";
    /// Memory items panel.
    pub const MEMORY: &str = "memory";
    /// Overview / stats panel.
    pub const OVERVIEW: &str = "overview";
    /// Git status / diff panel.
    pub const GIT: &str = "git";
    /// Git command result panel.
    pub const GIT_RESULT: &str = "git_result";
    /// GitHub CLI result panel.
    pub const GITHUB_RESULT: &str = "github_result";
    /// Scratchpad cells panel.
    pub const SCRATCHPAD: &str = "scratchpad";
    /// Prompt library panel.
    pub const LIBRARY: &str = "library";
    /// Loaded skill panel.
    pub const SKILL: &str = "skill";
    /// Detached conversation history panel.
    pub const CONVERSATION_HISTORY: &str = "conversation_history";
    /// Spine / notifications panel.
    pub const SPINE: &str = "spine";
    /// Log entries panel.
    pub const LOGS: &str = "logs";
    /// Console process output panel.
    pub const CONSOLE: &str = "console";
    /// Callback definitions panel.
    pub const CALLBACK: &str = "callback";
    /// Tools overview panel.
    pub const TOOLS: &str = "tools";
    /// Queue status panel.
    pub const QUEUE: &str = "queue";
    /// Chat dashboard panel (always-on room list, server status).
    pub const CHAT_DASHBOARD: &str = "chat-dashboard";

    /// Returns true if this is a fixed/system context type (looked up from registry).
    #[must_use]
    pub fn is_fixed(&self) -> bool {
        get_context_type_meta(self.0.as_str()).is_some_and(|m| m.is_fixed)
    }

    /// Get icon for this context type (normalized to 2 cells, looked up from registry + theme).
    #[must_use]
    pub fn icon(&self) -> String {
        let icon_id = get_context_type_meta(self.0.as_str()).map_or("file", |m| m.icon_id);
        let raw = active_theme().context.get(icon_id).unwrap_or("📄");
        normalize_icon(raw)
    }

    /// Returns true if this context type uses `cached_content` from background loading.
    #[must_use]
    pub fn needs_cache(&self) -> bool {
        get_context_type_meta(self.0.as_str()).is_some_and(|m| m.needs_cache)
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A single context panel in the LLM prompt — the core unit of the context window.
/// Fixed panels (P1–P7) are always present; dynamic panels (P8+) are created by tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Display ID (e.g., P1, P2, ... for UI/LLM)
    pub id: String,
    /// UID for dynamic panels (None for fixed P1-P7, Some for P8+)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    /// What kind of panel this is (e.g., `"todo"`, `"file"`, `"console"`).
    pub context_type: Kind,
    /// Human-readable panel name shown in sidebar and LLM header.
    pub name: String,
    /// Token count for this panel (current page if paginated).
    pub token_count: usize,
    /// Generic metadata bag for module-specific panel data.
    /// Keys are module-defined strings (e.g., "`file_path`", "`tmux_pane_id`").
    /// Replaces former hardcoded Option<> fields per module.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,

    // === Caching fields (not persisted) ===
    /// Cached content for LLM context and UI rendering
    #[serde(skip)]
    pub cached_content: Option<String>,
    /// Frozen Message objects for `ConversationHistory` panels (UI rendering)
    #[serde(skip)]
    pub history_messages: Option<Vec<super::data::message::Message>>,
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
    /// Full content token count (before pagination). `token_count` reflects current page.
    #[serde(skip)]
    pub full_token_count: usize,
    /// Whether this panel was a cache hit on the last LLM tick (prefix match)
    #[serde(skip)]
    pub panel_cache_hit: bool,
    /// Accumulated cost of this panel across all ticks ($USD). Never resets.
    #[serde(skip)]
    pub panel_total_cost: f64,

    // === Freeze fields (prompt cache preservation) ===
    /// Consecutive times this panel's changed content was suppressed to preserve the cache prefix.
    #[serde(skip)]
    pub freeze_count: u8,
    /// Total lifetime freezes — how many times this panel was frozen instead of updated. Persisted.
    #[serde(default)]
    pub total_freezes: u64,
    /// Total lifetime cache misses — how many times this panel's content changed and was emitted. Persisted.
    #[serde(default)]
    pub total_cache_misses: u64,
    /// Content string last emitted to the LLM (used as substitute when frozen).
    #[serde(skip)]
    pub last_emitted_content: Option<String>,
    /// SHA-256 of `last_emitted_content` (compared against fresh content to detect changes).
    #[serde(skip)]
    pub last_emitted_hash: Option<String>,
    /// Snapshot of the last `ContextItem` sent to the LLM for this panel.
    /// When the queue is active, this frozen copy is emitted instead of fresh
    /// content — ensuring zero token churn while the queue batches tool calls.
    #[serde(skip)]
    pub last_emitted_context: Option<crate::panels::ContextItem>,
}

// === Entry metadata helpers ===
impl Entry {
    /// Get a typed value from the metadata bag.
    #[must_use]
    pub fn get_meta<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let v = self.metadata.get(key)?;
        serde_json::from_value(v.clone()).ok()
    }

    /// Set a typed value in the metadata bag.
    pub fn set_meta<T: Serialize>(&mut self, key: &str, value: &T) {
        if let Ok(v) = serde_json::to_value(value) {
            drop(self.metadata.insert(key.to_string(), v));
        }
    }

    /// Fast path: get a metadata value as &str (avoids clone/deser for the common string case).
    #[must_use]
    pub fn get_meta_str(&self, key: &str) -> Option<&str> {
        let v = self.metadata.get(key)?;
        v.as_str()
    }

    /// Fast path: get a metadata `value.to_usize()`.
    #[must_use]
    pub fn get_meta_usize(&self, key: &str) -> Option<usize> {
        self.metadata.get(key).and_then(serde_json::Value::as_u64).map(Safe::to_usize)
    }
}

/// Estimate tokens from text (uses `CHARS_PER_TOKEN` constant)
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    (text.len().to_f32() / CHARS_PER_TOKEN).ceil().to_usize()
}

/// Compute total pages for a given token count using `PANEL_PAGE_TOKENS`
#[must_use]
pub const fn compute_total_pages(token_count: usize) -> usize {
    let max = crate::config::constants::PANEL_PAGE_TOKENS;
    if token_count <= max { 1 } else { token_count.div_ceil(max) }
}

/// Create a default `Entry` for a fixed or dynamic panel.
#[must_use]
pub fn make_default_entry(id: &str, context_type: Kind, name: &str, cache_deprecated: bool) -> Entry {
    Entry {
        id: id.to_string(),
        uid: None,
        context_type,
        name: name.to_string(),
        token_count: 0,
        metadata: HashMap::new(),
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
        freeze_count: 0,
        total_freezes: 0,
        total_cache_misses: 0,
        last_emitted_content: None,
        last_emitted_hash: None,
        last_emitted_context: None,
    }
}
