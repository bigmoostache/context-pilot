use ratatui::{
    prelude::{Frame, Line, Rect, Span, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::infra::config::{THEME_ORDER, get_theme};
use crate::infra::constants::{chars, theme};
use crate::state::State;

mod budget_bars;
use budget_bars::format_tokens_compact;

/// Render the configuration overlay (Ctrl+H) centered on the given area.
pub(crate) fn render_config_overlay(frame: &mut Frame<'_>, state: &State, area: Rect) {
    // Center the overlay, clamped to available area
    let overlay_width = 56u16.min(area.width);
    let overlay_height = 38u16.min(area.height); // Reduced from 50
    let half_width = area.width.saturating_sub(overlay_width).saturating_div(2);
    let x = area.x.saturating_add(half_width);
    let half_height = area.height.saturating_sub(overlay_height).saturating_div(2);
    let y = area.y.saturating_add(half_height);
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Tab indicator
    let showing_main = !state.flags.config.config_secondary_mode;
    let tab_text = if showing_main { "Main Model" } else { "Secondary Model (Reverie)" };
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("Tab", Style::default().fg(theme::warning())),
        Span::styled(" to switch • ", Style::default().fg(theme::text_muted())),
        Span::styled(tab_text, Style::default().fg(theme::accent()).bold()),
    ]));
    add_separator(&mut lines);

    render_provider_section(&mut lines, state);
    add_separator(&mut lines);

    if showing_main {
        render_model_section(&mut lines, state);
    } else {
        render_secondary_model_section(&mut lines, state);
    }

    add_separator(&mut lines);
    render_api_check(&mut lines, state);
    add_separator(&mut lines);
    budget_bars::render_budget_section(&mut lines, state);
    add_separator(&mut lines);
    render_theme_section(&mut lines, state);
    add_separator(&mut lines);
    render_toggles_section(&mut lines, state);

    // Help text
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("1-6", Style::default().fg(theme::warning())),
        Span::styled(" provider  ", Style::default().fg(theme::text_muted())),
        Span::styled("a-d", Style::default().fg(theme::warning())),
        Span::styled(" model  ", Style::default().fg(theme::text_muted())),
        Span::styled("t", Style::default().fg(theme::warning())),
        Span::styled(" theme  ", Style::default().fg(theme::text_muted())),
        Span::styled("r", Style::default().fg(theme::warning())),
        Span::styled(" reverie  ", Style::default().fg(theme::text_muted())),
        Span::styled("s", Style::default().fg(theme::warning())),
        Span::styled(" auto", Style::default().fg(theme::text_muted())),
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
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        format!("  {}", chars::HORIZONTAL.repeat(50)),
        Style::default().fg(theme::border()),
    )]));
    lines.push(Line::from(""));
}

/// Render the provider section (always visible regardless of Tab mode)
fn render_provider_section(lines: &mut Vec<Line<'_>>, state: &State) {
    use crate::llms::LlmProvider;

    lines.push(Line::from(vec![Span::styled("  LLM Provider", Style::default().fg(theme::text_secondary()).bold())]));
    lines.push(Line::from(""));

    // Show selection indicator for main or secondary provider depending on Tab mode
    let active_provider =
        if state.flags.config.config_secondary_mode { state.secondary_provider } else { state.llm_provider };

    let providers = [
        (LlmProvider::Anthropic, "1", "Anthropic Claude"),
        (LlmProvider::ClaudeCode, "2", "Claude Code (OAuth)"),
        (LlmProvider::ClaudeCodeApiKey, "6", "Claude Code (API Key)"),
        (LlmProvider::Grok, "3", "Grok (xAI)"),
        (LlmProvider::Groq, "4", "Groq"),
        (LlmProvider::DeepSeek, "5", "DeepSeek"),
        (LlmProvider::MiniMax, "7", "MiniMax (Token Plan)"),
    ];

    for (provider, key, name) in providers {
        let is_selected = active_provider == provider;
        let indicator = if is_selected { ">" } else { " " };
        let check = if is_selected { "[x]" } else { "[ ]" };
        let style =
            if is_selected { Style::default().fg(theme::accent()).bold() } else { Style::default().fg(theme::text()) };

        lines.push(Line::from(vec![
            Span::styled(format!("  {indicator} "), Style::default().fg(theme::accent())),
            Span::styled(format!("{key} "), Style::default().fg(theme::warning())),
            Span::styled(format!("{check} "), style),
            Span::styled(name.to_string(), style),
        ]));
    }
}

