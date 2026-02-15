use ratatui::{prelude::*, widgets::Paragraph};

use super::{chars, helpers::*, spinner, theme};
use crate::constants::SIDEBAR_HELP_HEIGHT;
use crate::state::{ContextType, State};

/// Maximum number of dynamic contexts (P7+) to show per page
const MAX_DYNAMIC_PER_PAGE: usize = 10;

pub fn render_sidebar(frame: &mut Frame, state: &State, area: Rect) {
    let _guard = crate::profile!("ui::sidebar");
    let base_style = Style::default().bg(theme::bg_base());

    // Sidebar layout: context list + help hints
    let sidebar_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                      // Context list
            Constraint::Length(SIDEBAR_HELP_HEIGHT), // Help hints
        ])
        .split(area);

    // Context list
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("CONTEXT", Style::default().fg(theme::text_muted()).bold()),
        ]),
        Line::from(""),
    ];

    let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let max_tokens = state.effective_context_budget();
    let threshold_tokens = state.cleaning_threshold_tokens();

    // Calculate ID width for alignment based on longest ID
    let id_width = state.context.iter().map(|c| c.id.len()).max().unwrap_or(2);

    let spin = spinner::spinner(state.spinner_frame);

    // Sort contexts by ID for display (P0, P1, P2, ...)
    let mut sorted_indices: Vec<usize> = (0..state.context.len()).collect();
    sorted_indices.sort_by(|&a, &b| {
        let id_a = state.context[a].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
        let id_b = state.context[b].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
        id_a.cmp(&id_b)
    });

    // Separate fixed (P1-P9) and dynamic (P10+) contexts, skipping Conversation (it's the chat feed, not a numbered panel)
    let (fixed_indices, dynamic_indices): (Vec<_>, Vec<_>) = sorted_indices
        .into_iter()
        .filter(|&i| state.context[i].context_type != ContextType::new(ContextType::CONVERSATION))
        .partition(|&i| state.context[i].context_type.is_fixed());

    // Render Conversation entry (special: no Px ID, highlights when selected)
    if let Some(conv_idx) = state.context.iter().position(|c| c.context_type == ContextType::new(ContextType::CONVERSATION))
    {
        let is_selected = conv_idx == state.selected_context;
        let indicator = if is_selected { chars::ARROW_RIGHT } else { " " };
        let indicator_color = if is_selected { theme::accent() } else { theme::bg_base() };
        let name_color = if is_selected { theme::accent() } else { theme::text_secondary() };
        let icon = ContextType::new(ContextType::CONVERSATION).icon();
        let conv_tokens = format_number(state.context[conv_idx].token_count);

        lines.push(Line::from(vec![
            Span::styled(format!(" {}", indicator), Style::default().fg(indicator_color)),
            Span::styled(format!(" {:>width$} ", "", width = id_width), Style::default().fg(theme::text_muted())),
            Span::styled(icon, Style::default().fg(if is_selected { theme::accent() } else { theme::text_muted() })),
            Span::styled(format!("{:<18}", "Conversation"), Style::default().fg(name_color)),
            Span::styled(format!("{:>6}", conv_tokens), Style::default().fg(theme::accent_dim())),
            Span::styled(" ", base_style),
        ]));
    }

    // Render fixed contexts (always visible)
    for &i in &fixed_indices {
        let ctx = &state.context[i];
        render_context_line(&mut lines, ctx, i, state, id_width, spin, base_style);
    }

    // Calculate pagination for dynamic contexts
    let total_dynamic = dynamic_indices.len();
    let total_pages = if total_dynamic == 0 { 1 } else { total_dynamic.div_ceil(MAX_DYNAMIC_PER_PAGE) };

    // Determine current page based on selected context
    let current_page = if let Some(selected_pos) = dynamic_indices.iter().position(|&i| i == state.selected_context) {
        selected_pos / MAX_DYNAMIC_PER_PAGE
    } else {
        0 // Default to first page if a fixed context is selected
    };

    // Get dynamic contexts for current page
    let page_start = current_page * MAX_DYNAMIC_PER_PAGE;
    let page_end = (page_start + MAX_DYNAMIC_PER_PAGE).min(total_dynamic);
    let page_indices: Vec<usize> = dynamic_indices[page_start..page_end].to_vec();

    // Add separator if there are dynamic contexts
    if total_dynamic > 0 {
        lines
            .push(Line::from(vec![Span::styled(format!("  {:─<32}", ""), Style::default().fg(theme::border_muted()))]));

        // Render dynamic contexts for current page
        for &i in &page_indices {
            let ctx = &state.context[i];
            render_context_line(&mut lines, ctx, i, state, id_width, spin, base_style);
        }

        // Page indicator (only if more than one page)
        if total_pages > 1 {
            lines.push(Line::from(vec![Span::styled(
                format!("  page {}/{}", current_page + 1, total_pages),
                Style::default().fg(theme::text_muted()),
            )]));
        }
    }

    // Separator
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        format!(" {}", chars::HORIZONTAL.repeat(34)),
        Style::default().fg(theme::border()),
    )]));

    // Token usage bar - full width
    let bar_width = 34usize;
    let threshold_pct = state.cleaning_threshold;
    let usage_pct = (total_tokens as f64 / max_tokens as f64).min(1.0);

    // Calculate bar positions
    let filled = (usage_pct * bar_width as f64) as usize;
    let threshold_pos = (threshold_pct as f64 * bar_width as f64) as usize;

    // Color based on threshold
    let bar_color = if total_tokens >= threshold_tokens {
        theme::error()
    } else if total_tokens as f64 >= threshold_tokens as f64 * 0.9 {
        theme::warning()
    } else {
        theme::accent()
    };

    // Format: "12.5K / 140K threshold / 200K budget"
    let current = format_number(total_tokens);
    let threshold = format_number(threshold_tokens);
    let budget = format_number(max_tokens);

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled(current, Style::default().fg(theme::text()).bold()),
        Span::styled(" / ", Style::default().fg(theme::text_muted())),
        Span::styled(threshold, Style::default().fg(theme::warning())),
        Span::styled(" / ", Style::default().fg(theme::text_muted())),
        Span::styled(budget, Style::default().fg(theme::accent())),
    ]));

    // Build bar with threshold marker
    let mut bar_spans = vec![Span::styled(" ", base_style)];
    for i in 0..bar_width {
        let char = if i == threshold_pos && threshold_pos < bar_width {
            "|" // Threshold marker
        } else if i < filled {
            chars::BLOCK_FULL
        } else {
            chars::BLOCK_LIGHT
        };

        let color = if i == threshold_pos {
            theme::warning()
        } else if i < filled {
            bar_color
        } else {
            theme::bg_elevated()
        };

        bar_spans.push(Span::styled(char, Style::default().fg(color)));
    }
    lines.push(Line::from(bar_spans));

    // Separator before token stats
    lines.push(Line::from(""));

    // Token stats (cache hit / cache miss / output) — only when any value is non-zero
    if state.cache_hit_tokens > 0 || state.cache_miss_tokens > 0 || state.total_output_tokens > 0 {
        // Get current model pricing
        let hit_price = state.cache_hit_price_per_mtok();
        let miss_price = state.cache_miss_price_per_mtok();
        let out_price = state.output_price_per_mtok();

        // Helper: format cost in dollars with appropriate precision
        let format_cost = |tokens: usize, price_per_mtok: f32| -> String {
            let cost = crate::state::State::token_cost(tokens, price_per_mtok);
            if cost < 0.001 {
                String::new()
            } else if cost < 0.01 {
                format!("{:.3}", cost)
            } else if cost < 1.0 {
                format!("{:.2}", cost)
            } else {
                format!("{:.1}", cost)
            }
        };

        // Build table rows: each row has [label, ↑hit, ✗miss, ↓out]
        // We interleave counts rows and costs rows
        let hit_icon = chars::ARROW_UP.to_string();
        let miss_icon = chars::CROSS.to_string();
        let out_icon = chars::ARROW_DOWN.to_string();

        let header = [
            Cell::new("", Style::default()),
            Cell::right(format!("{} hit", hit_icon), Style::default().fg(theme::success())),
            Cell::right(format!("{} miss", miss_icon), Style::default().fg(theme::warning())),
            Cell::right(format!("{} out", out_icon), Style::default().fg(theme::accent_dim())),
        ];

        let mut rows: Vec<Vec<Cell>> = Vec::new();

        // Helper to build a counts row
        let counts_row = |label: &str, hit: usize, miss: usize, out: usize| -> Vec<Cell> {
            vec![
                Cell::new(label, Style::default().fg(theme::text_muted())),
                Cell::right(format_number(hit), Style::default().fg(theme::success())),
                Cell::right(format_number(miss), Style::default().fg(theme::warning())),
                Cell::right(format_number(out), Style::default().fg(theme::accent_dim())),
            ]
        };

        // Helper to build a costs row
        let costs_row = |hit: usize, miss: usize, out: usize| -> Option<Vec<Cell>> {
            let hit_cost = format_cost(hit, hit_price);
            let miss_cost = format_cost(miss, miss_price);
            let out_cost = format_cost(out, out_price);

            if hit_cost.is_empty() && miss_cost.is_empty() && out_cost.is_empty() {
                return None;
            }

            let fmt = |cost: &str| -> String { if cost.is_empty() { String::new() } else { format!("${}", cost) } };

            Some(vec![
                Cell::new("", Style::default()),
                Cell::right(fmt(&hit_cost), Style::default().fg(theme::text_muted())),
                Cell::right(fmt(&miss_cost), Style::default().fg(theme::text_muted())),
                Cell::right(fmt(&out_cost), Style::default().fg(theme::text_muted())),
            ])
        };

        // tot row
        rows.push(counts_row("tot", state.cache_hit_tokens, state.cache_miss_tokens, state.total_output_tokens));
        if let Some(row) = costs_row(state.cache_hit_tokens, state.cache_miss_tokens, state.total_output_tokens) {
            rows.push(row);
        }

        // strm row
        if state.stream_output_tokens > 0 || state.stream_cache_hit_tokens > 0 || state.stream_cache_miss_tokens > 0 {
            rows.push(counts_row(
                "strm",
                state.stream_cache_hit_tokens,
                state.stream_cache_miss_tokens,
                state.stream_output_tokens,
            ));
            if let Some(row) =
                costs_row(state.stream_cache_hit_tokens, state.stream_cache_miss_tokens, state.stream_output_tokens)
            {
                rows.push(row);
            }
        }

        // tick row
        if state.tick_output_tokens > 0 || state.tick_cache_hit_tokens > 0 || state.tick_cache_miss_tokens > 0 {
            rows.push(counts_row(
                "tick",
                state.tick_cache_hit_tokens,
                state.tick_cache_miss_tokens,
                state.tick_output_tokens,
            ));
            if let Some(row) =
                costs_row(state.tick_cache_hit_tokens, state.tick_cache_miss_tokens, state.tick_output_tokens)
            {
                rows.push(row);
            }
        }

        lines.extend(render_table(&header, &rows, None, 1));

        // Total cost below the table
        let total_cost = crate::state::State::token_cost(state.cache_hit_tokens, hit_price)
            + crate::state::State::token_cost(state.cache_miss_tokens, miss_price)
            + crate::state::State::token_cost(state.total_output_tokens, out_price);
        if total_cost >= 0.001 {
            let total_str =
                if total_cost < 0.01 { format!("${:.3}", total_cost) } else { format!("${:.2}", total_cost) };
            lines.push(Line::from(vec![Span::styled(
                format!(" total: {}", total_str),
                Style::default().fg(theme::text_muted()),
            )]));
        }
    }

    let paragraph = Paragraph::new(lines).style(base_style);
    frame.render_widget(paragraph, sidebar_layout[0]);

    // Help hints at bottom of sidebar
    let help_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Tab", Style::default().fg(theme::accent())),
            Span::styled(" next panel", Style::default().fg(theme::text_muted())),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("↑↓", Style::default().fg(theme::accent())),
            Span::styled(" scroll", Style::default().fg(theme::text_muted())),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Ctrl+P", Style::default().fg(theme::accent())),
            Span::styled(" commands", Style::default().fg(theme::text_muted())),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Ctrl+Q", Style::default().fg(theme::accent())),
            Span::styled(" quit", Style::default().fg(theme::text_muted())),
        ]),
    ];

    let help_paragraph = Paragraph::new(help_lines).style(base_style);
    frame.render_widget(help_paragraph, sidebar_layout[1]);
}

