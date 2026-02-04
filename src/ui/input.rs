use ratatui::{
    prelude::*,
    widgets::Paragraph,
};

use crate::llms::{LlmProvider, ModelInfo};
use crate::state::State;
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

    if state.waiting_for_panels {
        spans.push(Span::styled(
            format!(" {} LOADING FILES ", spin),
            Style::default().fg(theme::bg_base()).bg(theme::warning()).bold()
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

    // Git branch (if available)
    if let Some(branch) = &state.git_branch {
        spans.push(Span::styled(
            format!(" {} ", branch),
            Style::default().fg(Color::White).bg(Color::Blue)
        ));
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
