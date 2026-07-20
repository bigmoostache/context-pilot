//! IR-to-ratatui adapter and frame builders.
//!
//! This module contains:
//! - **Sub-builders** (`sidebar`, `status_bar`, `panel`, `build_frame`) that
//!   assemble [`cp_render::frame::Frame`] from application state.
//! - **Adapter** (`blocks_to_lines`) that converts IR blocks into ratatui
//!   `Line` vectors for the existing panel renderer.

/// Progress bar animation: smooth fill, color crossfade, streaming pulse.
pub(super) mod bar_animation;
/// Conversation region builder: messages, history, streaming tools, overlays.
mod conversation;
/// Conversation adapter: renders conversation → ratatui with scrollbar + caching.
pub(crate) mod render_conversation;
/// Panel IR adapter: renders [`PanelContent`] → bordered scrollable widget.
/// (Merged from `render_panel.rs` — kept inline to stay under the 8-entry dir cap.)
pub(crate) mod render_panel {
    use cp_render::frame::PanelContent;
    use ratatui::prelude::{Frame, Line, Rect, Span, Style};
    use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

    use crate::state::State;
    use crate::ui::{helpers::count_wrapped_lines, theme};
    use cp_base::cast::Safe as _;

    /// Render the active panel from its IR snapshot.
    pub(crate) fn render_panel_from_ir(
        frame: &mut Frame<'_>,
        state: &mut State,
        area: Rect,
        panel_content: &PanelContent,
    ) {
        let base_style = Style::default().bg(theme::bg_surface());

        let inner_area = Rect::new(area.x.saturating_add(1), area.y, area.width.saturating_sub(2), area.height);

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::border()))
            .style(base_style)
            .title(Span::styled(format!(" {} ", panel_content.title), Style::default().fg(theme::accent()).bold()));

        if let Some(bottom) = panel_content.refreshed_ago.as_ref() {
            block = block.title_bottom(Span::styled(format!(" {bottom} "), Style::default().fg(theme::text_muted())));
        }

        let content_area = block.inner(inner_area);
        frame.render_widget(block, inner_area);

        // Resolve content from IR blocks
        let text: Vec<Line<'static>> =
            if panel_content.blocks.is_empty() { Vec::new() } else { super::blocks_to_lines(&panel_content.blocks) };

        // Calculate scroll bounds from wrapped content height
        let viewport_width = content_area.width.to_usize();
        let viewport_height = content_area.height.to_usize();
        let content_height: usize = {
            let _guard = crate::profile!("panel::scroll_calc");
            text.iter().map(|line| count_wrapped_lines(line, viewport_width)).sum()
        };
        let max_scroll = content_height.saturating_sub(viewport_height).to_f32();
        state.max_scroll = max_scroll;
        state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

        let paragraph = {
            let _guard = crate::profile!("panel::paragraph_new");
            Paragraph::new(text)
                .style(base_style)
                .wrap(Wrap { trim: false })
                .scroll((state.scroll_offset.round().to_u16(), 0))
        };

        {
            let _guard = crate::profile!("panel::frame_render");
            frame.render_widget(paragraph, content_area);
        }
    }
}
/// Sidebar adapter: renders [`cp_render::frame::Sidebar`] → ratatui.
pub(crate) mod render_sidebar;
/// Token statistics sub-module for sidebar (rounded-border table).
mod render_sidebar_stats;
/// Status bar adapter: renders [`cp_render::frame::StatusBar`] → ratatui.
pub(crate) mod render_status_bar;
/// Sidebar region builder.
mod sidebar;

use cp_render::{Align, Semantic, Span as IrSpan, TreeNode};
use ratatui::prelude::{Line, Span, Style};
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr as _;

use super::theme;

// ── Semantic → Style ─────────────────────────────────────────────────

