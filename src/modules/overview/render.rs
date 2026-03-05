use ratatui::prelude::{Line, Span, Style};

use crate::modules::all_modules;
use crate::state::{State, get_context_type_meta};
use crate::ui::{
    chars,
    helpers::{Cell, format_number, render_table},
    theme,
};
use cp_base::cast::SafeCast as _;
use cp_mod_git::types::GitChangeType;

/// Horizontal separator line.
pub(super) fn separator() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!(" {}", chars::HORIZONTAL.repeat(60)),
            Style::default().fg(theme::border()),
        )]),
        Line::from(""),
    ]
}

/// Render the TOKEN USAGE section with progress bar.
pub(super) fn render_token_usage(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line<'_>> = Vec::new();

    let system_prompt = cp_mod_prompt::seed::get_active_agent_content(state);
    let system_prompt_tokens = crate::state::estimate_tokens(&system_prompt).saturating_mul(2);
    let tool_def_tokens = super::context::estimate_tool_definitions_tokens(state);
    let panel_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let total_tokens = system_prompt_tokens.saturating_add(tool_def_tokens).saturating_add(panel_tokens);
    let budget = state.effective_context_budget();
    let threshold = state.cleaning_threshold_tokens();
    let usage_pct = (total_tokens.to_f64() / budget.to_f64() * 100.0).min(100.0);

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("TOKEN USAGE".to_string(), Style::default().fg(theme::text_muted()).bold()),
    ]));
    text.push(Line::from(""));

    let current = format_number(total_tokens);
    let threshold_str = format_number(threshold);
    let budget_str = format_number(budget);
    let pct = format!("{usage_pct:.1}%");

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled(current, Style::default().fg(theme::text()).bold()),
        Span::styled(" / ".to_string(), Style::default().fg(theme::text_muted())),
        Span::styled(threshold_str, Style::default().fg(theme::warning())),
        Span::styled(" / ".to_string(), Style::default().fg(theme::text_muted())),
        Span::styled(budget_str, Style::default().fg(theme::accent()).bold()),
        Span::styled(format!(" ({pct})"), Style::default().fg(theme::text_muted())),
    ]));

    // Progress bar with threshold marker
    let bar_width = 60usize;
    let threshold_pct = state.cleaning_threshold;
    let filled = ((usage_pct / 100.0) * bar_width.to_f64()).to_usize();
    let threshold_pos = (threshold_pct.to_f64() * bar_width.to_f64()).to_usize();

    let bar_color = if total_tokens >= threshold {
        theme::error()
    } else if total_tokens.to_f64() >= threshold.to_f64() * 0.9 {
        theme::warning()
    } else {
        theme::accent()
    };

    let mut bar_spans = vec![Span::styled(" ".to_string(), base_style)];
    for i in 0..bar_width {
        let ch = if i == threshold_pos && threshold_pos < bar_width {
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

        bar_spans.push(Span::styled(ch, Style::default().fg(color)));
    }
    text.push(Line::from(bar_spans));

    text
}

