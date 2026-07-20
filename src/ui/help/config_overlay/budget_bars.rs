//! Budget bars rendering for the configuration overlay.
//!
//! Consumes the pre-built IR [`ConfigOverlay`] — no direct state access.

use cp_render::conversation::ConfigOverlay;
use ratatui::prelude::{Line, Span, Style};

use crate::infra::constants::chars;
use crate::ui::ir::semantic_to_style;
use cp_base::cast::float_math;

/// Bar width in cells (shared for all budget bars).
const BAR_WIDTH: usize = 24;

/// Render the budget bars section from IR data.
pub(super) fn render_budget_section(lines: &mut Vec<Line<'_>>, config: &ConfigOverlay) {
    for budget_bar in &config.budget_bars {
        render_bar(lines, budget_bar);
    }
}

/// Render a single budget bar line from IR data.
fn render_bar(lines: &mut Vec<Line<'_>>, budget_bar: &cp_render::conversation::ConfigBudgetBar) {
    use crate::infra::constants::theme;

    let is_selected = budget_bar.selected;
    let indicator = if is_selected { ">" } else { " " };
    let label_style = if is_selected {
        Style::default().fg(theme::accent()).bold()
    } else {
        Style::default().fg(theme::text_secondary()).bold()
    };
    let arrow_color = if is_selected { theme::accent() } else { theme::text_muted() };

    let filled = float_math::fill_from_ratio(budget_bar.fill_ratio, BAR_WIDTH).min(BAR_WIDTH);

    lines.push(Line::from(vec![
        Span::styled(format!(" {indicator} "), Style::default().fg(theme::accent())),
        Span::styled(budget_bar.label.clone(), label_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("   ◀ ", Style::default().fg(arrow_color)),
        Span::styled(chars::BLOCK_FULL.repeat(filled), semantic_to_style(budget_bar.semantic)),
        Span::styled(
            chars::BLOCK_LIGHT.repeat(BAR_WIDTH.saturating_sub(filled)),
            Style::default().fg(theme::bg_elevated()),
        ),
        Span::styled(" ▶ ", Style::default().fg(arrow_color)),
        Span::styled(budget_bar.value_display.clone(), Style::default().fg(theme::text()).bold()),
        Span::styled(
            budget_bar.extra.as_deref().map_or(String::new(), |e| format!("  {e}")),
            Style::default().fg(theme::text_muted()),
        ),
    ]));
}
