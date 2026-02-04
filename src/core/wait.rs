//! Wait logic for ensuring panels are loaded before continuing stream.
//!
//! When the LLM uses tools like file_open, we need to wait for the background
//! cache system to load the file content before continuing the stream.

use std::io;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;

use crate::cache::CacheUpdate;
use crate::panels::now_ms;
use crate::state::{ContextType, State};
use crate::ui;

/// Wait for dirty file panels to be loaded before continuing stream.
/// This ensures the LLM has access to newly opened file content.
///
/// Returns immediately if no file panels are dirty.
/// Has a 5 second timeout to avoid hanging indefinitely.
pub fn wait_for_panels(
    state: &mut State,
    cache_rx: &Receiver<CacheUpdate>,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    process_cache_updates: impl Fn(&mut State, &Receiver<CacheUpdate>),
) {
    // Check if any File panels are dirty - if not, return immediately
    if !has_dirty_file_panels(state) {
        return;
    }

    // Set flag for UI indicator
    state.waiting_for_panels = true;
    state.dirty = true;

    let timeout = Duration::from_secs(5);
    let start = Instant::now();
    let mut last_render_ms = 0u64;

    loop {
        if !has_dirty_file_panels(state) {
            break; // All file panels loaded
        }

        if start.elapsed() > timeout {
            break; // Timeout - continue anyway
        }

        // Process any pending cache updates
        process_cache_updates(state, cache_rx);

        // Update spinner and redraw periodically
        let current_ms = now_ms();
        if current_ms.saturating_sub(last_render_ms) >= 50 {
            state.spinner_frame = state.spinner_frame.wrapping_add(1);
            let _ = terminal.draw(|frame| {
                ui::render(frame, state);
            });
            last_render_ms = current_ms;
        }

        // Small sleep to avoid busy-waiting
        std::thread::sleep(Duration::from_millis(10));
    }

    // Clear flag
    state.waiting_for_panels = false;
    state.dirty = true;
}

/// Check if any File context panels have cache_deprecated = true
fn has_dirty_file_panels(state: &State) -> bool {
    state.context.iter().any(|c| {
        c.context_type == ContextType::File && c.cache_deprecated
    })
}
