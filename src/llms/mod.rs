//! LLM provider abstraction layer.
//!
//! Provides a unified interface for different LLM providers (Anthropic, Grok, Groq, Claude Code OAuth)

pub mod anthropic;
pub mod claude_code;
pub mod grok;
pub mod groq;

use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::panels::ContextItem;
use crate::state::Message;
use crate::tool_defs::ToolDefinition;
use crate::tools::{ToolResult, ToolUse};

/// Events emitted during streaming
#[derive(Debug)]
pub enum StreamEvent {
    /// Text chunk from the response
    Chunk(String),
    /// Tool use request from the LLM
    ToolUse(ToolUse),
    /// Stream completed with token usage
    Done { input_tokens: usize, output_tokens: usize },
    /// Error occurred
    Error(String),
}

/// Result of API check
#[derive(Debug, Clone)]
pub struct ApiCheckResult {
    pub auth_ok: bool,
    pub streaming_ok: bool,
    pub tools_ok: bool,
    pub error: Option<String>,
}

impl ApiCheckResult {
    pub fn all_ok(&self) -> bool {
        self.auth_ok && self.streaming_ok && self.tools_ok
    }
}

/// Model metadata trait for context window and pricing info
pub trait ModelInfo {
    /// API model identifier
    fn api_name(&self) -> &'static str;
    /// Human-readable display name
    fn display_name(&self) -> &'static str;
    /// Maximum context window in tokens
    fn context_window(&self) -> usize;
    /// Input price per million tokens in USD
    fn input_price_per_mtok(&self) -> f32;
    /// Output price per million tokens in USD
    fn output_price_per_mtok(&self) -> f32;
}

/// Available LLM providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    #[default]
    Anthropic,
    #[serde(alias = "claudecode")]
    ClaudeCode,
    Grok,
    Groq,
}

/// Available models for Anthropic
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnthropicModel {
    #[default]
    ClaudeOpus45,
    ClaudeSonnet45,
    ClaudeHaiku45,
}

impl ModelInfo for AnthropicModel {
    fn api_name(&self) -> &'static str {
        match self {
            AnthropicModel::ClaudeOpus45 => "claude-opus-4-5",
            AnthropicModel::ClaudeSonnet45 => "claude-sonnet-4-5",
            AnthropicModel::ClaudeHaiku45 => "claude-haiku-4-5",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            AnthropicModel::ClaudeOpus45 => "Opus 4.5",
            AnthropicModel::ClaudeSonnet45 => "Sonnet 4.5",
            AnthropicModel::ClaudeHaiku45 => "Haiku 4.5",
        }
    }

    fn context_window(&self) -> usize {
        200_000
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            AnthropicModel::ClaudeOpus45 => 5.0,
            AnthropicModel::ClaudeSonnet45 => 3.0,
            AnthropicModel::ClaudeHaiku45 => 1.0,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            AnthropicModel::ClaudeOpus45 => 25.0,
            AnthropicModel::ClaudeSonnet45 => 15.0,
            AnthropicModel::ClaudeHaiku45 => 5.0,
        }
    }
}

/// Available models for Grok (fast models optimized for tool calling)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrokModel {
    #[default]
    Grok41Fast,
    Grok4Fast,
}

impl ModelInfo for GrokModel {
    fn api_name(&self) -> &'static str {
        match self {
            GrokModel::Grok41Fast => "grok-4-1-fast",
            GrokModel::Grok4Fast => "grok-4-fast",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            GrokModel::Grok41Fast => "Grok 4.1 Fast",
            GrokModel::Grok4Fast => "Grok 4 Fast",
        }
    }

    fn context_window(&self) -> usize {
        match self {
            GrokModel::Grok41Fast => 2_000_000,
            GrokModel::Grok4Fast => 2_000_000,
        }
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            GrokModel::Grok41Fast => 0.20,  // $0.20/1M input
            GrokModel::Grok4Fast => 0.20,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            GrokModel::Grok41Fast => 0.50,  // $0.50/1M output
            GrokModel::Grok4Fast => 0.50,
        }
    }
}

/// Available models for Groq
/// - GPT-OSS models: Support BOTH custom tools AND built-in tools (browser search, code exec)
/// - Llama models: Custom tools only
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroqModel {
    #[default]
    GptOss120b,
    GptOss20b,
    Llama33_70b,
    Llama31_8b,
}

