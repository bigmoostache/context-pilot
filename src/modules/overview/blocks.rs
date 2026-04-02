//! IR block builders for the Overview panel.
//!
//! Produces `Vec<Block>` equivalents of `render.rs` functions, using
//! `cp_render` types instead of ratatui. Called from `OverviewPanel::blocks()`.

use cp_render::{Align, Block, Cell, Column, ProgressSegment, Semantic, Span};

use crate::modules::all_modules;
use crate::state::{State, get_context_type_meta};
use crate::ui::helpers::format_number as fmt_num;
use cp_base::cast::Safe as _;
use cp_mod_git::types::GitChangeType;

/// TOKEN USAGE section: header, usage line, and progress bar.
pub(super) fn token_usage_blocks(state: &State) -> Vec<Block> {
    let mut out = Vec::new();

    let system_prompt = cp_mod_prompt::seed::get_active_agent_content(state);
    let system_prompt_tokens = crate::state::estimate_tokens(&system_prompt).saturating_mul(2);
    let tool_def_tokens = super::context::estimate_tool_definitions_tokens(state);
    let panel_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let total_tokens = system_prompt_tokens.saturating_add(tool_def_tokens).saturating_add(panel_tokens);
    let budget = state.effective_context_budget();
    let threshold = state.cleaning_threshold_tokens();
    let usage_pct = (total_tokens.to_f64() / budget.to_f64() * 100.0).min(100.0);

    out.push(Block::Header(vec![Span::styled("TOKEN USAGE".to_owned(), Semantic::Muted)]));
    out.push(Block::Empty);

    let current = fmt_num(total_tokens);
    let threshold_str = fmt_num(threshold);
    let budget_str = fmt_num(budget);
    let pct = format!("{usage_pct:.1}%");

    out.push(Block::line(vec![
        Span::new(format!(" {current}")).bold(),
        Span::muted(" / ".to_owned()),
        Span::warning(threshold_str),
        Span::muted(" / ".to_owned()),
        Span::accent(budget_str).bold(),
        Span::muted(format!(" ({pct})")),
    ]));

    // Progress bar with two segments: used + remaining
    let used_pct = usage_pct.round().to_u8();
    let bar_semantic = if total_tokens >= threshold {
        Semantic::Error
    } else if total_tokens.to_f64() >= threshold.to_f64() * 0.9 {
        Semantic::Warning
    } else {
        Semantic::Accent
    };

    let threshold_pct = (state.cleaning_threshold * 100.0).round().to_u8();
    let label = format!("│ threshold {threshold_pct}%");

    out.push(Block::ProgressBar {
        segments: vec![ProgressSegment { percent: used_pct, semantic: bar_semantic, label: None }],
        label: Some(label),
    });

    out
}

/// GIT section: branch + file changes table.
pub(super) fn git_blocks(state: &State) -> Vec<Block> {
    let mut out = Vec::new();
    let gs = cp_mod_git::types::GitState::get(state);

    if !gs.is_repo {
        return out;
    }

    out.push(Block::Header(vec![Span::styled("GIT".to_owned(), Semantic::Muted)]));
    out.push(Block::Empty);

    // Branch name
    if let Some(branch) = &gs.branch {
        let branch_semantic = if branch.starts_with("detached:") { Semantic::Warning } else { Semantic::Accent };
        out.push(Block::line(vec![
            Span::muted(" Branch: ".to_owned()),
            Span::styled(branch.clone(), branch_semantic).bold(),
        ]));
    }

    if gs.file_changes.is_empty() {
        out.push(Block::line(vec![Span::success(" Working tree clean".to_owned())]));
    } else {
        out.push(Block::Empty);

        let columns = vec![
            Column { header: "File".to_owned(), align: Align::Left },
            Column { header: "+".to_owned(), align: Align::Right },
            Column { header: "-".to_owned(), align: Align::Right },
            Column { header: "Net".to_owned(), align: Align::Right },
        ];

        let mut total_add: i32 = 0;
        let mut total_del: i32 = 0;

        let mut rows: Vec<Vec<Cell>> = Vec::new();
        for file in &gs.file_changes {
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
                format!("{type_char} {}", file.path)
            };

            let net_semantic = match net.cmp(&0) {
                std::cmp::Ordering::Greater => Semantic::Success,
                std::cmp::Ordering::Less => Semantic::Error,
                std::cmp::Ordering::Equal => Semantic::Muted,
            };
            let net_str = if net > 0 { format!("+{net}") } else { format!("{net}") };

            rows.push(vec![
                Cell::styled(display_path, Semantic::Default),
                Cell::right(Span::success(format!("+{}", file.additions))),
                Cell::right(Span::error(format!("-{}", file.deletions))),
                Cell::right(Span::styled(net_str, net_semantic)),
            ]);
        }

        // Footer row (totals)
        let total_net = total_add.saturating_sub(total_del);
        let total_net_semantic = match total_net.cmp(&0) {
            std::cmp::Ordering::Greater => Semantic::Success,
            std::cmp::Ordering::Less => Semantic::Error,
            std::cmp::Ordering::Equal => Semantic::Muted,
        };
        let total_net_str = if total_net > 0 { format!("+{total_net}") } else { format!("{total_net}") };

        rows.push(vec![
            Cell::styled("Total".to_owned(), Semantic::Default),
            Cell::right(Span::success(format!("+{total_add}"))),
            Cell::right(Span::error(format!("-{total_del}"))),
            Cell::right(Span::styled(total_net_str, total_net_semantic)),
        ]);

        out.push(Block::Table { columns, rows });
    }

    out
}

