#![recursion_limit = "512"]
//! OpenAPI 3.0.3 spec generator for the orchestrator REST API.
//!
//! Run with: `cargo test -p cp-orchestrator generate_openapi -- --ignored`
//! Writes `openapi.json` at the workspace root.

use argon2 as _;
use calamine as _;
use cp_mod_bridge as _;
use cp_oplog as _;
use cp_orchestrator as _;
use cp_wire as _;
use csv as _;
use nix as _;
use notify as _;
use portable_pty as _;
use rusqlite as _;
use serde as _;
use serde_yaml as _;
use tempfile as _;
use tiny_http as _;
use utoipa as _;

use serde_json::{json, Value};

// ── helpers ──────────────────────────────────────────────────────────

fn r(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}
fn arr(item: Value) -> Value {
    json!({ "type": "array", "items": item })
}
fn ok(schema: Value) -> Value {
    json!({ "200": { "description": "OK", "content": { "application/json": { "schema": schema } } } })
}
fn err() -> Value {
    json!({ "default": { "description": "Error", "content": { "application/json": { "schema": r("Error") } } } })
}
fn get(tag: &str, summary: &str, resp: Value) -> Value {
    json!({ "get": { "tags": [tag], "summary": summary, "responses": merge(ok(resp), err()) } })
}
fn post(tag: &str, summary: &str, body: Option<Value>, resp: Value) -> Value {
    let mut op = json!({ "tags": [tag], "summary": summary, "responses": merge(ok(resp), err()) });
    if let Some(b) = body {
        op["requestBody"] = json!({ "required": true, "content": { "application/json": { "schema": b } } });
    }
    json!({ "post": op })
}
fn del(tag: &str, summary: &str) -> Value {
    json!({ "delete": { "tags": [tag], "summary": summary, "responses": merge(ok(json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } })), err()) } })
}
fn merge(a: Value, b: Value) -> Value {
    let mut m = a.as_object().cloned().unwrap_or_default();
    if let Some(bm) = b.as_object() {
        m.extend(bm.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    Value::Object(m)
}
fn agent_param() -> Value {
    json!([{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }])
}
fn with_agent(mut v: Value) -> Value {
    // Inject path parameters into every operation in the path item
    for key in ["get", "post", "put", "patch", "delete"] {
        if v.get(key).is_some() {
            v[key]["parameters"] = agent_param();
        }
    }
    v
}

// ── schemas ──────────────────────────────────────────────────────────

fn schemas() -> Value {
    json!({
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
                "hasAvatar": { "type": "boolean" }
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
                "ts": { "type": "string" },
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
                "kind": { "type": "string" },
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
        },
        "CommandReceipt": {
            "type": "object",
            "properties": {
                "cmd_id": { "type": "string" },
                "dedup_token": { "type": "string" },
                "rev": { "type": "integer", "nullable": true },
                "status": { "type": "string" }
            },
            "required": ["cmd_id", "dedup_token", "status"]
        },
        "CreateAgentReceipt": {
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "folder": { "type": "string" },
                "pid": { "type": "integer" }
            },
            "required": ["status", "folder", "pid"]
        },
        "RestartReceipt": {
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "folder": { "type": "string" },
                "pid": { "type": "integer" }
            },
            "required": ["status", "folder", "pid"]
        },
        "RetireReceipt": {
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "id": { "type": "string" },
                "folder": { "type": "string" }
            },
            "required": ["status", "id", "folder"]
        },
        "UnretireReceipt": {
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "id": { "type": "string" },
                "folder": { "type": "string" },
                "pid": { "type": "integer" }
            },
            "required": ["status", "id", "folder", "pid"]
        },
        "CreateCommandReceipt": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "status": { "type": "string" }
            },
            "required": ["id", "status"]
        },
        "AuthStatus": {
            "type": "object",
            "properties": {
                "enabled": { "type": "boolean" },
                "hasUsers": { "type": "boolean" }
            },
            "required": ["enabled"]
        },
        "AuthUser": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "email": { "type": "string" },
                "name": { "type": "string" },
                "role": { "type": "string", "enum": ["admin", "user"] },
                "created_at": { "type": "integer" },
                "updated_at": { "type": "integer" }
            },
            "required": ["id", "email", "name", "role"]
        },
        "AuthLogin": {
            "type": "object",
            "properties": {
                "token": { "type": "string" },
                "user": r("AuthUser")
            },
            "required": ["token", "user"]
        },
        "AclEntry": {
            "type": "object",
            "properties": {
                "agent_id": { "type": "string" },
                "user_id": { "type": "string" },
                "role": { "type": "string", "enum": ["agent-admin", "agent-user"] },
                "granted_at": { "type": "integer" },
                "granted_by": { "type": "string", "nullable": true },
                "user_email": { "type": "string" },
                "user_name": { "type": "string" }
            },
            "required": ["agent_id", "user_id", "role", "user_email", "user_name"]
        },
        "EnvKeyStatus": {
            "type": "object",
            "properties": {
                "env": { "type": "string" },
                "label": { "type": "string" },
                "exists": { "type": "boolean" }
            },
            "required": ["env", "label", "exists"]
        },
        "EnvKeyReveal": {
            "type": "object",
            "properties": {
                "env": { "type": "string" },
                "label": { "type": "string" },
                "exists": { "type": "boolean" },
                "value": { "type": "string" },
                "masked": { "type": "string" }
            },
            "required": ["env", "label", "exists"]
        },
        "BodyPayload": {
            "type": "object",
            "properties": {
                "bytes": arr(json!({ "type": "integer" }))
            },
            "required": ["bytes"]
        },
        "OkResponse": {
            "type": "object",
            "properties": { "ok": { "type": "boolean" } },
            "required": ["ok"]
        },
        "TicketResponse": {
            "type": "object",
            "properties": { "ticket": { "type": "string" } },
            "required": ["ticket"]
        }
    })
}