impl ModelInfo for GroqModel {
    fn api_name(&self) -> &'static str {
        match self {
            GroqModel::GptOss120b => "openai/gpt-oss-120b",
            GroqModel::GptOss20b => "openai/gpt-oss-20b",
            GroqModel::Llama33_70b => "llama-3.3-70b-versatile",
            GroqModel::Llama31_8b => "llama-3.1-8b-instant",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            GroqModel::GptOss120b => "GPT-OSS 120B (+web)",
            GroqModel::GptOss20b => "GPT-OSS 20B (+web)",
            GroqModel::Llama33_70b => "Llama 3.3 70B",
            GroqModel::Llama31_8b => "Llama 3.1 8B",
        }
    }

    fn context_window(&self) -> usize {
        131_072 // All models have 131K context
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            GroqModel::GptOss120b => 1.20,
            GroqModel::GptOss20b => 0.20,
            GroqModel::Llama33_70b => 0.59,
            GroqModel::Llama31_8b => 0.05,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            GroqModel::GptOss120b => 1.20,
            GroqModel::GptOss20b => 0.20,
            GroqModel::Llama33_70b => 0.79,
            GroqModel::Llama31_8b => 0.08,
        }
    }
}

/// Configuration for an LLM request
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub context_items: Vec<ContextItem>,
    pub tools: Vec<ToolDefinition>,
    pub tool_results: Option<Vec<ToolResult>>,
    pub system_prompt: Option<String>,
    pub extra_context: Option<String>,
}

/// Trait for LLM providers
pub trait LlmClient: Send + Sync {
    /// Start a streaming response
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), String>;

    /// Check API connectivity: auth, streaming, and tool calling
    fn check_api(&self, model: &str) -> ApiCheckResult;
}

/// Get the appropriate LLM client for the given provider
pub fn get_client(provider: LlmProvider) -> Box<dyn LlmClient> {
    match provider {
        LlmProvider::Anthropic => Box::new(anthropic::AnthropicClient::new()),
        LlmProvider::ClaudeCode => Box::new(claude_code::ClaudeCodeClient::new()),
        LlmProvider::Grok => Box::new(grok::GrokClient::new()),
        LlmProvider::Groq => Box::new(groq::GroqClient::new()),
    }
}

/// Start API check in background
pub fn start_api_check(
    provider: LlmProvider,
    model: String,
    tx: Sender<ApiCheckResult>,
) {
    let client = get_client(provider);
    std::thread::spawn(move || {
        let result = client.check_api(&model);
        let _ = tx.send(result);
    });
}

/// Start streaming with the specified provider and model
pub fn start_streaming(
    provider: LlmProvider,
    model: String,
    messages: Vec<Message>,
    context_items: Vec<ContextItem>,
    tools: Vec<ToolDefinition>,
    tool_results: Option<Vec<ToolResult>>,
    system_prompt: String,
    tx: Sender<StreamEvent>,
) {
    let client = get_client(provider);

    std::thread::spawn(move || {
        let request = LlmRequest {
            model,
            messages,
            context_items,
            tools,
            tool_results,
            system_prompt: Some(system_prompt),
            extra_context: None,
        };

        if let Err(e) = client.stream(request, tx.clone()) {
            let _ = tx.send(StreamEvent::Error(e));
        }
    });
}

/// Start context cleaning with specialized prompt
pub fn start_cleaning(
    provider: LlmProvider,
    model: String,
    messages: Vec<Message>,
    context_items: Vec<ContextItem>,
    tools: Vec<ToolDefinition>,
    state: &crate::state::State,
    tx: Sender<StreamEvent>,
) {
    let client = get_client(provider);

    let cleaner_context = crate::context_cleaner::build_cleaner_context(state);
    let system_prompt = crate::context_cleaner::get_cleaner_system_prompt(state);

    std::thread::spawn(move || {
        let request = LlmRequest {
            model,
            messages,
            context_items,
            tools,
            tool_results: None,
            system_prompt: Some(system_prompt),
            extra_context: Some(cleaner_context),
        };

        if let Err(e) = client.stream(request, tx.clone()) {
            let _ = tx.send(StreamEvent::Error(e));
        }
    });
}

// Re-export common types used by providers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: Value },
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, content: String },
}

#[derive(Debug, Serialize)]
pub struct ApiMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
}
