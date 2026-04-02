//! IR-to-ratatui adapter and frame builders.
//!
//! This module contains:
//! - **Sub-builders** (`sidebar`, `status_bar`, `panel`, `build_frame`) that
//!   assemble [`cp_render::frame::Frame`] from application state.
//! - **Adapter** (`blocks_to_lines`) that converts IR blocks into ratatui
//!   `Line` vectors for the existing panel renderer.

/// Conversation region builder: messages, history, streaming tools, overlays.
mod conversation;
/// Conversation adapter: renders conversation → ratatui with scrollbar + caching.
pub(crate) mod render_conversation;
/// Panel IR adapter: renders [`PanelContent`] → bordered scrollable widget.
pub(crate) mod render_panel;
/// Sidebar adapter: renders [`cp_render::frame::Sidebar`] → ratatui.
pub(crate) mod render_sidebar;
/// Status bar adapter: renders [`cp_render::frame::StatusBar`] → ratatui.
pub(crate) mod render_status_bar;
/// Sidebar region builder.
mod sidebar;
/// Status bar region builder.
mod status_bar;

use cp_render::{Align, Semantic, Span as IrSpan, TreeNode};
use ratatui::prelude::{Line, Span, Style};
use ratatui::style::Modifier;

use super::theme;

// ── Semantic → Style ─────────────────────────────────────────────────

/// Map an IR semantic token to a concrete ratatui [`Style`].
fn semantic_to_style(semantic: Semantic) -> Style {
    match semantic {
        Semantic::Accent | Semantic::Active | Semantic::KeyHint | Semantic::Header => {
            Style::default().fg(theme::accent())
        }
        Semantic::AccentDim | Semantic::Info => Style::default().fg(theme::accent_dim()),
        Semantic::Muted => Style::default().fg(theme::text_muted()),
        Semantic::Success | Semantic::DiffAdd => Style::default().fg(theme::success()),
        Semantic::Warning => Style::default().fg(theme::warning()),
        Semantic::Error | Semantic::DiffRemove => Style::default().fg(theme::error()),
        Semantic::Code => Style::default().fg(theme::text_secondary()),
        Semantic::Border => Style::default().fg(theme::border()),
        // Default, Bold, and any future non-exhaustive variants.
        Semantic::Default | Semantic::Bold | _ => Style::default().fg(theme::text()),
    }
}

/// Convert a single IR span to a ratatui `Span`.
fn ir_span_to_ratatui(ir: &IrSpan) -> Span<'static> {
    let mut style = if let Some((r, g, b)) = ir.color {
        // Raw RGB override — syntax highlighting bypass
        Style::default().fg(ratatui::style::Color::Rgb(r, g, b))
    } else {
        semantic_to_style(ir.semantic)
    };
    if ir.bold || matches!(ir.semantic, Semantic::Bold | Semantic::Active | Semantic::KeyHint | Semantic::Header) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if ir.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if ir.dimmed {
        style = style.add_modifier(Modifier::DIM);
    }
    Span::styled(ir.text.clone(), style)
}

// ── Block → Lines ────────────────────────────────────────────────────

/// Convert a sequence of IR blocks into ratatui lines.
///
/// This is the main entry point for panel rendering through the IR
/// pipeline. Called by `render_panel_default` when `panel.blocks()`
/// returns a non-empty result.
#[must_use]
pub(crate) fn blocks_to_lines(blocks: &[cp_render::Block]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for block in blocks {
        render_block(block, &mut lines);
    }
    lines
}

