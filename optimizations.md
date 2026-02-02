# UX Optimizations

## High Impact

### 1. Dirty Flag Rendering ✅ IMPLEMENTED
Only re-render when state changes. Saves CPU by avoiding widget tree construction when nothing changed.

**Implementation:**
- `dirty: bool` field in `State`
- Main loop only calls `terminal.draw()` when `dirty == true`
- All state mutations set `dirty = true`:
  - Stream events (chunks, tool use, done, error)
  - Typewriter character release
  - Cleaning events
  - TL;DR results
  - Tool execution
  - All user actions

Note: Ratatui already does cell-level diffing, so the dirty flag's main benefit is avoiding widget tree construction.

### 2. Async Tool Execution ✅ IMPLEMENTED
Tools block the main thread. Move I/O-heavy tools to background:

**Implementation:**
- Tools create context elements with `cached_content: None` and `cache_deprecated: true`
- Background cache system populates content asynchronously
- Panels show "Loading..." placeholder while content is being fetched
- `check_timer_based_deprecation()` triggers initial refresh for new elements

Converted tools:
- `open_file` - No longer reads file content synchronously
- `glob` - No longer walks directory synchronously
- `grep` - No longer searches files synchronously
- `create_tmux_pane` - Already async (captures content via background timer)

### 3. Optimistic UI Updates
Show user message immediately, don't wait for stream to start:

```rust
// Currently: User presses Enter → API call starts → message appears
// Better: User presses Enter → message appears instantly → API call starts
```

### 4. Virtualized Scrolling
For long conversations, only render visible messages:

```rust
// Calculate visible range based on scroll position
let visible_range = calculate_visible_messages(scroll_offset, viewport_height);
for msg in &messages[visible_range] {
    render_message(msg);
}
```

## Medium Impact

### 5. Debounced Syntax Highlighting
Cache highlighted output, only re-highlight on content change:

```rust
struct HighlightCache {
    content_hash: u64,
    highlighted: Vec<Line>,
}
```

### 6. Typewriter Tuning
Current settings feel slightly sluggish. Consider:

```rust
// Faster minimum delay
pub const TYPEWRITER_MIN_DELAY_MS: f64 = 2.0;  // was 5.0

// Faster catch-up when stream is done
if self.stream_done {
    chars_to_release.max(10)  // was 2
}
```

### 7. Input Echo Latency ✅ IMPLEMENTED
Ensure keystroke → screen is always < 16ms:

**Implementation:**
- Main loop now processes input FIRST with non-blocking `poll(Duration::ZERO)`
- If input is available, handle it and render immediately
- Background processing (streams, cache, etc.) happens AFTER input is on screen
- Blocking poll at end waits for next event without consuming CPU

```rust
loop {
    // Input first - non-blocking check
    if event::poll(Duration::ZERO)? {
        handle_input();
        render();  // Immediate feedback
    }
    // Background processing
    process_streams();
    process_cache();
    // ...
    render();  // If dirty from background
    // Wait for next event
    event::poll(Duration::from_millis(EVENT_POLL_MS))?;
}
```

### 8. Progressive Context Loading
Load file contents lazily, show skeleton first:

```rust
// Instead of blocking on file read:
ContextElement {
    content: ContentState::Loading,  // Show placeholder
}
// Background thread loads, updates to ContentState::Ready(...)
```

## Low Impact (Polish)

### 9. Smooth Scrolling
Animate scroll position instead of jumping:

```rust
// Lerp toward target scroll position each frame
scroll_position += (target_scroll - scroll_position) * 0.3;
```

### 10. Loading Indicators ✅ IMPLEMENTED
Visual feedback during operations:

**Implementation:**
- Added `spinner_frame: u64` to State for animation timing
- Created `src/ui/spinner.rs` with braille spinner animation
- Spinner shown in status bar badges during: STREAMING, CLEANING, SUMMARIZING, LOADING
- Spinner shown next to context items in sidebar while loading (instead of token count)
- Input box title shows animated spinner during streaming
- `update_spinner_animation()` increments frame and triggers re-render during active operations

Locations with spinners:
- Status bar: All active operation badges
- Sidebar: Context items loading content
- Input box: Title during streaming

### 11. Input Buffering
Queue keystrokes during blocking operations, replay after:

```rust
input_buffer: VecDeque<Event>,
// During tool execution, buffer input
// After completion, process buffered events
```

---

**Biggest wins**: #1 (dirty rendering) and #2 (async tools) would have the most noticeable impact.
