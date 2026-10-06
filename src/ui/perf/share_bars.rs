//! F12 overlay share-bars: three stacked horizontal bars (mean / variance /
//! max) where each coloured segment is one main-loop substep's share of that
//! metric, plus a colour legend. Data comes pre-computed from the IR
//! ([`PerfShareBar`]); this module only maps it to ratatui spans.

use cp_render::Semantic;
use cp_render::conversation::PerfShareBar;
use ratatui::prelude::{Color, Line, Span, Style};

use crate::ui::chars;
use crate::ui::ir::semantic_to_style;
use cp_base::cast::Safe as _;
use cp_base::cast::float_math;

/// Cell width of each stacked bar.
const BAR_CELLS: usize = 44;
/// Legend entries per line (3 × 19 cols fits the 60-col overlay interior).
const LEGEND_PER_LINE: usize = 3;

/// Segment colours, cycled across substeps (same order as the HTML report).
const PALETTE: [Color; 12] = [
    Color::Rgb(78, 121, 167),
    Color::Rgb(242, 142, 43),
    Color::Rgb(225, 87, 89),
    Color::Rgb(118, 183, 178),
    Color::Rgb(89, 161, 79),
    Color::Rgb(237, 201, 72),
    Color::Rgb(176, 122, 161),
    Color::Rgb(255, 157, 167),
    Color::Rgb(156, 117, 95),
    Color::Rgb(186, 176, 172),
    Color::Rgb(134, 188, 182),
    Color::Rgb(211, 114, 149),
];

/// Append the share-bars + legend to `lines`. No-op when no loop substep has
/// been recorded yet.
pub(super) fn render_share_bars(names: &[String], share_bars: &[PerfShareBar], lines: &mut Vec<Line<'static>>) {
    if names.is_empty() {
        return;
    }
    lines.push(Line::from(Span::styled(" Loop substep share", semantic_to_style(Semantic::Accent).bold())));
    for share_bar in share_bars {
        lines.push(render_one(share_bar));
    }
    let entries: Vec<(&String, Color)> = names.iter().zip(PALETTE.iter().copied().cycle()).collect();
    for chunk in entries.chunks(LEGEND_PER_LINE) {
        let mut spans = vec![Span::raw(" ")];
        for &(name, colour) in chunk {
            spans.push(Span::styled(chars::BLOCK_FULL, Style::default().fg(colour)));
            spans.push(Span::styled(format!(" {name:<17}"), semantic_to_style(Semantic::Muted)));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
}

/// One stacked bar: label, coloured segments sized by share, metric total.
///
/// Colour = substep index (stable across frames). Cells are allocated by
/// largest remainder so the segments always sum to exactly [`BAR_CELLS`]
/// (per-segment rounding + clipping used to drop or truncate the tail steps).
fn render_one(share_bar: &PerfShareBar) -> Line<'static> {
    let mut spans = vec![Span::styled(format!(" {:<5}", share_bar.label), semantic_to_style(Semantic::Muted))];
    let cells = allocate_cells(&share_bar.shares);
    let mut used = 0usize;
    for (&n, colour) in cells.iter().zip(PALETTE.iter().copied().cycle()) {
        if n > 0 {
            spans.push(Span::styled(chars::BLOCK_FULL.repeat(n), Style::default().fg(colour)));
            used = used.saturating_add(n);
        }
    }
    spans.push(Span::styled(
        chars::BLOCK_LIGHT.repeat(BAR_CELLS.saturating_sub(used)),
        semantic_to_style(Semantic::Muted),
    ));
    spans.push(Span::styled(format!(" {}", share_bar.total_display), semantic_to_style(Semantic::Muted)));
    Line::from(spans)
}

/// Largest-remainder apportionment of [`BAR_CELLS`] over percentage shares.
///
/// Returns all zeros when the shares sum to zero (nothing recorded yet).
fn allocate_cells(shares: &[f64]) -> Vec<usize> {
    let sum: f64 = shares.iter().sum();
    if sum <= 0.0f64 {
        return vec![0; shares.len()];
    }
    let exact: Vec<f64> =
        shares.iter().map(|&pct| float_math::mul(float_math::div(pct, sum), BAR_CELLS.to_f64())).collect();
    let cells_and_rem: Vec<(usize, f64)> =
        exact.iter().map(|&v| (v.floor().to_usize(), float_math::sub(v, v.floor()))).collect();
    let mut cells: Vec<usize> = cells_and_rem.iter().map(|&(c, _)| c).collect();
    let mut order: Vec<usize> = (0..exact.len()).collect();
    order.sort_by(|&a, &b| {
        let ra = cells_and_rem.get(a).map_or(0.0f64, |&(_, r)| r);
        let rb = cells_and_rem.get(b).map_or(0.0f64, |&(_, r)| r);
        rb.partial_cmp(&ra).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
    });
    let mut left = BAR_CELLS.saturating_sub(cells.iter().sum());
    for idx in order {
        if left == 0 {
            break;
        }
        if let Some(c) = cells.get_mut(idx) {
            *c = c.saturating_add(1);
            left = left.saturating_sub(1);
        }
    }
    cells
}
