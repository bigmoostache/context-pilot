# Intermediate Rendering (IR) Architecture

## Motivation

Context Pilot's TUI rendering is tightly coupled to Ratatui's `Line`/`Span` types.
Every panel's `content()` method returns `Vec<Line<'static>>` — a terminal-specific type
that bakes styling directly into ANSI-compatible constructs. The sidebar, status bar,
and conversation renderer are similarly hardcoded to Ratatui widgets.

This coupling means the ~80% of the codebase that is frontend-agnostic (State, tools,
LLM streaming, modules, tool execution) cannot be reused by a web or native GUI frontend
without reimplementing all rendering from scratch.

## Goal

Introduce an **Intermediate Rendering (IR)** layer that decouples "what to display" from
"how to display it". The IR is a full-frame snapshot of the entire UI state, emitted once
per render tick. Any frontend (TUI, web, native GUI) consumes this snapshot and renders it
using its own widget toolkit.

## Design Principles

1. **Full-frame snapshot** — The IR describes the COMPLETE screen state in one struct.
   No incremental diffs, no retained state. Each tick produces a fresh `Frame`.

2. **Semantic styling** — The IR carries style *intent* (Error, Accent, Muted), never
   concrete colors (RGB, ANSI). Each frontend maps semantic tokens to its own palette.

3. **Typed regions** — Screen regions with fixed structure (sidebar, status bar) have
   dedicated typed structs. Only panel content uses a generic `Block` enum.

4. **Serialize-ready** — All IR types derive `Serialize`. A web backend can do
   `serde_json::to_string(&frame)` and push it over a WebSocket with zero adapter code.

5. **Extensible blocks** — The `Block` enum uses `#[non_exhaustive]` so new variants
   can be added without breaking existing frontends (they skip unknown variants).

## Architecture

```
State (runtime data, tools, LLM, modules)
  │
  ▼
build_frame(state) → cp_render::Frame
  │
  ├── TUI adapter: Frame → ratatui Line/Span → crossterm → terminal
  ├── Web adapter: Frame → JSON → WebSocket → React/Svelte components
  └── GUI adapter: Frame → egui/iced widgets → native window
```

## IR Type Hierarchy

### Top-Level

```rust
pub struct Frame {
    pub sidebar: Sidebar,
    pub active_panel: PanelContent,
    pub status_bar: StatusBar,
    pub conversation: Conversation,
    pub overlays: Vec<Overlay>,
}
```

### Shared Primitives

```rust
/// Semantic style token — frontend maps to actual visual style.
pub enum Semantic {
    Default,        // Normal text
    Accent,         // Primary highlight
    AccentDim,      // Subdued accent
    Success,        // Positive / green
    Warning,        // Caution / orange
    Error,          // Negative / red
    Text,           // Standard foreground
    TextSecondary,  // Slightly dimmed
    TextMuted,      // Significantly dimmed
    Border,         // Structural separators
    BorderMuted,    // Lighter separators
    BgBase,         // Base background
    BgSurface,      // Surface background
    BgElevated,     // Elevated background
    Code,           // Inline code
    Assistant,      // Assistant messages
}

/// A styled text fragment.
pub struct Span {
    pub text: String,
    pub semantic: Semantic,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Column alignment.
pub enum Align { Left, Right }

/// A table cell.
pub struct Cell {
    pub text: String,
    pub semantic: Semantic,
    pub align: Align,
    pub bold: bool,
}

/// Table column header.
pub struct Column {
    pub label: String,
    pub align: Align,
}
```

### Block Enum (Panel Content)

```rust
pub enum Block {
    /// A line of styled spans.
    Line(Vec<Span>),
    /// An empty line.
    Empty,
    /// A section header.
    Header { text: String, level: u8 },
    /// A structured table.
    Table {
        columns: Vec<Column>,
        rows: Vec<Vec<Cell>>,
        footer: Option<Vec<Cell>>,
    },
    /// A progress bar.
    ProgressBar {
        segments: Vec<ProgressSegment>,
        total: f64,
        threshold: Option<f64>,
    },
    /// A tree view.
    Tree { roots: Vec<TreeNode> },
    /// A horizontal rule.
    Separator,
    /// A key-value pair.
    KeyValue { key: Vec<Span>, value: Vec<Span> },
}
```

