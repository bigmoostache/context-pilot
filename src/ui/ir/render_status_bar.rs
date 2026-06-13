//! Status bar IR adapter — renders [`StatusBar`] to a single ratatui line.
//!
//! Replaces `ui::input::render_status_bar` by consuming the pre-built
//! IR snapshot instead of reading application state directly.

use cp_render::Semantic;
use cp_render::frame::{
    AgentCard, AutoContinue, Badge, GitChanges, QueueCard, ReverieCard, SkillCard, StatusBar, StopReason, ThinkCard,
};
use ratatui::prelude::{Frame, Line, Rect, Span, Style};
use ratatui::widgets::Paragraph;

use crate::infra::config::normalize_icon;
use crate::state::State;
use crate::ui::{helpers::spinner, theme};
use cp_base::cast::Safe as _;

/// Render the status bar from its IR snapshot.
pub(crate) fn render_status_bar_from_ir(frame: &mut Frame<'_>, status: &StatusBar, area: Rect) {
    let base_style = Style::default().bg(theme::bg_base()).fg(theme::text_muted());
    let spin = spinner();

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
            Style::default().fg(theme::card_text()).bg(theme::card_agent_bg()).bold(),
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
        spans.push(Span::styled(
            format!(" {} ", git.branch),
            Style::default().fg(theme::card_text()).bg(theme::accent()),
        ));
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
            Style::default().fg(theme::card_text()).bg(theme::card_reverie_bg()).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // === Queue card ===
    if let Some(ref queue) = status.queue {
        spans.push(Span::styled(
            format!(" ⏳ Queue ({}) ", queue.count),
            Style::default().fg(theme::card_text()).bg(theme::card_queue_bg()).bold(),
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // === Think balance card ===
    if let Some(ref think) = status.think {
        spans.push(Span::styled(
            format!(" 🧠 Think ({}) ", think.balance),
            Style::default().fg(theme::card_text()).bg(theme::card_think_bg()).bold(),
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
fn badge_colors(semantic: Semantic, _spin: &str) -> (ratatui::style::Color, ratatui::style::Color) {
    match semantic {
        Semantic::Success => (theme::bg_base(), theme::success()),
        Semantic::Info => (theme::card_text(), theme::accent()),
        Semantic::Warning => (theme::bg_base(), theme::warning()),
        Semantic::Error => (theme::bg_base(), theme::error()),
        Semantic::AccentDim => (theme::card_text(), theme::accent_dim()),
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

// ── Builder (merged from status_bar.rs) ─────────────────────────────

/// Build the status bar from application state.
#[must_use]
pub(crate) fn build_status_bar(state: &State) -> StatusBar {
    StatusBar {
        badge: build_badge(state),
        agent: build_agent(state),
        skills: build_skills(state),
        git: build_git(state),
        auto_continue: Some(build_auto_continue(state)),
        reveries: build_reveries(state),
        queue: build_queue(state),
        think: build_think(state),
        stop_reason: build_stop_reason(state),
        retry_count: state.api_retry_count.to_u8(),
        max_retries: crate::infra::constants::MAX_API_RETRIES.to_u8(),
        loading_count: state
            .context
            .iter()
            .filter(|c| c.cached_content.is_none() && c.context_type.needs_cache())
            .count()
            .to_u16(),
        input_char_count: state.input.chars().count().to_u32(),
    }
}

// ── Primary badge ────────────────────────────────────────────────────

/// The primary status badge (STREAMING / TOOLING / READY / BLOCKED / etc.).
fn build_badge(state: &State) -> Badge {
    let has_timed_watcher = {
        use cp_base::state::watchers::WatcherRegistry;
        state
            .get_ext::<WatcherRegistry>()
            .is_some_and(|reg| reg.active_watchers().iter().any(|w| w.fire_at_ms().is_some()))
    };

    if state.guard_rail_blocked.is_some() {
        Badge {
            label: format!("BLOCKED: {}", state.guard_rail_blocked.as_deref().unwrap_or("?")),
            semantic: Semantic::Error,
        }
    } else if state.flags.stream.phase.is_streaming() && !state.flags.stream.phase.is_tooling() {
        Badge { label: "STREAMING".into(), semantic: Semantic::Success }
    } else if state.flags.stream.phase.is_streaming() && state.flags.stream.phase.is_tooling() {
        Badge { label: "TOOLING".into(), semantic: Semantic::Info }
    } else if has_timed_watcher {
        Badge { label: "WAITING".into(), semantic: Semantic::AccentDim }
    } else {
        Badge { label: "READY".into(), semantic: Semantic::Muted }
    }
}

// ── Agent + skills ───────────────────────────────────────────────────

/// Build active agent card.
fn build_agent(state: &State) -> Option<AgentCard> {
    let ps = cp_mod_prompt::types::PromptState::get(state);
    let agent_id = ps.active_agent_id.as_ref()?;
    let agents = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Agent);
    let name = agents.iter().find(|a| &a.id == agent_id).map_or_else(|| agent_id.clone(), |a| a.name.clone());
    Some(AgentCard { name })
}

/// Build loaded skill cards.
fn build_skills(state: &State) -> Vec<SkillCard> {
    let ps = cp_mod_prompt::types::PromptState::get(state);
    let skills = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Skill);
    ps.loaded_skill_ids
        .iter()
        .map(|id| {
            let name = skills.iter().find(|s| s.id == *id).map_or_else(|| id.clone(), |s| s.name.clone());
            SkillCard { name }
        })
        .collect()
}

// ── Git ──────────────────────────────────────────────────────────────

/// Build git branch + changes summary.
fn build_git(state: &State) -> Option<GitChanges> {
    let gs = cp_mod_git::types::GitState::get(state);
    let branch = gs.branch.as_ref()?;

    let mut additions = 0i32;
    let mut deletions = 0i32;
    for file in &gs.file_changes {
        additions = additions.saturating_add(file.additions);
        deletions = deletions.saturating_add(file.deletions);
    }

    Some(GitChanges {
        branch: branch.clone(),
        files_changed: gs.file_changes.len().to_u32(),
        additions: additions.unsigned_abs(),
        deletions: deletions.unsigned_abs(),
    })
}

// ── Auto-continue ────────────────────────────────────────────────────

/// Build auto-continuation indicator.
fn build_auto_continue(state: &State) -> AutoContinue {
    let cfg = &cp_mod_spine::types::SpineState::get(state).config;
    AutoContinue {
        count: cfg.auto_continuation_count.to_u32(),
        max: cfg.max_auto_retries.map(cp_base::cast::Safe::to_u32),
    }
}

// ── Reverie ──────────────────────────────────────────────────────────

/// Build active reverie cards (all concurrent reveries, sorted by key).
fn build_reveries(state: &State) -> Vec<ReverieCard> {
    let agents = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Agent);
    let mut sorted_keys: Vec<_> = state.reveries.keys().collect();
    sorted_keys.sort();

    sorted_keys
        .into_iter()
        .filter_map(|key| {
            let rev = state.reveries.get(key)?;
            let agent_name =
                agents.iter().find(|a| a.id == rev.agent_id).map_or_else(|| rev.agent_id.clone(), |a| a.name.clone());
            Some(ReverieCard { agent: agent_name, tool_count: rev.tool_call_count.to_u32() })
        })
        .collect()
}

// ── Queue ────────────────────────────────────────────────────────────

/// Build queue status card.
fn build_queue(state: &State) -> Option<QueueCard> {
    let qs = cp_mod_queue::types::QueueState::get(state);
    if !qs.active {
        return None;
    }
    Some(QueueCard { count: qs.queued_calls.len().to_u32(), active: true })
}

// ── Stop reason ──────────────────────────────────────────────────────

/// Build stop reason indicator from last completion.
fn build_stop_reason(state: &State) -> Option<StopReason> {
    if state.flags.stream.phase.is_streaming() {
        return None;
    }
    let reason = state.last_stop_reason.as_ref()?;
    let semantic = if reason == "max_tokens" { Semantic::Error } else { Semantic::Muted };
    Some(StopReason { reason: reason.clone(), semantic })
}

// ── Think ────────────────────────────────────────────────────────────

/// Build think tool balance card (only shown when balance is negative).
fn build_think(state: &State) -> Option<ThinkCard> {
    let ts = state.get_ext::<crate::modules::questions::ThinkState>()?;
    if ts.consecutive_count >= 0 {
        return None;
    }
    Some(ThinkCard { balance: ts.consecutive_count })
}
