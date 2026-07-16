//! Core OpenAPI schemas — domain types that the spec references.

use serde_json::{Value, json};

use super::{arr, r};

/// Agent/thread/panel/tool schemas — the domain model.
pub(super) fn core() -> Value {
    json!({
        // ── Shared enums ────────────────────────────────────────
        "AccentToken": {
            "type": "string",
            "enum": ["signal", "interactive", "ok", "warn", "danger"]
        },
        // ── Domain types ────────────────────────────────────────
        "Error": {
            "type": "object",
            "properties": { "error": { "type": "string" } },
            "required": ["error"]
        },
        "Agent": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "folder": { "type": "string" },
                "branch": { "type": "string" },
                "model": { "type": "string" },
                "provider": { "type": "string" },
                "status": { "type": "string", "enum": ["working", "needs-you", "idle", "disconnected"] },
                "phase": { "type": "string", "enum": ["idle", "streaming", "tooling"] },
                "costUsd": { "type": "number" },
                "inputTokens": { "type": "integer" },
                "outputTokens": { "type": "integer" },
                "contextUsed": { "type": "integer" },
                "contextThreshold": { "type": "integer" },
                "contextBudget": { "type": "integer" },
                "contextHit": { "type": "integer" },
                "contextMiss": { "type": "integer" },
                "task": { "type": "string" },
                "threads": { "type": "integer" },
                "lastActivity": { "type": "string" },
                "hasAvatar": { "type": "boolean" },
                "accent": r("AccentToken")
            },
            "required": ["id", "name", "folder", "status", "costUsd", "threads", "lastActivity"]
        },
        "AgentMetrics": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "stream": { "type": "object", "properties": {
                    "subscribers": { "type": "integer" },
                    "droppedFrames": { "type": "integer" },
                    "degraded": { "type": "boolean" }
                }},
                "rev": { "type": "object", "properties": {
                    "view": { "type": "integer" },
                    "oplogHead": { "type": "integer", "nullable": true },
                    "lag": { "type": "integer" }
                }},
                "tokens": { "type": "object", "properties": {
                    "input": { "type": "integer" },
                    "output": { "type": "integer" }
                }},
                "phase": { "type": "string", "nullable": true },
                "lifecycle": { "type": "string", "nullable": true }
            },
            "required": ["id", "stream", "rev"]
        },
        "Vital": {
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "category": { "type": "string" },
                "status": { "type": "string", "enum": ["ok", "error", "unavailable"] },
                "latencyMs": { "type": "integer", "nullable": true },
                "detail": { "type": "string" }
            },
            "required": ["name", "category", "status"]
        },
        "ThreadDetail": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "status": { "type": "string", "enum": ["MY_TURN", "THEIR_TURN", "ACTIVE"] },
                "agentId": { "type": "string" },
                "agent": { "type": "string" },
                "createdAt": { "type": "string" },
                "lastActivity": { "type": "string" },
                "lastActivityMs": { "type": "integer" },
                "unread": { "type": "integer" },
                "archived": { "type": "boolean" },
                "paused": { "type": "boolean" },
                "focused": { "type": "boolean" },
                "log": arr(r("ThreadMsg"))
            },
            "required": ["id", "name", "status", "agentId", "log"]
        },
        "ThreadMsg": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "author": { "type": "string", "enum": ["user", "assistant"] },
                "text": { "type": "string" },
                "ts": { "type": "integer" },
                "tool": r("ToolCall"),
                "fileRef": { "type": "string" },
                "auto": { "type": "boolean" }
            },
            "required": ["id", "author"]
        },
        "ToolCall": {
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "intent": { "type": "string" },
                "verb": { "type": "string" },
                "params": { "type": "object", "additionalProperties": { "type": "string" } },
                "result": { "type": "string" },
                "isError": { "type": "boolean" }
            },
            "required": ["name"]
        },
        "ThreadsResponse": {
            "type": "object",
            "properties": {
                "focusedThreadId": { "type": "string", "nullable": true },
                "threads": arr(r("ThreadDetail"))
            },
            "required": ["threads"]
        },
        "LibraryItem": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "kind": { "type": "string", "enum": ["agent", "skill", "command"] },
                "description": { "type": "string" },
                "meta": { "type": "string" },
                "body": { "type": "string" },
                "builtin": { "type": "boolean" },
                "active": { "type": "boolean" }
            },
            "required": ["id", "name", "kind", "description"]
        }
    })
}
