//! `OpenRouter` API implementation.
//!
//! `OpenRouter` is an aggregator exposing many vendors' models behind one
//! OpenAI-compatible chat-completions endpoint; the model id is a full
//! `vendor/slug[:tag]` string. Message building is delegated to the shared
//! `openai_compat` module.
//!
//! Unlike the other OpenAI-compatible providers this one is **always direct** —
//! it deliberately ignores `CP_LLM_GATEWAY`. `OpenRouter` is itself a routing
//! layer with its own key and its own catalogue, so proxying it through a second
//! gateway (which would substitute its own key) makes no sense.

use std::sync::mpsc::Sender;

use cp_mod_utilities::secret::Redacted;
use reqwest::blocking::Client;
use serde::Serialize;

use super::super::error::LlmError;
use super::super::{LlmClient, LlmRequest, StreamEvent};
use super::openai_compat::{self, BuildOptions, OaiMessage};

/// `OpenRouter` chat completions API endpoint.
const OPENROUTER_API_ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";

/// `OpenRouter` client.
pub(crate) struct OpenRouterClient {
    /// `OpenRouter` API key, resolved from vault (`"openrouter"`).
    api_key: Option<Redacted>,
}

impl OpenRouterClient {
    /// Create a new `OpenRouterClient`, reading the API key from the vault.
    pub(crate) fn new() -> Self {
        let _r = dotenvy::dotenv().ok();
        Self { api_key: cp_vault::vault().get("openrouter").map(|s| Redacted::new(s.expose().to_owned())) }
    }
}

impl Default for OpenRouterClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable request body for the `OpenRouter` chat completions API.
#[derive(Debug, Serialize)]
struct OpenRouterRequest {
    /// Model identifier — a full `vendor/slug[:tag]` id.
    model: String,
    /// Conversation messages.
    messages: Vec<OaiMessage>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<openai_compat::OaiTool>,
    /// Tool selection strategy (e.g. `"auto"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    /// Maximum number of tokens to generate.
    max_tokens: u32,
    /// Whether to stream the response via SSE.
    stream: bool,
}

impl LlmClient for OpenRouterClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        let key = self.api_key.as_ref().ok_or_else(|| LlmError::Auth("OPENROUTER_API_KEY not set".into()))?;

        let client = Client::new();

        // Collect pending tool result IDs.
        let pending_tool_ids: Vec<String> = request
            .tool_results
            .as_ref()
            .map(|results: &Vec<crate::infra::tools::ToolResult>| {
                results.iter().map(|r| r.tool_use_id.clone()).collect()
            })
            .unwrap_or_default();

        // Build messages using the shared builder.
        let mut messages = openai_compat::build_messages(
            &request.messages,
            &request.context_items,
            &BuildOptions {
                system_prompt: request.system_prompt.clone(),
                system_suffix: None,
                extra_context: request.extra_context.clone(),
                pending_tool_result_ids: pending_tool_ids,
            },
            &request.api_messages,
        );

        // Add tool results if present.
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

        let tools = openai_compat::tools_to_oai(&request.tools);
        let tool_choice = if tools.is_empty() { None } else { Some("auto".to_owned()) };

        let api_request = OpenRouterRequest {
            model: request.model.clone(),
            messages,
            tools,
            tool_choice,
            max_tokens: request.max_output_tokens,
            stream: true,
        };

        super::openai_streaming::dump_request(&request.worker_id, "openrouter", &api_request);

        let ep = super::openai_streaming::OaiEndpoint {
            client: &client,
            url: OPENROUTER_API_ENDPOINT,
            key: key.expose_secret(),
        };
        let acc = super::openai_streaming::run_oai_stream(&ep, &api_request, &tx)?;
        super::openai_streaming::send_stream_done(&tx, acc);
        Ok(())
    }

    fn check_api(&self, model: &str) -> super::super::ApiCheckResult {
        let Some(key) = self.api_key.as_ref() else {
            return super::super::ApiCheckResult::failure(Some("OPENROUTER_API_KEY not set".to_owned()));
        };
        let client = Client::new();
        let ep = super::openai_streaming::OaiEndpoint {
            client: &client,
            url: OPENROUTER_API_ENDPOINT,
            key: key.expose_secret(),
        };
        super::openai_streaming::oai_check_api(&ep, model, "max_tokens")
    }
}
