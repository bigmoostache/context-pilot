//! Status bar IR builder — assembles [`StatusBar`] from application state.
//!
//! Extracts the data logic from `ui::input::render_status_bar` into a
//! pure function returning IR types. No ratatui, no Frame.

use cp_render::Semantic;
use cp_render::frame::{
    AgentCard, AutoContinue, Badge, GitChanges, QueueCard, ReverieCard, SkillCard, StatusBar, StopReason,
};

use crate::llms::{LlmProvider, ModelInfo as _};
use crate::state::State;
use cp_base::cast::Safe as _;

/// Build the status bar from application state.
#[must_use]
pub(crate) fn build_status_bar(state: &State) -> StatusBar {
    StatusBar {
        badge: build_badge(state),
        provider: Some(build_provider_label(state)),
        model: Some(build_model_label(state)),
        agent: build_agent(state),
        skills: build_skills(state),
        git: build_git(state),
        auto_continue: Some(build_auto_continue(state)),
        reveries: build_reveries(state),
        queue: build_queue(state),
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
    let has_question_form = state.get_ext::<cp_base::ui::question_form::PendingForm>().is_some();
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
    } else if has_question_form {
        Badge { label: "QUESTIONING".into(), semantic: Semantic::Warning }
    } else if has_timed_watcher {
        Badge { label: "WAITING".into(), semantic: Semantic::AccentDim }
    } else {
        Badge { label: "READY".into(), semantic: Semantic::Muted }
    }
}

// ── Model label ──────────────────────────────────────────────────────

/// Build the LLM provider display label.
fn build_provider_label(state: &State) -> String {
    match state.llm_provider {
        LlmProvider::Anthropic => "Claude",
        LlmProvider::ClaudeCode => "OAuth",
        LlmProvider::ClaudeCodeApiKey => "APIKey",
        LlmProvider::Grok => "Grok",
        LlmProvider::Groq => "Groq",
        LlmProvider::DeepSeek => "DeepSeek",
        LlmProvider::MiniMax => "MiniMax",
    }
    .to_string()
}

/// Build the active model display string.
fn build_model_label(state: &State) -> String {
    let (_provider_name, model_name) = match state.llm_provider {
        LlmProvider::Anthropic => ("Claude", state.anthropic_model.display_name()),
        LlmProvider::ClaudeCode => ("OAuth", state.anthropic_model.display_name()),
        LlmProvider::ClaudeCodeApiKey => ("APIKey", state.anthropic_model.display_name()),
        LlmProvider::Grok => ("Grok", state.grok_model.display_name()),
        LlmProvider::Groq => ("Groq", state.groq_model.display_name()),
        LlmProvider::DeepSeek => ("DeepSeek", state.deepseek_model.display_name()),
        LlmProvider::MiniMax => ("MiniMax", state.minimax_model.display_name()),
    };
    model_name.to_string()
}

// ── Agent + skills ───────────────────────────────────────────────────

/// Build active agent card.
fn build_agent(state: &State) -> Option<AgentCard> {
    let ps = cp_mod_prompt::types::PromptState::get(state);
    let agent_id = ps.active_agent_id.as_ref()?;
    let name = ps.agents.iter().find(|a| &a.id == agent_id).map_or_else(|| agent_id.clone(), |a| a.name.clone());
    Some(AgentCard { name })
}

/// Build loaded skill cards.
fn build_skills(state: &State) -> Vec<SkillCard> {
    let ps = cp_mod_prompt::types::PromptState::get(state);
    ps.loaded_skill_ids
        .iter()
        .map(|id| {
            let name = ps.skills.iter().find(|s| s.id == *id).map_or_else(|| id.clone(), |s| s.name.clone());
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
    let ps = cp_mod_prompt::types::PromptState::get(state);
    let mut sorted_keys: Vec<_> = state.reveries.keys().collect();
    sorted_keys.sort();

    sorted_keys
        .into_iter()
        .filter_map(|key| {
            let rev = state.reveries.get(key)?;
            let agent_name = ps
                .agents
                .iter()
                .find(|a| a.id == rev.agent_id)
                .map_or_else(|| rev.agent_id.clone(), |a| a.name.clone());
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
