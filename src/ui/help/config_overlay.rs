use ratatui::{
    prelude::{Color, Frame, Line, Rect, Span, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::infra::config::{THEME_ORDER, get_theme};
use crate::infra::constants::{chars, theme};
use crate::state::State;
use cp_base::cast::Safe as _;

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
    render_budget_bars(&mut lines, state);
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
    use crate::llms::{AnthropicModel, DeepSeekModel, GrokModel, GroqModel, LlmProvider};

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
            for (model, key) in [
                (GroqModel::GptOss120b, "a"),
                (GroqModel::GptOss20b, "b"),
                (GroqModel::Llama33_70b, "c"),
                (GroqModel::Llama31_8b, "d"),
            ] {
                render_model_line_with_info(lines, state.groq_model == model, key, &model);
            }
        }
        LlmProvider::DeepSeek => {
            for (model, key) in [(DeepSeekModel::DeepseekChat, "a"), (DeepSeekModel::DeepseekReasoner, "b")] {
                render_model_line_with_info(lines, state.deepseek_model == model, key, &model);
            }
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

/// Format a token count as a compact human-readable string (e.g. "128K", "1.5M").
#[expect(
    clippy::integer_division_remainder_used,
    reason = "format_tokens_compact: truncating division for human-readable K/M token display"
)]
fn format_tokens_compact(tokens: usize) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens.to_f64() / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{}K", tokens / 1_000)
    } else {
        format!("{tokens}")
    }
}

/// Render the budget bars (context budget, cleaning threshold, cleaning target, max cost).
fn render_budget_bars(lines: &mut Vec<Line<'_>>, state: &State) {
    let bar_width = 24usize;
    let max_budget = state.model_context_window();
    let effective_budget = state.effective_context_budget();
    let selected = state.config_selected_bar;

    // 1. Context Budget
    let budget_pct = (effective_budget.to_f64() / max_budget.to_f64() * 100.0).to_usize();
    let budget_filled = ((effective_budget.to_f64() / max_budget.to_f64()) * bar_width.to_f64()).to_usize();
    render_bar(
        lines,
        &BarConfig {
            selected,
            idx: 0,
            label: "Context Budget",
            pct: budget_pct,
            filled: budget_filled,
            bar_width,
            tokens_str: &format_tokens_compact(effective_budget),
            bar_color: theme::success(),
            extra: None,
        },
    );

    // 2. Cleaning Threshold
    let threshold_pct = (state.cleaning_threshold * 100.0).to_usize();
    let threshold_tokens = state.cleaning_threshold_tokens();
    let threshold_filled = ((state.cleaning_threshold * bar_width.to_f32()).to_usize()).min(bar_width);
    render_bar(
        lines,
        &BarConfig {
            selected,
            idx: 1,
            label: "Clean Trigger",
            pct: threshold_pct,
            filled: threshold_filled,
            bar_width,
            tokens_str: &format_tokens_compact(threshold_tokens),
            bar_color: theme::warning(),
            extra: None,
        },
    );

    // 3. Target Cleaning
    let target_pct = (state.cleaning_target_proportion * 100.0).to_usize();
    let target_tokens = state.cleaning_target_tokens();
    let target_abs_pct = (state.cleaning_target() * 100.0).to_usize();
    let target_filled = ((state.cleaning_target_proportion * bar_width.to_f32()).to_usize()).min(bar_width);
    let extra = format!(" ({target_abs_pct}%)");
    render_bar(
        lines,
        &BarConfig {
            selected,
            idx: 2,
            label: "Clean Target",
            pct: target_pct,
            filled: target_filled,
            bar_width,
            tokens_str: &format_tokens_compact(target_tokens),
            bar_color: theme::accent(),
            extra: Some(&extra),
        },
    );

    // 4. Max Cost Guard Rail
    let spine_cfg = &cp_mod_spine::types::SpineState::get(state).config;
    let max_cost = spine_cfg.max_cost.unwrap_or(0.0);
    let max_display = 20.0f64;
    let cost_filled = ((max_cost / max_display) * bar_width.to_f64()).min(bar_width.to_f64()).to_usize();
    let cost_label = if max_cost <= 0.0 { "disabled".to_string() } else { format!("${max_cost:.2}") };
    let is_selected = selected == 3;
    let indicator = if is_selected { ">" } else { " " };
    let label_style = if is_selected {
        Style::default().fg(theme::accent()).bold()
    } else {
        Style::default().fg(theme::text_secondary()).bold()
    };
    let arrow_color = if is_selected { theme::accent() } else { theme::text_muted() };

    lines.push(Line::from(vec![
        Span::styled(format!(" {indicator} "), Style::default().fg(theme::accent())),
        Span::styled("Max Cost".to_string(), label_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("   ◀ ", Style::default().fg(arrow_color)),
        Span::styled(chars::BLOCK_FULL.repeat(cost_filled.min(bar_width)), Style::default().fg(theme::error())),
        Span::styled(
            chars::BLOCK_LIGHT.repeat(bar_width.saturating_sub(cost_filled)),
            Style::default().fg(theme::bg_elevated()),
        ),
        Span::styled(" ▶ ", Style::default().fg(arrow_color)),
        Span::styled(cost_label, Style::default().fg(theme::text()).bold()),
        Span::styled("  (guard rail)", Style::default().fg(theme::text_muted())),
    ]));
}

/// Configuration for rendering a single budget bar.
struct BarConfig<'cfg> {
    /// Index of the currently selected bar.
    selected: usize,
    /// Index of this bar (for selection comparison).
    idx: usize,
    /// Display label shown before the bar.
    label: &'cfg str,
    /// Percentage value to show after the bar.
    pct: usize,
    /// Number of filled cells in the bar.
    filled: usize,
    /// Total width of the bar in cells.
    bar_width: usize,
    /// Formatted token count string.
    tokens_str: &'cfg str,
    /// Color used for the filled portion of the bar.
    bar_color: Color,
    /// Optional extra text appended after the token count.
    extra: Option<&'cfg str>,
}

