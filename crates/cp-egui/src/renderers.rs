//! Block renderers — convert [`cp_render::Block`] variants into egui widgets.
//!
//! Each `Block` variant gets a dedicated render function. The central
//! [`render_blocks`] dispatcher matches on the variant and delegates.

use cp_render::{Align, Block, Cell, KeyValuePair, ProgressSegment, Semantic, Span, TreeNode};
use eframe::egui::{self, Color32, RichText, Ui};

use crate::theme::{BODY_FONT_SIZE, HEADER_FONT_SIZE, semantic_color, span_to_rich_text, spans_to_layout_job};

// ── Central dispatcher ──────────────────────────────────────────────

/// Render a slice of [`Block`]s into the given `Ui` region.
pub fn render_blocks(ui: &mut Ui, blocks: &[Block]) {
    for block in blocks {
        match block {
            Block::Line(spans) => render_line(ui, spans),
            Block::Empty => render_empty(ui),
            Block::Header(spans) => render_header(ui, spans),
            Block::Table { columns, rows } => render_table(ui, columns, rows),
            Block::ProgressBar { segments, label } => render_progress_bar(ui, segments, label.as_deref()),
            Block::Tree(nodes) => render_tree(ui, nodes, 0),
            Block::Separator => render_separator(ui),
            Block::KeyValue(pairs) => render_key_value(ui, pairs),
            _ => {}
        }
    }
}

// ── Individual renderers ────────────────────────────────────────────

/// Render a single line of styled spans.
fn render_line(ui: &mut Ui, spans: &[Span]) {
    if spans.is_empty() {
        drop(ui.label(""));
        return;
    }
    if spans.len() == 1 {
        if let Some(first) = spans.first() {
            drop(ui.label(span_to_rich_text(first)));
        }
        return;
    }
    let job = spans_to_layout_job(spans);
    drop(ui.label(job));
}

/// Render an empty line (vertical spacing).
fn render_empty(ui: &mut Ui) {
    ui.add_space(BODY_FONT_SIZE * 0.5);
}

/// Render a section header with larger font.
fn render_header(ui: &mut Ui, spans: &[Span]) {
    ui.add_space(4.0);
    if spans.len() == 1 {
        if let Some(first) = spans.first() {
            drop(ui.label(
                RichText::new(&first.text).color(semantic_color(Semantic::Header)).size(HEADER_FONT_SIZE).strong(),
            ));
        }
    } else {
        // Multi-span header: build a layout job with header sizing.
        let mut job = egui::text::LayoutJob::default();
        for span in spans {
            let color =
                span.color.map_or_else(|| semantic_color(span.semantic), |(r, g, b)| Color32::from_rgb(r, g, b));
            let font_id = egui::FontId::proportional(HEADER_FONT_SIZE);
            let format = egui::text::TextFormat { font_id, color, ..egui::text::TextFormat::default() };
            job.append(&span.text, 0.0, format);
        }
        drop(ui.label(job));
    }
    ui.add_space(2.0);
}

/// Render a horizontal separator.
fn render_separator(ui: &mut Ui) {
    drop(ui.separator());
}

/// Render a data table with optional column headers.
fn render_table(ui: &mut Ui, columns: &[cp_render::Column], rows: &[Vec<Cell>]) {
    let col_count = columns.len();
    if col_count == 0 {
        return;
    }

    let has_headers = columns.iter().any(|c| !c.header.is_empty());

    drop(egui::Grid::new(ui.next_auto_id()).num_columns(col_count).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
        // Header row.
        if has_headers {
            for col in columns {
                let rt = RichText::new(&col.header).color(semantic_color(Semantic::Header)).strong();
                drop(ui.label(rt));
            }
            ui.end_row();
        }

        // Data rows.
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                let align = columns.get(i).map_or(Align::Left, |c| c.align);
                render_cell(ui, cell, align);
            }
            ui.end_row();
        }
    }));
}
/// Render a single table cell with alignment.
fn render_cell(ui: &mut Ui, cell: &Cell, col_align: Align) {
    let effective_align = match cell.align {
        Align::Left => col_align,
        Align::Center | Align::Right => cell.align,
    };

    let layout = match effective_align {
        Align::Left => egui::Layout::left_to_right(egui::Align::Center),
        Align::Center => egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        Align::Right => egui::Layout::right_to_left(egui::Align::Center),
    };

    drop(ui.with_layout(layout, |ui| {
        if cell.spans.is_empty() {
            drop(ui.label(""));
        } else if cell.spans.len() == 1 {
            if let Some(first) = cell.spans.first() {
                drop(ui.label(span_to_rich_text(first)));
            }
        } else {
            let job = spans_to_layout_job(&cell.spans);
            drop(ui.label(job));
        }
    }));
}

