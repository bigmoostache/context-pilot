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

/// Left indent for non-entry content (separators, bars, stats).
/// Entries handle their own col-0 indicator, but everything else gets this padding.
const CONTENT_INDENT: usize = 1;

/// Compute available content width given the full area width and the left indent.
const fn content_width(area_width: u16) -> usize {
    (area_width as usize).saturating_sub(CONTENT_INDENT)
}

/// Create a line with structural left-indent (1 space prefix).
/// Use this for ALL non-entry sidebar lines to enforce consistent padding.
pub(super) fn padded(spans: Vec<Span<'static>>) -> Line<'static> {
    let mut all = Vec::with_capacity(spans.len().saturating_add(1));
    all.push(Span::raw(" "));
    all.extend(spans);
    Line::from(all)
}

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
    let cw = content_width(area.width);

    let sidebar_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(SIDEBAR_HELP_HEIGHT)])
        .split(area);
    debug_assert!(sidebar_layout.len() >= 2, "sidebar layout must have at least 2 chunks");

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Token bar in rounded border box (above entries)
    if let Some(ref tb) = sidebar.token_bar {
        render_token_bar_box(&mut lines, tb, cw);
    }

    // Separate fixed (id is empty for conversation, or is_fixed) from dynamic entries
    let (fixed_entries, dynamic_entries): (Vec<_>, Vec<_>) = sidebar.entries.iter().partition(|e| e.fixed);

    // Render fixed entries (conversation first, then P1-P9)
    for entry in &fixed_entries {
        render_normal_entry(&mut lines, entry, cw);
    }

    // Dynamic entries with pagination
    let total_dynamic = dynamic_entries.len();
    if total_dynamic > 0 {
        // Find which page the selected entry is on
        let total_pages = if total_dynamic == 0 { 1 } else { total_dynamic.div_ceil(MAX_DYNAMIC_PER_PAGE) };
        let current_page = dynamic_entries
            .iter()
            .position(|e| e.active)
            .map_or(0, |pos| pos.checked_div(MAX_DYNAMIC_PER_PAGE).unwrap_or(0));

        // Separator with embedded page indicator: ──────── 1/2 ─
        if total_pages > 1 {
            let page_text = format!("{}/{}", current_page.saturating_add(1), total_pages);
            let suffix_len = page_text.len().saturating_add(3); // space + text + space + trailing ─
            let fill = cw.saturating_sub(suffix_len);
            lines.push(padded(vec![
                Span::styled("─".repeat(fill), Style::default().fg(theme::border_muted())),
                Span::styled(format!(" {page_text} "), Style::default().fg(theme::text_muted())),
                Span::styled("─", Style::default().fg(theme::border_muted())),
            ]));
        } else {
            lines.push(padded(vec![Span::styled("─".repeat(cw), Style::default().fg(theme::border_muted()))]));
        }

        let page_start = current_page.saturating_mul(MAX_DYNAMIC_PER_PAGE);
        let page_end = page_start.saturating_add(MAX_DYNAMIC_PER_PAGE).min(total_dynamic);

        for entry in dynamic_entries.get(page_start..page_end).unwrap_or(&[]) {
            render_normal_entry(&mut lines, entry, cw);
        }
    }

    // PR card
    if let Some(ref pr) = sidebar.pr_card {
        lines.push(Line::from(""));
        render_pr_card(&mut lines, pr, cw);
    }

    // Token stats (rendered with rounded border)
    if let Some(ref stats) = sidebar.token_stats {
        lines.push(Line::from(""));
        render_token_stats(&mut lines, stats, cw);
    }

    let paragraph = Paragraph::new(lines).style(base_style);
    let Some(&context_area) = sidebar_layout.first() else { return };
    frame.render_widget(paragraph, context_area);

    // Help hints at bottom
    let mut help_lines: Vec<Line<'_>> = Vec::new();
    help_lines.push(Line::from("")); // separator line for visibility
    help_lines.extend(sidebar.help_hints.iter().map(|hint| {
        padded(vec![
            Span::styled(hint.key.clone(), Style::default().fg(theme::accent())),
            Span::styled(format!(" {}", hint.description), Style::default().fg(theme::text_muted())),
        ])
    }));

    let help_paragraph = Paragraph::new(help_lines).style(base_style);
    let Some(&help_area) = sidebar_layout.get(1) else { return };
    frame.render_widget(help_paragraph, help_area);
}