/// CONTEXT ELEMENTS section: big table of all panels with token/cache info.
pub(super) fn context_elements_blocks(state: &State) -> Vec<Block> {
    let mut out = Vec::new();

    out.push(Block::Header(vec![Span::styled("CONTEXT ELEMENTS".to_owned(), Semantic::Muted)]));
    out.push(Block::Empty);

    let columns = vec![
        Column { header: "ID".to_owned(), align: Align::Left },
        Column { header: "Type".to_owned(), align: Align::Left },
        Column { header: "Tokens".to_owned(), align: Align::Right },
        Column { header: "Acc".to_owned(), align: Align::Right },
        Column { header: "Cost".to_owned(), align: Align::Right },
        Column { header: "Hit".to_owned(), align: Align::Left },
        Column { header: "❄".to_owned(), align: Align::Right },
        Column { header: "Miss".to_owned(), align: Align::Right },
        Column { header: "Refreshed".to_owned(), align: Align::Left },
        Column { header: "Details".to_owned(), align: Align::Left },
    ];

    let mut accumulated = 0usize;
    let now_ms = crate::app::panels::now_ms();
    let modules = all_modules();

    let mut rows: Vec<Vec<Cell>> = Vec::new();

    // System prompt entry
    let system_prompt = cp_mod_prompt::seed::get_active_agent_content(state);
    let system_prompt_tokens = crate::state::estimate_tokens(&system_prompt).saturating_mul(2);
    accumulated = accumulated.saturating_add(system_prompt_tokens);
    rows.push(vec![
        Cell::styled("--".to_owned(), Semantic::Muted),
        Cell::styled("system-prompt (×2)".to_owned(), Semantic::Muted),
        Cell::right(Span::accent(fmt_num(system_prompt_tokens))),
        Cell::right(Span::muted(fmt_num(accumulated))),
        Cell::styled("—".to_owned(), Semantic::Muted),
        Cell::styled("—".to_owned(), Semantic::Muted),
        Cell::right(Span::muted("—".to_owned())),
        Cell::right(Span::muted("—".to_owned())),
        Cell::styled("—".to_owned(), Semantic::Muted),
        Cell::empty(),
    ]);

    // Tool definitions entry
    let tool_def_tokens = super::context::estimate_tool_definitions_tokens(state);
    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    accumulated = accumulated.saturating_add(tool_def_tokens);
    rows.push(vec![
        Cell::styled("--".to_owned(), Semantic::Muted),
        Cell::styled(format!("tool-defs ({enabled_count} enabled)"), Semantic::Muted),
        Cell::right(Span::accent(fmt_num(tool_def_tokens))),
        Cell::right(Span::muted(fmt_num(accumulated))),
        Cell::styled("—".to_owned(), Semantic::Muted),
        Cell::styled("—".to_owned(), Semantic::Muted),
        Cell::right(Span::muted("—".to_owned())),
        Cell::right(Span::muted("—".to_owned())),
        Cell::styled("—".to_owned(), Semantic::Muted),
        Cell::empty(),
    ]);

    // Panels sorted by last_refresh_ms, conversation forced to end
    let mut sorted_contexts: Vec<&crate::state::Entry> = state.context.iter().collect();
    sorted_contexts.sort_by_key(|ctx| ctx.last_refresh_ms);

    let (mut panels, mut conversation): (Vec<_>, Vec<_>) =
        sorted_contexts.into_iter().partition(|ctx| ctx.id != "chat");
    panels.append(&mut conversation);

    for ctx in &panels {
        let type_name =
            get_context_type_meta(ctx.context_type.as_str()).map_or(ctx.context_type.as_str(), |m| m.display_name);

        let details = modules.iter().find_map(|m| m.context_detail(ctx)).unwrap_or_default();

        let truncated_details = if details.len() > 30 {
            format!("{}...", &details.get(..details.floor_char_boundary(27)).unwrap_or(""))
        } else {
            details
        };

        let refreshed = if ctx.last_refresh_ms < 1_577_836_800_000 {
            "—".to_string()
        } else if now_ms > ctx.last_refresh_ms {
            crate::ui::helpers::format_time_ago(now_ms.saturating_sub(ctx.last_refresh_ms))
        } else {
            "now".to_string()
        };

        let icon = ctx.context_type.icon();
        let id_with_icon = format!("{icon}{}", ctx.id);

        let cost_str = format!("${:.2}", ctx.panel_total_cost);
        let (hit_str, hit_semantic) = if ctx.panel_cache_hit {
            ("\u{2713}".to_string(), Semantic::Success)
        } else if ctx.freeze_count > 0 {
            let panel = crate::app::panels::get_panel(&ctx.context_type);
            let max = panel.max_freezes();
            (format!("\u{2717} ({}/{})", ctx.freeze_count, max), Semantic::Warning)
        } else {
            ("\u{2717}".to_string(), Semantic::Error)
        };

        let freeze_str = if ctx.total_freezes > 0 { format!("{}", ctx.total_freezes) } else { String::new() };
        let freeze_semantic = if ctx.total_freezes > 0 { Semantic::AccentDim } else { Semantic::Muted };

        let miss_str = if ctx.total_cache_misses > 0 { format!("{}", ctx.total_cache_misses) } else { String::new() };
        let miss_semantic = if ctx.total_cache_misses > 0 { Semantic::Warning } else { Semantic::Muted };

        accumulated = accumulated.saturating_add(ctx.token_count);

        rows.push(vec![
            Cell::styled(id_with_icon, Semantic::AccentDim),
            Cell::styled(type_name.to_owned(), Semantic::Muted),
            Cell::right(Span::accent(fmt_num(ctx.token_count))),
            Cell::right(Span::muted(fmt_num(accumulated))),
            Cell::right(Span::muted(cost_str)),
            Cell::styled(hit_str, hit_semantic),
            Cell::right(Span::styled(freeze_str, freeze_semantic)),
            Cell::right(Span::styled(miss_str, miss_semantic)),
            Cell::styled(refreshed, Semantic::Muted),
            Cell::styled(truncated_details, Semantic::Muted),
        ]);
    }

    out.push(Block::Table { columns, rows });
    out
}

