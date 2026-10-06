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
/// Legend entries per line.
const LEGEND_PER_LINE: usize = 4;

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
            spans.push(Span::styled(format!(" {name:<12}"), semantic_to_style(Semantic::Muted)));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
}

/// One stacked bar: label, coloured segments sized by share, metric total.
fn render_one(share_bar: &PerfShareBar) -> Line<'static> {
    let mut spans = vec![Span::styled(format!(" {:<5}", share_bar.label), semantic_to_style(Semantic::Muted))];
    let mut used = 0usize;
    for (&pct, colour) in share_bar.shares.iter().zip(PALETTE.iter().copied().cycle()) {
        let want = float_math::mul(float_math::div(pct, 100.0f64), BAR_CELLS.to_f64()).round().to_usize();
        let cells = want.min(BAR_CELLS.saturating_sub(used));
        if cells > 0 {
            spans.push(Span::styled(chars::BLOCK_FULL.repeat(cells), Style::default().fg(colour)));
            used = used.saturating_add(cells);
        }
    }
    spans.push(Span::styled(
        chars::BLOCK_LIGHT.repeat(BAR_CELLS.saturating_sub(used)),
        semantic_to_style(Semantic::Muted),
    ));
    spans.push(Span::styled(format!(" {}", share_bar.total_display), semantic_to_style(Semantic::Muted)));
    Line::from(spans)
}