/// Render a single context line
fn render_context_line(
    lines: &mut Vec<Line<'static>>,
    ctx: &crate::state::ContextElement,
    array_index: usize,
    state: &State,
    id_width: usize,
    spin: &str,
    base_style: Style,
) {
    let is_selected = array_index == state.selected_context;
    let icon = ctx.context_type.icon();

    // Check if this context is loading (has no cached content but needs it)
    let is_loading = ctx.cached_content.is_none() && ctx.context_type.needs_cache();

    // Build the line with right-aligned ID
    let shortcut = format!("{:>width$}", &ctx.id, width = id_width);
    let name = truncate_string(&ctx.name, 18);

    // Show spinner instead of token count when loading
    // Show page indicator for paginated panels
    let tokens_or_spinner = if is_loading {
        format!("{:>6}", spin)
    } else if ctx.total_pages > 1 {
        format!("{}/{}", ctx.current_page + 1, ctx.total_pages)
    } else {
        format_number(ctx.token_count)
    };

    let indicator = if is_selected { chars::ARROW_RIGHT } else { " " };

    // Selected element: orange text, no background change
    // Loading elements: dimmed
    let name_color = if is_loading {
        theme::text_muted()
    } else if is_selected {
        theme::accent()
    } else {
        theme::text_secondary()
    };
    let indicator_color = if is_selected { theme::accent() } else { theme::bg_base() };
    let tokens_color = if is_loading { theme::warning() } else { theme::accent_dim() };

    lines.push(Line::from(vec![
        Span::styled(format!(" {}", indicator), Style::default().fg(indicator_color)),
        Span::styled(format!(" {} ", shortcut), Style::default().fg(theme::text_muted())),
        Span::styled(icon, Style::default().fg(if is_selected { theme::accent() } else { theme::text_muted() })),
        Span::styled(format!("{:<18}", name), Style::default().fg(name_color)),
        Span::styled(format!("{:>6}", tokens_or_spinner), Style::default().fg(tokens_color)),
        Span::styled(" ", base_style),
    ]));
}
