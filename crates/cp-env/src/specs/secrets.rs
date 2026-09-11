//! Credentials (resolved by the vault) and registered external names.

use crate::spec::{Fallback, Group, Kind, Scope, Spec};

/// One vault-resolved credential.
const fn secret(name: &'static str, doc: &'static str) -> Spec {
    Spec {
        name,
        kind: Kind::Secret,
        fallback: Fallback::None,
        scope: Scope::Both,
        group: Group::Secrets,
        doc,
        ..Spec::BASE
    }
    .sensitive()
}

/// One name neither binary parses.
const fn external(name: &'static str, doc: &'static str) -> Spec {
    Spec {
        name,
        kind: Kind::Str,
        fallback: Fallback::None,
        scope: Scope::ChildOnly,
        group: Group::External,
        doc,
        ..Spec::BASE
    }
}

/// Credentials. Mirrors the vault registry; never read here.
pub(super) static SECRETS: &[Spec] = &[
    secret("ANTHROPIC_API_KEY", "Anthropic."),
    secret("XAI_API_KEY", "Grok (xAI)."),
    secret("DEEPSEEK_API_KEY", "DeepSeek."),
    secret("GROQ_API_KEY", "Groq."),
    secret("MINIMAX_API_KEY", "MiniMax."),
    secret("BRAVE_API_KEY", "Brave Search (web search tool)."),
    secret("FIRECRAWL_API_KEY", "Firecrawl (web scraping tool)."),
    secret("DATALAB_API_KEY", "Datalab (OCR tool)."),
    secret("VOYAGE_API_KEY", "Voyage AI (embeddings for search)."),
    secret("GITHUB_TOKEN", "GitHub (the github module and the `gh` CLI it drives)."),
    secret("TELEGRAM_BOT_TOKEN", "Telegram bot bridge."),
    secret("DISCORD_BOT_TOKEN", "Discord bot bridge."),
    secret("SLACK_BOT_TOKEN", "Slack bot bridge."),
    secret("GOOGLECHAT_BOT_TOKEN", "Google Chat bot bridge."),
    secret("TELEGRAM_API_ID", "Telegram API id (user-account bridge)."),
    secret("TELEGRAM_API_HASH", "Telegram API hash (user-account bridge)."),
];

/// `CP_`-prefixed names consumed elsewhere.
pub(super) static EXTERNAL: &[Spec] = &[
    external("CP_CHANGED_FILES", "Injected into global callback scripts: newline-separated changed paths."),
    external("CP_CHANGED_FILE", "Injected into local callback scripts: the one changed path."),
    external("CP_PROJECT_ROOT", "Injected into callback scripts: the project root."),
    external("CP_CALLBACK_NAME", "Injected into callback scripts: the callback's name."),
    external("CP_CRASH_CHILD_DIR", "Test harness (`cp-oplog` crash replay): the child's oplog directory."),
    external("CP_CRASH_CHILD_MODE", "Test harness (`cp-oplog` crash replay): the child's mode."),
    external("CP_PORT", "Docker Compose only: the host port the cockpit is published on."),
    external("CP_API_URL", "Playwright only: the orchestrator the end-to-end tests target."),
    external("CP_WEB_URL", "Playwright only: the dev server the end-to-end tests target."),
    external("CP_AGENT_ID", "Playwright only: the agent the regression probes target."),
    external(
        "CP_AGENT_LOCK_FD",
        "Reserved (design doc): the registry lock descriptor passed across a deadman re-exec. Not implemented.",
    ),
];