/// Render a single entry line in the full sidebar.
fn render_normal_entry(lines: &mut Vec<Line<'static>>, entry: &SidebarEntry, cw: usize) {
    let indicator = if entry.active { chars::ARROW_RIGHT } else { " " };
    let indicator_color = if entry.active { theme::accent() } else { theme::bg_base() };
    let name_color = if entry.active { theme::accent() } else { theme::text_secondary() };
    let icon_color = if entry.active { theme::accent() } else { theme::text_muted() };
    let shortcut_color = if entry.active { theme::accent() } else { theme::accent_dim() };
    let tokens_color = token_count_color(entry.tokens);

    // Shortcut width for alignment (enough for "P99" or 3-digit badge counts)
    let shortcut_width = 3;

    // Dynamic label width: fill remaining space after fixed-width columns
    // indicator(1) + icon(2 display cols) + shortcut(3) + space(1) + tokens(6) = 13 fixed cols
    let entry_width = cw.saturating_add(CONTENT_INDENT); // = area.width
    let fixed_cols = 13usize;
    let label_width = entry_width.saturating_sub(fixed_cols);

    lines.push(Line::from(vec![
        Span::styled(indicator, Style::default().fg(indicator_color)),
        Span::styled(entry.icon.clone(), Style::default().fg(icon_color)),
        Span::styled(
            format!("{:>width$} ", entry.shortcut, width = shortcut_width),
            Style::default().fg(shortcut_color),
        ),
        Span::styled(format!("{:<width$}", entry.label, width = label_width), Style::default().fg(name_color)),
        Span::styled(format!("{:>6}", format_number(entry.tokens.to_usize())), Style::default().fg(tokens_color)),
    ]));
}

// ── Collapsed sidebar ────────────────────────────────────────────────

/// Color for token count based on magnitude.
fn token_count_color(tokens: u32) -> ratatui::style::Color {
    if tokens >= 5000 {
        theme::warning()
    } else if tokens >= 1500 {
        theme::accent_dim()
    } else {
        theme::text_muted()
    }
}

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
        lines.push(Line::from(vec![Span::styled(" ──────────", Style::default().fg(theme::border_muted()))]));
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
            Line::from(vec![Span::styled(
                format_number(tb.used.to_usize()),
                Style::default().fg(theme::text()).bold(),
            )]),
            Line::from(vec![Span::styled(
                format_number(tb.threshold.to_usize()),
                Style::default().fg(theme::warning()),
            )]),
            Line::from(vec![Span::styled(format_number(tb.budget.to_usize()), Style::default().fg(theme::accent()))]),
        ];
        let token_paragraph = Paragraph::new(token_lines).style(base_style);
        let Some(&token_area) = layout.get(1) else { return };
        frame.render_widget(token_paragraph, token_area);
    }
}

/// Render a single collapsed entry line: arrow + icon + badge + tokens.
fn render_collapsed_entry(lines: &mut Vec<Line<'static>>, entry: &SidebarEntry, _base_style: Style) {
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
    let tokens_color = token_count_color(entry.tokens);

    lines.push(Line::from(vec![
        Span::styled(arrow, Style::default().fg(arrow_color)),
        Span::styled(entry.icon.clone(), Style::default().fg(icon_color)),
        Span::styled(label, Style::default().fg(label_color)),
        Span::styled(format!("{tokens:>5}"), Style::default().fg(tokens_color)),
    ]));
}

// ── Token bar ────────────────────────────────────────────────────────

