//! Incoming face of the contract: `WebCommand` → `Action` mapping and
//! read-only query answering. Mirrors `handle_event()` for the web.

use std::sync::mpsc::Sender;

use cp_web_server::protocol::{ConfigScope, QuestionAnswerPayload, WebCommand, WebQuery, result_frame};
use serde_json::{Value, json};

use crate::app::App;
use crate::app::actions::Action;
use crate::infra::api::StreamEvent;
use crate::state::State;

/// Apply one web command to the app (the web counterpart of a keystroke).
pub(crate) fn apply_command(app: &mut App, tx: &Sender<StreamEvent>, cmd: WebCommand) {
    match cmd {
        WebCommand::Submit { text } => {
            // The browser owns input editing — it sends the final text.
            app.state.input = text;
            app.state.input_cursor = app.state.input.len();
            app.state.input_selection_anchor = None;
            app.handle_action(Action::InputSubmit, tx);
        }
        WebCommand::Stop => app.handle_action(Action::StopStreaming, tx),
        WebCommand::SelectPanel { id } => app.handle_action(Action::SelectContextById(id), tx),
        WebCommand::ClearConversation => app.handle_action(Action::ClearConversation, tx),
        WebCommand::NewContext => app.handle_action(Action::NewContext, tx),
        WebCommand::ResetCosts => app.handle_action(Action::ResetSessionCosts, tx),
        WebCommand::Reload => {
            app.state.flags.lifecycle.reload_pending = true;
        }
        WebCommand::SetProvider { scope, provider } => {
            if let Some(action) = provider_action(scope, &provider) {
                app.handle_action(action, tx);
            }
        }
        WebCommand::SetModel { scope, model } => {
            if let Some(action) = model_action(&app.state, scope, &model) {
                app.handle_action(action, tx);
            }
        }
        WebCommand::SetTheme { theme } => app.handle_action(Action::ConfigSetTheme(theme), tx),
        WebCommand::ToggleAutoContinue => app.handle_action(Action::ConfigToggleAutoContinue, tx),
        WebCommand::ToggleReverie => app.handle_action(Action::ConfigToggleReverie, tx),
        WebCommand::SetContextBudget { tokens } => app.handle_action(Action::ConfigSetContextBudget(tokens), tx),
        WebCommand::SetCleaningThreshold { value } => {
            app.handle_action(Action::ConfigSetCleaningThreshold(value), tx);
        }
        WebCommand::SetCleaningTarget { value } => app.handle_action(Action::ConfigSetCleaningTarget(value), tx),
        WebCommand::SetMaxCost { value } => app.handle_action(Action::ConfigSetMaxCost(value), tx),
        WebCommand::SetThinkThreshold { value } => app.handle_action(Action::ConfigSetThinkThreshold(value), tx),
        WebCommand::AnswerQuestion { tool_use_id, answers } => {
            answer_question(&mut app.state, &tool_use_id, &answers);
        }
        WebCommand::DismissQuestion { tool_use_id } => dismiss_question(&mut app.state, &tool_use_id),
    }
}

/// Map a provider-selection command to its `Action`.
fn provider_action(scope: ConfigScope, provider: &str) -> Option<Action> {
    let parsed = serde_json::from_value::<cp_base::config::llm_types::LlmProvider>(json!(provider)).ok()?;
    Some(match scope {
        ConfigScope::Primary => Action::ConfigSelectProvider(parsed),
        ConfigScope::Secondary => Action::ConfigSelectSecondaryProvider(parsed),
    })
}

