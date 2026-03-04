use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use super::{helpers::spinner, theme};
use crate::llms::{LlmProvider, ModelInfo};
use crate::state::State;

use cp_base::cast::SafeCast;
use cp_mod_prompt::PromptState;
use cp_mod_queue::QueueState;

pub(super) fn render_status_bar(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let base_style = Style::default().bg(theme::bg_base()).fg(theme::text_muted());
    let spin = spinner(state.spinner_frame);

    let mut spans = vec![Span::styled(" ", base_style)];

    // === Primary status badge (mutually exclusive, priority order) ===
    let has_question_form = state.get_ext::<cp_base::ui::PendingQuestionForm>().is_some();
    let has_timed_watcher = {
        use cp_base::state::watchers::WatcherRegistry;
        state
            .get_ext::<WatcherRegistry>()
            .is_some_and(|reg| reg.active_watchers().iter().any(|w| w.fire_at_ms().is_some()))
    };

    if let Some(ref reason) = state.guard_rail_blocked {
        // BLOCKED — guard rail is preventing auto-continuation
        spans.push(Span::styled(
            format!(" BLOCKED: {} ", reason),
            Style::default().fg(theme::bg_base()).bg(theme::error()).bold(),
        ));
    } else if state.is_streaming && !state.is_tooling {
        // STREAMING — actively receiving tokens from API
        spans.push(Span::styled(
            format!(" {} STREAMING ", spin),
            Style::default().fg(theme::bg_base()).bg(theme::success()).bold(),
        ));
    } else if state.is_streaming && state.is_tooling {
        // TOOLING — stream active but executing tool calls (same blue as branch name)
        spans.push(Span::styled(
            format!(" {} TOOLING ", spin),
            Style::default().fg(Color::White).bg(Color::Blue).bold(),
        ));
    } else if has_question_form {
        // QUESTIONING — waiting for user to answer a question form
        spans.push(Span::styled(" QUESTIONING ", Style::default().fg(theme::bg_base()).bg(theme::warning()).bold()));
    } else if has_timed_watcher {
        // WAITING — idle but a coucou/watcher will fire
        spans.push(Span::styled(
            format!(" {} WAITING ", spin),
            Style::default().fg(Color::White).bg(Color::Magenta).bold(),
        ));
    } else {
        // READY — default idle state
        spans.push(Span::styled(" READY ", Style::default().fg(theme::bg_base()).bg(theme::text_muted()).bold()));
    }
    spans.push(Span::styled(" ", base_style));

    // Show retry count when retrying after API errors (secondary badge)
    if state.api_retry_count > 0 {
        spans.push(Span::styled(
            format!(" RETRY {}/{} ", state.api_retry_count, crate::infra::constants::MAX_API_RETRIES),
            Style::default().fg(theme::bg_base()).bg(theme::error()).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // Count loading context elements (secondary badge)
    let loading_count =
        state.context.iter().filter(|c| c.cached_content.is_none() && c.context_type.needs_cache()).count();

    if loading_count > 0 {
        spans.push(Span::styled(
            format!(" {} LOADING {} ", spin, loading_count),
            Style::default().fg(theme::bg_base()).bg(theme::text_muted()).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // Show current LLM provider and model
    let (provider_name, model_name) = match state.llm_provider {
        LlmProvider::Anthropic => ("Claude", state.anthropic_model.display_name()),
        LlmProvider::ClaudeCode => ("OAuth", state.anthropic_model.display_name()),
        LlmProvider::ClaudeCodeApiKey => ("APIKey", state.anthropic_model.display_name()),
        LlmProvider::Grok => ("Grok", state.grok_model.display_name()),
        LlmProvider::Groq => ("Groq", state.groq_model.display_name()),
        LlmProvider::DeepSeek => ("DeepSeek", state.deepseek_model.display_name()),
    };
    spans.push(Span::styled(
        format!(" {} ", provider_name),
        Style::default().fg(theme::bg_base()).bg(theme::accent_dim()).bold(),
    ));
    spans.push(Span::styled(" ", base_style));
    spans.push(Span::styled(format!(" {} ", model_name), Style::default().fg(theme::text()).bg(theme::bg_elevated())));
    spans.push(Span::styled(" ", base_style));

    // Stop reason from last stream (highlight max_tokens as warning)
    if !state.is_streaming
        && let Some(ref reason) = state.last_stop_reason
    {
        let (label, style) = if reason == "max_tokens" {
            ("MAX_TOKENS".to_string(), Style::default().fg(theme::bg_base()).bg(theme::error()).bold())
        } else {
            (reason.to_uppercase(), Style::default().fg(theme::text()).bg(theme::bg_elevated()))
        };
        spans.push(Span::styled(format!(" {} ", label), style));
        spans.push(Span::styled(" ", base_style));
    }

    // Active agent card
    let ps = PromptState::get(state);
    if let Some(ref agent_id) = ps.active_agent_id {
        let agent_name =
            ps.agents.iter().find(|a| &a.id == agent_id).map_or_or(agent_id.as_str(), agent_id.as_str(), |a| a.name.as_str())yled(
            format!(" 🤖 {} ", agent_name),
            Style::default().fg(Color::White).bg(Color::Rgb(130, 80, 200)).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // Loaded skill cards
    for skill_id in &ps.loaded_skill_ids {
        let skill_name =
            ps.skills.iter().find(|s| s.id == *skill_id).map_or_or(skill_id.as_str(), skill_id.as_str(), |s| s.name.as_str())yled(
            format!(" 📚 {} ", skill_name),
            Style::default().fg(theme::bg_base()).bg(theme::assistant()).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // Git branch (if available)
    let gs = cp_mod_git::GitState::get(state);
    if let Some(branch) = &gs.git_branch {
        spans.push(Span::styled(format!(" {} ", branch), Style::default().fg(Color::White).bg(Color::Blue)));
        spans.push(Span::styled(" ", base_style));
    }

    // Git change stats (if there are any changes)
    if !gs.git_file_changes.is_empty() {
        use cp_mod_git::GitChangeType;

        let mut total_additions: i32 = 0;
        let mut total_deletions: i32 = 0;
        let mut untracked_count = 0;
        let mut modified_count = 0;
        let mut deleted_count = 0;

        for file in &gs.git_file_changes {
            total_additions += file.additions;
            total_deletions += file.deletions;
            match file.change_type {
                GitChangeType::Untracked => untracked_count += 1,
                GitChangeType::Modified => modified_count += 1,
                GitChangeType::Deleted => deleted_count += 1,
                GitChangeType::Added => modified_count += 1,
                GitChangeType::Renamed => modified_count += 1,
            }
        }

        // Line changes card: +N/-N/net
        let net_change = total_additions - total_deletions;
        let (net_prefix, net_color) = if net_change >= 0 { ("+", theme::success()) } else { ("", theme::error()) };
        let bg = theme::bg_elevated();

        spans.push(Span::styled(format!(" +{}", total_additions), Style::default().fg(theme::success()).bg(bg).bold()));
        spans.push(Span::styled(format!("/-{}", total_deletions), Style::default().fg(theme::error()).bg(bg).bold()));
        spans.push(Span::styled(
            format!("/{}{} ", net_prefix, net_change.abs()),
            Style::default().fg(net_color).bg(bg).bold(),
        ));
        spans.push(Span::styled(" ", base_style));

        // File changes card: U/M/D
        spans.push(Span::styled(format!(" U{}", untracked_count), Style::default().fg(theme::success()).bg(bg).bold()));
        spans.push(Span::styled(format!("/M{}", modified_count), Style::default().fg(theme::warning()).bg(bg).bold()));
        spans.push(Span::styled(format!("/D{} ", deleted_count), Style::default().fg(theme::error()).bg(bg).bold()));
        spans.push(Span::styled(" ", base_style));
    }

    // Auto-continuation status card (always visible)
    {
        use crate::infra::config::normalize_icon;
        use cp_mod_spine::SpineState;
        let spine_cfg = &SpineState::get(state).config;
        let (icon, bg_color) = if spine_cfg.continue_until_todos_done {
            (normalize_icon("🔁"), theme::warning())
        } else {
            (normalize_icon("🔄"), theme::text_muted())
        };
        let label = if spine_cfg.continue_until_todos_done { "Auto-continue" } else { "No Auto-continue" };
        spans.push(Span::styled(
            format!(" {}{} ", icon, label),
            Style::default().fg(theme::bg_base()).bg(bg_color).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // Active reverie cards — one per running background optimizer
    for rev in state.reveries.values() {
        // Look up the agent's display name from PromptState
        let ps = PromptState::get(state);
        let agent_name =
            ps.agents.iter().find(|a| a.id == rev.agent_id).map_or_or(&rev.agent_id, &rev.agent_id, |a| a.name.as_str())= rev.tool_call_count;
        let rev_spin = if rev.is_streaming { format!("{} ", spin) } else { String::new() };
        spans.push(Span::styled(
            format!(" {}🧠 {} ({} tools) ", rev_spin, agent_name, tools_done),
            Style::default().fg(Color::White).bg(Color::Rgb(100, 60, 160)).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // Queue status card — visible when queue is active
    {
        let qs = QueueState::get(state);
        if qs.active {
            let n = qs.queued_calls.len();
            spans.push(Span::styled(
                format!(" ⏳ Queue ({}) ", n),
                Style::default().fg(Color::White).bg(Color::Rgb(180, 120, 40)).bold(),
            ));
            spans.push(Span::styled(" ", base_style));
        }
    }

    // Right side info
    let char_count = state.input.chars().count();
    let right_info = if char_count > 0 { format!("{} chars ", char_count) } else { String::new() };

    let left_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let right_width = right_info.len();
    let padding = (area.width.to_usize()).saturating_sub(left_width + right_width);

    spans.push(Span::styled(" ".repeat(padding), base_style));
    spans.push(Span::styled(&right_info, base_style));

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

/// Calculate the height needed for the question form
pub(super) fn calculate_question_form_height(form: &cp_base::ui::PendingQuestionForm) -> u16 {
    let q = &form.questions[form.current_question];
    // Header line + question text + blank + options (including Other) + blank + nav hint
    let option_lines = q.options.len().to_u16() + 1; // +1 for "Other"
    let header_lines = 2u16; // header + question text
    let chrome = 4u16; // borders (2) + spacing + nav hint
    (header_lines + option_lines * 2 + chrome).min(20) // each option: label + description
}

/// Render the question form at the bottom of the screen
pub(super) fn render_question_form(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let Some(form) = state.get_ext::<cp_base::ui::PendingQuestionForm>() else { return };

    let q_idx = form.current_question;
    let q = &form.questions[q_idx];
    let ans = &form.answers[q_idx];
    let other_idx = q.options.len();

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Progress indicator
    let progress =
        if form.questions.len() > 1 { format!(" ({}/{}) ", q_idx + 1, form.questions.len()) } else { String::new() };

    // Question text
    lines.push(Line::from(vec![
        Span::styled(format!(" {} ", q.header), Style::default().fg(theme::bg_base()).bg(theme::accent()).bold()),
        Span::styled(format!(" {}", q.question), Style::default().fg(theme::text()).bold()),
    ]));
    lines.push(Line::from(""));

    // Options
    for (i, opt) in q.options.iter().enumerate() {
        let is_cursor = ans.cursor == i;
        let is_selected = ans.selected.contains(&i);

        let indicator = if is_selected && q.multi_select {
            "[x]"
        } else if is_selected {
            "(●)"
        } else if q.multi_select {
            "[ ]"
        } else {
            "( )"
        };

        let cursor_marker = if is_cursor { ">" } else { " " };

        let label_style = if is_cursor {
            Style::default().fg(theme::accent()).bold()
        } else if is_selected {
            Style::default().fg(theme::success()).bold()
        } else {
            Style::default().fg(theme::text())
        };

        let desc_style = if is_cursor {
            Style::default().fg(theme::text_secondary())
        } else {
            Style::default().fg(theme::text_muted())
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", cursor_marker), Style::default().fg(theme::accent())),
            Span::styled(format!("{} ", indicator), label_style),
            Span::styled(opt.label.clone(), label_style),
            Span::styled(format!("  {}", opt.description), desc_style),
        ]));
    }

    // "Other" option
    {
        let is_cursor = ans.cursor == other_idx;
        let is_typing = ans.typing_other;

        let cursor_marker = if is_cursor { ">" } else { " " };
        let indicator = if is_typing { "(●)" } else { "( )" };

        let label_style = if is_cursor {
            Style::default().fg(theme::accent()).bold()
        } else if is_typing {
            Style::default().fg(theme::success()).bold()
        } else {
            Style::default().fg(theme::text())
        };

        if is_typing {
            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", cursor_marker), Style::default().fg(theme::accent())),
                Span::styled(format!("{} ", indicator), label_style),
                Span::styled("Other: ", label_style),
                Span::styled(
                    format!("{}▏", ans.other_text),
                    Style::default().fg(theme::text()).bg(theme::bg_elevated()),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", cursor_marker), Style::default().fg(theme::accent())),
                Span::styled(format!("{} ", indicator), label_style),
                Span::styled("Other", label_style),
                Span::styled("  Type your own answer", Style::default().fg(theme::text_muted())),
            ]));
        }
    }

    // Navigation hint
    lines.push(Line::from(""));
    let hint_spans = if q.multi_select {
        vec![
            Span::styled(" ↑↓", Style::default().fg(theme::accent())),
            Span::styled(" navigate  ", Style::default().fg(theme::text_muted())),
            Span::styled("←→", Style::default().fg(theme::accent())),
            Span::styled(" questions  ", Style::default().fg(theme::text_muted())),
            Span::styled("Space", Style::default().fg(theme::accent())),
            Span::styled(" toggle  ", Style::default().fg(theme::text_muted())),
            Span::styled("Enter", Style::default().fg(theme::accent())),
            Span::styled(" confirm  ", Style::default().fg(theme::text_muted())),
            Span::styled("Esc", Style::default().fg(theme::accent())),
            Span::styled(" dismiss", Style::default().fg(theme::text_muted())),
        ]
    } else {
        vec![
            Span::styled(" ↑↓", Style::default().fg(theme::accent())),
            Span::styled(" navigate  ", Style::default().fg(theme::text_muted())),
            Span::styled("←→", Style::default().fg(theme::accent())),
            Span::styled(" questions  ", Style::default().fg(theme::text_muted())),
            Span::styled("Enter", Style::default().fg(theme::accent())),
            Span::styled(" select & next  ", Style::default().fg(theme::text_muted())),
            Span::styled("Esc", Style::default().fg(theme::accent())),
            Span::styled(" dismiss", Style::default().fg(theme::text_muted())),
        ]
    };
    lines.push(Line::from(hint_spans));

    let title = format!(" Question{} ", progress);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::accent()))
        .style(Style::default().bg(theme::bg_surface()))
        .title(Span::styled(title, Style::default().fg(theme::accent()).bold()));

    let paragraph = Paragraph::new(lines).block(block).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Calculate the height needed for the autocomplete popup
pub(super) fn calculate_autocomplete_height(ac: &cp_base::autocomplete::AutocompleteState) -> u16 {
    let visible = ac.visible_matches().len().to_u16();
    // matches + border chrome (2)
    (visible + 2).clamp(4, 12)
}

/// Render the @ autocomplete popup above the input area (bottom of content panel, growing upward)
pub(super) fn render_autocomplete_popup(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let ac = match state.get_ext::<cp_base::autocomplete::AutocompleteState>() {
        Some(ac) if ac.active => ac,
        _ => return,
    };

    let popup_width = 60u16.min(area.width.saturating_sub(2));
    let popup_height = calculate_autocomplete_height(ac);

    // The input field (🦊 ...) occupies `input_visual_lines` at the bottom of the
    // conversation panel viewport. We want the popup's bottom edge to sit just above
    // the first line of the input field.
    //
    // area = the content region (right of sidebar, above status bar).
    // The conversation panel fills this area with a 1-cell border on each side,
    // so usable inner height = area.height - 2 (top/bottom border).
    // The input starts at: area.bottom() - 1 (bottom border) - input_visual_lines
    // We place the popup bottom at that position.
    let border_chrome = 2u16; // top + bottom border of the conversation panel
    let input_lines = ac.input_visual_lines;
    let scroll_padding = 2u16; // padding lines below input in the conversation panel
    let popup_bottom = area.y + area.height.saturating_sub(border_chrome + input_lines + scroll_padding);
    let popup_top = popup_bottom.saturating_sub(popup_height);
    // Clamp: don't go above the top of the content area (+1 for border)
    let y = popup_top.max(area.y + 1);
    let clamped_height = popup_bottom.saturating_sub(y);
    if clamped_height < 3 {
        return; // Not enough space to render
    }

    let x = area.x + 1; // +1 to clear the panel's left border
    let popup_area = Rect::new(x, y, popup_width, clamped_height);

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Show matches
    let visible = ac.visible_matches();
    if visible.is_empty() {
        lines.push(Line::from(vec![Span::styled("  No matches", Style::default().fg(theme::text_muted()))]));
    } else {
        for (i, entry) in visible.iter().enumerate() {
            let abs_idx = ac.scroll_offset + i;
            let is_selected = abs_idx == ac.selected;

            let cursor_marker = if is_selected { ">" } else { " " };
            let path_style = if is_selected {
                Style::default().fg(theme::accent()).bold()
            } else {
                Style::default().fg(theme::text())
            };

            let suffix = if entry.is_dir { "/" } else { "" };
            let icon = if entry.is_dir { "📁 " } else { "   " };
            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", cursor_marker), Style::default().fg(theme::accent())),
                Span::styled(icon.to_string(), Style::default()),
                Span::styled(format!("{}{}", entry.name, suffix), path_style),
            ]));
        }
    }

    // Count indicator
    let dir_label = if ac.dir_prefix.is_empty() { ".".to_string() } else { ac.dir_prefix.clone() };
    let count_text = format!(" @{} — {}/{} in {}/ ", ac.query, ac.matches.len(), ac.matches.len(), dir_label);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::accent()))
        .style(Style::default().bg(theme::bg_surface()))
        .title(Span::styled(count_text, Style::default().fg(theme::accent()).bold()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup_area);
    frame.render_widget(paragraph, popup_area);
}
