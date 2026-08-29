//! What the product knows about the LLMs it talks to.
//!
//! Grouped as one sub-module because the three parts are read together: a
//! provider is chosen ([`types`]), a model of that provider is picked
//! ([`models`]), and the destination of the call is resolved ([`gateway`]) —
//! its own API, or the proxy an operator put in front of it.

/// Optional LLM gateway (`CP_LLM_GATEWAY`) and the providers it carries.
pub mod gateway;
/// Per-provider model enums (Anthropic, Grok, Groq, DeepSeek, MiniMax, Claude Code V2).
pub mod models;
/// LLM provider/model type definitions and capabilities.
pub mod types;
