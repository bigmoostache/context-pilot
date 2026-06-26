//! Core OpenAPI schemas — domain types that the spec references.

use serde_json::{json, Value};

use super::{arr, r};

/// Agent/thread/panel/tool schemas — the domain model.
pub(super) fn core() -> Value {
    json!({
        // ── Shared enums ────────────────────────────────────────
        "PanelKind": {
            "type": "string",
            "enum": [
                "tree", "memory", "threads", "spine", "stats", "entities",
                "search", "file", "git", "console", "queue", "todo",
                "callback", "scratchpad", "tools", "radar"
            ]
        },
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
                "status": { "type": "string", "enum": ["working", "needs-you", "idle"] },
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
                "breaker": { "type": "object", "properties": {
                    "tripped": { "type": "boolean" },
                    "spendUsd": { "type": "number" },
                    "budgetUsd": { "type": "number" }
                }},
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
            "required": ["id", "breaker", "stream", "rev"]
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
                "questions": arr(r("ThreadQuestion")),
                "fileRef": { "type": "string" },
                "auto": { "type": "boolean" }
            },
            "required": ["id", "author"]
        },
        "ThreadQuestion": {
            "type": "object",
            "properties": {
                "header": { "type": "string" },
                "prompt": { "type": "string" },
                "options": arr(json!({ "type": "string" })),
                "multi": { "type": "boolean" },
                "allowOther": { "type": "boolean" },
                "answered": arr(json!({ "type": "string" }))
            },
            "required": ["prompt", "options"]
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
        "ContextPanel": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "kind": r("PanelKind"),
                "name": { "type": "string" },
                "tokens": { "type": "integer" },
                "costUsd": { "type": "number" },
                "cached": { "type": "boolean" },
                "frozen": { "type": "integer", "nullable": true },
                "misses": { "type": "integer" },
                "fixed": { "type": "boolean" }
            },
            "required": ["id", "kind", "name", "tokens"]
        },
        "MemoryCard": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "tldr": { "type": "string" },
                "importance": { "type": "string", "enum": ["low", "medium", "high", "critical"] },
                "labels": arr(json!({ "type": "string" }))
            },
            "required": ["id", "tldr", "importance", "labels"]
        },
        "TodoItem": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "status": { "type": "string", "enum": ["pending", "in_progress", "done"] },
                "depth": { "type": "integer" }
            },
            "required": ["id", "name", "status", "depth"]
        },
        "SpineNotif": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "kind": { "type": "string", "enum": ["user", "reload", "custom"] },
                "time": { "type": "string" },
                "text": { "type": "string" },
                "processed": { "type": "boolean" }
            },
            "required": ["id", "kind", "text", "processed"]
        },
        "QueueAction": {
            "type": "object",
            "properties": {
                "index": { "type": "integer" },
                "tool": { "type": "string" },
                "intent": { "type": "string" },
                "preview": { "type": "string" }
            },
            "required": ["index", "tool"]
        },
        "ScratchCell": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "title": { "type": "string" },
                "preview": { "type": "string" }
            },
            "required": ["id", "title"]
        },
        "TreeRow": {
            "type": "object",
            "properties": {
                "depth": { "type": "integer" },
                "name": { "type": "string" },
                "kind": { "type": "string", "enum": ["dir", "file"] },
                "size": { "type": "string" },
                "desc": { "type": "string" },
                "changed": { "type": "boolean" },
                "open": { "type": "boolean" }
            },
            "required": ["depth", "name", "kind"]
        },
        "CallbackRow": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "pattern": { "type": "string" },
                "blocking": { "type": "boolean" },
                "timeout": { "type": "string" },
                "scope": { "type": "string" },
                "cwd": { "type": "string" }
            },
            "required": ["id", "name", "pattern"]
        },
        "ToolRow": {
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "status": { "type": "string", "enum": ["on", "off"] },
                "desc": { "type": "string" }
            },
            "required": ["name", "status"]
        },
        "ToolGroup": {
            "type": "object",
            "properties": {
                "category": { "type": "string" },
                "tools": arr(r("ToolRow"))
            },
            "required": ["category", "tools"]
        },
        "RadarAnchor": {
            "type": "object",
            "properties": {
                "time": { "type": "string" },
                "signal": { "type": "string" }
            },
            "required": ["time", "signal"]
        },
        "RadarResult": {
            "type": "object",
            "properties": {
                "content": { "type": "string" },
                "datetime": { "type": "string" },
                "importance": { "type": "string", "enum": ["low", "medium", "high", "critical"] },
                "score": { "type": "number" }
            },
            "required": ["content", "datetime", "importance", "score"]
        },
        "RadarData": {
            "type": "object",
            "properties": {
                "anchors": arr(r("RadarAnchor")),
                "results": arr(r("RadarResult"))
            },
            "required": ["anchors", "results"]
        },
        "EntityTable": {
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "rows": { "type": "integer" },
                "columns": { "type": "string" },
                "samples": arr(json!({ "type": "string" }))
            },
            "required": ["name", "rows", "columns", "samples"]
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
