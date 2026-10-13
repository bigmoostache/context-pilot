//! Core `OpenAPI` schemas — domain types that the spec references.

use serde_json::{Value, json};

use super::{arr, r};

/// Agent/agent/panel/tool schemas — the domain model.
#[expect(
    clippy::too_many_lines,
    reason = "flat OpenAPI schema literal: one linear json! object enumerating the domain schemas; splitting it into sub-builders is churn with no readability gain — the openapi spec-builder twin of the flat State::default initializer"
)]
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
        "Fleet": {
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
                "agents": { "type": "integer" },
                "lastActivity": { "type": "string" },
                "hasAvatar": { "type": "boolean" },
                "accent": r("AccentToken")
            },
            "required": ["id", "name", "folder", "status", "costUsd", "agents", "lastActivity"]
        },
        "FleetMetrics": {
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
        "AgentDetail": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "status": { "type": "string", "enum": ["MY_TURN", "THEIR_TURN", "ACTIVE"] },
                "fleetId": { "type": "string" },
                "fleet": { "type": "string" },
                "createdAt": { "type": "string" },
                "lastActivity": { "type": "string" },
                "lastActivityMs": { "type": "integer" },
                "unread": { "type": "integer" },
                "archived": { "type": "boolean" },
                "paused": { "type": "boolean" },
                "focused": { "type": "boolean" },
                // Parent agent id when this agent was branched out of another
                // (branch_agent command); null for an agent created from scratch.
                "branchedFrom": { "type": "string", "nullable": true },
                "log": arr(r("AgentMsg")),
                "tasks": arr(r("AgentTask")),
                "notes": arr(r("AgentNote"))
            },
            "required": ["id", "name", "status", "fleetId", "log"]
        },
        "AgentTask": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "parentId": { "type": "string", "nullable": true },
                "name": { "type": "string" },
                "description": { "type": "string" },
                "status": { "type": "string", "enum": ["planned", "in_progress", "done"] }
            },
            "required": ["id", "name", "status"]
        },
        "AgentNote": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "title": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["id", "title", "content"]
        },
        "AgentMsg": {
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
        "AgentsResponse": {
            "type": "object",
            "properties": {
                "focusedAgentId": { "type": "string", "nullable": true },
                "agents": arr(r("AgentDetail"))
            },
            "required": ["agents"]
        },
        "LibraryItem": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "kind": { "type": "string", "enum": ["system", "skill", "command"] },
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