/// Render the GIT STATUS section (branch + file changes summary table).
pub(super) fn render_git_status(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line<'_>> = Vec::new();
    let gs = cp_mod_git::types::GitState::get(state);

    if !gs.is_repo {
        return text;
    }

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("GIT".to_string(), Style::default().fg(theme::text_muted()).bold()),
    ]));
    text.push(Line::from(""));

    // Branch name
    if let Some(branch) = &gs.branch {
        let branch_color = if branch.starts_with("detached:") { theme::warning() } else { theme::accent() };
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("Branch: ".to_string(), Style::default().fg(theme::text_secondary())),
            Span::styled(branch.clone(), Style::default().fg(branch_color).bold()),
        ]));
    }

    if gs.file_changes.is_empty() {
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("Working tree clean".to_string(), Style::default().fg(theme::success())),
        ]));
    } else {
        text.push(Line::from(""));

        let mut total_add: i32 = 0;
        let mut total_del: i32 = 0;

        let header = [
            Cell::new("File", Style::default()),
            Cell::right("+", Style::default()),
            Cell::right("-", Style::default()),
            Cell::right("Net", Style::default()),
        ];

        let rows: Vec<Vec<Cell>> = gs
            .file_changes
            .iter()
            .map(|file| {
                total_add = total_add.saturating_add(file.additions);
                total_del = total_del.saturating_add(file.deletions);
                let net = file.additions.saturating_sub(file.deletions);

                let type_char = match file.change_type {
                    GitChangeType::Added => "A",
                    GitChangeType::Untracked => "U",
                    GitChangeType::Deleted => "D",
                    GitChangeType::Modified => "M",
                    GitChangeType::Renamed => "R",
                };

                let display_path = if file.path.len() > 38 {
                    format!("{}...{}", type_char, &file.path.get(file.path.len().saturating_sub(35)..).unwrap_or(""))
                } else {
                    format!("{} {}", type_char, file.path)
                };

                let net_color = match net.cmp(&0) {
                    std::cmp::Ordering::Greater => theme::success(),
                    std::cmp::Ordering::Less => theme::error(),
                    std::cmp::Ordering::Equal => theme::text_muted(),
                };
                let net_str = if net > 0 { format!("+{net}") } else { format!("{net}") };

                vec![
                    Cell::new(display_path, Style::default().fg(theme::text())),
                    Cell::right(format!("+{}", file.additions), Style::default().fg(theme::success())),
                    Cell::right(format!("-{}", file.deletions), Style::default().fg(theme::error())),
                    Cell::right(net_str, Style::default().fg(net_color)),
                ]
            })
            .collect();

        let total_net = total_add.saturating_sub(total_del);
        let total_net_color = match total_net.cmp(&0) {
            std::cmp::Ordering::Greater => theme::success(),
            std::cmp::Ordering::Less => theme::error(),
            std::cmp::Ordering::Equal => theme::text_muted(),
        };
        let total_net_str = if total_net > 0 { format!("+{total_net}") } else { format!("{total_net}") };

        let footer = [
            Cell::new("Total", Style::default().fg(theme::text())),
            Cell::right(format!("+{total_add}"), Style::default().fg(theme::success())),
            Cell::right(format!("-{total_del}"), Style::default().fg(theme::error())),
            Cell::right(total_net_str, Style::default().fg(total_net_color)),
        ];

        text.extend(render_table(&header, &rows, Some(&footer), 1));
    }

    text
}

