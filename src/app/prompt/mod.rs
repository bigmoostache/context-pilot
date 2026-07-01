//! Centralized prompt assembly for all LLM providers.
//!
//! - [`builder`] assembles the wire message list (panel injection → conversation
//!   → alternation) and is the single funnel every provider streams through.
//! - [`repair`] runs as the final assembly phase, enforcing the Anthropic
//!   tool-call adjacency invariant so an orphaned `tool_use` can never reach the
//!   API (self-heals reshuffled/truncated histories).

/// Wire message assembly: panel injection, conversation, alternation.
pub(crate) mod builder;
/// Final-phase tool-pairing repair (adjacency invariant enforcement).
mod repair;

pub(crate) use builder::assemble_prompt;
