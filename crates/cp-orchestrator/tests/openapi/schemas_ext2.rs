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
        // ── Auto-update (O5.1, update-policy §5.9) ──────────────
        "UpdateWindow": {
            "type": "object",
            "properties": { "start": { "type": "string" }, "end": { "type": "string" } },
            "required": ["start", "end"]
        },
        "UpdateLastResult": {
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["success", "rolled_back", "failed"] },
                "from": { "type": "string", "nullable": true },
                "to": { "type": "string", "nullable": true },
                "attempted": { "type": "string", "nullable": true },
                "message": { "type": "string", "nullable": true },
                "at_ms": { "type": "integer" }
            },
            "required": ["kind", "at_ms"]
        },
        "UpdateStatus": {
            "type": "object",
            "properties": {
                "current": { "type": "string" },
                "active_tag": { "type": "string", "nullable": true },
                "channel": { "type": "string" },
                "arch": { "type": "string" },
                "mode": { "type": "string", "enum": ["auto", "manual", "paused"] },
                "window": r("UpdateWindow"),
                "poll_interval_hours": { "type": "integer" },
                "available": { "type": "string", "nullable": true },
                "notes_url": { "type": "string", "nullable": true },
                "last_check_ms": { "type": "integer", "nullable": true },
                "last_result": { "allOf": [r("UpdateLastResult")], "nullable": true },
                "apply_in_flight": { "type": "boolean" }
            },
            "required": ["current", "channel", "arch", "mode", "window", "poll_interval_hours", "apply_in_flight"]
        },
        "UpdateApplyResponse": {
            "type": "object",
            "properties": {
                "status": { "type": "string", "enum": ["applying", "up_to_date"] },
                "current": { "type": "string", "nullable": true },
                "from": { "type": "string", "nullable": true },
                "to": { "type": "string", "nullable": true }
            },
            "required": ["status"]
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
        },
        // ── Conversation search (T671) ──────────────────────────────
        "ConvHit": {
            "type": "object",
            "properties": {
                "thread_id": { "type": "string" },
                "thread_name": { "type": "string" },
                "author": { "type": "string" },
                "text": { "type": "string" },
                "ts_ms": { "type": "integer", "format": "int64" },
                "score": { "type": "number", "format": "double" }
            },
            "required": ["thread_id", "thread_name", "author", "text", "ts_ms", "score"]
        },
        "ConversationSearchResult": {
            "type": "object",
            "properties": {
                "hits": arr(r("ConvHit"))
            },
            "required": ["hits"]
        },
        // ── Internet uplink + Wi-Fi AP (docs/design-network-uplink.md) ──
        // Secrets are write-only: the bearer password, the SIM PIN and the Wi-Fi
        // PSK are POSTed but never returned, so every read schema carries a
        // `*_set` boolean in their place (FR-NET-13).
        "ItNetworkWwan": {
            "type": "object",
            "description": "5G bearer settings — VENDOR-MANAGED. Writing them needs \
                `can_manage_secrets` (superadmin), not `can_manage_it`: the SIM and the data plan \
                are the vendor's, so the APN that goes with them is a fleet-wide decision. \
                `password_set`/`pin_set` stand in for the write-only credentials.",
            "properties": {
                "apn": { "type": "string" },
                "username": { "type": "string", "nullable": true },
                "password_set": { "type": "boolean" },
                "pin_set": { "type": "boolean" },
                "roaming": { "type": "boolean" },
                "standby": { "type": "string", "enum": ["hot", "cold"] }
            },
            "required": ["apn", "username", "password_set", "pin_set", "roaming", "standby"]
        },
        "ItNetworkAp": {
            "type": "object",
            "description": "Wi-Fi access-point settings. `country` is a functional prerequisite, \
                not a nicety: under the world-default regulatory domain every 5 GHz channel is \
                `no IR` and the AP cannot start (FR-NET-14).",
            "properties": {
                "enabled": { "type": "boolean" },
                "ssid": { "type": "string" },
                "passphrase_set": { "type": "boolean" },
                "band": { "type": "string", "enum": ["bg", "a"] },
                "channel": { "type": "integer" },
                "country": { "type": "string" },
                "hidden": { "type": "boolean" },
                "share_internet": { "type": "boolean" }
            },
            "required": ["enabled", "ssid", "passphrase_set", "band", "channel", "country", "hidden", "share_internet"]
        },
        "ItNetworkProbe": {
            "type": "object",
            "description": "Failover probe tuning. The probe is interface-bound, so it tests the \
                WAN path itself rather than whatever the default route currently prefers.",
            "properties": {
                "targets": arr(json!({ "type": "string" })),
                "fail_threshold": { "type": "integer" },
                "ok_threshold": { "type": "integer" },
                "interval_s": { "type": "integer" }
            },
            "required": ["targets", "fail_threshold", "ok_threshold", "interval_s"]
        },
        "ItNetworkConfig": {
            "type": "object",
            "properties": {
                "mode": { "type": "string", "enum": ["wan", "wan_5g", "5g"] },
                // Null for a caller without `can_manage_secrets`: the bearer's
                // CONFIGURATION is vendor state. `status.wwan` is not elided —
                // registration, operator and signal are diagnostics the client's
                // own admin needs when the box loses its uplink.
                "wwan": { "allOf": [r("ItNetworkWwan")], "nullable": true },
                "ap": r("ItNetworkAp"),
                "probe": r("ItNetworkProbe")
            },
            "required": ["mode", "wwan", "ap", "probe"]
        },
        "ItNetworkWanStatus": {
            "type": "object",
            "properties": {
                "carrier": { "type": "boolean" },
                "ip": { "type": "string", "nullable": true },
                "gateway": { "type": "string", "nullable": true },
                "has_default_route": { "type": "boolean" }
            },
            "required": ["carrier", "ip", "gateway", "has_default_route"]
        },
        "ItNetworkWwanStatus": {
            "type": "object",
            "properties": {
                "state": { "type": "string", "nullable": true },
                "operator": { "type": "string", "nullable": true },
                "tech": { "type": "string", "nullable": true },
                "signal_dbm": { "type": "integer", "nullable": true },
                "ip": { "type": "string", "nullable": true },
                "registered": { "type": "boolean" }
            },
            "required": ["state", "operator", "tech", "signal_dbm", "ip", "registered"]
        },
        "ItNetworkApStatus": {
            "type": "object",
            "properties": {
                "running": { "type": "boolean" },
                "ssid": { "type": "string", "nullable": true },
                "clients": { "type": "integer" },
                "channel": { "type": "integer", "nullable": true },
                "country": { "type": "string", "nullable": true }
            },
            "required": ["running", "ssid", "clients", "channel", "country"]
        },
        "ItNetworkStatus": {
            "type": "object",
            "description": "Live read-back from `ip route`, `nmcli`, `mmcli` and `iw`. Every \
                field degrades to null rather than erroring when a tool is absent, so a dev \
                machine with no env gates set still answers 200.",
            "properties": {
                "active_uplink": { "type": "string", "enum": ["wan", "wwan", "none"], "nullable": true },
                "wan": { "allOf": [r("ItNetworkWanStatus")], "nullable": true },
                "wwan": { "allOf": [r("ItNetworkWwanStatus")], "nullable": true },
                "ap": { "allOf": [r("ItNetworkApStatus")], "nullable": true }
            },
            "required": ["active_uplink", "wan", "wwan", "ap"]
        },
        "ItNetworkResponse": {
            "type": "object",
            "properties": {
                "config": r("ItNetworkConfig"),
                "status": r("ItNetworkStatus")
            },
            "required": ["config", "status"]
        },
        "ItNetworkModeResult": {
            "type": "object",
            "properties": {
                "mode": { "type": "string", "enum": ["wan", "wan_5g", "5g"] },
                "applied": { "type": "boolean" }
            },
            "required": ["mode", "applied"]
        },
        "ItNetworkApResult": {
            "type": "object",
            "properties": {
                "ap": r("ItNetworkAp"),
                "applied": { "type": "boolean" }
            },
            "required": ["ap", "applied"]
        },
        "ItNetworkWwanResult": {
            "type": "object",
            "properties": {
                "wwan": r("ItNetworkWwan"),
                "applied": { "type": "boolean" }
            },
            "required": ["wwan", "applied"]
        }
    })
}
