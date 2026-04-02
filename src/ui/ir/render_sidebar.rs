//! Sidebar IR adapter — renders [`Sidebar`] to ratatui widgets.
//!
//! Replaces `ui::sidebar::full` and `ui::sidebar::collapsed` by consuming
//! the pre-built IR snapshot instead of reading application state directly.

use cp_render::frame::{Sidebar, SidebarEntry, SidebarMode, TokenBar, TokenStats};
use ratatui::prelude::{Constraint, Direction, Frame, Layout, Line, Rect, Span, Style};
use ratatui::widgets::Paragraph;

use crate::ui::{chars, helpers::format_number, theme};
use cp_base::cast::Safe as _;

use crate::infra::constants::SIDEBAR_HELP_HEIGHT;

/// Maximum dynamic entries per sidebar page.
const MAX_DYNAMIC_PER_PAGE: usize = 10;

/// Render the sidebar region from its IR snapshot.
pub(crate) fn render_sidebar_from_ir(frame: &mut Frame<'_>, sidebar: &Sidebar, area: Rect) {
    match sidebar.mode {
        SidebarMode::Normal => render_normal(frame, sidebar, area),
        SidebarMode::Collapsed => render_collapsed(frame, sidebar, area),
        SidebarMode::Hidden => {}
    }
}

// ── Normal (full) sidebar ────────────────────────────────────────────

/// Render the full sidebar with context list, token bar, PR card, stats, and help hints.
fn render_normal(frame: &mut Frame<'_>, sidebar: &Sidebar, area: Rect) {
    let _guard = crate::profile!("ir::sidebar_normal");
    let base_style = Style::default().bg(theme::bg_base());

    let sidebar_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(SIDEBAR_HELP_HEIGHT)])
        .split(area);
    debug_assert!(sidebar_layout.len() >= 2, "sidebar layout must have at least 2 chunks");

    let mut lines: Vec<Line<'_>> = vec![
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("CONTEXT", Style::default().fg(theme::text_muted()).bold()),
        ]),
        Line::from(""),
    ];

    // Separate fixed (id is empty for conversation, or is_fixed) from dynamic entries
    let (fixed_entries, dynamic_entries): (Vec<_>, Vec<_>) = sidebar.entries.iter().partition(|e| e.fixed);

    // Render fixed entries (conversation first, then P1-P9)
    for entry in &fixed_entries {
        render_normal_entry(&mut lines, entry, base_style);
    }

    // Dynamic entries with pagination
    let total_dynamic = dynamic_entries.len();
    if total_dynamic > 0 {
        lines
            .push(Line::from(vec![Span::styled(format!("  {:─<32}", ""), Style::default().fg(theme::border_muted()))]));

        // Find which page the selected entry is on
        let total_pages = if total_dynamic == 0 { 1 } else { total_dynamic.div_ceil(MAX_DYNAMIC_PER_PAGE) };
        let current_page = dynamic_entries
            .iter()
            .position(|e| e.active)
            .map_or(0, |pos| pos.checked_div(MAX_DYNAMIC_PER_PAGE).unwrap_or(0));

        let page_start = current_page.saturating_mul(MAX_DYNAMIC_PER_PAGE);
        let page_end = page_start.saturating_add(MAX_DYNAMIC_PER_PAGE).min(total_dynamic);

        for entry in dynamic_entries.get(page_start..page_end).unwrap_or(&[]) {
            render_normal_entry(&mut lines, entry, base_style);
        }

        if total_pages > 1 {
            lines.push(Line::from(vec![Span::styled(
                format!("  page {}/{}", current_page.saturating_add(1), total_pages),
                Style::default().fg(theme::text_muted()),
            )]));
        }
    }

    // Separator + token bar
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        format!(" {}", chars::HORIZONTAL.repeat(34)),
        Style::default().fg(theme::border()),
    )]));

    if let Some(ref tb) = sidebar.token_bar {
        render_token_bar(&mut lines, tb, base_style);
    }

    // PR card
    if let Some(ref pr) = sidebar.pr_card {
        render_pr_card(&mut lines, pr, base_style);
    }

    // Token stats
    if let Some(ref stats) = sidebar.token_stats {
        render_token_stats(&mut lines, stats);
    }

    let paragraph = Paragraph::new(lines).style(base_style);
    let Some(&context_area) = sidebar_layout.first() else { return };
    frame.render_widget(paragraph, context_area);

    // Help hints at bottom
    let help_lines: Vec<Line<'_>> = sidebar
        .help_hints
        .iter()
        .map(|hint| {
            Line::from(vec![
                Span::styled("  ", base_style),
                Span::styled(hint.key.clone(), Style::default().fg(theme::accent())),
                Span::styled(format!(" {}", hint.description), Style::default().fg(theme::text_muted())),
            ])
        })
        .collect();

    let help_paragraph = Paragraph::new(help_lines).style(base_style);
    let Some(&help_area) = sidebar_layout.get(1) else { return };
    frame.render_widget(help_paragraph, help_area);
}