/// Render the main model section with model list and pricing.
fn render_model_section(lines: &mut Vec<Line<'_>>, state: &State) {
    use crate::llms::{AnthropicModel, DeepSeekModel, GrokModel, GroqModel, LlmProvider, MiniMaxModel};

    lines.push(Line::from(vec![Span::styled("  Model", Style::default().fg(theme::text_secondary()).bold())]));
    lines.push(Line::from(""));

    match state.llm_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
            for (model, key) in [
                (AnthropicModel::ClaudeOpus45, "a"),
                (AnthropicModel::ClaudeSonnet45, "b"),
                (AnthropicModel::ClaudeHaiku45, "c"),
            ] {
                render_model_line_with_info(lines, state.anthropic_model == model, key, &model);
            }
        }
        LlmProvider::Grok => {
            for (model, key) in [(GrokModel::Grok41Fast, "a"), (GrokModel::Grok4Fast, "b")] {
                render_model_line_with_info(lines, state.grok_model == model, key, &model);
            }
        }
        LlmProvider::Groq => {
            render_model_line_with_info(lines, state.groq_model == GroqModel::GptOss120b, "a", &GroqModel::GptOss120b);
            render_model_line_with_info(lines, state.groq_model == GroqModel::GptOss20b, "b", &GroqModel::GptOss20b);
            render_model_line_with_info(
                lines,
                state.groq_model == GroqModel::Llama33_70b,
                "c",
                &GroqModel::Llama33_70b,
            );
            render_model_line_with_info(lines, state.groq_model == GroqModel::Llama31_8b, "d", &GroqModel::Llama31_8b);
        }
        LlmProvider::DeepSeek => {
            for (model, key) in [(DeepSeekModel::DeepseekChat, "a"), (DeepSeekModel::DeepseekReasoner, "b")] {
                render_model_line_with_info(lines, state.deepseek_model == model, key, &model);
            }
        }
        LlmProvider::MiniMax => {
            render_model_line_with_info(lines, state.minimax_model == MiniMaxModel::M27, "a", &MiniMaxModel::M27);
            render_model_line_with_info(
                lines,
                state.minimax_model == MiniMaxModel::M27Highspeed,
                "b",
                &MiniMaxModel::M27Highspeed,
            );
        }
    }
}

/// Render the API check status line (spinner while checking, result when done).
fn render_api_check(lines: &mut Vec<Line<'_>>, state: &State) {
    if state.flags.lifecycle.api_check_in_progress {
        let spin = crate::ui::helpers::spinner(state.spinner_frame);
        lines.push(Line::from(vec![
            Span::styled(format!("  {spin} "), Style::default().fg(theme::accent())),
            Span::styled("Checking API...", Style::default().fg(theme::text_muted())),
        ]));
    } else if let Some(result) = &state.api_check_result {
        use crate::infra::config::normalize_icon;
        let result: &cp_base::config::llm_types::ApiCheckResult = result;
        let (icon, color, msg) = if result.all_ok() {
            (normalize_icon("✓"), theme::success(), "API OK")
        } else if let Some(err) = &result.error {
            (normalize_icon("✗"), theme::error(), err.as_str())
        } else {
            (normalize_icon("!"), theme::warning(), "Issues detected")
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {icon}"), Style::default().fg(color)),
            Span::styled(msg.to_string(), Style::default().fg(color)),
        ]));
    }
}

/// Render the theme section with current theme info and navigation.
fn render_theme_section(lines: &mut Vec<Line<'_>>, state: &State) {
    lines.push(Line::from(vec![Span::styled("  Theme", Style::default().fg(theme::text_secondary()).bold())]));
    lines.push(Line::from(""));

    let Some(current_theme) = get_theme(&state.active_theme) else { return };
    let fallback_icon = "📄".to_string();

    lines.push(Line::from(vec![
        Span::styled("   ◀ ", Style::default().fg(theme::accent())),
        Span::styled(format!("{:<12}", current_theme.name), Style::default().fg(theme::accent()).bold()),
        Span::styled(" ▶  ", Style::default().fg(theme::accent())),
        Span::styled(
            format!(
                "{} {} {} {}",
                current_theme.messages.user,
                current_theme.messages.assistant,
                current_theme.context.get("tree").unwrap_or(&fallback_icon),
                current_theme.context.get("file").unwrap_or(&fallback_icon),
            ),
            Style::default().fg(theme::text()),
        ),
    ]));
    lines.push(Line::from(vec![Span::styled(
        format!("     {}", current_theme.description),
        Style::default().fg(theme::text_muted()),
    )]));

    let current_idx = THEME_ORDER.iter().position(|&t| t == state.active_theme).unwrap_or(0);
    lines.push(Line::from(vec![Span::styled(
        format!("     ({}/{})", current_idx.saturating_add(1), THEME_ORDER.len()),
        Style::default().fg(theme::text_muted()),
    )]));
}

