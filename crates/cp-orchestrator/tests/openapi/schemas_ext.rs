//! Transport-layer OpenAPI schemas — receipts, auth, env-keys, filesystem.

use serde_json::{Value, json};

use super::{arr, r};

/// Receipt, auth, env-key, filesystem, and conversation schemas.
pub(super) fn transport() -> Value {
    json!({
        // ── LLM provider registry ────────────────────────────────────
        "ModelDef": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "apiName": { "type": "string" },
                "displayName": { "type": "string" },
                "contextWindow": { "type": "integer" },
                "maxOutput": { "type": "integer" },
                "inputPrice": { "type": "number" },
                "outputPrice": { "type": "number" },
                "badge": { "type": "string", "nullable": true },
                "isDefault": { "type": "boolean" }
            },
            "required": ["id", "apiName", "displayName", "contextWindow", "maxOutput", "inputPrice", "outputPrice"]
        },
        "ProviderDef": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "description": { "type": "string" },
                "available": { "type": "boolean" },
                "models": arr(r("ModelDef"))
            },
            "required": ["id", "name", "description", "available", "models"]
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
        // ── Auth ────────────────────────────────────────────────────
        "AuthStatus": {
            "type": "object",
            "properties": {
                "enabled": { "type": "boolean" },
                "bootstrapped": { "type": "boolean" }
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
                "must_change_password": { "type": "boolean" },
                "created_at": { "type": "integer" },
                "updated_at": { "type": "integer" }
            },
            "required": ["id", "email", "name", "role", "must_change_password"]
        },
        "AuthLogin": {
            "type": "object",
            "properties": {
                "token": { "type": "string" },
                "user": r("AuthUser")
            },
            "required": ["token", "user"]
        },
        "RegisterResponse": {
            "type": "object",
            "properties": { "user": r("AuthUser") },
            "required": ["user"]
        },
        "CreateUserResponse": {
            "type": "object",
            "properties": { "user": r("AuthUser") },
            "required": ["user"]
        },
        "ForceLogoutResponse": {
            "type": "object",
            "properties": {
                "ok": { "type": "boolean" },
                "revoked_sessions": { "type": "integer" }
            },
            "required": ["ok"]
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
        // ── Env-keys ────────────────────────────────────────────────
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
                "exists": { "type": "boolean" },
                "value": { "type": "string" },
                "masked": { "type": "string" }
            },
            "required": ["env", "exists"]
        },
        "EnvKeyUpdateResult": {
            "type": "object",
            "properties": {
                "env": { "type": "string" },
                "value": { "type": "string" },
                "masked": { "type": "string" },
                "exists": { "type": "boolean" },
                "persisted": { "type": "boolean" }
            },
            "required": ["env", "exists"]
        },
        // ── Body ────────────────────────────────────────────────────
        "BodyPayload": {
            "type": "object",
            "properties": {
                "bytes": arr(json!({ "type": "integer" }))
            },
            "required": ["bytes"]
        },
        // ── Generic responses ───────────────────────────────────────
        "OkResponse": {
            "type": "object",
            "properties": { "ok": { "type": "boolean" } },
            "required": ["ok"]
        },
        "TicketResponse": {
            "type": "object",
            "properties": { "ticket": { "type": "string" } },
            "required": ["ticket"]
        },
        // ── Shared enums ────────────────────────────────────────
        "FinderKind": {
            "type": "string",
            "enum": [
                "folder", "code", "doc", "pdf", "sheet", "slides",
                "image", "markdown", "json", "archive", "audio",
                "video", "binary"
            ]
        },
        // ── Filesystem ──────────────────────────────────────────────
        "FinderNode": {
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "path": { "type": "string" },
                "kind": r("FinderKind"),
                "modified": { "type": "integer", "description": "Epoch ms" },
                "size": { "type": "integer", "nullable": true },
                "count": { "type": "integer", "nullable": true, "description": "Visible children (folders only)" }
            },
            "required": ["name", "path", "kind"]
        },
        "FsDescriptions": {
            "type": "object",
            "additionalProperties": { "type": "string" },
            "description": "Map of relative path to description"
        },
        "FsPreview": {
            "type": "object",
            "properties": {
                "content": { "type": "string" },
                "size": { "type": "integer" },
                "truncated": { "type": "boolean" }
            },
            "required": ["content", "size", "truncated"]
        },
        "SheetTab": {
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "rows": { "type": "array", "items": { "type": "array", "items": { "type": "string" } } }
            },
            "required": ["name", "rows"]
        },
        "SheetData": {
            "type": "object",
            "properties": {
                "sheets": arr(r("SheetTab")),
                "truncated": { "type": "boolean" }
            },
            "required": ["sheets", "truncated"]
        },
        "WriteResult": {
            "type": "object",
            "properties": { "written": { "type": "integer" }, "path": { "type": "string" } },
            "required": ["written", "path"]
        },
        "MkdirResult": {
            "type": "object",
            "properties": { "created": { "type": "string" } },
            "required": ["created"]
        },
        "RenameResult": {
            "type": "object",
            "properties": { "renamed": { "type": "string" } },
            "required": ["renamed"]
        },
        "MoveResult": {
            "type": "object",
            "properties": { "moved": { "type": "integer" }, "skipped": { "type": "integer" } },
            "required": ["moved", "skipped"]
        },
        "TrashResult": {
            "type": "object",
            "properties": { "trashed": { "type": "integer" }, "skipped": { "type": "integer" } },
            "required": ["trashed", "skipped"]
        },
        "UploadResult": {
            "type": "object",
            "properties": { "written": { "type": "integer" }, "path": { "type": "string" } },
            "required": ["written", "path"]
        },
        "UploadUniqueResult": {
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "name": { "type": "string" },
                "size": { "type": "integer" }
            },
            "required": ["path", "name", "size"]
        },
        "ConversationMsg": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "uid": { "type": "string" },
                "role": { "type": "string" },
                "content": { "type": "string" },
                "timestamp_ms": { "type": "integer" },
                "message_type": { "type": "string", "nullable": true },
                "tool_uses": { "type": "array", "nullable": true, "items": {
                    "type": "object", "additionalProperties": true
                }},
                "tool_results": { "type": "array", "nullable": true, "items": {
                    "type": "object", "additionalProperties": true
                }}
            },
            "required": ["id", "uid", "role", "content", "timestamp_ms"]
        },
        // ── SSE delta protocol (oplog push plane) ───────────────────
        "OpEntryKind": {
            "type": "object",
            "description": "Discriminated-union payload of a single oplog delta, keyed by `kind`.",
            "properties": {
                "kind": { "type": "string" },
                "thread_id": { "type": "string" },
                "name": { "type": "string" },
                "status": { "type": "string" },
                "timestamp_ms": { "type": "integer" },
                "phase": { "type": "string" },
                "cost_usd": { "type": "number" },
                "input_tokens": { "type": "integer" },
                "output_tokens": { "type": "integer" },
                "used_tokens": { "type": "integer" },
                "threshold_tokens": { "type": "integer" },
                "budget_tokens": { "type": "integer" },
                "hit_tokens": { "type": "integer" },
                "miss_tokens": { "type": "integer" },
                "message_id": { "type": "string" },
                "head": { "type": "string" },
                "inline_body": { "type": "string" },
                "message_ts": { "type": "integer" }
            },
            "required": ["kind"]
        },
        "OpEntry": {
            "type": "object",
            "description": "One rev-numbered oplog delta carried by an SSE `delta` event.",
            "properties": {
                "rev": { "type": "integer" },
                "timestamp_ms": { "type": "integer" },
                "kind": r("OpEntryKind")
            },
            "required": ["rev", "kind"]
        },
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
            "required": ["onboarding_completed", "is_admin", "auth_enabled", "providers", "allowed_models"]
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
        }
    })
}
