//! Key registry — single source of truth for all known credentials.
//!
//! Every key the system manages is declared here.  Modules reference keys by
//! canonical name (e.g. `"anthropic"`) or env var name (e.g. `"ANTHROPIC_API_KEY"`).

/// Category of a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyCategory {
    /// LLM provider API key (Anthropic, xAI, DeepSeek, etc.)
    LlmProvider,
    /// Web tool API key (Brave, Firecrawl, Datalab, Voyage)
    WebTool,
    /// Version control system token (GitHub)
    Vcs,
    /// Chat bridge bot token (Telegram, Discord, Slack)
    Bridge,
    /// Internal operational env var (`CP_BRIDGE`, `CP_FLAMEGRAPH`)
    Internal,
}

/// How a credential is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthMechanism {
    /// Standard environment variable lookup.
    EnvVar,
    /// macOS Keychain first, then credential file fallback (Claude OAuth).
    KeychainThenFile,
}

/// Definition of a well-known credential.
///
/// `#[non_exhaustive]`: constructed only in-crate (the `ALL_KEYS` table);
/// external code reads fields via `resolve_definition`, so adding a field
/// is not a breaking change.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct KeyDefinition {
    /// Short canonical name used in vault API calls (e.g. `"anthropic"`).
    pub canonical: &'static str,
    /// Environment variable name (e.g. `"ANTHROPIC_API_KEY"`).  Empty for
    /// credentials that don't use env vars (e.g. OAuth via Keychain).
    pub env_var: &'static str,
    /// Human-readable display label.
    pub display: &'static str,
    /// Category for grouping in UI and health checks.
    pub category: KeyCategory,
    /// Resolution mechanism.
    pub mechanism: AuthMechanism,
}

/// All known credentials managed by the vault.
///
/// This single array replaces the three disconnected registries that existed
/// before (`global.rs KEY_ENV_MAP`, orchestrator `KNOWN_KEYS`, per-module
/// hardcoded env var strings).
pub static ALL_KEYS: &[KeyDefinition] = &[
    // ── LLM Providers ──────────────────────────────────────────────────────
    KeyDefinition {
        canonical: "anthropic",
        env_var: "ANTHROPIC_API_KEY",
        display: "Anthropic",
        category: KeyCategory::LlmProvider,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "xai",
        env_var: "XAI_API_KEY",
        display: "Grok (xAI)",
        category: KeyCategory::LlmProvider,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "deepseek",
        env_var: "DEEPSEEK_API_KEY",
        display: "DeepSeek",
        category: KeyCategory::LlmProvider,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "groq",
        env_var: "GROQ_API_KEY",
        display: "Groq",
        category: KeyCategory::LlmProvider,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "minimax",
        env_var: "MINIMAX_API_KEY",
        display: "MiniMax",
        category: KeyCategory::LlmProvider,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "claude_oauth",
        env_var: "",
        display: "Claude Code (OAuth)",
        category: KeyCategory::LlmProvider,
        mechanism: AuthMechanism::KeychainThenFile,
    },
    // ── Web Tools ──────────────────────────────────────────────────────────
    KeyDefinition {
        canonical: "brave",
        env_var: "BRAVE_API_KEY",
        display: "Brave Search",
        category: KeyCategory::WebTool,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "firecrawl",
        env_var: "FIRECRAWL_API_KEY",
        display: "Firecrawl",
        category: KeyCategory::WebTool,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "datalab",
        env_var: "DATALAB_API_KEY",
        display: "Datalab OCR",
        category: KeyCategory::WebTool,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "voyage",
        env_var: "VOYAGE_API_KEY",
        display: "Voyage AI",
        category: KeyCategory::WebTool,
        mechanism: AuthMechanism::EnvVar,
    },
    // ── VCS ────────────────────────────────────────────────────────────────
    KeyDefinition {
        canonical: "github",
        env_var: "GITHUB_TOKEN",
        display: "GitHub",
        category: KeyCategory::Vcs,
        mechanism: AuthMechanism::EnvVar,
    },
    // ── Bridge Bot Tokens ──────────────────────────────────────────────────
    KeyDefinition {
        canonical: "telegram_bot",
        env_var: "TELEGRAM_BOT_TOKEN",
        display: "Telegram Bot",
        category: KeyCategory::Bridge,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "discord_bot",
        env_var: "DISCORD_BOT_TOKEN",
        display: "Discord Bot",
        category: KeyCategory::Bridge,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "slack_bot",
        env_var: "SLACK_BOT_TOKEN",
        display: "Slack Bot",
        category: KeyCategory::Bridge,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "googlechat_bot",
        env_var: "GOOGLECHAT_BOT_TOKEN",
        display: "Google Chat Bot",
        category: KeyCategory::Bridge,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "telegram_api_id",
        env_var: "TELEGRAM_API_ID",
        display: "Telegram API ID",
        category: KeyCategory::Bridge,
        mechanism: AuthMechanism::EnvVar,
    },
    KeyDefinition {
        canonical: "telegram_api_hash",
        env_var: "TELEGRAM_API_HASH",
        display: "Telegram API Hash",
        category: KeyCategory::Bridge,
        mechanism: AuthMechanism::EnvVar,
    },
];

/// Resolve a [`KeyDefinition`] by canonical name or env var name.
///
/// Accepts either form: `"anthropic"` or `"ANTHROPIC_API_KEY"`.
#[must_use]
pub fn resolve_definition(key: &str) -> Option<&'static KeyDefinition> {
    ALL_KEYS.iter().find(|k| k.canonical == key || k.env_var == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_by_canonical() {
        let def = resolve_definition("anthropic");
        assert!(def.is_some());
        assert_eq!(def.map(|d| d.env_var), Some("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn resolve_by_env_var() {
        let def = resolve_definition("BRAVE_API_KEY");
        assert!(def.is_some());
        assert_eq!(def.map(|d| d.canonical), Some("brave"));
    }

    #[test]
    fn resolve_unknown_returns_none() {
        assert!(resolve_definition("nonexistent_key_xyz").is_none());
    }

    #[test]
    fn all_keys_have_unique_canonicals() {
        let mut seen = std::collections::HashSet::new();
        for key in ALL_KEYS {
            assert!(seen.insert(key.canonical), "duplicate canonical: {}", key.canonical);
        }
    }
}