/// Map an IR semantic token to a concrete ratatui [`Style`].
pub(crate) fn semantic_to_style(semantic: Semantic) -> Style {
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
    if ir.reversed {
        style = style.add_modifier(Modifier::REVERSED);
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
    use cp_render::Block as B;
    match block.clone() {
        B::Line(spans) | B::Header(spans) => {
            lines.push(Line::from(spans.iter().map(ir_span_to_ratatui).collect::<Vec<_>>()));
        }
        B::Table { columns, rows } => render_table(&columns, &rows, lines),
        B::ProgressBar { segments, label } => render_progress_bar(&segments, label.as_deref(), lines),
        B::Tree(nodes) => {
            for node in &nodes {
                render_tree_node(node, 0, lines);
            }
        }
        B::Separator => lines.push(Line::from(Span::styled(
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            semantic_to_style(Semantic::Border),
        ))),
        B::KeyValue(pairs) => {
            for pair in &pairs {
                let (key, value) = (&pair.0, &pair.1);
                let mut spans: Vec<Span<'static>> = key.iter().map(ir_span_to_ratatui).collect();
                spans.push(Span::raw("  "));
                spans.extend(value.iter().map(ir_span_to_ratatui));
                lines.push(Line::from(spans));
            }
        }
        // Empty, and any future block variants — render as empty line.
        B::Empty | _ => lines.push(Line::from("")),
    }
}

// ── Table rendering ──────────────────────────────────────────────────

/// Render a table block as aligned text lines with full box borders.
///
/// Computes column widths from headers + data, then renders each row
/// inside a rounded-corner box with `│` column separators and `─┼─`
/// row separators — matching the conversation panel's markdown table style.
fn render_table(columns: &[cp_render::Column], rows: &[Vec<cp_render::Cell>], lines: &mut Vec<Line<'static>>) {
    if columns.is_empty() {
        return;
    }

    let border_style = semantic_to_style(Semantic::Border);
    let widths = compute_table_widths(columns, rows);

    // Top border: ╭───┬───┬───╮
    render_border_row(&widths, ("\u{256d}", "\u{252c}", "\u{256e}"), border_style, lines);

    // Render header row (if any column has a non-empty header).
    if columns.iter().any(|c| !c.header.is_empty()) {
        render_header_row(columns, &widths, border_style, lines);
        // Header/data separator row: ├─────┼───────┼─────┤
        render_border_row(&widths, ("\u{251c}", "\u{253c}", "\u{2524}"), border_style, lines);
    }

    // Render data rows with thin separators between them.
    let row_ctx = RowCtx { columns, widths: &widths, border_style };
    for (row_idx, row) in rows.iter().enumerate() {
        render_data_row(&row_ctx, row, lines);

        // Thin separator between data rows (not after the last row).
        if row_idx < rows.len().saturating_sub(1) {
            render_border_row(&widths, ("\u{251c}", "\u{253c}", "\u{2524}"), border_style, lines);
        }
    }

    // Bottom border: ╰───┴───┴───╯
    render_border_row(&widths, ("\u{2570}", "\u{2534}", "\u{256f}"), border_style, lines);
}

/// Compute per-column display widths: max of the header width and every data
/// cell's rendered width in that column.
fn compute_table_widths(columns: &[cp_render::Column], rows: &[Vec<cp_render::Cell>]) -> Vec<usize> {
    let mut widths: Vec<usize> = columns.iter().map(|c| c.header.width()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                let cell_len: usize = cell.spans.iter().map(|s| s.text.width()).sum();
                if cell_len > *w {
                    *w = cell_len;
                }
            }
        }
    }
    widths
}

/// Render the header row: `│ Header │ Header │` with per-column alignment.
fn render_header_row(
    columns: &[cp_render::Column],
    widths: &[usize],
    border_style: Style,
    lines: &mut Vec<Line<'static>>,
) {
    let mut spans = vec![Span::styled("\u{2502} ", border_style)];
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" \u{2502} ", border_style));
        }
        let w = widths.get(i).copied().unwrap_or(0);
        let padded = pad_str(&col.header, w, col.align);
        spans.push(Span::styled(padded, semantic_to_style(Semantic::Header)));
    }
    spans.push(Span::styled(" \u{2502}", border_style));
    lines.push(Line::from(spans));
}

/// Shared column layout + border style for rendering table data rows.
struct RowCtx<'ctx> {
    /// Column definitions (headers + alignment).
    columns: &'ctx [cp_render::Column],
    /// Computed per-column display widths.
    widths: &'ctx [usize],
    /// Border/separator style.
    border_style: Style,
}

/// Render one data row: `│ cell │ cell │` with per-cell alignment.
fn render_data_row(ctx: &RowCtx<'_>, row: &[cp_render::Cell], lines: &mut Vec<Line<'static>>) {
    let mut spans = vec![Span::styled("\u{2502} ", ctx.border_style)];
    for (i, cell) in row.iter().enumerate() {
        let Some(col) = ctx.columns.get(i) else { break };
        if i > 0 {
            spans.push(Span::styled(" \u{2502} ", ctx.border_style));
        }
        let w = ctx.widths.get(i).copied().unwrap_or(0);
        let align = if cell.align == Align::Left { col.align } else { cell.align };
        let content: String = cell.spans.iter().map(|s| s.text.as_str()).collect();
        let padding = w.saturating_sub(content.width());
        push_cell_spans(&mut spans, cell, align, padding);
    }
    spans.push(Span::styled(" \u{2502}", ctx.border_style));
    lines.push(Line::from(spans));
}

/// Push one aligned cell's spans (with padding) into a row's span list.
fn push_cell_spans(spans: &mut Vec<Span<'static>>, cell: &cp_render::Cell, align: Align, padding: usize) {
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
}

/// Render a horizontal border row: `left───mid───right` (e.g. `╭───┬───╮`).
fn render_border_row(
    widths: &[usize],
    (left, mid, right): (&str, &str, &str),
    border_style: Style,
    lines: &mut Vec<Line<'static>>,
) {
    let mut spans = vec![Span::styled(left.to_owned(), border_style)];
    for (i, width) in widths.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(mid.to_owned(), border_style));
        }
        // +2 for the space padding on each side of cell content
        spans.push(Span::styled("\u{2500}".repeat(width.saturating_add(2)), border_style));
    }
    spans.push(Span::styled(right.to_owned(), border_style));
    lines.push(Line::from(spans));
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
    let padding = width.saturating_sub(s.width());
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
        let seg_chars_raw =
            cp_base::panels::time_arith::div_const::<100>(usize::from(seg.percent).saturating_mul(BAR_WIDTH));
        let seg_chars = seg_chars_raw.min(BAR_WIDTH.saturating_sub(filled));
        if seg_chars > 0 {
            spans.push(Span::styled("\u{2588}".repeat(seg_chars), semantic_to_style(seg.semantic)));
            filled = filled.saturating_add(seg_chars);
        }
    }
    if filled < BAR_WIDTH {
        spans.push(Span::styled(
            "\u{2591}".repeat(BAR_WIDTH.saturating_sub(filled)),
            semantic_to_style(Semantic::Muted),
        ));
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
    let status_bar = render_status_bar::build_status_bar(state);
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
