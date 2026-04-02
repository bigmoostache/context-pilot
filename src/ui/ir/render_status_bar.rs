//! Status bar IR adapter — renders [`StatusBar`] to a single ratatui line.
//!
//! Replaces `ui::input::render_status_bar` by consuming the pre-built
//! IR snapshot instead of reading application state directly.

use cp_render::Semantic;
use cp_render::frame::StatusBar;
use ratatui::prelude::{Color, Frame, Line, Rect, Span, Style};
use ratatui::widgets::Paragraph;

use crate::infra::config::normalize_icon;
use crate::ui::{helpers::spinner, theme};
use cp_base::cast::Safe as _;

/// Render the status bar from its IR snapshot.
pub(crate) fn render_status_bar_from_ir(frame: &mut Frame<'_>, status: &StatusBar, area: Rect, spinner_frame: u64) {
    let base_style = Style::default().bg(theme::bg_base()).fg(theme::text_muted());
    let spin = spinner(spinner_frame);

    let mut spans = vec![Span::styled(" ", base_style)];

    // === Primary badge ===
    let (fg_badge, bg_badge) = badge_colors(status.badge.semantic, spin);
    let badge_label = if needs_spinner(status.badge.semantic) {
        format!(" {spin} {} ", status.badge.label)
    } else {
        format!(" {} ", status.badge.label)
    };
    spans.push(Span::styled(badge_label, Style::default().fg(fg_badge).bg(bg_badge).bold()));
    spans.push(Span::styled(" ", base_style));

    // === Retry badge ===
    if status.retry_count > 0 {
        spans.push(Span::styled(
            format!(" RETRY {}/{} ", status.retry_count, status.max_retries),
            Style::default().fg(theme::bg_base()).bg(theme::error()).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // === Loading badge ===
    if status.loading_count > 0 {
        spans.push(Span::styled(
            format!(" {spin} LOADING {} ", status.loading_count),
            Style::default().fg(theme::bg_base()).bg(theme::text_muted()).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // === Provider + model ===
    if let Some(ref provider) = status.provider {
        spans.push(Span::styled(
            format!(" {provider} "),
            Style::default().fg(theme::bg_base()).bg(theme::accent_dim()).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }
    if let Some(ref model) = status.model {
        spans.push(Span::styled(format!(" {model} "), Style::default().fg(theme::text()).bg(theme::bg_elevated())));
        spans.push(Span::styled(" ", base_style));
    }

    // === Stop reason ===
    if let Some(ref sr) = status.stop_reason {
        let label = sr.reason.to_uppercase();
        let style = if sr.semantic == Semantic::Error {
            Style::default().fg(theme::bg_base()).bg(theme::error()).bold()
        } else {
            Style::default().fg(theme::text()).bg(theme::bg_elevated())
        };
        spans.push(Span::styled(format!(" {label} "), style));
        spans.push(Span::styled(" ", base_style));
    }

    // === Agent card ===
    if let Some(ref agent) = status.agent {
        spans.push(Span::styled(
            format!(" 🤖 {} ", agent.name),
            Style::default().fg(Color::White).bg(Color::Rgb(130, 80, 200)).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // === Skill cards ===
    for skill in &status.skills {
        spans.push(Span::styled(
            format!(" 📚 {} ", skill.name),
            Style::default().fg(theme::bg_base()).bg(theme::assistant()).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // === Git branch + changes ===
    if let Some(ref git) = status.git {
        spans.push(Span::styled(format!(" {} ", git.branch), Style::default().fg(Color::White).bg(Color::Blue)));
        spans.push(Span::styled(" ", base_style));

        if git.files_changed > 0 {
            let net = i64::from(git.additions).saturating_sub(i64::from(git.deletions));
            let (net_prefix, net_color) = if net >= 0 { ("+", theme::success()) } else { ("", theme::error()) };
            let bg = theme::bg_elevated();

            spans.push(Span::styled(
                format!(" +{}", git.additions),
                Style::default().fg(theme::success()).bg(bg).bold(),
            ));
            spans.push(Span::styled(format!("/-{}", git.deletions), Style::default().fg(theme::error()).bg(bg).bold()));
            spans.push(Span::styled(
                format!("/{}{} ", net_prefix, net.unsigned_abs()),
                Style::default().fg(net_color).bg(bg).bold(),
            ));
            spans.push(Span::styled(" ", base_style));
        }
    }

    // === Auto-continue ===
    if let Some(ref ac) = status.auto_continue {
        let (icon, bg_color) = if ac.max.is_some() {
            (normalize_icon("🔁"), theme::warning())
        } else {
            (normalize_icon("🔄"), theme::text_muted())
        };
        let label = if ac.max.is_some() { "Auto-continue" } else { "No Auto-continue" };
        spans.push(Span::styled(format!(" {icon}{label} "), Style::default().fg(theme::bg_base()).bg(bg_color).bold()));
        spans.push(Span::styled(" ", base_style));
    }

    // === Reverie cards ===
    for rev in &status.reveries {
        let rev_spin = format!("{spin} ");
        spans.push(Span::styled(
            format!(" {rev_spin}🧠 {} ({} tools) ", rev.agent, rev.tool_count),
            Style::default().fg(Color::White).bg(Color::Rgb(100, 60, 160)).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // === Queue card ===
    if let Some(ref queue) = status.queue {
        spans.push(Span::styled(
            format!(" ⏳ Queue ({}) ", queue.count),
            Style::default().fg(Color::White).bg(Color::Rgb(180, 120, 40)).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // === Right-aligned char count ===
    let right_info =
        if status.input_char_count > 0 { format!("{} chars ", status.input_char_count) } else { String::new() };

    let left_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let right_width = right_info.len();
    let padding = (area.width.to_usize()).saturating_sub(left_width.saturating_add(right_width));

    spans.push(Span::styled(" ".repeat(padding), base_style));
    spans.push(Span::styled(&right_info, base_style));

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Map a badge semantic to (foreground, background) colours.
fn badge_colors(semantic: Semantic, _spin: &str) -> (Color, Color) {
    match semantic {
        Semantic::Success => (theme::bg_base(), theme::success()),
        Semantic::Info => (Color::White, Color::Blue),
        Semantic::Warning => (theme::bg_base(), theme::warning()),
        Semantic::Error => (theme::bg_base(), theme::error()),
        Semantic::AccentDim => (Color::White, Color::Magenta),
        // Muted = READY, Default = fallback
        Semantic::Default
        | Semantic::Muted
        | Semantic::Active
        | Semantic::KeyHint
        | Semantic::Code
        | Semantic::DiffAdd
        | Semantic::DiffRemove
        | Semantic::Header
        | Semantic::Border
        | Semantic::Bold
        | _ => (theme::bg_base(), theme::text_muted()),
    }
}

/// Whether a badge semantic should show the spinner prefix.
const fn needs_spinner(semantic: Semantic) -> bool {
    matches!(semantic, Semantic::Success | Semantic::Info | Semantic::AccentDim)
}