/// Render a single entry line in the full sidebar.
fn render_normal_entry(lines: &mut Vec<Line<'static>>, entry: &SidebarEntry, base_style: Style) {
    let indicator = if entry.active { chars::ARROW_RIGHT } else { " " };
    let indicator_color = if entry.active { theme::accent() } else { theme::bg_base() };
    let name_color = if entry.active { theme::accent() } else { theme::text_secondary() };
    let icon_color = if entry.active { theme::accent() } else { theme::text_muted() };
    let tokens_color = theme::accent_dim();

    // For conversation (empty id), just show icon + label + tokens
    if entry.id.is_empty() {
        let tokens_str = format_number(entry.tokens.to_usize());
        lines.push(Line::from(vec![
            Span::styled(format!(" {indicator}"), Style::default().fg(indicator_color)),
            Span::styled("     ", Style::default().fg(theme::text_muted())),
            Span::styled(entry.icon.clone(), Style::default().fg(icon_color)),
            Span::styled(format!("{:<18}", &entry.label), Style::default().fg(name_color)),
            Span::styled(format!("{tokens_str:>6}"), Style::default().fg(tokens_color)),
            Span::styled(" ", base_style),
        ]));
        return;
    }

    // entry.label already contains "shortcut name    tokens" for full mode
    lines.push(Line::from(vec![
        Span::styled(format!(" {indicator}"), Style::default().fg(indicator_color)),
        Span::styled(" ", Style::default().fg(theme::text_muted())),
        Span::styled(entry.icon.clone(), Style::default().fg(icon_color)),
        Span::styled(entry.label.clone(), Style::default().fg(name_color)),
        Span::styled(" ", base_style),
    ]));
}

// ── Collapsed sidebar ────────────────────────────────────────────────

/// Render the collapsed sidebar (icon + badge strip).
fn render_collapsed(frame: &mut Frame<'_>, sidebar: &Sidebar, area: Rect) {
    let _guard = crate::profile!("ir::sidebar_collapsed");
    let base_style = Style::default().bg(theme::bg_base());

    let token_area_height = 5u16;
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(token_area_height)])
        .split(area);
    debug_assert!(layout.len() >= 2, "collapsed sidebar layout must have at least 2 chunks");

    let mut lines: Vec<Line<'_>> = Vec::new();
    lines.push(Line::from(""));

    let (fixed_entries, dynamic_entries): (Vec<_>, Vec<_>) = sidebar.entries.iter().partition(|e| e.fixed);

    for entry in &fixed_entries {
        render_collapsed_entry(&mut lines, entry, base_style);
    }

    if !dynamic_entries.is_empty() {
        lines.push(Line::from(vec![Span::styled("  ──────────", Style::default().fg(theme::border_muted()))]));
        for entry in &dynamic_entries {
            render_collapsed_entry(&mut lines, entry, base_style);
        }
    }

    let paragraph = Paragraph::new(lines).style(base_style);
    let Some(&panel_area) = layout.first() else { return };
    frame.render_widget(paragraph, panel_area);

    // Token summary at bottom
    if let Some(ref tb) = sidebar.token_bar {
        let token_lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(" ", base_style),
                Span::styled(format_number(tb.used.to_usize()), Style::default().fg(theme::text()).bold()),
            ]),
            Line::from(vec![
                Span::styled(" ", base_style),
                Span::styled(format_number(tb.threshold.to_usize()), Style::default().fg(theme::warning())),
            ]),
            Line::from(vec![
                Span::styled(" ", base_style),
                Span::styled(format_number(tb.budget.to_usize()), Style::default().fg(theme::accent())),
            ]),
        ];
        let token_paragraph = Paragraph::new(token_lines).style(base_style);
        let Some(&token_area) = layout.get(1) else { return };
        frame.render_widget(token_paragraph, token_area);
    }
}