/// Render a single block into one or more lines.
fn render_block(block: &cp_render::Block, lines: &mut Vec<Line<'static>>) {
    match block {
        cp_render::Block::Line(spans) | cp_render::Block::Header(spans) => {
            lines.push(Line::from(spans.iter().map(ir_span_to_ratatui).collect::<Vec<_>>()));
        }
        cp_render::Block::Table { columns, rows } => {
            render_table(columns, rows, lines);
        }
        cp_render::Block::ProgressBar { segments, label } => {
            render_progress_bar(segments, label.as_deref(), lines);
        }
        cp_render::Block::Tree(nodes) => {
            for node in nodes {
                render_tree_node(node, 0, lines);
            }
        }
        cp_render::Block::Separator => {
            lines.push(Line::from(Span::styled(
                "────────────────────────────────────────",
                semantic_to_style(Semantic::Border),
            )));
        }
        cp_render::Block::KeyValue(pairs) => {
            for (key, value) in pairs {
                let mut spans: Vec<Span<'static>> = key.iter().map(ir_span_to_ratatui).collect();
                spans.push(Span::raw("  "));
                spans.extend(value.iter().map(ir_span_to_ratatui));
                lines.push(Line::from(spans));
            }
        }
        // Empty, and any future block variants — render as empty line.
        cp_render::Block::Empty | _ => {
            lines.push(Line::from(""));
        }
    }
}

// ── Table rendering ──────────────────────────────────────────────────

/// Render a table block as aligned text lines.
///
/// Computes column widths from headers + data, then renders each row
/// with appropriate padding and alignment.
fn render_table(columns: &[cp_render::Column], rows: &[Vec<cp_render::Cell>], lines: &mut Vec<Line<'static>>) {
    if columns.is_empty() {
        return;
    }

    let col_count = columns.len();
    let mut widths: Vec<usize> = columns.iter().map(|c| c.header.len()).collect();

    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                let cell_len: usize = cell.spans.iter().map(|s| s.text.len()).sum();
                if cell_len > *w {
                    *w = cell_len;
                }
            }
        }
    }

    // Render header row (if any column has a non-empty header).
    let has_headers = columns.iter().any(|c| !c.header.is_empty());
    if has_headers {
        let mut spans = Vec::new();
        for (i, col) in columns.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(0);
            let padded = pad_str(&col.header, w, col.align);
            spans.push(Span::styled(padded, semantic_to_style(Semantic::Header)));
            if i < col_count.saturating_sub(1) {
                spans.push(Span::raw("  "));
            }
        }
        lines.push(Line::from(spans));
    }

    // Render data rows.
    for row in rows {
        let mut spans = Vec::new();
        for (i, cell) in row.iter().enumerate() {
            let Some(col) = columns.get(i) else { break };
            let w = widths.get(i).copied().unwrap_or(0);
            let align = if cell.align == Align::Left { col.align } else { cell.align };
            let content: String = cell.spans.iter().map(|s| s.text.as_str()).collect();
            let padding = w.saturating_sub(content.len());

            match align {
                Align::Right => {
                    spans.push(Span::raw(" ".repeat(padding)));
                    spans.extend(cell.spans.iter().map(ir_span_to_ratatui));
                }
                Align::Center => {
                    let (left_pad, right_pad) = center_padding(padding);
                    spans.push(Span::raw(" ".repeat(left_pad)));
                    spans.extend(cell.spans.iter().map(ir_span_to_ratatui));
                    spans.push(Span::raw(" ".repeat(right_pad)));
                }
                Align::Left => {
                    spans.extend(cell.spans.iter().map(ir_span_to_ratatui));
                    spans.push(Span::raw(" ".repeat(padding)));
                }
            }

            if i < col_count.saturating_sub(1) {
                spans.push(Span::raw("  "));
            }
        }
        lines.push(Line::from(spans));
    }
}

/// Split total padding into (left, right) halves for centre alignment.
///
/// Routes through [`time_arith::div_const`] to satisfy the
/// `integer_division_remainder_used` lint.
const fn center_padding(total: usize) -> (usize, usize) {
    let left = cp_base::panels::time_arith::div_const::<2>(total);
    let right = total.saturating_sub(left);
    (left, right)
}

/// Pad a string to a given width with the specified alignment.
fn pad_str(s: &str, width: usize, align: Align) -> String {
    let padding = width.saturating_sub(s.len());
    match align {
        Align::Left => format!("{s}{}", " ".repeat(padding)),
        Align::Right => format!("{}{s}", " ".repeat(padding)),
        Align::Center => {
            let (left, right) = center_padding(padding);
            format!("{}{s}{}", " ".repeat(left), " ".repeat(right))
        }
    }
}