/// Render the token usage section wrapped in a rounded border box.
/// Line 1: ⚓ Context Pilot
/// Line 2: used / threshold / budget (styled)
/// Line 3: gauge bar
fn render_token_bar_box(lines: &mut Vec<Line<'static>>, token_bar: &TokenBar, cw: usize) {
    let border_style = Style::default().fg(theme::border_muted());
    let inner_width = cw.saturating_sub(2); // space between │ and │

    // Get animated values (smooth fill + pulse)
    let anim = super::bar_animation::tick(token_bar);

    let current = format_number(anim.used_tokens.to_usize());
    let threshold = format_number(token_bar.threshold.to_usize());
    let budget = format_number(token_bar.budget.to_usize());

    // Build content lines
    let mut content: Vec<Line<'static>> = Vec::new();

    // Line 1: ⚓ Context Pilot
    content.push(Line::from(vec![
        Span::styled("⚓ ", Style::default().fg(theme::accent())),
        Span::styled("Context Pilot", Style::default().fg(theme::text()).bold()),
    ]));

    // Line 2: used / threshold / budget
    content.push(Line::from(vec![
        Span::styled(current, Style::default().fg(theme::text()).bold()),
        Span::styled(" / ", Style::default().fg(theme::border_muted())),
        Span::styled(threshold, Style::default().fg(theme::warning())),
        Span::styled(" / ", Style::default().fg(theme::border_muted())),
        Span::styled(budget, Style::default().fg(theme::accent())),
    ]));

    // Line 3: gauge bar (using animated fractional positions)
    let bar_width = inner_width;
    let bar_width_f = bar_width.to_f64();

    // Fractional fill positions for smooth animation
    let hit_filled_f = anim.hit_pct * bar_width_f / 100.0;
    let miss_filled_f = anim.miss_pct * bar_width_f / 100.0;
    let total_filled_f = (hit_filled_f + miss_filled_f).min(bar_width_f);

    // Integer positions for cell-level decisions
    let hit_filled = hit_filled_f.floor().to_usize().min(bar_width);
    let total_filled = total_filled_f.floor().to_usize().min(bar_width);

    // Fractional remainder at boundaries for color crossfade
    let hit_frac = hit_filled_f.fract();
    let total_frac = total_filled_f.fract();

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

    let hit_color = theme::success();
    let miss_color = theme::warning();
    let empty_color = theme::bg_elevated();

    let mut bar_spans: Vec<Span<'static>> = Vec::new();
    for i in 0..bar_width {
        let is_threshold = i == threshold_pos && threshold_pos < bar_width;

        // Determine the base fill color for this cell
        let base_color = if i < hit_filled {
            hit_color
        } else if i == hit_filled && hit_frac > 0.01 && total_filled_f > hit_filled_f {
            // Boundary cell: crossfade from hit → miss
            super::bar_animation::lerp_color(miss_color, hit_color, hit_frac)
        } else if i < total_filled {
            miss_color
        } else if i == total_filled && total_frac > 0.01 {
            // Boundary cell: crossfade from filled → empty
            let fill = if hit_filled_f > total_filled_f.floor() { hit_color } else { miss_color };
            super::bar_animation::lerp_color(empty_color, fill, total_frac)
        } else {
            empty_color
        };

        // Apply streaming pulse to filled cells
        let is_filled_cell = i < total_filled || (i == total_filled && total_frac > 0.01);
        let color = anim.pulse_brightness.map_or(base_color, |brightness| {
            if is_filled_cell { super::bar_animation::pulse_color(base_color, brightness) } else { base_color }
        });

        if is_threshold {
            bar_spans.push(Span::styled("|", Style::default().fg(theme::warning()).bg(color)));
        } else {
            let ch = if i < total_filled || (i == total_filled && total_frac > 0.5) {
                chars::BLOCK_FULL
            } else {
                chars::BLOCK_LIGHT
            };
            bar_spans.push(Span::styled(ch, Style::default().fg(color)));
        }
    }
    content.push(Line::from(bar_spans));

    // Wrap in rounded border
    // Top: ╭───...───╮
    lines.push(padded(vec![
        Span::styled("╭", border_style),
        Span::styled("─".repeat(inner_width), border_style),
        Span::styled("╮", border_style),
    ]));

    // Content lines: │ content ... │
    for content_line in content {
        let line_width: usize =
            content_line.spans.iter().map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref())).sum();
        let pad = inner_width.saturating_sub(line_width);
        let mut spans = Vec::with_capacity(content_line.spans.len().saturating_add(4));
        spans.push(Span::raw(" ")); // structural indent
        spans.push(Span::styled("│", border_style));
        spans.extend(content_line.spans);
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled("│", border_style));
        lines.push(Line::from(spans));
    }

    // Bottom: ╰───...───╯
    lines.push(padded(vec![
        Span::styled("╰", border_style),
        Span::styled("─".repeat(inner_width), border_style),
        Span::styled("╯", border_style),
    ]));
}

// ── PR card ──────────────────────────────────────────────────────────

/// Render the PR summary card.
fn render_pr_card(lines: &mut Vec<Line<'static>>, pr: &cp_render::frame::PrCard, cw: usize) {
    // PR number + state (infer state from review_status presence)
    lines.push(padded(vec![Span::styled(format!("PR#{}", pr.number), Style::default().fg(theme::accent()).bold())]));

    // Title (truncated)
    let title = crate::ui::helpers::truncate_string(&pr.title, cw.saturating_sub(2));
    lines.push(padded(vec![Span::styled(title, Style::default().fg(theme::text_secondary()))]));

    // +/- stats and review/checks
    let mut detail_spans = Vec::new();
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
    if !detail_spans.is_empty() {
        lines.push(padded(detail_spans));
    }

    lines.push(padded(vec![Span::styled(chars::HORIZONTAL.repeat(cw), Style::default().fg(theme::border()))]));
    lines.push(Line::from(""));
}

// ── Token stats ──────────────────────────────────────────────────────

/// Render the token statistics table — delegates to extracted module.
fn render_token_stats(lines: &mut Vec<Line<'static>>, stats: &TokenStats, cw: usize) {
    super::render_sidebar_stats::render_token_stats(lines, stats, cw);
}
