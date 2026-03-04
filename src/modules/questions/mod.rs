mod ask_question;

use crate::app::panels::Panel;
use crate::infra::tools::{ParamType, ToolDefinition, ToolParam, ToolTexts};
use crate::infra::tools::{ToolResult, ToolUse};
use crate::state::{ContextType, State};

use super::Module;

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
    serde_yaml::from_str(include_str!("../../../yamls/tools/questions.yaml"))
        .expect("Failed to parse questions tool YAML")
});

pub(crate) struct QuestionsModule;

impl Module for QuestionsModule {
    fn id(&self) -> &'static str {
        "questions"
    }
    fn name(&self) -> &'static str {
        "Questions"
    }
    fn description(&self) -> &'static str {
        "Interactive user question forms"
    }
    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Context", "Manage conversation context and system prompts")]
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("ask_user_question", t)
                .short_desc("Ask user multiple-choice questions")
                .category("Context")
                .param_array(
                    "questions",
                    ParamType::Object(vec![
                        ToolParam::new("question", ParamType::String)
                            .desc("The complete question text. Should be clear, specific, and end with ?")
                            .required(),
                        ToolParam::new("header", ParamType::String)
                            .desc("Very short label (max 12 chars). E.g. \"Auth method\", \"Library\"")
                            .required(),
                        ToolParam::new(
                            "options",
                            ParamType::Array(Box::new(ParamType::Object(vec![
                                ToolParam::new("label", ParamType::String).desc("Display text (1-5 words)").required(),
                                ToolParam::new("description", ParamType::String)
                                    .desc("Explanation of what this option means")
                                    .required(),
                            ]))),
                        )
                        .desc("2-4 available choices. An \"Other\" free-text option is appended automatically.")
                        .required(),
                        ToolParam::new("multiSelect", ParamType::Boolean)
                            .desc("If true, user can select multiple options")
                            .required(),
                    ]),
                    true,
                )
                .build(),
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "ask_user_question" => Some(self::ask_question::execute(tool, state)),
            _ => None,
        }
    }

    fn create_panel(&self, _context_type: &ContextType) -> Option<Box<dyn Panel>> {
        None
    }
}