/// STATISTICS section: panel counts, message counts, todos, memories.
pub(super) fn statistics_blocks(state: &State) -> Vec<Block> {
    use cp_mod_memory::types::{MemoryImportance, MemoryState};
    use cp_mod_todo::types::{TodoState, TodoStatus};

    let mut out = Vec::new();

    out.push(Block::Header(vec![Span::styled("STATISTICS".to_owned(), Semantic::Muted)]));
    out.push(Block::Empty);

    // Panel counts
    let panel_count = state.context.len().saturating_add(2);
    let fixed_count = state.context.iter().filter(|c| c.context_type.is_fixed()).count();
    let dynamic_count = panel_count.saturating_sub(fixed_count).saturating_sub(2);

    out.push(Block::line(vec![
        Span::muted(" Panels: ".to_owned()),
        Span::new(format!("{panel_count}")).bold(),
        Span::muted(format!(" ({fixed_count} fixed, {dynamic_count} dynamic, 1 system, 1 tools)")),
    ]));

    // Message counts
    let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
    let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
    let total_msgs = state.messages.len();

    out.push(Block::line(vec![
        Span::muted(" Messages: ".to_owned()),
        Span::new(format!("{total_msgs}")).bold(),
        Span::muted(format!(" ({user_msgs} user, {assistant_msgs} assistant)")),
    ]));

    // Todos
    let ts = TodoState::get(state);
    let total_todos = ts.todos.len();
    if total_todos > 0 {
        let done_todos = ts.todos.iter().filter(|t| t.status == TodoStatus::Done).count();
        let in_progress = ts.todos.iter().filter(|t| t.status == TodoStatus::InProgress).count();
        let pending = total_todos.saturating_sub(done_todos).saturating_sub(in_progress);

        out.push(Block::line(vec![
            Span::muted(" Todos: ".to_owned()),
            Span::success(format!("{done_todos}/{total_todos}")).bold(),
            Span::muted(" done".to_owned()),
            Span::muted(format!(", {in_progress} in progress, {pending} pending")),
        ]));
    }

    // Memories
    let mems = &MemoryState::get(state).memories;
    let total_memories = mems.len();
    if total_memories > 0 {
        let critical = mems.iter().filter(|m| m.importance == MemoryImportance::Critical).count();
        let high = mems.iter().filter(|m| m.importance == MemoryImportance::High).count();
        let medium = mems.iter().filter(|m| m.importance == MemoryImportance::Medium).count();
        let low = mems.iter().filter(|m| m.importance == MemoryImportance::Low).count();

        out.push(Block::line(vec![
            Span::muted(" Memories: ".to_owned()),
            Span::new(format!("{total_memories}")).bold(),
            Span::muted(format!(" ({critical} critical, {high} high, {medium} medium, {low} low)")),
        ]));
    }

    out
}