### Sidebar

```rust
pub struct Sidebar {
    pub mode: SidebarMode,
    pub entries: Vec<SidebarEntry>,
    pub token_bar: TokenBar,
    pub pr_card: Option<PrCard>,
    pub token_stats: TokenStats,
    pub help_hints: Vec<HelpHint>,
}
```

### Status Bar

```rust
pub struct StatusBar {
    pub primary_badge: Badge,
    pub retry_badge: Option<Badge>,
    pub loading_badge: Option<Badge>,
    pub provider: String,
    pub model: String,
    pub stop_reason: Option<StopReason>,
    pub agent: Option<AgentCard>,
    pub skills: Vec<SkillCard>,
    pub git_branch: Option<String>,
    pub git_changes: Option<GitChanges>,
    pub auto_continue: AutoContinue,
    pub reveries: Vec<ReverieCard>,
    pub queue: Option<QueueCard>,
    pub char_count: usize,
}
```

### Conversation

```rust
pub struct Conversation {
    pub history_sections: Vec<HistorySection>,
    pub messages: Vec<ConversationMessage>,
    pub streaming_tool: Option<StreamingTool>,
    pub input: InputArea,
    pub is_streaming: bool,
}
```

### Overlays

```rust
pub enum Overlay {
    QuestionForm(QuestionForm),
    Autocomplete(Autocomplete),
    ConfigOverlay(Vec<Block>),
    PerfOverlay(Vec<Block>),
}
```

## Panel Trait Change

The `Panel` trait's primary rendering method changes:

```rust
// Before (ratatui-coupled)
fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>>;

// After (IR-based)
fn blocks(&self, state: &State) -> Vec<cp_render::Block>;
```

The `base_style` parameter is removed — the IR carries semantic tokens, not styles.
The `render()` override method is also removed — the TUI adapter handles all rendering.

## Centralized Frame Builder

```rust
// src/ir/mod.rs
pub fn build_frame(state: &State) -> cp_render::Frame {
    Frame {
        sidebar: build_sidebar(state),
        active_panel: build_active_panel(state),
        status_bar: build_status_bar(state),
        conversation: build_conversation(state),
        overlays: build_overlays(state),
    }
}
```

Each sub-builder extracts data from `State` and produces its typed IR struct.
Panel content comes from `panel.blocks(state)` via the Panel trait.

## TUI Adapter

The TUI adapter converts `Frame` → ratatui widgets:

```rust
// src/ui/ir_adapter.rs
pub fn render_frame(frame: &cp_render::Frame, ratatui_frame: &mut Frame, area: Rect) {
    render_sidebar(&frame.sidebar, ratatui_frame, sidebar_area);
    render_panel(&frame.active_panel, ratatui_frame, content_area);
    render_status_bar(&frame.status_bar, ratatui_frame, status_area);
    render_conversation(&frame.conversation, ratatui_frame, content_area);
    render_overlays(&frame.overlays, ratatui_frame, area);
}
```

The `Semantic → Color` mapping lives here, not in the IR.

## Migration Strategy

**Big bang:** All ~30 panels rewrite `content()` → `blocks()` in one PR. The centralized
builder and TUI adapter are implemented simultaneously. Old rendering code is deleted.

## File Impact Summary

| Area | Files affected | Nature of change |
|------|---------------|-----------------|
| New crate | `crates/cp-render/` | ~500 lines of type definitions |
| Panel trait | `crates/cp-base/src/panels.rs` | Method signature change |
| Panel impls | ~23 panel.rs files across module crates | `content()` → `blocks()` |
| Frame builder | `src/ir/mod.rs` (new) | ~400 lines, centralized |
| TUI adapter | `src/ui/ir_adapter.rs` (new) | ~600 lines, replaces old render |
| UI entry point | `src/ui/mod.rs` | Rewrite to use IR |
| Sidebar | `src/ui/sidebar/*.rs` | Replaced by adapter |
| Status bar | `src/ui/input.rs` | Replaced by adapter |
| Conversation | `src/modules/conversation/panel.rs` | Major rewrite |

## Non-Goals (This PR)

- Web frontend implementation (just the IR + TUI adapter)
- GUI frontend implementation
- Changing State, LLM streaming, tool execution, or module system
- Performance optimization of the IR (optimize later if needed)