/// Render a single collapsed entry line: arrow + icon + badge + tokens.
fn render_collapsed_entry(lines: &mut Vec<Line<'static>>, entry: &SidebarEntry, base_style: Style) {
    let arrow = if entry.active { "▸" } else { " " };
    let arrow_color = if entry.active { theme::accent() } else { theme::bg_base() };
    let icon_color = if entry.active { theme::accent() } else { theme::text_muted() };

    let label = entry.badge.as_deref().map_or_else(
        || {
            if entry.fixed {
                "   ".to_string()
            } else {
                format!("{:>3}", entry.id.strip_prefix('P').unwrap_or(&entry.id))
            }
        },
        |b| format!("{b:>3}"),
    );
    let label_color = if entry.active { theme::accent() } else { theme::text_muted() };
    let tokens = format_number(entry.tokens.to_usize());

    lines.push(Line::from(vec![
        Span::styled(format!(" {arrow}"), Style::default().fg(arrow_color)),
        Span::styled(entry.icon.clone(), Style::default().fg(icon_color)),
        Span::styled(label, Style::default().fg(label_color)),
        Span::styled(format!("{tokens:>5}"), Style::default().fg(theme::accent_dim())),
        Span::styled(" ", base_style),
    ]));
}

// ── Token bar ────────────────────────────────────────────────────────

/// Render the token usage gauge bar with hit/miss coloring.
fn render_token_bar(lines: &mut Vec<Line<'static>>, token_bar: &TokenBar, base_style: Style) {
    let bar_width = 34usize;

    let current = format_number(token_bar.used.to_usize());
    let threshold = format_number(token_bar.threshold.to_usize());
    let budget = format_number(token_bar.budget.to_usize());

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled(current, Style::default().fg(theme::text()).bold()),
        Span::styled(" / ", Style::default().fg(theme::text_muted())),
        Span::styled(threshold, Style::default().fg(theme::warning())),
        Span::styled(" / ", Style::default().fg(theme::text_muted())),
        Span::styled(budget, Style::default().fg(theme::accent())),
    ]));

    // Build the gauge bar from IR segments
    let hit_pct = token_bar.segments.first().map_or(0, |s| s.percent);
    let miss_pct = token_bar.segments.get(1).map_or(0, |s| s.percent);

    let hit_filled = cp_base::panels::time_arith::div_const::<100>(usize::from(hit_pct).saturating_mul(bar_width));
    let miss_filled = cp_base::panels::time_arith::div_const::<100>(usize::from(miss_pct).saturating_mul(bar_width));
    let total_filled = hit_filled.saturating_add(miss_filled).min(bar_width);

    // Threshold marker position
    let threshold_pos = if token_bar.budget > 0 {
        cp_base::panels::time_arith::div_const::<100>(
            token_bar
                .threshold
                .to_usize()
                .saturating_mul(100)
                .checked_div(token_bar.budget.to_usize())
                .unwrap_or(0)
                .saturating_mul(bar_width)
                .checked_div(100)
                .unwrap_or(0)
                .saturating_mul(100),
        )
    } else {
        0
    };

    let mut bar_spans = vec![Span::styled(" ", base_style)];
    for i in 0..bar_width {
        let is_threshold = i == threshold_pos && threshold_pos < bar_width;
        let ch = if is_threshold {
            "|"
        } else if i < total_filled {
            chars::BLOCK_FULL
        } else {
            chars::BLOCK_LIGHT
        };

        let color = if is_threshold {
            theme::warning()
        } else if i < hit_filled {
            theme::success()
        } else if i < total_filled {
            theme::warning()
        } else {
            theme::bg_elevated()
        };

        bar_spans.push(Span::styled(ch, Style::default().fg(color)));
    }
    lines.push(Line::from(bar_spans));
}

// ── PR card ──────────────────────────────────────────────────────────

