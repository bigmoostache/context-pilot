//! Panel trait and implementations for different context types.
//!
//! Each panel type implements the Panel trait, providing a consistent
//! interface for rendering AND context generation for the LLM.
//!
//! ## Caching Architecture
//!
//! Panels use a two-level caching system:
//! - `cache_deprecated`: Source data changed, cache needs regeneration
//! - `cached_content`: The actual cached content string
//!
//! When `refresh()` is called:
//! 1. Check if cache is deprecated (or missing)
//! 2. If so, regenerate cache from source data
//! 3. Update token count from cached content
//!
//! `context()` returns the cached content without regenerating.

use std::any::Any;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest as _, Sha256};

use crossterm::event::{KeyCode, KeyEvent};

use crate::cast::Safe as _;
use crate::config::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::actions::Action;
use crate::state::context::{Entry, Kind};
use crate::state::runtime::State;

// =============================================================================
// Key Helpers
// =============================================================================

/// Map a key event to a scroll action (Up/Down/PageUp/PageDown).
///
/// Returns `None` for any non-scroll key. Centralizes the scroll-key
/// mapping so individual panel `handle_key()` implementations can avoid
/// matching on `KeyCode` directly.
#[must_use]
pub const fn scroll_key_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Up => Some(Action::ScrollUp(SCROLL_ARROW_AMOUNT)),
        KeyCode::Down => Some(Action::ScrollDown(SCROLL_ARROW_AMOUNT)),
        KeyCode::PageUp => Some(Action::ScrollUp(SCROLL_PAGE_AMOUNT)),
        KeyCode::PageDown => Some(Action::ScrollDown(SCROLL_PAGE_AMOUNT)),
        KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::Esc
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => None,
    }
}

// =============================================================================
// Cache Types
// =============================================================================

/// Result of a background cache operation
pub enum CacheUpdate {
    /// Generic content update (used by File, Tree, Glob, Grep, Tmux, `GitResult`, `GithubResult`)
    Content {
        /// Context element ID (e.g., "P7").
        context_id: String,
        /// New panel content string.
        content: String,
        /// Estimated token count for the new content.
        token_count: usize,
    },
    /// Content unchanged — clear `cache_in_flight` without updating content
    Unchanged {
        /// Context element ID.
        context_id: String,
    },
    /// Module-specific update requiring downcast (e.g., git status populating `GitState`)
    ModuleSpecific {
        /// Panel type to match against.
        context_type: Kind,
        /// Type-erased module data (downcast by the module's `apply_cache_update`).
        data: Box<dyn Any + Send>,
    },
}

impl fmt::Debug for CacheUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Content { context_id, token_count, .. } => {
                f.debug_struct("Content").field("context_id", context_id).field("token_count", token_count).finish()
            }
            Self::Unchanged { context_id } => f.debug_struct("Unchanged").field("context_id", context_id).finish(),
            Self::ModuleSpecific { context_type, .. } => {
                f.debug_struct("ModuleSpecific").field("context_type", context_type).finish()
            }
        }
    }
}

/// Generic request for background cache operations.
/// Each module defines its own request data struct and wraps it in `data`.
pub struct CacheRequest {
    /// Panel type that originated this request.
    pub context_type: Kind,
    /// Type-erased request payload (downcast by the module's `refresh_cache`).
    pub data: Box<dyn Any + Send>,
}

impl fmt::Debug for CacheRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CacheRequest").field("context_type", &self.context_type).finish_non_exhaustive()
    }
}

/// Hash content for change detection (SHA-256, collision-resistant)
#[must_use]
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:064x}", hasher.finalize())
}

// =============================================================================
// Panel Helpers
// =============================================================================

/// Specification for a filesystem path to watch.
#[derive(Debug)]
pub enum WatchSpec {
    /// Watch a single file (non-recursive)
    File(String),
    /// Watch a directory (non-recursive, immediate children only)
    Dir(String),
    /// Watch a directory recursively
    DirRecursive(String),
}

/// Get current time in milliseconds since UNIX epoch
#[must_use]
pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64())
}

