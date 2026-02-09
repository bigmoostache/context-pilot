use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use ratatui::text::Line;

/// Cached rendered lines for a message (using Rc to avoid clones)
#[derive(Clone)]
pub struct MessageRenderCache {
    /// Pre-rendered lines for this message
    pub lines: Rc<Vec<Line<'static>>>,
    /// Hash of content that affects rendering
    pub content_hash: u64,
    /// Viewport width used for wrapping
    pub viewport_width: u16,
}

/// Cached rendered lines for input area (using Rc to avoid clones)
#[derive(Clone)]
pub struct InputRenderCache {
    /// Pre-rendered lines for input
    pub lines: Rc<Vec<Line<'static>>>,
    /// Hash of input + cursor position
    pub input_hash: u64,
    /// Viewport width used for wrapping
    pub viewport_width: u16,
}

/// Top-level cache for entire conversation content
#[derive(Clone)]
pub struct FullContentCache {
    /// Complete rendered output
    pub lines: Rc<Vec<Line<'static>>>,
    /// Hash of all inputs that affect rendering
    pub content_hash: u64,
}

/// Hash helper for cache invalidation
pub fn hash_values<T: Hash>(values: &[T]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for v in values {
        v.hash(&mut hasher);
    }
    hasher.finish()
}
