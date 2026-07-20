use ratatui::{
    prelude::{Frame, Line, Rect, Span, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use cp_render::conversation::ConfigOverlay;

use crate::infra::constants::{chars, theme};

mod budget_bars;
mod builder;
pub(crate) use builder::build_config_overlay;

/// Render the configuration overlay (Ctrl+H) centered on the given area.
///
/// Consumes the pre-built IR [`ConfigOverlay`] snapshot — no direct state access.
pub(crate) fn render_config_overlay(frame: &mut Frame<'_>, config: &ConfigOverlay, area: Rect) {
    // Center the overlay, clamped to available area
    let overlay_width = 56u16.min(area.width);
    let overlay_height = 34u16.min(area.height);
    let half_width = area.width.saturating_sub(overlay_width).saturating_div(2);
    let x = area.x.saturating_add(half_width);
    let half_height = area.height.saturating_sub(overlay_height).saturating_div(2);
    let y = area.y.saturating_add(half_height);
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Provider section
    render_provider_section(&mut lines, config);
    add_separator(&mut lines);

    // Model section
    render_model_section(&mut lines, config);
    add_separator(&mut lines);

    // Budget bars
    budget_bars::render_budget_section(&mut lines, config);
    add_separator(&mut lines);

    // Toggles
    render_toggles_section(&mut lines, config);

    // Help text
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("1-6", Style::default().fg(theme::warning())),
        Span::styled(" provider  ", Style::default().fg(theme::text_muted())),
        Span::styled("a-d", Style::default().fg(theme::warning())),
        Span::styled(" model  ", Style::default().fg(theme::text_muted())),
        Span::styled("r", Style::default().fg(theme::warning())),
        Span::styled(" reverie  ", Style::default().fg(theme::text_muted())),
        Span::styled("s", Style::default().fg(theme::warning())),
        Span::styled(" auto  ", Style::default().fg(theme::text_muted())),
        Span::styled("[]", Style::default().fg(theme::warning())),
        Span::styled(" think", Style::default().fg(theme::text_muted())),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::accent()))
        .style(Style::default().bg(theme::bg_surface()))
        .title(Span::styled(" Configuration ", Style::default().fg(theme::accent()).bold()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(paragraph, overlay_area);
}

/// Append a horizontal separator line to the output.
fn add_separator(lines: &mut Vec<Line<'_>>) {
    lines.push(Line::from(vec![Span::styled(
        format!("  {}", chars::HORIZONTAL.repeat(50)),
        Style::default().fg(theme::border()),
    )]));
}

/// Render the provider section from IR data.
fn render_provider_section(lines: &mut Vec<Line<'_>>, config: &ConfigOverlay) {
    lines.push(Line::from(vec![Span::styled("  LLM Provider", Style::default().fg(theme::text_secondary()).bold())]));

    for provider in &config.providers {
        let indicator = if provider.selected { ">" } else { " " };
        let check = if provider.selected { "[x]" } else { "[ ]" };
        let style = if provider.selected {
            Style::default().fg(theme::accent()).bold()
        } else {
            Style::default().fg(theme::text())
        };

        lines.push(Line::from(vec![
            Span::styled(format!("  {indicator} "), Style::default().fg(theme::accent())),
            Span::styled(format!("{} ", provider.key), Style::default().fg(theme::warning())),
            Span::styled(format!("{check} "), style),
            Span::styled(provider.name.clone(), style),
        ]));
    }
}

/// Render the model section from IR data.
fn render_model_section(lines: &mut Vec<Line<'_>>, config: &ConfigOverlay) {
    lines.push(Line::from(vec![Span::styled(
        format!("  {}", config.model_section_title),
        Style::default().fg(theme::text_secondary()).bold(),
    )]));

    for model in &config.models {
        let indicator = if model.selected { ">" } else { " " };
        let check = if model.selected { "[x]" } else { "[ ]" };
        let style = if model.selected {
            Style::default().fg(theme::accent()).bold()
        } else {
            Style::default().fg(theme::text())
        };

        lines.push(Line::from(vec![
            Span::styled(format!("  {indicator} "), Style::default().fg(theme::accent())),
            Span::styled(format!("{} ", model.key), Style::default().fg(theme::warning())),
            Span::styled(format!("{check} "), style),
            Span::styled(format!("{:<12}", model.name), style),
            Span::styled(format!("{:>4} ", model.context_window), Style::default().fg(theme::text_muted())),
            Span::styled(model.pricing.clone(), Style::default().fg(theme::text_muted())),
        ]));
    }
}

/// Render the toggle section from IR data.
fn render_toggles_section(lines: &mut Vec<Line<'_>>, config: &ConfigOverlay) {
    for toggle in &config.toggles {
        let (check, color) = if toggle.enabled { ("[x]", theme::success()) } else { ("[ ]", theme::text_muted()) };

        let mut spans = vec![
            Span::styled(
                format!("  {:<17}", format!("{}:", toggle.label)),
                Style::default().fg(theme::text_secondary()).bold(),
            ),
            Span::styled(format!("{check} "), Style::default().fg(color).bold()),
            Span::styled(toggle.value_display.clone(), Style::default().fg(color).bold()),
        ];

        if let Some((k1, k2)) = &toggle.adjust_keys {
            spans.push(Span::styled("  (press ", Style::default().fg(theme::text_muted())));
            spans.push(Span::styled(k1.clone(), Style::default().fg(theme::warning())));
            spans.push(Span::styled("/", Style::default().fg(theme::text_muted())));
            spans.push(Span::styled(k2.clone(), Style::default().fg(theme::warning())));
            spans.push(Span::styled(" to adjust)", Style::default().fg(theme::text_muted())));
        } else if !toggle.key_hint.is_empty() {
            spans.push(Span::styled("  (press ", Style::default().fg(theme::text_muted())));
            spans.push(Span::styled(toggle.key_hint.clone(), Style::default().fg(theme::warning())));
            spans.push(Span::styled(" to toggle)", Style::default().fg(theme::text_muted())));
        } else {
            // No adjust keys and no key hint — value shown without a hint.
        }

        lines.push(Line::from(spans));
    }
}
