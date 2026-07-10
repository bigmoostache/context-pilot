//! Deploy-plane OpenAPI schemas — release management, app settings/profile,
//! and Claude Code OAuth usage + login.
//!
//! Split out of [`schemas_ext`](super::schemas_ext) to keep each file within
//! the line budget; merged alongside it in the spec builder.

use serde_json::{Value, json};

use super::{arr, r};

/// Release management, settings/session, and Claude Code OAuth schemas.
pub(super) fn deploy() -> Value {
    json!({
        // ── Release management (T427) ───────────────────────────────
        "ReleaseEntry": {
            "type": "object",
            "properties": {
                "tag": { "type": "string" },
                "name": { "type": "string" },
                "publishedAt": { "type": "string", "nullable": true },
                "assetUrl": { "type": "string", "nullable": true },
                "assetSize": { "type": "integer", "nullable": true },
                "isLatest": { "type": "boolean" },
                "local": { "type": "boolean" },
                "selected": { "type": "boolean" },
                "binarySize": { "type": "integer", "nullable": true }
            },
            "required": ["tag", "name", "local", "selected"]
        },
        "ReleasesResponse": {
            "type": "object",
            "properties": {
                "arch": { "type": "string" },
                "archAuto": { "type": "boolean" },
                "activeTag": { "type": "string", "nullable": true },
                "currentBinary": { "type": "string" },
                "knownArchs": arr(json!({ "type": "string" })),
                "releases": arr(r("ReleaseEntry"))
            },
            "required": ["arch", "archAuto", "currentBinary", "knownArchs", "releases"]
        },
        "ArchResponse": {
            "type": "object",
            "properties": {
                "arch": { "type": "string" },
                "archAuto": { "type": "boolean" }
            },
            "required": ["arch", "archAuto"]
        },
        "DownloadResponse": {
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "tag": { "type": "string" }
            },
            "required": ["status", "tag"]
        },
        "SelectResponse": {
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "tag": { "type": "string" },
                "binaryPath": { "type": "string" }
            },
            "required": ["status", "tag", "binaryPath"]
        },
        // ── Settings & profile (auth-rbac) ──────────────────────
        "AppSettings": {
            "type": "object",
            "properties": {
                "default_provider": { "type": "string", "nullable": true },
                "default_model": { "type": "string", "nullable": true },
                "onboarding_completed": { "type": "boolean" },
                "is_admin": { "type": "boolean" },
                "auth_enabled": { "type": "boolean" },
                // Access-control master flag (design §13.10) — server-authoritative.
                "access_control": { "type": "boolean" },
                "providers": arr(json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "configured": { "type": "boolean" }
                    },
                    "required": ["id", "configured"]
                })),
                "allowed_models": arr(json!({ "type": "string" }))
            },
            "required": ["onboarding_completed", "is_admin", "auth_enabled", "access_control", "providers", "allowed_models"]
        },
        "SessionInfo": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "created_at": { "type": "integer" },
                "expires_at": { "type": "integer" },
                "user_agent": { "type": "string", "nullable": true },
                "current": { "type": "boolean" }
            },
            "required": ["id", "created_at", "expires_at", "current"]
        },
        "DeployResponse": {
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "tag": { "type": "string" },
                "restarted": { "type": "array", "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "pid": { "type": "integer" }
                    },
                    "required": ["id", "pid"]
                }},
                "errors": arr(json!({ "type": "string" }))
            },
            "required": ["status", "tag", "restarted", "errors"]
        },
        "RestartOrchestratorResponse": {
            "type": "object",
            "properties": {
                "status": { "type": "string" }
            },
            "required": ["status"]
        },
        // ── Claude Code usage (T451) ────────────────────────────────
        "ClaudeUsageLimit": {
            "type": "object",
            "properties": {
                "kind": { "type": "string" },
                "group": { "type": "string" },
                "percent": { "type": "integer" },
                "severity": { "type": "string" },
                "resets_at": { "type": "string", "nullable": true },
                "scope": { "type": "string", "nullable": true },
                "is_active": { "type": "boolean" }
            },
            "required": ["kind", "group", "percent", "severity", "is_active"]
        },
        "ClaudeUsageResponse": {
            "type": "object",
            "description": "Claude Code OAuth usage limits (proxied from Anthropic).",
            "properties": {
                "limits": arr(r("ClaudeUsageLimit"))
            }
        },
        // ── Claude Code OAuth login (T451) ──────────────────────────
        "ClaudeTokenStatus": {
            "type": "object",
            "properties": {
                "valid": { "type": "boolean" },
                "account_email": { "type": "string", "nullable": true },
                "expires_at": { "type": "integer", "nullable": true },
                "subscription_type": { "type": "string", "nullable": true },
                "rate_limit_tier": { "type": "string", "nullable": true }
            },
            "required": ["valid"]
        },
        "ClaudeLoginStartResponse": {
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "already_valid": { "type": "boolean", "nullable": true }
            },
            "required": ["url"]
        },
        "ClaudeLoginCompleteRequest": {
            "type": "object",
            "properties": {
                "code": { "type": "string" }
            },
            "required": ["code"]
        },
        "ClaudeLoginCompleteResponse": {
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "expires_at": { "type": "integer", "nullable": true }
            },
            "required": ["status"]
        },
        // ── Claude multi-account token vault ────────────────────────
        "ClaudeAccountSummary": {
            "type": "object",
            "properties": {
                "email": { "type": "string" },
                "expires_at": { "type": "integer", "nullable": true },
                "valid": { "type": "boolean" }
            },
            "required": ["email", "valid"]
        },
        "ClaudeAccountsListResponse": {
            "type": "object",
            "properties": {
                "accounts": arr(r("ClaudeAccountSummary"))
            },
            "required": ["accounts"]
        }
    })
}
