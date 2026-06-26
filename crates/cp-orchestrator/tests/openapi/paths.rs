//! OpenAPI path definitions — all REST endpoints.

use serde_json::{json, Value};

use super::{arr, del, err, get, merge, ok, post, r, with_agent};

/// Required query parameter shorthand.
fn qp(name: &str) -> Value {
    json!({ "name": name, "in": "query", "required": true, "schema": { "type": "string" } })
}

/// Optional query parameter shorthand.
fn qp_opt(name: &str) -> Value {
    json!({ "name": name, "in": "query", "schema": { "type": "string" } })
}

/// All API route definitions.
pub(super) fn paths() -> Value {
    json!({
        "/api/health": get("health", "Health check", json!({
            "type": "object", "properties": { "status": { "type": "string" } }
        })),
        // ── Fleet ───────────────────────────────────────────────────
        "/api/fleet": get("fleet", "Raw fleet view (rev-envelope)", json!({ "type": "object" })),
        "/api/fleet/meta": get("fleet", "List all agents (enriched)", arr(r("Agent"))),
        "/api/fleet/retired": get("fleet", "List retired agents", arr(r("Agent"))),
        "/api/fleet/create": post("fleet", "Create a new agent", Some(json!({
            "type": "object",
            "properties": { "name": { "type": "string" }, "folder": { "type": "string" }, "model": { "type": "string" } },
            "required": ["name"]
        })), r("CreateAgentReceipt")),
        "/api/metrics": get("fleet", "Fleet-wide metrics", arr(r("AgentMetrics"))),
        // ── Env-keys ────────────────────────────────────────────────
        "/api/env-keys": get("env", "List environment keys", arr(r("EnvKeyStatus"))),
        "/api/env-keys/{name}": merge(
            json!({ "get": {
                "tags": ["env"], "summary": "Reveal environment key value",
                "parameters": [{ "name": "name", "in": "path", "required": true, "schema": { "type": "string" } }],
                "responses": merge(ok(r("EnvKeyReveal")), err())
            }}),
            json!({ "put": {
                "tags": ["env"], "summary": "Update environment key",
                "parameters": [{ "name": "name", "in": "path", "required": true, "schema": { "type": "string" } }],
                "requestBody": { "required": true, "content": { "application/json": { "schema": {
                    "type": "object",
                    "properties": { "value": { "type": "string" } },
                    "required": ["value"]
                } } } },
                "responses": merge(ok(r("EnvKeyUpdateResult")), err())
            }})
        ),
        // ── Ticket ──────────────────────────────────────────────────
        "/api/ticket": post("ticket", "Mint SSE upgrade ticket", None, r("TicketResponse")),
        // ── Auth ────────────────────────────────────────────────────
        "/api/auth/status": get("auth", "Auth status", r("AuthStatus")),
        "/api/auth/login": post("auth", "Login", Some(json!({
            "type": "object",
            "properties": { "email": { "type": "string" }, "password": { "type": "string" } },
            "required": ["email", "password"]
        })), r("AuthLogin")),
        "/api/auth/register": post("auth", "Register", Some(json!({
            "type": "object",
            "properties": { "email": { "type": "string" }, "name": { "type": "string" }, "password": { "type": "string" } },
            "required": ["email", "name", "password"]
        })), r("RegisterResponse")),
        "/api/auth/logout": post("auth", "Logout", None, r("OkResponse")),
        "/api/auth/me": get("auth", "Current user", r("AuthUser")),
        "/api/auth/users": merge(
            get("auth", "List users (admin)", arr(r("AuthUser"))),
            post("auth", "Create user (admin)", Some(json!({
                "type": "object",
                "properties": { "email": { "type": "string" }, "name": { "type": "string" }, "password": { "type": "string" }, "role": { "type": "string" } },
                "required": ["email", "name", "password"]
            })), r("CreateUserResponse"))
        ),
        "/api/auth/users/{userId}": json!({ "delete": {
            "tags": ["auth"], "summary": "Delete user (admin)",
            "parameters": [{ "name": "userId", "in": "path", "required": true, "schema": { "type": "string" } }],
            "responses": merge(ok(r("OkResponse")), err())
        }}),
        "/api/auth/users/{userId}/logout": json!({ "post": {
            "tags": ["auth"], "summary": "Force-logout user (admin)",
            "parameters": [{ "name": "userId", "in": "path", "required": true, "schema": { "type": "string" } }],
            "responses": merge(ok(r("ForceLogoutResponse")), err())
        }}),
        // ── Agent (single) ──────────────────────────────────────────
        "/api/agent/{id}": with_agent(get("agent", "Raw agent view (rev-envelope)", json!({ "type": "object" }))),
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
        "/api/agent/{id}/conversation": with_agent(get("agent", "Conversation feed", arr(r("ConversationMsg")))),
        // ── Filesystem ──────────────────────────────────────────────
        "/api/agent/{id}/fs": with_agent(json!({ "get": {
            "tags": ["fs"], "summary": "List directory",
            "parameters": [qp_opt("path")],
            "responses": merge(ok(arr(r("FinderNode"))), err())
        }})),
        "/api/agent/{id}/fs/descriptions": with_agent(get("fs", "File/folder descriptions", r("FsDescriptions"))),
        "/api/agent/{id}/fs/preview": with_agent(json!({ "get": {
            "tags": ["fs"], "summary": "Text preview",
            "parameters": [qp("path")],
            "responses": merge(ok(r("FsPreview")), err())
        }})),
        "/api/agent/{id}/fs/sheet": with_agent(json!({ "get": {
            "tags": ["fs"], "summary": "Spreadsheet data",
            "parameters": [qp("path")],
            "responses": merge(ok(r("SheetData")), err())
        }})),
        "/api/agent/{id}/fs/download": with_agent(json!({ "get": {
            "tags": ["fs"], "summary": "Download file or folder",
            "parameters": [qp("path")],
            "responses": { "200": { "description": "Raw file bytes", "content": {
                "application/octet-stream": { "schema": { "type": "string", "format": "binary" } }
            }}}
        }})),
        "/api/agent/{id}/fs/raw": with_agent(json!({ "get": {
            "tags": ["fs"], "summary": "Inline raw file (image/PDF preview)",
            "parameters": [qp("path")],
            "responses": { "200": { "description": "Raw file bytes inline", "content": {
                "*/*": { "schema": { "type": "string", "format": "binary" } }
            }}}
        }})),
        "/api/agent/{id}/fs/write": with_agent(json!({ "post": {
            "tags": ["fs"], "summary": "Write file",
            "parameters": [qp("path")],
            "requestBody": { "required": true, "content": {
                "text/plain": { "schema": { "type": "string" } }
            }},
            "responses": merge(ok(r("WriteResult")), err())
        }})),
        "/api/agent/{id}/fs/mkdir": with_agent(json!({ "post": {
            "tags": ["fs"], "summary": "Create folder",
            "parameters": [qp("path"), qp("name")],
            "responses": merge(ok(r("MkdirResult")), err())
        }})),
        "/api/agent/{id}/fs/rename": with_agent(json!({ "post": {
            "tags": ["fs"], "summary": "Rename item",
            "parameters": [qp("path"), qp("name")],
            "responses": merge(ok(r("RenameResult")), err())
        }})),
        "/api/agent/{id}/fs/move": with_agent(post("fs", "Move items", Some(json!({
            "type": "object",
            "properties": { "items": arr(json!({ "type": "string" })), "dest": { "type": "string" } },
            "required": ["items", "dest"]
        })), r("MoveResult"))),
        "/api/agent/{id}/fs/trash": with_agent(post("fs", "Trash items", Some(json!({
            "type": "object",
            "properties": { "items": arr(json!({ "type": "string" })) },
            "required": ["items"]
        })), r("TrashResult"))),
        "/api/agent/{id}/fs/upload": with_agent(json!({ "post": {
            "tags": ["fs"], "summary": "Upload file",
            "parameters": [qp("path"), qp("name")],
            "requestBody": { "required": true, "content": {
                "application/octet-stream": { "schema": { "type": "string", "format": "binary" } }
            }},
            "responses": merge(ok(r("UploadResult")), err())
        }})),
        "/api/agent/{id}/fs/upload-unique": with_agent(json!({ "post": {
            "tags": ["fs"], "summary": "Upload file (unique name)",
            "parameters": [qp("path"), qp("name")],
            "requestBody": { "required": true, "content": {
                "application/octet-stream": { "schema": { "type": "string", "format": "binary" } }
            }},
            "responses": merge(ok(r("UploadUniqueResult")), err())
        }})),
        // ── Agent lifecycle + misc ──────────────────────────────────
        "/api/agent/{id}/body/{hash}": json!({ "get": {
            "tags": ["agent"], "summary": "Hydrate content body",
            "parameters": [
                { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                { "name": "hash", "in": "path", "required": true, "schema": { "type": "string" } }
            ],
            "responses": merge(ok(r("BodyPayload")), err())
        }}),
        "/api/agent/{id}/command": with_agent(post("agent", "Send command to agent",
            Some(json!({ "type": "object" })), r("CommandReceipt"))),
        "/api/agent/{id}/restart": with_agent(post("agent", "Restart agent", None, r("RestartReceipt"))),
        "/api/agent/{id}/retire": with_agent(post("agent", "Retire agent", None, r("RetireReceipt"))),
        "/api/agent/{id}/unretire": with_agent(post("agent", "Unretire agent", None, r("UnretireReceipt"))),
        "/api/agent/{id}/rename": with_agent(post("agent", "Rename agent", Some(json!({
            "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"]
        })), r("OkResponse"))),
        "/api/agent/{id}/avatar": merge(
            with_agent(json!({ "get": {
                "tags": ["agent"], "summary": "Get agent avatar image",
                "responses": { "200": { "description": "Avatar image bytes", "content": {
                    "image/*": { "schema": { "type": "string", "format": "binary" } }
                }}}
            }})),
            merge(
                with_agent(post("agent", "Upload avatar",
                    Some(json!({ "type": "string", "format": "binary" })), r("OkResponse"))),
                with_agent(del("agent", "Delete avatar"))
            )
        ),
        "/api/agent/{id}/library/command": with_agent(post("agent", "Create command", Some(json!({
            "type": "object",
            "properties": { "name": { "type": "string" }, "description": { "type": "string" }, "body": { "type": "string" } },
            "required": ["name", "body"]
        })), r("CreateCommandReceipt"))),
        // ── ACL ─────────────────────────────────────────────────────
        "/api/agent/{id}/acl": merge(
            with_agent(get("auth", "List agent ACL", arr(r("AclEntry")))),
            with_agent(post("auth", "Grant agent access", Some(json!({
                "type": "object",
                "properties": { "user_id": { "type": "string" }, "role": { "type": "string" } },
                "required": ["user_id"]
            })), r("AclEntry")))
        ),
        "/api/agent/{id}/acl/{userId}": merge(
            json!({ "patch": {
                "tags": ["auth"], "summary": "Update agent role",
                "parameters": [
                    { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                    { "name": "userId", "in": "path", "required": true, "schema": { "type": "string" } }
                ],
                "requestBody": { "required": true, "content": { "application/json": { "schema": {
                    "type": "object", "properties": { "role": { "type": "string" } }, "required": ["role"]
                }}}},
                "responses": merge(ok(r("AclEntry")), err())
            }}),
            json!({ "delete": {
                "tags": ["auth"], "summary": "Revoke agent access",
                "parameters": [
                    { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                    { "name": "userId", "in": "path", "required": true, "schema": { "type": "string" } }
                ],
                "responses": merge(ok(r("OkResponse")), err())
            }})
        )
    })
}
