//! Fixed-size character-based splitter.
//!
//! Splits file content into chunks of approximately [`FALLBACK_CHUNK_SIZE`]
//! characters, breaking only on line boundaries.  Used as the catch-all
//! fallback when no AST-aware splitter is available for a file extension.

use std::path::Path;

use crate::types::FALLBACK_CHUNK_SIZE;
use crate::splitter::Splitter;
use crate::types::Chunk;

/// Splits files into fixed-size character chunks on line boundaries.
///
/// Every file extension is supported — this is the catch-all fallback
/// at the end of the [`SplitterChain`](super::SplitterChain).
pub(crate) struct FixedSizeSplitter {
    /// Target chunk size in characters.
    chunk_size: usize,
}

impl FixedSizeSplitter {
    /// Create a new splitter with the default chunk size.
    pub(crate) const fn new() -> Self {
        Self { chunk_size: FALLBACK_CHUNK_SIZE }
    }
}

impl Splitter for FixedSizeSplitter {
    fn supports(&self, _extension: &str) -> bool {
        // Catch-all fallback — supports every extension
        true
    }

    fn split(&self, content: &str, _path: &Path) -> Vec<Chunk> {
        if content.is_empty() {
            return Vec::new();
        }

        let mut chunks = Vec::new();
        let mut chunk_start_char: u32 = 0;
        let mut chunk_start_line: u32 = 1;
        let mut current_line: u32 = 1;
        let mut current_pos: usize = 0;
        let mut buf = String::new();

        for line in content.split_inclusive('\n') {
            let line_len = line.len();

            // If adding this line would exceed the chunk size AND the buffer
            // is non-empty, flush the current chunk first.
            if !buf.is_empty() && buf.len().saturating_add(line_len) > self.chunk_size {
                let chunk_end_line = current_line.saturating_sub(1);
                let chunk_end_char = current_pos.try_into().unwrap_or(u32::MAX);

                chunks.push(Chunk {
                    content: buf.clone(),
                    kind: "raw".to_string(),
                    name: String::new(),
                    line_start: chunk_start_line,
                    line_end: chunk_end_line,
                    char_start: chunk_start_char,
                    char_end: chunk_end_char,
                });

                buf.clear();
                chunk_start_char = current_pos.try_into().unwrap_or(u32::MAX);
                chunk_start_line = current_line;
            }

            buf.push_str(line);
            current_pos = current_pos.saturating_add(line_len);
            current_line = current_line.saturating_add(1);
        }

        // Flush remaining content
        if !buf.is_empty() {
            let chunk_end_line = current_line.saturating_sub(1);
            let chunk_end_char = current_pos.try_into().unwrap_or(u32::MAX);

            chunks.push(Chunk {
                content: buf,
                kind: "raw".to_string(),
                name: String::new(),
                line_start: chunk_start_line,
                line_end: chunk_end_line,
                char_start: chunk_start_char,
                char_end: chunk_end_char,
            });
        }

        chunks
    }
}