/// Map a model-selection command to its `Action`, based on the scope's
/// currently selected provider.
fn model_action(state: &State, scope: ConfigScope, model: &str) -> Option<Action> {
    use cp_base::config::llm_types::LlmProvider;
    let provider = match scope {
        ConfigScope::Primary => state.llm_provider,
        ConfigScope::Secondary => state.secondary_provider,
    };
    let value = json!(model);
    let primary = matches!(scope, ConfigScope::Primary);
    match provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
            let parsed = serde_json::from_value(value).ok()?;
            Some(if primary {
                Action::ConfigSelectAnthropicModel(parsed)
            } else {
                Action::ConfigSelectSecondaryAnthropicModel(parsed)
            })
        }
        LlmProvider::Grok => {
            let parsed = serde_json::from_value(value).ok()?;
            Some(if primary {
                Action::ConfigSelectGrokModel(parsed)
            } else {
                Action::ConfigSelectSecondaryGrokModel(parsed)
            })
        }
        LlmProvider::Groq => {
            let parsed = serde_json::from_value(value).ok()?;
            Some(if primary {
                Action::ConfigSelectGroqModel(parsed)
            } else {
                Action::ConfigSelectSecondaryGroqModel(parsed)
            })
        }
        LlmProvider::DeepSeek => {
            let parsed = serde_json::from_value(value).ok()?;
            Some(if primary {
                Action::ConfigSelectDeepSeekModel(parsed)
            } else {
                Action::ConfigSelectSecondaryDeepSeekModel(parsed)
            })
        }
        LlmProvider::MiniMax => {
            let parsed = serde_json::from_value(value).ok()?;
            Some(if primary {
                Action::ConfigSelectMiniMaxModel(parsed)
            } else {
                Action::ConfigSelectSecondaryMiniMaxModel(parsed)
            })
        }
    }
}

/// Fill and submit the pending question form (web counterpart of the TUI
/// form keys — same direct state mutation, same downstream pipeline).
fn answer_question(state: &mut State, tool_use_id: &str, answers: &[QuestionAnswerPayload]) {
    let Some(form) = state.get_ext_mut::<cp_base::ui::question_form::PendingForm>() else { return };
    if form.resolved || form.tool_use_id != tool_use_id {
        return;
    }
    for (idx, payload) in answers.iter().enumerate() {
        let option_count = form.questions.get(idx).map_or(0, |question| question.options.len());
        let Some(answer) = form.answers.get_mut(idx) else { continue };
        answer.selected = payload.selected.iter().copied().filter(|&sel| sel < option_count).collect();
        if let Some(other) = &payload.other_text
            && !other.is_empty()
        {
            answer.typing_other = true;
            answer.other_text.clone_from(other);
        }
    }
    form.submit();
    state.flags.ui.dirty = true;
}

/// Dismiss the pending question form.
fn dismiss_question(state: &mut State, tool_use_id: &str) {
    let Some(form) = state.get_ext_mut::<cp_base::ui::question_form::PendingForm>() else { return };
    if form.resolved || form.tool_use_id != tool_use_id {
        return;
    }
    form.dismiss();
    state.flags.ui.dirty = true;
}

/// Answer a read-only query with a `{"t":"result"}` frame.
pub(crate) fn answer_query(state: &State, req_id: &str, query: WebQuery) -> String {
    let data = match query {
        WebQuery::ListDir { dir, prefix } => list_dir(state, &dir, &prefix),
        WebQuery::PanelContent { id } => state
            .context
            .iter()
            .find(|ctx| ctx.id == id)
            .map_or(Value::Null, super::build::panel_content_value),
        WebQuery::PromptHistory { limit } => prompt_history(limit),
        WebQuery::IndexStatus => {
            let overlay = crate::ui::search_overlay::build_search_index_overlay(state);
            json!({ "text": crate::ui::search_overlay::text::build_overlay_text(&overlay) })
        }
    };
    result_frame(req_id, &data)
}

/// Directory listing for the web `@` autocomplete.
fn list_dir(state: &State, dir: &str, prefix: &str) -> Value {
    let filter = cp_mod_tree::types::TreeState::get(state).filter.clone();
    let entries = cp_mod_tree::tools::list_dir_entries(&filter, dir, prefix);
    json!({
        "entries": entries.iter().map(|entry| json!({
            "name": entry.name, "is_dir": entry.is_dir,
        })).collect::<Vec<Value>>(),
    })
}

/// Most recent prompt-history entries (newest first).
fn prompt_history(limit: Option<usize>) -> Value {
    let mut entries = crate::state::persistence::message::load_prompt_history();
    entries.reverse();
    if let Some(max) = limit {
        entries.truncate(max);
    }
    json!({ "entries": entries })
}
