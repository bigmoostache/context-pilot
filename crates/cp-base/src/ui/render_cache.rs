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
#[non_exhaustive]
pub struct MessageCache {
    /// Pre-rendered IR blocks for this message.
    pub blocks: Rc<[cp_render::Block]>,
    /// Hash of content that affects rendering.
    pub content_hash: u64,
    /// Viewport width used for wrapping.
    pub viewport_width: u16,
}

impl MessageCache {
    /// Cache pre-rendered blocks for a message at a given content hash and width.
    #[must_use]
    pub const fn new(blocks: Rc<[cp_render::Block]>, content_hash: u64, viewport_width: u16) -> Self {
        Self { blocks, content_hash, viewport_width }
    }
}

/// Cached rendered blocks for input area.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InputCache {
    /// Pre-rendered IR blocks for input.
    pub blocks: Rc<[cp_render::Block]>,
    /// Hash of input + cursor position.
    pub input_hash: u64,
    /// Viewport width used for wrapping.
    pub viewport_width: u16,
}

impl InputCache {
    /// Cache pre-rendered input-area blocks at a given input hash and width.
    #[must_use]
    pub const fn new(blocks: Rc<[cp_render::Block]>, input_hash: u64, viewport_width: u16) -> Self {
        Self { blocks, input_hash, viewport_width }
    }
}

/// Top-level cache for entire conversation content.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FullCache {
    /// Complete rendered IR blocks.
    pub blocks: Rc<[cp_render::Block]>,
    /// Hash of all inputs that affect rendering.
    pub content_hash: u64,
}

impl FullCache {
    /// Cache the complete rendered conversation blocks at a content hash.
    #[must_use]
    pub const fn new(blocks: Rc<[cp_render::Block]>, content_hash: u64) -> Self {
        Self { blocks, content_hash }
    }
}

/// Hash helper for cache invalidation.
pub fn hash_values<T>(values: &[T]) -> u64
where
    T: Hash,
{
    let mut hasher = DefaultHasher::new();
    for v in values {
        v.hash(&mut hasher);
    }
    hasher.finish()
}
