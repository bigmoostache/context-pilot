//! Render cache types for conversation panel performance.
//!
//! Caches pre-rendered IR blocks per message and for the input area,
//! avoiding re-rendering on every frame. The TUI adapter converts
//! `Vec<Block>` → `Vec<Line>` once per cache miss.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher as _};
use std::rc::Rc;

/// Cached rendered blocks for a message.
#[derive(Debug, Clone)]
pub struct MessageCache {
    /// Pre-rendered IR blocks for this message.
    pub blocks: Rc<[cp_render::Block]>,
    /// Hash of content that affects rendering.
    pub content_hash: u64,
    /// Viewport width used for wrapping.
    pub viewport_width: u16,
}

/// Cached rendered blocks for input area.
#[derive(Debug, Clone)]
pub struct InputCache {
    /// Pre-rendered IR blocks for input.
    pub blocks: Rc<[cp_render::Block]>,
    /// Hash of input + cursor position.
    pub input_hash: u64,
    /// Viewport width used for wrapping.
    pub viewport_width: u16,
}

/// Top-level cache for entire conversation content.
#[derive(Debug, Clone)]
pub struct FullCache {
    /// Complete rendered IR blocks.
    pub blocks: Rc<[cp_render::Block]>,
    /// Hash of all inputs that affect rendering.
    pub content_hash: u64,
}

/// Hash helper for cache invalidation.
pub fn hash_values<T: Hash>(values: &[T]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for v in values {
        v.hash(&mut hasher);
    }
    hasher.finish()
}