// ── Progress bar rendering ───────────────────────────────────────────

/// Render a progress bar as a single styled line.
fn render_progress_bar(segments: &[cp_render::ProgressSegment], label: Option<&str>, lines: &mut Vec<Line<'static>>) {
    // Fixed 40-char bar width.
    const BAR_WIDTH: usize = 40;
    let mut spans = Vec::new();
    spans.push(Span::styled("[", semantic_to_style(Semantic::Border)));

    let mut filled: usize = 0;
    for seg in segments {
        let seg_chars =
            cp_base::panels::time_arith::div_const::<100>(usize::from(seg.percent).saturating_mul(BAR_WIDTH));
        let seg_chars = seg_chars.min(BAR_WIDTH.saturating_sub(filled));
        if seg_chars > 0 {
            spans.push(Span::styled("█".repeat(seg_chars), semantic_to_style(seg.semantic)));
            filled = filled.saturating_add(seg_chars);
        }
    }
    if filled < BAR_WIDTH {
        spans.push(Span::styled("░".repeat(BAR_WIDTH.saturating_sub(filled)), semantic_to_style(Semantic::Muted)));
    }

    spans.push(Span::styled("]", semantic_to_style(Semantic::Border)));
    if let Some(lbl) = label {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(lbl.to_owned(), semantic_to_style(Semantic::Muted)));
    }
    lines.push(Line::from(spans));
}

// ── Tree rendering ───────────────────────────────────────────────────

/// Render a tree node with indentation.
fn render_tree_node(node: &TreeNode, depth: usize, lines: &mut Vec<Line<'static>>) {
    let indent = "  ".repeat(depth);
    let mut spans = vec![Span::raw(indent)];
    spans.extend(node.label.iter().map(ir_span_to_ratatui));
    lines.push(Line::from(spans));

    if node.expanded {
        for child in &node.children {
            render_tree_node(child, depth.saturating_add(1), lines);
        }
    }
}

// ── Frame builder ────────────────────────────────────────────────────

use cp_render::frame::{Frame as IrFrame, PanelContent};

use crate::app::panels;
use crate::state::State;
use cp_base::panels::now_ms;

/// Build a complete frame snapshot from application state.
///
/// Called once per render tick. Returns a pure-data `Frame` with no
/// ratatui dependencies — the adapter converts it to terminal widgets.
#[must_use]
pub(crate) fn build_frame(state: &State) -> IrFrame {
    let sidebar = sidebar::build_sidebar(state);
    let status_bar = status_bar::build_status_bar(state);
    let active_panel = build_active_panel(state);

    let conversation = conversation::build_conversation(state);
    let overlays = conversation::build_overlays(state);

    IrFrame { sidebar, active_panel, status_bar, conversation, overlays }
}

/// Build the active panel content from application state.
///
/// Calls `blocks()` on the panel for the currently selected context element.
/// Returns a [`PanelContent`] with title, blocks, and optional refresh timestamp.
#[must_use]
fn build_active_panel(state: &State) -> PanelContent {
    let context_type = state.context.get(state.selected_context).map_or_else(
        || cp_base::state::context::Kind::new(cp_base::state::context::Kind::CONVERSATION),
        |c| c.context_type.clone(),
    );

    let panel = panels::get_panel(&context_type);
    let title = panel.title(state);
    let blocks = panel.blocks(state);

    // Build "refreshed N ago" for dynamic panels
    let refreshed_ago =
        state.context.get(state.selected_context).filter(|ctx| !ctx.context_type.is_fixed()).and_then(|ctx| {
            let ts = ctx.last_refresh_ms;
            if ts < 1_577_836_800_000 {
                return None;
            }
            let now = now_ms();
            if now <= ts {
                return None;
            }
            Some(crate::ui::helpers::format_time_ago(now.saturating_sub(ts)))
        });

    PanelContent { title, blocks, refreshed_ago }
}