/// Truncating time arithmetic — the sole choke-point for `/ 1000`, `% 3600`, etc.
///
/// Every millisecond-to-second conversion and HMS decomposition in the
/// codebase funnels through these helpers so no other file triggers
/// `clippy::integer_division_remainder_used`.
#[expect(
    clippy::integer_division_remainder_used,
    clippy::arithmetic_side_effects,
    reason = "sole choke-point for truncating time/division arithmetic"
)]
pub mod time_arith {
    use std::time::Duration;

    use crate::cast::Safe as _;

    /// Truncating conversion from milliseconds to whole seconds.
    #[must_use]
    pub const fn ms_to_secs(ms: u64) -> u64 {
        Duration::from_millis(ms).as_secs()
    }

    /// Truncating conversion from microseconds to whole milliseconds.
    #[must_use]
    pub fn us_to_ms(us: u64) -> u64 {
        Duration::from_micros(us).as_millis().to_u64()
    }

    /// Decompose seconds into `(hours, minutes, seconds)` within a 24 h window.
    #[must_use]
    pub const fn secs_to_hms(total_secs: u64) -> (u64, u64, u64) {
        let hours = (total_secs % 86400) / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        (hours, minutes, seconds)
    }

    /// Decompose seconds into `(hours, minutes, seconds)` without 24 h wrap.
    #[must_use]
    pub const fn secs_to_hms_unwrapped(total_secs: u64) -> (u64, u64, u64) {
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        (hours, minutes, seconds)
    }

    /// 5 % of a value (`value / 20`), for budget stepping.
    #[must_use]
    pub const fn five_pct(value: usize) -> usize {
        value / 20
    }

    /// 10 % of a value (`value / 10`), for budget floor.
    #[must_use]
    pub const fn ten_pct(value: usize) -> usize {
        value / 10
    }

    /// Truncating integer division by a compile-time constant divisor.
    #[must_use]
    pub const fn div_const<const D: usize>(value: usize) -> usize {
        value / D
    }
}

/// Update `last_refresh_ms` only if content actually changed (hash differs).
/// Returns true if content changed.
pub fn update_if_changed(ctx: &mut Entry, content: &str) -> bool {
    let new_hash = hash_content(content);
    if ctx.content_hash.as_deref() == Some(&new_hash) {
        return false;
    }
    ctx.content_hash = Some(new_hash);
    ctx.last_refresh_ms = now_ms();
    true
}

/// Mark all panels of a given context type as cache-deprecated (dirty).
/// Also sets `state.dirty = true` so the UI re-renders.
/// Resets freeze protection so the next prompt assembly emits fresh content.
pub fn mark_panels_dirty(state: &mut State, context_type: &str) {
    for ctx in &mut state.context {
        if ctx.context_type.as_str() == context_type {
            ctx.cache_deprecated = true;
            ctx.freeze_count = u8::MAX;
        }
    }
    state.flags.ui.dirty = true;
}

/// Paginate content for LLM context output.
///
/// Returns the original content unchanged when `total_pages` <= 1.
/// Otherwise slices by approximate token offset, snaps to line boundaries,
/// and prepends a page header.
#[must_use]
pub fn paginate_content(full_content: &str, current_page: usize, total_pages: usize) -> String {
    use crate::config::constants::{CHARS_PER_TOKEN, PANEL_PAGE_TOKENS};

    if total_pages <= 1 {
        return full_content.to_string();
    }

    let chars_per_page = PANEL_PAGE_TOKENS.to_f32() * CHARS_PER_TOKEN;
    let start_char = (current_page.to_f32() * chars_per_page).to_usize();

    // Snap start to next line boundary
    let start = if start_char == 0 {
        0
    } else if start_char >= full_content.len() {
        full_content.len()
    } else {
        // Find next newline after start_char
        full_content
            .get(start_char..)
            .unwrap_or("")
            .find('\n')
            .map_or(full_content.len(), |pos| start_char.saturating_add(pos).saturating_add(1))
    };

    let end_char = start.saturating_add(chars_per_page.to_usize());
    let end = if end_char >= full_content.len() {
        full_content.len()
    } else {
        // Find next newline after end_char to snap to line boundary
        full_content
            .get(end_char..)
            .unwrap_or("")
            .find('\n')
            .map_or(full_content.len(), |pos| end_char.saturating_add(pos).saturating_add(1))
    };

    let page_content = full_content.get(start..end).unwrap_or("");
    format!(
        "[Page {}/{} — use panel_goto_page to navigate]\n{}",
        current_page.saturating_add(1),
        total_pages,
        page_content
    )
}