/// Render a segmented progress bar.
fn render_progress_bar(ui: &mut Ui, segments: &[ProgressSegment], label: Option<&str>) {
    let available_width = ui.available_width().min(400.0);
    let bar_height = 16.0;

    // Optional label above the bar.
    if let Some(text) = label {
        drop(ui.label(RichText::new(text).size(BODY_FONT_SIZE).color(semantic_color(Semantic::Muted))));
    }

    let (rect, _response) = ui.allocate_exact_size(egui::Vec2::new(available_width, bar_height), egui::Sense::hover());

    let painter = ui.painter_at(rect);

    // Background.
    let _ = painter.rect_filled(rect, 3.0, Color32::from_rgb(40, 40, 40));

    // Segments.
    let mut x_offset = rect.left();
    for seg in segments {
        let width = available_width * f32::from(seg.percent) / 100.0;
        if width < 0.5 {
            continue;
        }
        let seg_rect =
            egui::Rect::from_min_size(egui::Pos2::new(x_offset, rect.top()), egui::Vec2::new(width, bar_height));
        let _ = painter.rect_filled(seg_rect, 3.0, semantic_color(seg.semantic));

        // Segment label (if it fits).
        if let Some(ref lbl) = seg.label
            && width > 30.0
        {
            let _ = painter.text(
                seg_rect.center(),
                egui::Align2::CENTER_CENTER,
                lbl,
                egui::FontId::proportional(10.0),
                Color32::BLACK,
            );
        }
        x_offset += width;
    }
}

/// Public progress-bar renderer with custom bar height.
///
/// Used by the sidebar token bar which needs a thinner gauge.
pub fn render_progress_bar_raw(ui: &mut Ui, segments: &[ProgressSegment], bar_height: f32) {
    let available_width = ui.available_width().min(400.0);
    let (rect, _response) = ui.allocate_exact_size(egui::Vec2::new(available_width, bar_height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let _ = painter.rect_filled(rect, 3.0, Color32::from_rgb(40, 40, 40));

    let mut x_offset = rect.left();
    for seg in segments {
        let width = available_width * f32::from(seg.percent) / 100.0;
        if width < 0.5 {
            continue;
        }
        let seg_rect =
            egui::Rect::from_min_size(egui::Pos2::new(x_offset, rect.top()), egui::Vec2::new(width, bar_height));
        let _ = painter.rect_filled(seg_rect, 3.0, semantic_color(seg.semantic));
        x_offset += width;
    }
}

/// Render a tree hierarchy with indentation.
fn render_tree(ui: &mut Ui, nodes: &[TreeNode], depth: u16) {
    for node in nodes {
        let indent = f32::from(depth) * 16.0;
        drop(ui.horizontal(|ui| {
            ui.add_space(indent);
            let arrow = if node.children.is_empty() {
                "  "
            } else if node.expanded {
                "▼ "
            } else {
                "▶ "
            };
            drop(ui.label(RichText::new(arrow).color(semantic_color(Semantic::Muted))));
            if node.label.len() == 1 {
                if let Some(first) = node.label.first() {
                    drop(ui.label(span_to_rich_text(first)));
                }
            } else {
                let job = spans_to_layout_job(&node.label);
                drop(ui.label(job));
            }
        }));
        if node.expanded && !node.children.is_empty() {
            render_tree(ui, &node.children, depth.saturating_add(1));
        }
    }
}

/// Render key-value pairs in a two-column layout.
fn render_key_value(ui: &mut Ui, pairs: &[KeyValuePair]) {
    drop(egui::Grid::new(ui.next_auto_id()).num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
        for (key_spans, value_spans) in pairs {
            // Key column.
            if key_spans.len() == 1 {
                if let Some(first) = key_spans.first() {
                    drop(ui.label(span_to_rich_text(first)));
                }
            } else {
                let job = spans_to_layout_job(key_spans);
                drop(ui.label(job));
            }
            // Value column.
            if value_spans.len() == 1 {
                if let Some(first) = value_spans.first() {
                    drop(ui.label(span_to_rich_text(first)));
                }
            } else {
                let job = spans_to_layout_job(value_spans);
                drop(ui.label(job));
            }
            ui.end_row();
        }
    }));
}