/// Render a single budget bar line with selection indicator, bar, and label.
fn render_bar(lines: &mut Vec<Line<'_>>, cfg: &BarConfig<'_>) {
    let is_selected = cfg.selected == cfg.idx;
    let indicator = if is_selected { ">" } else { " " };
    let label_style = if is_selected {
        Style::default().fg(theme::accent()).bold()
    } else {
        Style::default().fg(theme::text_secondary()).bold()
    };
    let arrow_color = if is_selected { theme::accent() } else { theme::text_muted() };

    lines.push(Line::from(vec![
        Span::styled(format!(" {indicator} "), Style::default().fg(theme::accent())),
        Span::styled(cfg.label.to_string(), label_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("   ◀ ", Style::default().fg(arrow_color)),
        Span::styled(chars::BLOCK_FULL.repeat(cfg.filled.min(cfg.bar_width)), Style::default().fg(cfg.bar_color)),
        Span::styled(
            chars::BLOCK_LIGHT.repeat(cfg.bar_width.saturating_sub(cfg.filled)),
            Style::default().fg(theme::bg_elevated()),
        ),
        Span::styled(" ▶ ", Style::default().fg(arrow_color)),
        Span::styled(format!("{}%", cfg.pct), Style::default().fg(theme::text()).bold()),
        Span::styled(
            format!("  {} tok{}", cfg.tokens_str, cfg.extra.unwrap_or("")),
            Style::default().fg(theme::text_muted()),
        ),
    ]));
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
    use crate::llms::{AnthropicModel, DeepSeekModel, GrokModel, GroqModel, LlmProvider};

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
            for (model, key) in [
                (GroqModel::GptOss120b, "a"),
                (GroqModel::GptOss20b, "b"),
                (GroqModel::Llama33_70b, "c"),
                (GroqModel::Llama31_8b, "d"),
            ] {
                render_model_line_with_info(lines, state.secondary_groq_model == model, key, &model);
            }
        }
        LlmProvider::DeepSeek => {
            for (model, key) in [(DeepSeekModel::DeepseekChat, "a"), (DeepSeekModel::DeepseekReasoner, "b")] {
                render_model_line_with_info(lines, state.secondary_deepseek_model == model, key, &model);
            }
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