/// A single context item to be sent to the LLM
#[derive(Debug, Clone)]
pub struct ContextItem {
    /// Context element ID (e.g., "P7", "P8") for LLM reference
    pub id: String,
    /// Header/title for this context (e.g., "File: src/main.rs" or "Todo List")
    pub header: String,
    /// The actual content
    pub content: String,
    /// Last refresh timestamp in milliseconds since UNIX epoch (for sorting panels)
    pub last_refresh_ms: u64,
}

impl ContextItem {
    /// Create a context item from its components.
    pub fn new<I: Into<String>, H: Into<String>, C: Into<String>>(
        id: I,
        header: H,
        content: C,
        last_refresh_ms: u64,
    ) -> Self {
        Self { id: id.into(), header: header.into(), content: content.into(), last_refresh_ms }
    }
}

/// Trait for all panel types
pub trait Panel {
    /// Generate the panel's title for display
    fn title(&self, state: &State) -> String;

    /// Generate semantic IR blocks for platform-agnostic rendering.
    ///
    /// Returns a `Vec<Block>` that the TUI adapter converts to
    /// ratatui `Line` vectors for display. This is the primary
    /// rendering method — all panels must implement it.
    fn blocks(&self, _state: &State) -> Vec<cp_render::Block> {
        Vec::new()
    }

    /// Handle keyboard input for this panel
    /// Returns None to use default handling, Some(action) to override
    fn handle_key(&self, _key: &KeyEvent, _state: &State) -> Option<Action> {
        None // Default: use global key handling
    }

    /// Whether this panel uses background caching (`cached_content` from background loading)
    fn needs_cache(&self) -> bool {
        false
    }

    /// Refresh token counts and any cached data (called before generating context)
    fn refresh(&self, _state: &mut State) {
        // Default: no refresh needed
    }

    /// Compute a cache update for this panel in the background.
    /// Called from a background thread — implementations should do blocking I/O here.
    /// Returns None if no update is needed (e.g., content unchanged).
    fn refresh_cache(&self, _request: CacheRequest) -> Option<CacheUpdate> {
        None
    }

    /// Build a cache request for the given context element.
    /// Returns None for panels without background caching.
    fn build_cache_request(&self, _ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        None
    }

    /// Apply a cache update to the context element and state.
    /// Returns true if content changed (caller sets state.dirty).
    fn apply_cache_update(&self, _update: CacheUpdate, _ctx: &mut Entry, _state: &mut State) -> bool {
        false
    }

    /// Timer interval in ms for auto-refresh. None = no timer (uses watchers or no refresh).
    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    /// Max consecutive freezes before a forced cache-breaking update.
    ///
    /// When a panel's content changes but no earlier panel has broken the
    /// prompt cache yet, emitting new content would break the cache prefix
    /// for every panel that follows. Returning a value > 0 allows the system
    /// to emit the **previous** content up to N times, preserving the cache.
    /// Once the counter is exhausted — or the cache is already broken
    /// upstream — the panel updates for free.
    ///
    /// Default `0` means "never freeze, always emit fresh content".
    fn max_freezes(&self) -> u8 {
        0
    }

    /// Generate context items to send to the LLM
    /// Returns empty vec if this panel doesn't contribute to LLM context
    fn context(&self, _state: &State) -> Vec<ContextItem> {
        Vec::new()
    }

    /// Check whether this panel should automatically close itself.
    /// Called every ~100ms for ALL panels. Implementations must be fast:
    ///
    /// - Default: instant `false`
    /// - `FilePanel`: only checks disk if still loading (no `cached_content`)
    /// - `ConsolePanel`: callback consoles check for newer siblings; others only check when loading
    ///
    /// Return `true` to kill the panel.
    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }
}