/// Render the PR summary card.
fn render_pr_card(lines: &mut Vec<Line<'static>>, pr: &cp_render::frame::PrCard, base_style: Style) {
    // PR number + state (infer state from review_status presence)
    lines.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled(format!("PR#{}", pr.number), Style::default().fg(theme::accent()).bold()),
    ]));

    // Title (truncated)
    let title = crate::ui::helpers::truncate_string(&pr.title, 32);
    lines.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled(title, Style::default().fg(theme::text_secondary())),
    ]));

    // +/- stats and review/checks
    let mut detail_spans = vec![Span::styled(" ", base_style)];
    if pr.additions > 0 || pr.deletions > 0 {
        detail_spans.push(Span::styled(format!("+{}", pr.additions), Style::default().fg(theme::success())));
        detail_spans.push(Span::styled(format!(" -{}", pr.deletions), Style::default().fg(theme::error())));
    }
    if let Some(ref review) = pr.review_status {
        let (icon, color) = match review.as_str() {
            "APPROVED" => (" ✓", theme::success()),
            "CHANGES_REQUESTED" => (" ✗", theme::error()),
            "REVIEW_REQUIRED" => (" ●", theme::warning()),
            _ => (" ?", theme::text_muted()),
        };
        detail_spans.push(Span::styled(icon, Style::default().fg(color)));
    }
    if let Some(ref checks) = pr.checks_status {
        let (icon, color) = match checks.as_str() {
            "passing" => (" ●", theme::success()),
            "failing" => (" ●", theme::error()),
            "pending" => (" ●", theme::warning()),
            _ => (" ●", theme::text_muted()),
        };
        detail_spans.push(Span::styled(icon, Style::default().fg(color)));
    }
    if detail_spans.len() > 1 {
        lines.push(Line::from(detail_spans));
    }

    lines.push(Line::from(vec![Span::styled(
        format!(" {}", chars::HORIZONTAL.repeat(34)),
        Style::default().fg(theme::border()),
    )]));
    lines.push(Line::from(""));
}

// ── Token stats ──────────────────────────────────────────────────────

/// Render the token statistics table from IR.
fn render_token_stats(lines: &mut Vec<Line<'static>>, stats: &TokenStats) {
    use crate::ui::helpers::{Cell, render_table};

    let format_cost = |cost: Option<f64>| -> String {
        cost.map_or(String::new(), |c| {
            if c < 0.01 {
                format!("${c:.3}")
            } else if c < 1.0 {
                format!("${c:.2}")
            } else {
                format!("${c:.1}")
            }
        })
    };

    let hit_icon = chars::ARROW_UP.to_string();
    let miss_icon = chars::CROSS.to_string();
    let out_icon = chars::ARROW_DOWN.to_string();

    let header_cells = [
        Cell::new("", Style::default()),
        Cell::right(format!("{hit_icon} hit"), Style::default().fg(theme::success())),
        Cell::right(format!("{miss_icon} miss"), Style::default().fg(theme::warning())),
        Cell::right(format!("{out_icon} out"), Style::default().fg(theme::accent_dim())),
    ];

    let mut rows: Vec<Vec<Cell>> = Vec::new();

    for row in &stats.rows {
        // Counts row
        rows.push(vec![
            Cell::new(&row.label, Style::default().fg(theme::text_muted())),
            Cell::right(format_number(row.hit.to_usize()), Style::default().fg(theme::success())),
            Cell::right(format_number(row.miss.to_usize()), Style::default().fg(theme::warning())),
            Cell::right(format_number(row.output.to_usize()), Style::default().fg(theme::accent_dim())),
        ]);

        // Costs row (only if any cost is present)
        let hit_cost = format_cost(row.hit_cost);
        let miss_cost = format_cost(row.miss_cost);
        let out_cost = format_cost(row.output_cost);

        if !hit_cost.is_empty() || !miss_cost.is_empty() || !out_cost.is_empty() {
            rows.push(vec![
                Cell::new("", Style::default()),
                Cell::right(hit_cost, Style::default().fg(theme::text_muted())),
                Cell::right(miss_cost, Style::default().fg(theme::text_muted())),
                Cell::right(out_cost, Style::default().fg(theme::text_muted())),
            ]);
        }
    }

    lines.extend(render_table(&header_cells, &rows, None, 1));

    // Total cost
    if let Some(total) = stats.total_cost {
        let total_str = if total < 0.01 { format!("${total:.3}") } else { format!("${total:.2}") };
        lines.push(Line::from(vec![Span::styled(
            format!(" total: {total_str}"),
            Style::default().fg(theme::text_muted()),
        )]));
    }
}
