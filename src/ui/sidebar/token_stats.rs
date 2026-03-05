use ratatui::prelude::{Line, Span, Style};

use super::super::{
    helpers::{Cell, format_number, render_table},
    theme,
};
use crate::state::State;

/// Render the token statistics section (cache hit / miss / output table + total cost).
///
/// Returns a `Vec<Line>` to be appended to the sidebar content.
pub(super) fn render_token_stats(state: &State) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    if state.cache_hit_tokens == 0 && state.cache_miss_tokens == 0 && state.total_output_tokens == 0 {
        return lines;
    }

    // Get current model pricing
    let hit_price = state.cache_hit_price_per_mtok();
    let miss_price = state.cache_miss_price_per_mtok();
    let out_price = state.output_price_per_mtok();

    // Helper: format cost in dollars with appropriate precision
    let format_cost = |tokens: usize, price_per_mtok: f32| -> String {
        let cost = State::token_cost(tokens, price_per_mtok);
        if cost < 0.001 {
            String::new()
        } else if cost < 0.01 {
            format!("{cost:.3}")
        } else if cost < 1.0 {
            format!("{cost:.2}")
        } else {
            format!("{cost:.1}")
        }
    };

    // Build table rows: each row has [label, ↑hit, ✗miss, ↓out]
    // We interleave counts rows and costs rows
    let hit_icon = super::super::chars::ARROW_UP.to_string();
    let miss_icon = super::super::chars::CROSS.to_string();
    let out_icon = super::super::chars::ARROW_DOWN.to_string();

    let header = [
        Cell::new("", Style::default()),
        Cell::right(format!("{hit_icon} hit"), Style::default().fg(theme::success())),
        Cell::right(format!("{miss_icon} miss"), Style::default().fg(theme::warning())),
        Cell::right(format!("{out_icon} out"), Style::default().fg(theme::accent_dim())),
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

        let fmt = |cost: &str| -> String { if cost.is_empty() { String::new() } else { format!("${cost}") } };

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
    let total_cost = State::token_cost(state.cache_hit_tokens, hit_price)
        + State::token_cost(state.cache_miss_tokens, miss_price)
        + State::token_cost(state.total_output_tokens, out_price);
    if total_cost >= 0.001 {
        let total_str = if total_cost < 0.01 { format!("${total_cost:.3}") } else { format!("${total_cost:.2}") };
        lines.push(Line::from(vec![Span::styled(
            format!(" total: {total_str}"),
            Style::default().fg(theme::text_muted()),
        )]));
    }

    lines
}
