//! Budget bars rendering for the configuration overlay.
//!
//! Extracted to keep `config_overlay.rs` under the 500-line structure limit.

use ratatui::prelude::{Color, Line, Span, Style};

use crate::infra::constants::chars;
use crate::state::State;
use cp_base::cast::Safe as _;

/// Format a token count as a compact human-readable string (e.g. "128K", "1.5M").
pub(super) fn format_tokens_compact(tokens: usize) -> String {
    crate::ui::helpers::format_number(tokens)
}

/// Render the budget bars section: context budget, clean trigger, clean target, max cost.
pub(super) fn render_budget_section(lines: &mut Vec<Line<'_>>, state: &State) {
    let bar_width = 24usize;
    let max_budget = state.model_context_window();
    let effective_budget = state.effective_context_budget();
    let selected = state.config_selected_bar;

    // 1. Context Budget
    let budget_pct = (effective_budget.to_f64() / max_budget.to_f64() * 100.0).to_usize();
    let budget_filled = ((effective_budget.to_f64() / max_budget.to_f64()) * bar_width.to_f64()).to_usize();
    render_bar(
        lines,
        &BarConfig {
            selected,
            idx: 0,
            label: "Context Budget",
            pct: budget_pct,
            filled: budget_filled,
            bar_width,
            tokens_str: &format_tokens_compact(effective_budget),
            bar_color: crate::infra::constants::theme::success(),
            extra: None,
        },
    );

    // 2. Cleaning Threshold
    let threshold_pct = (state.cleaning_threshold * 100.0).to_usize();
    let threshold_tokens = state.cleaning_threshold_tokens();
    let threshold_filled = ((state.cleaning_threshold * bar_width.to_f32()).to_usize()).min(bar_width);
    render_bar(
        lines,
        &BarConfig {
            selected,
            idx: 1,
            label: "Clean Trigger",
            pct: threshold_pct,
            filled: threshold_filled,
            bar_width,
            tokens_str: &format_tokens_compact(threshold_tokens),
            bar_color: crate::infra::constants::theme::warning(),
            extra: None,
        },
    );

    // 3. Target Cleaning
    let target_pct = (state.cleaning_target_proportion * 100.0).to_usize();
    let target_tokens = state.cleaning_target_tokens();
    let target_abs_pct = (state.cleaning_target() * 100.0).to_usize();
    let target_filled = ((state.cleaning_target_proportion * bar_width.to_f32()).to_usize()).min(bar_width);
    let extra = format!(" ({target_abs_pct}%)");
    render_bar(
        lines,
        &BarConfig {
            selected,
            idx: 2,
            label: "Clean Target",
            pct: target_pct,
            filled: target_filled,
            bar_width,
            tokens_str: &format_tokens_compact(target_tokens),
            bar_color: crate::infra::constants::theme::accent(),
            extra: Some(&extra),
        },
    );

    // 4. Max Cost Guard Rail
    let spine_cfg = &cp_mod_spine::types::SpineState::get(state).config;
    let max_cost = spine_cfg.max_cost.unwrap_or(0.0);
    let max_display = 20.0f64;
    let cost_filled = ((max_cost / max_display) * bar_width.to_f64()).min(bar_width.to_f64()).to_usize();
    let cost_label = if max_cost <= 0.0 { "disabled".to_string() } else { format!("${max_cost:.2}") };
    let is_selected = selected == 3;
    let indicator = if is_selected { ">" } else { " " };
    let label_style = if is_selected {
        Style::default().fg(crate::infra::constants::theme::accent()).bold()
    } else {
        Style::default().fg(crate::infra::constants::theme::text_secondary()).bold()
    };
    let arrow_color = if is_selected {
        crate::infra::constants::theme::accent()
    } else {
        crate::infra::constants::theme::text_muted()
    };

    lines.push(Line::from(vec![
        Span::styled(format!(" {indicator} "), Style::default().fg(crate::infra::constants::theme::accent())),
        Span::styled("Max Cost".to_string(), label_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("   ◀ ", Style::default().fg(arrow_color)),
        Span::styled(
            chars::BLOCK_FULL.repeat(cost_filled.min(bar_width)),
            Style::default().fg(crate::infra::constants::theme::error()),
        ),
        Span::styled(
            chars::BLOCK_LIGHT.repeat(bar_width.saturating_sub(cost_filled)),
            Style::default().fg(crate::infra::constants::theme::bg_elevated()),
        ),
        Span::styled(" ▶ ", Style::default().fg(arrow_color)),
        Span::styled(cost_label, Style::default().fg(crate::infra::constants::theme::text()).bold()),
        Span::styled("  (guard rail)", Style::default().fg(crate::infra::constants::theme::text_muted())),
    ]));
}

/// Configuration for rendering a single budget bar.
struct BarConfig<'cfg> {
    /// Index of the currently selected bar.
    selected: usize,
    /// Index of this bar (for selection comparison).
    idx: usize,
    /// Display label shown before the bar.
    label: &'cfg str,
    /// Percentage value to show after the bar.
    pct: usize,
    /// Number of filled cells in the bar.
    filled: usize,
    /// Total width of the bar in cells.
    bar_width: usize,
    /// Formatted token count string.
    tokens_str: &'cfg str,
    /// Color used for the filled portion of the bar.
    bar_color: Color,
    /// Optional extra text appended after the token count.
    extra: Option<&'cfg str>,
}

/// Render a single budget bar line.
fn render_bar(lines: &mut Vec<Line<'_>>, cfg: &BarConfig<'_>) {
    let is_selected = cfg.selected == cfg.idx;
    let indicator = if is_selected { ">" } else { " " };
    let label_style = if is_selected {
        Style::default().fg(crate::infra::constants::theme::accent()).bold()
    } else {
        Style::default().fg(crate::infra::constants::theme::text_secondary()).bold()
    };
    let arrow_color = if is_selected {
        crate::infra::constants::theme::accent()
    } else {
        crate::infra::constants::theme::text_muted()
    };

    lines.push(Line::from(vec![
        Span::styled(format!(" {indicator} "), Style::default().fg(crate::infra::constants::theme::accent())),
        Span::styled(cfg.label.to_string(), label_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("   ◀ ", Style::default().fg(arrow_color)),
        Span::styled(chars::BLOCK_FULL.repeat(cfg.filled.min(cfg.bar_width)), Style::default().fg(cfg.bar_color)),
        Span::styled(
            chars::BLOCK_LIGHT.repeat(cfg.bar_width.saturating_sub(cfg.filled)),
            Style::default().fg(crate::infra::constants::theme::bg_elevated()),
        ),
        Span::styled(" ▶ ", Style::default().fg(arrow_color)),
        Span::styled(format!("{}%", cfg.pct), Style::default().fg(crate::infra::constants::theme::text()).bold()),
        Span::styled(
            format!("  {} tok{}", cfg.tokens_str, cfg.extra.unwrap_or("")),
            Style::default().fg(crate::infra::constants::theme::text_muted()),
        ),
    ]));
}
