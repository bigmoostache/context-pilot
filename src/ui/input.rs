use ratatui::{
    prelude::*,
    widgets::Paragraph,
};

use crate::llms::{LlmProvider, ModelInfo};
use crate::state::State;
use crate::modules::git::types::GitChangeType;
use super::{theme, spinner};

pub fn render_status_bar(frame: &mut Frame, state: &State, area: Rect) {
    let base_style = Style::default().bg(theme::bg_base()).fg(theme::text_muted());
    let spin = spinner::spinner(state.spinner_frame);

    let mut spans = vec![
        Span::styled(" ", base_style),
    ];

    // Show all active states as separate badges with spinners
    if state.is_streaming {
        spans.push(Span::styled(
            format!(" {} STREAMING ", spin),
            Style::default().fg(theme::bg_base()).bg(theme::success()).bold()
        ));
        spans.push(Span::styled(" ", base_style));
    }

    if state.pending_tldrs > 0 {
        spans.push(Span::styled(
            format!(" {} SUMMARIZING {} ", spin, state.pending_tldrs),
            Style::default().fg(theme::bg_base()).bg(theme::warning()).bold()
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // Count loading context elements (those without cached content)
    let loading_count = state.context.iter()
        .filter(|c| c.cached_content.is_none() && c.context_type.needs_cache())
        .count();

    if loading_count > 0 {
        spans.push(Span::styled(
            format!(" {} LOADING {} ", spin, loading_count),
            Style::default().fg(theme::bg_base()).bg(theme::text_muted()).bold()
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // If nothing active, show READY
    if !state.is_streaming && state.pending_tldrs == 0 && loading_count == 0 {
        spans.push(Span::styled(" READY ", Style::default().fg(theme::bg_base()).bg(theme::text_muted()).bold()));
        spans.push(Span::styled(" ", base_style));
    }

    // Show current LLM provider and model
    let (provider_name, model_name) = match state.llm_provider {
        LlmProvider::Anthropic => ("Claude", state.anthropic_model.display_name()),
        LlmProvider::ClaudeCode => ("OAuth", state.anthropic_model.display_name()),
        LlmProvider::Grok => ("Grok", state.grok_model.display_name()),
        LlmProvider::Groq => ("Groq", state.groq_model.display_name()),
        LlmProvider::DeepSeek => ("DeepSeek", state.deepseek_model.display_name()),
    };
    spans.push(Span::styled(
        format!(" {} ", provider_name),
        Style::default().fg(theme::bg_base()).bg(theme::accent_dim()).bold()
    ));
    spans.push(Span::styled(" ", base_style));
    spans.push(Span::styled(
        format!(" {} ", model_name),
        Style::default().fg(theme::text()).bg(theme::bg_elevated())
    ));
    spans.push(Span::styled(" ", base_style));

    // Stop reason from last stream (highlight max_tokens as warning)
    if !state.is_streaming {
        if let Some(ref reason) = state.last_stop_reason {
            let (label, style) = if reason == "max_tokens" {
                ("MAX_TOKENS".to_string(), Style::default().fg(theme::bg_base()).bg(theme::error()).bold())
            } else {
                (reason.to_uppercase(), Style::default().fg(theme::text()).bg(theme::bg_elevated()))
            };
            spans.push(Span::styled(format!(" {} ", label), style));
            spans.push(Span::styled(" ", base_style));
        }
    }

    // Git branch (if available)
    if let Some(branch) = &state.git_branch {
        spans.push(Span::styled(
            format!(" {} ", branch),
            Style::default().fg(Color::White).bg(Color::Blue)
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // Git change stats (if there are any changes)
    if !state.git_file_changes.is_empty() {
        // Calculate line change statistics
        let mut total_additions = 0;
        let mut total_deletions = 0;
        let mut untracked_count = 0;
        let mut modified_count = 0;
        let mut deleted_count = 0;

        for file in &state.git_file_changes {
            total_additions += file.additions;
            total_deletions += file.deletions;
            match file.change_type {
                GitChangeType::Untracked => untracked_count += 1,
                GitChangeType::Modified => modified_count += 1,
                GitChangeType::Deleted => deleted_count += 1,
                GitChangeType::Added => modified_count += 1, // Added files count as modified for UI
                GitChangeType::Renamed => modified_count += 1, // Renamed files count as modified
            }
        }

        let net_change = total_additions - total_deletions;
        
        // Card 1: Line changes (additions/deletions/net) with industry standard colors
        // Format: "+11 -13 -2" (additions green, deletions red, net with appropriate color)
        // Add line change card with individual color spans
        // +{additions} (green) -{deletions} (red) {net} (green for positive, red for negative)
        let (net_prefix, net_value) = if net_change >= 0 {
            ("+", net_change)
        } else {
            ("", net_change)
        };
        
        spans.push(Span::styled(" +", Style::default().fg(theme::success())));
        spans.push(Span::styled(format!("{} ", total_additions), Style::default().fg(theme::success()).bold()));
        spans.push(Span::styled("-", Style::default().fg(theme::error())));
        spans.push(Span::styled(format!("{} ", total_deletions), Style::default().fg(theme::error()).bold()));
        spans.push(Span::styled(net_prefix, Style::default().fg(if net_change >= 0 { theme::success() } else { theme::error() })));
        spans.push(Span::styled(format!("{} ", net_value.abs()), Style::default().fg(if net_change >= 0 { theme::success() } else { theme::error() }).bold()));
        spans.push(Span::styled(" ", base_style));

        // Card 2: File changes (U/M/D) with industry standard colors
        // Format: "U0 M9 D0" (untracked green, modified orange, deleted red)
        spans.push(Span::styled("U", Style::default().fg(theme::success())));
        spans.push(Span::styled(format!("{} ", untracked_count), Style::default().fg(theme::success()).bold()));
        spans.push(Span::styled("M", Style::default().fg(theme::warning())));
        spans.push(Span::styled(format!("{} ", modified_count), Style::default().fg(theme::warning()).bold()));
        spans.push(Span::styled("D", Style::default().fg(theme::error())));
        spans.push(Span::styled(format!("{} ", deleted_count), Style::default().fg(theme::error()).bold()));
        spans.push(Span::styled(" ", base_style));
    }

    // Right side info
    let char_count = state.input.chars().count();
    let right_info = if char_count > 0 {
        format!("{} chars ", char_count)
    } else {
        String::new()
    };

    let left_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let right_width = right_info.len();
    let padding = (area.width as usize).saturating_sub(left_width + right_width);

    spans.push(Span::styled(" ".repeat(padding), base_style));
    spans.push(Span::styled(&right_info, base_style));

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}
