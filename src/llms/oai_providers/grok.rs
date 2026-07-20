//! xAI Grok API implementation.
//!
//! Grok uses an OpenAI-compatible API format.
//! Message building is delegated to the shared `openai_compat` module.

use std::sync::mpsc::Sender;

use cp_mod_utilities::secret::Redacted;
use reqwest::blocking::Client;
use serde::Serialize;

use super::super::error::LlmError;
use super::super::{LlmClient, LlmRequest, StreamEvent};
use super::openai_compat::{self, BuildOptions, OaiMessage};

/// xAI Grok chat completions API endpoint.
const GROK_API_ENDPOINT: &str = "https://api.x.ai/v1/chat/completions";

/// xAI Grok client
pub(crate) struct GrokClient {
    /// xAI API key, resolved from vault (`"xai"`).
    api_key: Option<Redacted>,
}

impl GrokClient {
    /// Create a new `GrokClient`, reading the API key from the vault.
    pub(crate) fn new() -> Self {
        let _r = dotenvy::dotenv().ok();
        Self { api_key: cp_vault::vault().get("xai").map(|s| Redacted::new(s.expose().to_owned())) }
    }
}

impl Default for GrokClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable request body for the Grok chat completions API.
#[derive(Debug, Serialize)]
struct GrokRequest {
    /// Model identifier (e.g. `"grok-3"`).
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

impl LlmClient for GrokClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        let api_key = self.api_key.as_ref().ok_or_else(|| LlmError::Auth("XAI_API_KEY not set".into()))?;

        let client = Client::new();

        // Collect pending tool result IDs
        let pending_tool_ids: Vec<String> = request
            .tool_results
            .as_ref()
            .map(|results: &Vec<crate::infra::tools::ToolResult>| {
                results.iter().map(|r| r.tool_use_id.clone()).collect()
            })
            .unwrap_or_default();

        // Build messages using shared builder
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

        let tools = openai_compat::tools_to_oai(&request.tools);
        let tool_choice = if tools.is_empty() { None } else { Some("auto".to_owned()) };

        let api_request = GrokRequest {
            model: request.model.clone(),
            messages,
            tools,
            tool_choice,
            max_tokens: request.max_output_tokens,
            stream: true,
        };

        super::openai_streaming::dump_request(&request.worker_id, "grok", &api_request);

        let ep = super::openai_streaming::OaiEndpoint {
            client: &client,
            url: GROK_API_ENDPOINT,
            key: api_key.expose_secret(),
        };
        let acc = super::openai_streaming::run_oai_stream(&ep, &api_request, &tx)?;
        super::openai_streaming::send_stream_done(&tx, acc);
        Ok(())
    }

    fn check_api(&self, model: &str) -> super::super::ApiCheckResult {
        let Some(api_key) = self.api_key.as_ref() else {
            return super::super::ApiCheckResult::failure(Some("XAI_API_KEY not set".to_owned()));
        };
        let client = Client::new();
        let ep = super::openai_streaming::OaiEndpoint {
            client: &client,
            url: GROK_API_ENDPOINT,
            key: api_key.expose_secret(),
        };
        super::openai_streaming::oai_check_api(&ep, model, "max_tokens")
    }
}