/// Render the CONTEXT ELEMENTS section.
pub(super) fn render_context_elements(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line<'_>> = Vec::new();

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("CONTEXT ELEMENTS".to_string(), Style::default().fg(theme::text_muted()).bold()),
    ]));
    text.push(Line::from(""));

    let header = [
        Cell::new("ID", Style::default()),
        Cell::new("Type", Style::default()),
        Cell::right("Tokens", Style::default()),
        Cell::right("Acc", Style::default()),
        Cell::right("Cost", Style::default()),
        Cell::new("Hit", Style::default()),
        Cell::new("Refreshed", Style::default()),
        Cell::new("Details", Style::default()),
    ];

    let mut accumulated = 0usize;
    let now_ms = crate::app::panels::now_ms();
    let modules = all_modules();

    let mut rows: Vec<Vec<Cell>> = Vec::new();

    // --- System prompt entry ---
    let system_prompt = cp_mod_prompt::seed::get_active_agent_content(state);
    let system_prompt_tokens = crate::state::estimate_tokens(&system_prompt).saturating_mul(2);
    accumulated = accumulated.saturating_add(system_prompt_tokens);
    rows.push(vec![
        Cell::new("--", Style::default().fg(theme::text_muted())),
        Cell::new("system-prompt (×2)", Style::default().fg(theme::text_secondary())),
        Cell::right(format_number(system_prompt_tokens), Style::default().fg(theme::accent())),
        Cell::right(format_number(accumulated), Style::default().fg(theme::text_muted())),
        Cell::right("—", Style::default().fg(theme::text_muted())),
        Cell::new("—", Style::default().fg(theme::text_muted())),
        Cell::new("—", Style::default().fg(theme::text_muted())),
        Cell::new("", Style::default()),
    ]);

    // --- Tool definitions entry ---
    let tool_def_tokens = super::context::estimate_tool_definitions_tokens(state);
    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    accumulated = accumulated.saturating_add(tool_def_tokens);
    rows.push(vec![
        Cell::new("--", Style::default().fg(theme::text_muted())),
        Cell::new(format!("tool-defs ({enabled_count} enabled)"), Style::default().fg(theme::text_secondary())),
        Cell::right(format_number(tool_def_tokens), Style::default().fg(theme::accent())),
        Cell::right(format_number(accumulated), Style::default().fg(theme::text_muted())),
        Cell::right("—", Style::default().fg(theme::text_muted())),
        Cell::new("—", Style::default().fg(theme::text_muted())),
        Cell::new("—", Style::default().fg(theme::text_muted())),
        Cell::new("", Style::default()),
    ]);

    // --- Panels sorted by last_refresh_ms, with Conversation forced to end ---
    let mut sorted_contexts: Vec<&crate::state::ContextElement> = state.context.iter().collect();
    sorted_contexts.sort_by_key(|ctx| ctx.last_refresh_ms);

    // Partition: conversation ("chat") always last
    let (mut panels, mut conversation): (Vec<_>, Vec<_>) =
        sorted_contexts.into_iter().partition(|ctx| ctx.id != "chat");
    panels.append(&mut conversation);

    for ctx in &panels {
        // Look up display_name from registry, fallback to raw context type string
        let type_name =
            get_context_type_meta(ctx.context_type.as_str()).map_or(ctx.context_type.as_str(), |m| m.display_name);

        // Ask modules for detail string
        let details = modules.iter().find_map(|m| m.context_detail(ctx)).unwrap_or_default();

        let truncated_details = if details.len() > 30 {
            format!("{}...", &details.get(..details.floor_char_boundary(27)).unwrap_or(""))
        } else {
            details
        };

        // Format refresh time as relative
        let refreshed = if ctx.last_refresh_ms < 1_577_836_800_000 {
            "—".to_string()
        } else if now_ms > ctx.last_refresh_ms {
            crate::ui::helpers::format_time_ago(now_ms.saturating_sub(ctx.last_refresh_ms))
        } else {
            "now".to_string()
        };

        let icon = ctx.context_type.icon();
        let id_with_icon = format!("{}{}", icon, ctx.id);

        let cost_str = format!("${:.2}", ctx.panel_total_cost);
        let (hit_str, hit_color) =
            if ctx.panel_cache_hit { ("\u{2713}", theme::success()) } else { ("\u{2717}", theme::error()) };

        accumulated = accumulated.saturating_add(ctx.token_count);

        rows.push(vec![
            Cell::new(id_with_icon, Style::default().fg(theme::accent_dim())),
            Cell::new(type_name, Style::default().fg(theme::text_secondary())),
            Cell::right(format_number(ctx.token_count), Style::default().fg(theme::accent())),
            Cell::right(format_number(accumulated), Style::default().fg(theme::text_muted())),
            Cell::right(cost_str, Style::default().fg(theme::text_muted())),
            Cell::new(hit_str, Style::default().fg(hit_color)),
            Cell::new(refreshed, Style::default().fg(theme::text_muted())),
            Cell::new(truncated_details, Style::default().fg(theme::text_muted())),
        ]);
    }

    text.extend(render_table(&header, &rows, None, 1));

    text
}

pub(super) use super::render_details::render_statistics;