/// Render the toggle section (auto-continue, reverie).
fn render_toggles_section(lines: &mut Vec<Line<'_>>, state: &State) {
    // Auto-continuation toggle
    let spine_cfg = &cp_mod_spine::types::SpineState::get(state).config;
    let auto_on = spine_cfg.continue_until_todos_done;
    let (auto_check, auto_status, auto_color) =
        if auto_on { ("[x]", "ON", theme::success()) } else { ("[ ]", "OFF", theme::text_muted()) };
    lines.push(Line::from(vec![
        Span::styled("  Auto-continue: ", Style::default().fg(theme::text_secondary()).bold()),
        Span::styled(format!("{auto_check} "), Style::default().fg(auto_color).bold()),
        Span::styled(auto_status, Style::default().fg(auto_color).bold()),
        Span::styled("  (press ", Style::default().fg(theme::text_muted())),
        Span::styled("s", Style::default().fg(theme::warning())),
        Span::styled(" to toggle)", Style::default().fg(theme::text_muted())),
    ]));

    // Reverie toggle
    let rev_on = state.flags.config.reverie_enabled;
    let (rev_check, rev_status, rev_color) =
        if rev_on { ("[x]", "ON", theme::success()) } else { ("[ ]", "OFF", theme::text_muted()) };
    lines.push(Line::from(vec![
        Span::styled("  Reverie:       ", Style::default().fg(theme::text_secondary()).bold()),
        Span::styled(format!("{rev_check} "), Style::default().fg(rev_color).bold()),
        Span::styled(rev_status, Style::default().fg(rev_color).bold()),
        Span::styled("  (press ", Style::default().fg(theme::text_muted())),
        Span::styled("r", Style::default().fg(theme::warning())),
        Span::styled(" to toggle)", Style::default().fg(theme::text_muted())),
    ]));
}

/// Render the secondary model section (Reverie model selection).
fn render_secondary_model_section(lines: &mut Vec<Line<'_>>, state: &State) {
    use crate::llms::{AnthropicModel, DeepSeekModel, GrokModel, GroqModel, LlmProvider, MiniMaxModel};

    lines.push(Line::from(vec![Span::styled(
        "  Secondary Model (Reverie)",
        Style::default().fg(theme::text_secondary()).bold(),
    )]));
    lines.push(Line::from(""));

    match state.secondary_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
            for (model, key) in [
                (AnthropicModel::ClaudeOpus45, "a"),
                (AnthropicModel::ClaudeSonnet45, "b"),
                (AnthropicModel::ClaudeHaiku45, "c"),
            ] {
                render_model_line_with_info(lines, state.secondary_anthropic_model == model, key, &model);
            }
        }
        LlmProvider::Grok => {
            for (model, key) in [(GrokModel::Grok41Fast, "a"), (GrokModel::Grok4Fast, "b")] {
                render_model_line_with_info(lines, state.secondary_grok_model == model, key, &model);
            }
        }
        LlmProvider::Groq => {
            render_model_line_with_info(
                lines,
                state.secondary_groq_model == GroqModel::GptOss120b,
                "a",
                &GroqModel::GptOss120b,
            );
            render_model_line_with_info(
                lines,
                state.secondary_groq_model == GroqModel::GptOss20b,
                "b",
                &GroqModel::GptOss20b,
            );
            render_model_line_with_info(
                lines,
                state.secondary_groq_model == GroqModel::Llama33_70b,
                "c",
                &GroqModel::Llama33_70b,
            );
            render_model_line_with_info(
                lines,
                state.secondary_groq_model == GroqModel::Llama31_8b,
                "d",
                &GroqModel::Llama31_8b,
            );
        }
        LlmProvider::DeepSeek => {
            for (model, key) in [(DeepSeekModel::DeepseekChat, "a"), (DeepSeekModel::DeepseekReasoner, "b")] {
                render_model_line_with_info(lines, state.secondary_deepseek_model == model, key, &model);
            }
        }
        LlmProvider::MiniMax => {
            render_model_line_with_info(
                lines,
                state.secondary_minimax_model == MiniMaxModel::M27,
                "a",
                &MiniMaxModel::M27,
            );
            render_model_line_with_info(
                lines,
                state.secondary_minimax_model == MiniMaxModel::M27Highspeed,
                "b",
                &MiniMaxModel::M27Highspeed,
            );
        }
    }
}

/// Render a single model line with context window size and pricing info.
fn render_model_line_with_info<M: crate::llms::ModelInfo>(
    lines: &mut Vec<Line<'_>>,
    is_selected: bool,
    key: &str,
    model: &M,
) {
    let indicator = if is_selected { ">" } else { " " };
    let check = if is_selected { "[x]" } else { "[ ]" };
    let style =
        if is_selected { Style::default().fg(theme::accent()).bold() } else { Style::default().fg(theme::text()) };

    let ctx_str = format_tokens_compact(model.context_window());
    let price_str = format!("${:.0}/${:.0}", model.input_price_per_mtok(), model.output_price_per_mtok());

    lines.push(Line::from(vec![
        Span::styled(format!("  {indicator} "), Style::default().fg(theme::accent())),
        Span::styled(format!("{key} "), Style::default().fg(theme::warning())),
        Span::styled(format!("{check} "), style),
        Span::styled(format!("{:<12}", model.display_name()), style),
        Span::styled(format!("{ctx_str:>4} "), Style::default().fg(theme::text_muted())),
        Span::styled(price_str, Style::default().fg(theme::text_muted())),
    ]));
}
