//! Groq API implementation.
//!
//! Groq uses an OpenAI-compatible API format.
//! Message building is delegated to the shared `openai_compat` module.

use std::sync::mpsc::Sender;

use cp_mod_utilities::secret::Redacted;
use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::Value;

use super::super::error::LlmError;
use super::super::{LlmClient, LlmRequest, StreamEvent};
use super::openai_compat::{self, BuildOptions, OaiMessage};
use crate::infra::tools::ToolDefinition;
use cp_base::config::INJECTIONS;

/// Groq chat completions API endpoint.
const GROQ_API_ENDPOINT: &str = "https://api.groq.com/openai/v1/chat/completions";

/// Groq client
pub(crate) struct GroqClient {
    /// Groq API key, resolved from vault (`"groq"`).
    api_key: Option<Redacted>,
}

impl GroqClient {
    /// Create a new `GroqClient`, reading the API key from the vault.
    pub(crate) fn new() -> Self {
        let _r = dotenvy::dotenv().ok();
        Self { api_key: cp_vault::vault().get("groq").map(|s| Redacted::new(s.expose().to_owned())) }
    }
}

impl Default for GroqClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable request body for the Groq chat completions API.
#[derive(Debug, Serialize)]
struct GroqRequest {
    /// Model identifier (e.g. `"llama-3.3-70b-versatile"`).
    model: String,
    /// Conversation messages.
    messages: Vec<OaiMessage>,
    /// Tool definitions (function tools or built-in tools like `browser_search`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
    /// Tool selection strategy (e.g. `"auto"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    /// Maximum number of completion tokens to generate.
    max_completion_tokens: u32,
    /// Whether to stream the response via SSE.
    stream: bool,
}

impl LlmClient for GroqClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        let api_key = self.api_key.as_ref().ok_or_else(|| LlmError::Auth("GROQ_API_KEY not set".into()))?;

        let client = Client::new();

        // Collect pending tool result IDs
        let pending_tool_ids: Vec<String> = request
            .tool_results
            .as_ref()
            .map(|results: &Vec<crate::infra::tools::ToolResult>| {
                results.iter().map(|r| r.tool_use_id.clone()).collect()
            })
            .unwrap_or_default();

        // GPT-OSS models get extra info about built-in tools
        let system_suffix = request
            .model
            .starts_with("openai/gpt-oss")
            .then(|| INJECTIONS.providers.gpt_oss_suffix.trim_end().to_owned());

        // Build messages using shared builder
        let mut messages = openai_compat::build_messages(
            &request.messages,
            &request.context_items,
            &BuildOptions {
                system_prompt: request.system_prompt.clone(),
                system_suffix,
                extra_context: request.extra_context.clone(),
                pending_tool_result_ids: pending_tool_ids,
            },
            &request.api_messages,
        );

        // Add tool results if present
        if let Some(results) = request.tool_results.as_ref() {
            for result in results {
                messages.push(OaiMessage {
                    role: "tool".to_owned(),
                    content: Some(result.content.clone()),
                    tool_calls: None,
                    tool_call_id: Some(result.tool_use_id.clone()),
                });
            }
        }

        let tools = tools_to_groq(&request.tools, &request.model);
        let tool_choice = if tools.is_empty() { None } else { Some("auto".to_owned()) };

        let api_request = GroqRequest {
            model: request.model.clone(),
            messages,
            tools,
            tool_choice,
            max_completion_tokens: request.max_output_tokens,
            stream: true,
        };

        super::openai_streaming::dump_request(&request.worker_id, "groq", &api_request);

        let ep = super::openai_streaming::OaiEndpoint {
            client: &client,
            url: GROQ_API_ENDPOINT,
            key: api_key.expose_secret(),
        };
        let acc = super::openai_streaming::run_oai_stream(&ep, &api_request, &tx)?;
        super::openai_streaming::send_stream_done(&tx, acc);
        Ok(())
    }

    fn check_api(&self, model: &str) -> super::super::ApiCheckResult {
        let Some(api_key) = self.api_key.as_ref() else {
            return super::super::ApiCheckResult::failure(Some("GROQ_API_KEY not set".to_owned()));
        };
        let client = Client::new();
        let ep = super::openai_streaming::OaiEndpoint {
            client: &client,
            url: GROQ_API_ENDPOINT,
            key: api_key.expose_secret(),
        };
        super::openai_streaming::oai_check_api(&ep, model, "max_completion_tokens")
    }
}

/// Convert tool definitions to Groq format.
/// For GPT-OSS models, also adds built-in tools (`browser_search`, `code_interpreter`).
fn tools_to_groq(tools: &[ToolDefinition], model: &str) -> Vec<Value> {
    let mut groq_tools: Vec<Value> = tools
        .iter()
        .filter(|t| t.enabled)
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.id,
                    "description": t.description,
                    "parameters": t.to_json_schema(),
                }
            })
        })
        .collect();

    // Add built-in tools for GPT-OSS models
    if model.starts_with("openai/gpt-oss") {
        groq_tools.push(serde_json::json!({"type": "browser_search"}));
        groq_tools.push(serde_json::json!({"type": "code_interpreter"}));
    }

    groq_tools
}
