//! OpenAPI path definitions — all REST endpoints.

use serde_json::{Value, json};

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
        "/api/providers": json!({ "get": {
            "tags": ["providers"],
            "summary": "LLM provider + model registry (usable providers only; ?allowed=1 applies the org model allowlist)",
            "parameters": [qp_opt("allowed")],
            "responses": merge(ok(arr(r("ProviderDef"))), err())
        }}),
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
        // ── Vault snapshot (BridgeVault cache warm-up) ──────────────
        "/api/vault/snapshot": get("env", "Bulk-fetch all set key values (BridgeVault cache)", json!({
            "type": "object", "additionalProperties": { "type": "string" }
        })),
        // ── Settings ────────────────────────────────────────────────
        "/api/settings": merge(
            get("settings", "Central defaults, onboarding state & configured providers", r("AppSettings")),
            post("settings", "Update new-agent defaults / onboarding flag (admin)", Some(json!({
                "type": "object",
                "properties": {
                    "default_provider": { "type": "string" },
                    "default_model": { "type": "string" },
                    "onboarding_completed": { "type": "boolean" },
                    "allowed_models": arr(json!({ "type": "string" })),
                    // Access-control master flag toggle (design §13.10).
                    "access_control": { "type": "boolean" }
                }
            })), r("AppSettings"))
        ),
        // ── IT infra (design §13.5, can_manage_it) ─────────────────
        "/api/it/ca.crt": json!({ "get": {
            "tags": ["it"], "summary": "Download the private-CA root certificate (PEM)",
            "responses": { "200": { "description": "CA root PEM bytes", "content": {
                "application/octet-stream": { "schema": { "type": "string", "format": "binary" } }
            }}}
        }}),
        "/api/it/ca/fingerprint": get("it", "CA root SHA-256 fingerprint", r("ItFingerprint")),
        "/api/it/identity": merge(
            get("it", "Current box network identity (name/IP), or null", r("ItIdentityResponse")),
            post("it", "Set box network identity — re-issues the leaf & reloads Caddy", Some(json!({
                "type": "object",
                "properties": { "name": { "type": "string" }, "ip": { "type": "string" } },
                "required": ["name", "ip"]
            })), r("ItSetIdentityResponse"))
        ),
        "/api/it/provisioned": get("it", "Whether the box has been provisioned", json!({
            "type": "object", "properties": { "provisioned": { "type": "boolean" } }, "required": ["provisioned"]
        })),
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
        "/api/auth/me": merge(
            get("auth", "Current user + post-login next action", r("AuthMe")),
            json!({ "patch": {
                "tags": ["auth"], "summary": "Update current user profile",
                "requestBody": { "required": true, "content": { "application/json": { "schema": {
                    "type": "object",
                    "properties": { "name": { "type": "string" }, "email": { "type": "string" } },
                    "required": ["name", "email"]
                } } } },
                "responses": merge(ok(json!({
                    "type": "object",
                    "properties": { "user": r("AuthUser") },
                    "required": ["user"]
                })), err())
            }})
        ),
        "/api/auth/password": post("auth", "Change current user's password", Some(json!({
            "type": "object",
            "properties": { "current": { "type": "string" }, "new": { "type": "string" } },
            "required": ["current", "new"]
        })), r("OkResponse")),
        "/api/auth/sessions": get("auth", "List active device sessions", json!({
            "type": "object",
            "properties": { "sessions": arr(r("SessionInfo")) },
            "required": ["sessions"]
        })),
        "/api/auth/sessions/{id}": json!({ "delete": {
            "tags": ["auth"], "summary": "Revoke one of the caller's device sessions",
            "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }],
            "responses": merge(ok(r("OkResponse")), err())
        }}),
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
        ),
        // ── Releases (T427, admin-only) ─────────────────────────────
        "/api/releases": get("releases", "List releases (local + remote)", r("ReleasesResponse")),
        "/api/releases/arch": json!({ "put": {
            "tags": ["releases"], "summary": "Set architecture",
            "requestBody": { "required": true, "content": { "application/json": { "schema": {
                "type": "object",
                "properties": { "arch": { "type": "string" }, "auto": { "type": "boolean" } }
            }}}},
            "responses": merge(ok(r("ArchResponse")), err())
        }}),
        "/api/releases/download": post("releases", "Download a release", Some(json!({
            "type": "object",
            "properties": { "tag": { "type": "string" } },
            "required": ["tag"]
        })), r("DownloadResponse")),
        "/api/releases/deploy": post("releases", "Deploy release to fleet", Some(json!({
            "type": "object",
            "properties": { "tag": { "type": "string" } }
        })), r("DeployResponse")),
        "/api/releases/restart-orchestrator": post("releases", "Restart orchestrator process", None, r("RestartOrchestratorResponse")),
        "/api/releases/select": json!({ "put": {
            "tags": ["releases"], "summary": "Select active release",
            "requestBody": { "required": true, "content": { "application/json": { "schema": {
                "type": "object",
                "properties": { "tag": { "type": "string" } },
                "required": ["tag"]
            }}}},
            "responses": merge(ok(r("SelectResponse")), err())
        }}),
        "/api/releases/{tag}": json!({ "delete": {
            "tags": ["releases"], "summary": "Delete downloaded release",
            "parameters": [{ "name": "tag", "in": "path", "required": true, "schema": { "type": "string" } }],
            "responses": merge(ok(r("OkResponse")), err())
        }}),
        // ── Auto-update (O5.1, update-policy §5.9, can_manage_it) ───
        "/api/update/status": get("update", "Auto-update status (version, channel, mode, window, last result)", r("UpdateStatus")),
        "/api/update/check": post("update", "Force a channel poll now", None, r("UpdateStatus")),
        "/api/update/apply": post("update", "Verify, download and apply the channel version now (off-window)", None, r("UpdateApplyResponse")),
        "/api/update/mode": json!({ "put": {
            "tags": ["update"], "summary": "Set update mode and/or maintenance window",
            "requestBody": { "required": true, "content": { "application/json": { "schema": {
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["auto", "manual", "paused"] },
                    "window": r("UpdateWindow")
                }
            }}}},
            "responses": merge(ok(r("UpdateStatus")), err())
        }}),
        // ── Claude Code usage + login ─────────────────────────────────
        "/api/claude-usage": get("usage", "Claude Code OAuth usage limits", r("ClaudeUsageResponse")),
        "/api/claude-login/status": get("usage", "Claude Code OAuth token status", r("ClaudeTokenStatus")),
        "/api/claude-login/start": post("usage", "Start Claude Code OAuth login (PKCE)", None, r("ClaudeLoginStartResponse")),
        "/api/claude-login/complete": post("usage", "Complete Claude Code OAuth login", Some(r("ClaudeLoginCompleteRequest")), r("ClaudeLoginCompleteResponse")),
        "/api/claude-login/refresh": post("usage", "Refresh Claude Code OAuth token", None, r("ClaudeLoginCompleteResponse")),
        // ── Claude multi-account token vault ────────────────────────
        "/api/claude-accounts": get("usage", "List stored Claude accounts", r("ClaudeAccountsListResponse")),
        "/api/claude-accounts/store": post("usage", "Store current active token under its account email", None, r("OkResponse")),
        "/api/claude-accounts/switch": post("usage", "Switch to a stored account", Some(json!({
            "type": "object",
            "properties": { "email": { "type": "string" } },
            "required": ["email"]
        })), r("OkResponse")),
        "/api/claude-accounts/{email}": json!({ "delete": {
            "tags": ["usage"], "summary": "Delete a stored account",
            "parameters": [{ "name": "email", "in": "path", "required": true, "schema": { "type": "string" } }],
            "responses": merge(ok(r("OkResponse")), err())
        }})
    })
}
