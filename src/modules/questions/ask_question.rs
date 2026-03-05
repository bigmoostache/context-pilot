use crate::infra::tools::{ToolResult, ToolUse};
use crate::state::State;
use cp_base::ui::question_form::{PendingForm, Question, QuestionOption};

/// Execute the `ask_user_question` tool.
/// Parses input, validates constraints, stores `PendingForm` in state.
/// Returns a placeholder result — the real result is produced when the user
/// submits or dismisses the form (handled by app.rs).
pub(super) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(questions_val) = tool.input.get("questions").and_then(serde_json::Value::as_array) else {
        return ToolResult::new(
            tool.id.clone(),
            "Missing 'questions' parameter (expected array of 1-4 questions)".to_string(),
            true,
        );
    };

    // Validate question count
    if questions_val.is_empty() || questions_val.len() > 4 {
        return ToolResult::new(tool.id.clone(), format!("Expected 1-4 questions, got {}", questions_val.len()), true);
    }

    let mut questions = Vec::new();

    for (i, q_val) in questions_val.iter().enumerate() {
        let question = match q_val.get("question").and_then(serde_json::Value::as_str) {
            Some(s) => s.to_string(),
            None => {
                return ToolResult::new(
                    tool.id.clone(),
                    format!("Question {}: missing 'question' field", i.saturating_add(1)),
                    true,
                );
            }
        };

        let header = match q_val.get("header").and_then(serde_json::Value::as_str) {
            Some(s) => {
                if s.chars().count() > 12 {
                    s.chars().take(12).collect()
                } else {
                    s.to_string()
                }
            }
            None => {
                return ToolResult::new(
                    tool.id.clone(),
                    format!("Question {}: missing 'header' field", i.saturating_add(1)),
                    true,
                );
            }
        };

        let multi_select = q_val.get("multiSelect").and_then(serde_json::Value::as_bool).unwrap_or(false);

        let Some(options_val) = q_val.get("options").and_then(serde_json::Value::as_array) else {
            return ToolResult::new(
                tool.id.clone(),
                format!("Question {}: missing 'options' field", i.saturating_add(1)),
                true,
            );
        };

        if options_val.len() < 2 || options_val.len() > 4 {
            return ToolResult::new(
                tool.id.clone(),
                format!("Question {}: expected 2-4 options, got {}", i.saturating_add(1), options_val.len()),
                true,
            );
        }

        let mut options = Vec::new();
        for (j, o_val) in options_val.iter().enumerate() {
            let label = match o_val.get("label").and_then(serde_json::Value::as_str) {
                Some(s) => s.to_string(),
                None => {
                    return ToolResult::new(
                        tool.id.clone(),
                        format!("Question {} option {}: missing 'label'", i.saturating_add(1), j.saturating_add(1)),
                        true,
                    );
                }
            };
            let description = match o_val.get("description").and_then(serde_json::Value::as_str) {
                Some(s) => s.to_string(),
                None => {
                    return ToolResult::new(
                        tool.id.clone(),
                        format!(
                            "Question {} option {}: missing 'description'",
                            i.saturating_add(1),
                            j.saturating_add(1)
                        ),
                        true,
                    );
                }
            };
            options.push(QuestionOption { label, description });
        }

        questions.push(Question { text: question, header, options, multi_select });
    }

    // Store the pending form in state
    let form = PendingForm::new(tool.id.clone(), questions);
    state.set_ext(form);

    // Return a placeholder — the real result is injected by app.rs when user responds
    ToolResult::new(tool.id.clone(), "__QUESTION_PENDING__".to_string(), false)
}