// ── paths ────────────────────────────────────────────────────────────

fn paths() -> Value {
    json!({
        "/api/health": get("health", "Health check", json!({ "type": "object", "properties": { "status": { "type": "string" } } })),
        "/api/fleet/meta": get("fleet", "List all agents (enriched)", arr(r("Agent"))),
        "/api/fleet/retired": get("fleet", "List retired agents", arr(r("Agent"))),
        "/api/fleet/create": post("fleet", "Create a new agent", Some(json!({
            "type": "object", "properties": { "name": { "type": "string" }, "folder": { "type": "string" }, "model": { "type": "string" } }, "required": ["name"]
        })), r("CreateAgentReceipt")),
        "/api/metrics": get("fleet", "Fleet-wide metrics", arr(r("AgentMetrics"))),
        "/api/env-keys": get("env", "List environment keys", arr(r("EnvKeyStatus"))),
        "/api/env-keys/{name}": merge(
            get("env", "Reveal environment key value", r("EnvKeyReveal")),
            json!({ "put": { "tags": ["env"], "summary": "Update environment key", "parameters": [{ "name": "name", "in": "path", "required": true, "schema": { "type": "string" } }], "requestBody": { "required": true, "content": { "text/plain": { "schema": { "type": "string" } } } }, "responses": merge(ok(r("OkResponse")), err()) } })
        ),
        "/api/ticket": post("ticket", "Mint SSE upgrade ticket", None, r("TicketResponse")),
        "/api/auth/status": get("auth", "Auth status", r("AuthStatus")),
        "/api/auth/login": post("auth", "Login", Some(json!({ "type": "object", "properties": { "email": { "type": "string" }, "password": { "type": "string" } }, "required": ["email", "password"] })), r("AuthLogin")),
        "/api/auth/register": post("auth", "Register", Some(json!({ "type": "object", "properties": { "email": { "type": "string" }, "name": { "type": "string" }, "password": { "type": "string" } }, "required": ["email", "name", "password"] })), r("AuthLogin")),
        "/api/auth/logout": post("auth", "Logout", None, r("OkResponse")),
        "/api/auth/me": get("auth", "Current user", r("AuthUser")),
        "/api/auth/users": merge(
            get("auth", "List users (admin)", arr(r("AuthUser"))),
            post("auth", "Create user (admin)", Some(json!({ "type": "object", "properties": { "email": { "type": "string" }, "name": { "type": "string" }, "password": { "type": "string" }, "role": { "type": "string" } }, "required": ["email", "name", "password"] })), r("AuthUser"))
        ),
        "/api/auth/users/{userId}": del("auth", "Delete user (admin)"),
        "/api/auth/users/{userId}/logout": post("auth", "Force-logout user (admin)", None, r("OkResponse")),
        "/api/agent/{id}/meta": with_agent(get("agent", "Agent enriched info", r("Agent"))),
        "/api/agent/{id}/metrics": with_agent(get("agent", "Agent metrics", r("AgentMetrics"))),
        "/api/agent/{id}/vitals": with_agent(get("agent", "Agent service vitals", arr(r("Vital")))),
        "/api/agent/{id}/threads": with_agent(get("agent", "Agent threads + conversation", r("ThreadsResponse"))),
        "/api/agent/{id}/panels": with_agent(get("agent", "Context panels", arr(r("ContextPanel")))),
        "/api/agent/{id}/memory": with_agent(get("agent", "Memory cards", arr(r("MemoryCard")))),
        "/api/agent/{id}/todos": with_agent(get("agent", "Todo items", arr(r("TodoItem")))),
        "/api/agent/{id}/spine": with_agent(get("agent", "Spine notifications", arr(r("SpineNotif")))),
        "/api/agent/{id}/queue": with_agent(get("agent", "Queued actions", arr(r("QueueAction")))),
        "/api/agent/{id}/scratchpad": with_agent(get("agent", "Scratchpad cells", arr(r("ScratchCell")))),
        "/api/agent/{id}/tree": with_agent(get("agent", "Directory tree", arr(r("TreeRow")))),
        "/api/agent/{id}/callbacks": with_agent(get("agent", "Callbacks", arr(r("CallbackRow")))),
        "/api/agent/{id}/tools": with_agent(get("agent", "Tool groups", arr(r("ToolGroup")))),
        "/api/agent/{id}/radar": with_agent(get("agent", "Context radar", r("RadarData"))),
        "/api/agent/{id}/entities": with_agent(get("agent", "Entity tables", arr(r("EntityTable")))),
        "/api/agent/{id}/usage": with_agent(get("agent", "Usage analytics", json!({ "type": "object" }))),
        "/api/agent/{id}/library": with_agent(get("agent", "Prompt library", arr(r("LibraryItem")))),
        "/api/agent/{id}/body/{hash}": json!({ "get": {
            "tags": ["agent"], "summary": "Hydrate content body",
            "parameters": [
                { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                { "name": "hash", "in": "path", "required": true, "schema": { "type": "string" } }
            ],
            "responses": merge(ok(r("BodyPayload")), err())
        }}),
        "/api/agent/{id}/command": with_agent(post("agent", "Send command to agent", Some(json!({ "type": "object" })), r("CommandReceipt"))),
        "/api/agent/{id}/restart": with_agent(post("agent", "Restart agent", None, r("RestartReceipt"))),
        "/api/agent/{id}/retire": with_agent(post("agent", "Retire agent", None, r("RetireReceipt"))),
        "/api/agent/{id}/unretire": with_agent(post("agent", "Unretire agent", None, r("UnretireReceipt"))),
        "/api/agent/{id}/rename": with_agent(post("agent", "Rename agent", Some(json!({ "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] })), r("OkResponse"))),
        "/api/agent/{id}/avatar": merge(
            with_agent(post("agent", "Upload avatar", Some(json!({ "type": "string", "format": "binary" })), r("OkResponse"))),
            with_agent(del("agent", "Delete avatar"))
        ),
        "/api/agent/{id}/library/command": with_agent(post("agent", "Create command", Some(json!({
            "type": "object", "properties": { "name": { "type": "string" }, "description": { "type": "string" }, "body": { "type": "string" } }, "required": ["name", "body"]
        })), r("CreateCommandReceipt"))),
        "/api/agent/{id}/acl": merge(
            with_agent(get("auth", "List agent ACL", arr(r("AclEntry")))),
            with_agent(post("auth", "Grant agent access", Some(json!({ "type": "object", "properties": { "user_id": { "type": "string" }, "role": { "type": "string" } }, "required": ["user_id"] })), r("AclEntry")))
        ),
        "/api/agent/{id}/acl/{userId}": merge(
            json!({ "patch": { "tags": ["auth"], "summary": "Update agent role", "parameters": [
                { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                { "name": "userId", "in": "path", "required": true, "schema": { "type": "string" } }
            ], "requestBody": { "required": true, "content": { "application/json": { "schema": { "type": "object", "properties": { "role": { "type": "string" } }, "required": ["role"] } } } }, "responses": merge(ok(r("AclEntry")), err()) } }),
            json!({ "delete": { "tags": ["auth"], "summary": "Revoke agent access", "parameters": [
                { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                { "name": "userId", "in": "path", "required": true, "schema": { "type": "string" } }
            ], "responses": merge(ok(r("OkResponse")), err()) } })
        )
    })
}

// ── spec assembly ────────────────────────────────────────────────────

fn build_spec() -> Value {
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "Context Pilot Orchestrator API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "REST API for the Context Pilot orchestration backend"
        },
        "servers": [{ "url": "http://localhost:7878" }],
        "components": {
            "schemas": schemas(),
            "securitySchemes": {
                "bearer": { "type": "http", "scheme": "bearer" }
            }
        },
        "paths": paths()
    })
}

// ── test entry point ─────────────────────────────────────────────────

#[test]
#[ignore]
fn generate_openapi() {
    let spec = build_spec();
    let json = serde_json::to_string_pretty(&spec).expect("serialize openapi spec");
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("openapi.json");
    std::fs::write(&out, &json).expect("write openapi.json");
    eprintln!("Wrote {} bytes to {}", json.len(), out.display());
}
