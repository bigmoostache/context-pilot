//! The non-streaming REST route table and the raw-bytes GET dispatcher.
//!
//! Split out of [`transport`](super) so the acceptor/router shell in `mod.rs`
//! stays within the workspace's 500-line file budget. [`route_rest`] is the
//! flat path-segment router every JSON REST route flows through; [`try_raw_route`]
//! is the GET-only dispatcher for the handful of routes that own the `Request`
//! to write a non-JSON body (avatar image, fs download / raw preview, IT CA
//! cert). Both draw their shared inputs from [`RouteCtx`](super::RouteCtx),
//! which the parent [`handle`](super::handle) builds per request.

use tiny_http::{Method, Request};

use super::{RouteCtx, auth, files, inspect, it, respond_json, rest};

/// GET-only dispatcher for the raw-bytes routes (avatar image, fs download / raw
/// preview, IT CA cert) that own the `Request` to write a non-JSON body and so
/// cannot go through [`route_rest`]. Returns `None` when a route handled (and
/// consumed) the request, `Some(request)` when none matched so the caller
/// continues the pipeline.
///
/// Matching `*segments` (the `[&str]` slice place) rather than `segments`
/// (`&[&str]`) binds each element at its real `&str` type, avoiding the
/// match-ergonomics auto-deref that `clippy::pattern_type_mismatch` (forbid)
/// rejects.
pub(super) fn try_raw_route(request: Request, segments: &[&str], ctx: RouteCtx<'_>) -> Option<Request> {
    match *segments {
        ["api", "agent", id, "avatar"] => files::handle_avatar(request, ctx.state, id),
        ["api", "agent", id, "fs", "download"] => files::handle_download(request, ctx.state, id, ctx.query),
        ["api", "agent", id, "fs", "raw"] => files::handle_raw(request, ctx.state, id, ctx.query),
        // IT: private-CA root download (design §13.5, re-homed from the maint
        // plane). Owns the `Request` for its non-JSON content type. Gate on
        // `can_manage_it` here (a `None` caller is god-mode, FR-v3-08); then
        // reuse the maintenance handler verbatim.
        ["api", "it", "ca.crt"] => {
            if ctx.auth_user.is_some_and(|u| !u.can_manage_it()) {
                respond_json(request, &rest::HttpReply::error(403, "IT management access required"));
            } else {
                it::ca::serve_ca_cert(request);
            }
        }
        _ => return Some(request),
    }
    None
}

/// Dispatch a non-streaming REST route to its handler.
#[expect(
    clippy::too_many_lines,
    reason = "flat path-segment router: a closed match over ~60 borrowed `(&Method, &[&str])` route shapes, each arm a one-line delegation. It cannot be split under the 60-line cap without a wildcard catch-all per sub-dispatcher (forbidden wildcard_enum_match_arm). The flat variant→handler table is the honest shape — the transport twin of src/app/actions/mod.rs::apply_action."
)]
pub(super) fn route_rest(method: &Method, segments: &[&str], ctx: RouteCtx<'_>) -> rest::HttpReply {
    let RouteCtx { state, body_bytes, query, auth_token, auth_user } = ctx;
    // Explicit `&` reference patterns on both tuple components: `method` is a
    // `&Method` and `segments` a `&[&str]`, so `&Method::Get` / `&["api", …]`
    // match the borrowed scrutinees WITHOUT match-ergonomics auto-deref — the
    // form clippy::pattern_type_mismatch (forbid) mandates. tiny_http's Method
    // is not Copy, so an owned scrutinee is impossible; the explicit-ref match
    // is the honest shape.
    match (method, segments) {
        (&Method::Get, &["api", "health"]) => rest::HttpReply { status: 200, body: "{\"status\":\"ok\"}".to_owned() },
        (&Method::Get, &["api", "providers"]) => inspect::providers::providers(query),

        // ── Auth routes (§6 of design doc) ──────────────────────────
        (&Method::Get, &["api", "auth", "status"]) => auth::auth_status(state),
        (&Method::Post, &["api", "auth", "login"]) => auth::login(state, body_bytes),
        (&Method::Post, &["api", "auth", "register"]) => auth::register(state, body_bytes, auth_user),
        (&Method::Post, &["api", "auth", "logout"]) => auth::logout(state, auth_token),
        (&Method::Get, &["api", "auth", "me"]) => auth::me(state, auth_user),
        (&Method::Patch, &["api", "auth", "me"]) => auth::update_me(state, body_bytes, auth_user),
        (&Method::Post, &["api", "auth", "password"]) => auth::change_password(state, body_bytes, auth_user),
        (&Method::Get, &["api", "auth", "sessions"]) => auth::list_sessions(state, auth_token, auth_user),
        (&Method::Delete, &["api", "auth", "sessions", sid]) => auth::revoke_session(state, sid, auth_user),
        (&Method::Get, &["api", "settings"]) => rest::get_settings(state, auth_user),
        (&Method::Post, &["api", "settings"]) => rest::update_settings(state, body_bytes, auth_user),
        (&Method::Get, &["api", "auth", "users"]) => auth::list_users(state, auth_user),
        (&Method::Post, &["api", "auth", "users"]) => auth::create_user(state, body_bytes, auth_user),
        (&Method::Delete, &["api", "auth", "users", user_id]) => auth::delete_user(state, user_id, auth_user),
        (&Method::Post, &["api", "auth", "users", user_id, "logout"]) => {
            auth::force_logout_user(state, user_id, auth_user)
        }

        // ── ACL routes (Phase 6, §6 of design doc) ─────────────────
        (&Method::Get, &["api", "agent", id, "acl"]) => auth::acl_list(state, id, auth_user),
        (&Method::Post, &["api", "agent", id, "acl"]) => auth::acl_grant(state, id, body_bytes, auth_user),
        (&Method::Patch, &["api", "agent", id, "acl", user_id]) => {
            auth::acl_update_role(state, id, user_id, body_bytes, auth_user)
        }
        (&Method::Delete, &["api", "agent", id, "acl", user_id]) => auth::acl_revoke(state, id, user_id, auth_user),

        // ── Fleet + agent routes ────────────────────────────────────
        (&Method::Get, &["api", "fleet"]) => rest::fleet(state, auth_user),
        (&Method::Get, &["api", "fleet", "meta"]) => inspect::meta::fleet(state, auth_user),
        (&Method::Get, &["api", "fleet", "retired"]) => inspect::meta::fleet_retired(state, auth_user),
        (&Method::Get, &["api", "metrics"]) => inspect::metrics::fleet(state, auth_user),

        // ── Env-key inspection (T399) + editing (T404) ────────────
        (&Method::Get, &["api", "env-keys"]) => rest::env_keys_list(),
        (&Method::Get, &["api", "env-keys", name]) => rest::env_key_reveal(name, auth_user),
        (&Method::Put, &["api", "env-keys", name]) => {
            let body = String::from_utf8_lossy(body_bytes);
            rest::env_key_update(name, auth_user, &body)
        }

        // ── Vault snapshot (BridgeVault cache warm-up) ──────────────
        (&Method::Get, &["api", "vault", "snapshot"]) => rest::vault_snapshot(auth_user),

        // ── IT infra (design §13.5, re-homed from the maint plane; can_manage_it) ──
        (&Method::Get, &["api", "it", "ca", "fingerprint"]) => rest::it_ca_fingerprint(auth_user),
        (&Method::Get, &["api", "it", "identity"]) => rest::it_get_identity(state, auth_user),
        (&Method::Post, &["api", "it", "identity"]) => rest::it_set_identity(state, body_bytes, auth_user),
        (&Method::Get, &["api", "it", "provisioned"]) => rest::it_provisioned(state, auth_user),

        // ── Internet uplink + Wi-Fi AP (can_manage_it) ──────────────────────────
        (&Method::Get, &["api", "it", "network"]) => rest::it_get_network(state, auth_user),
        (&Method::Post, &["api", "it", "network", "mode"]) => rest::it_set_network_mode(state, body_bytes, auth_user),
        (&Method::Post, &["api", "it", "network", "ap"]) => rest::it_set_network_ap(state, body_bytes, auth_user),
        (&Method::Post, &["api", "it", "network", "wwan"]) => rest::it_set_network_wwan(state, body_bytes, auth_user),

        (&Method::Get, &["api", "agent", id]) => rest::agent(state, id),
        (&Method::Get, &["api", "agent", id, "meta"]) => inspect::meta::agent(state, id),
        (&Method::Get, &["api", "agent", id, "metrics"]) => inspect::metrics::agent(state, id),
        (&Method::Get, &["api", "agent", id, "vitals"]) => inspect::vitals::agent(state, id),
        (&Method::Get, &["api", "agent", id, "body", hash]) => rest::body(state, id, hash),
        (&Method::Get, &["api", "agent", id, "threads"]) => rest::threads(state, id),
        (&Method::Get, &["api", "agent", id, "usage"]) => inspect::panels::usage(state, id, query),
        (&Method::Get, &["api", "agent", id, "library"]) => inspect::panels::library(state, id),
        (&Method::Get, &["api", "agent", id, "identity"]) => inspect::panels::identity(state, id),
        (&Method::Get, &["api", "agent", id, "fs"]) => inspect::finder::fs_list(state, id, query),
        (&Method::Get, &["api", "agent", id, "fs", "preview"]) => inspect::finder::fs_preview(state, id, query),
        (&Method::Get, &["api", "agent", id, "fs", "sheet"]) => inspect::finder::fs_sheet(state, id, query),
        (&Method::Get, &["api", "agent", id, "fs", "descriptions"]) => inspect::finder::fs_descriptions(state, id),
        (&Method::Get, &["api", "agent", id, "conversation"]) => inspect::finder::conversation(state, id),
        (&Method::Post, &["api", "agent", id, "command"]) => rest::command(state, id, body_bytes),
        (&Method::Post, &["api", "agent", id, "conversations", "search"]) => {
            rest::search_conversations(state, id, body_bytes)
        }
        (&Method::Post, &["api", "agent", id, "library", "command"]) => rest::create_command(state, id, body_bytes),
        (&Method::Put, &["api", "agent", id, "library", "command", item]) => {
            rest::upsert_library_command(state, id, item, body_bytes)
        }
        (&Method::Get, &["api", "agent", id, "library", "agent", item]) => rest::read_library_agent(state, id, item),
        (&Method::Put, &["api", "agent", id, "library", "agent", item]) => {
            rest::upsert_library_agent(state, id, item, body_bytes)
        }
        (&Method::Delete, &["api", "agent", id, "library", "agent", item]) => {
            rest::delete_library_agent(state, id, item)
        }
        (&Method::Post, &["api", "agent", id, "fs", "upload"]) => {
            inspect::finder::fs_upload(state, id, query, body_bytes)
        }
        (&Method::Post, &["api", "agent", id, "fs", "upload-unique"]) => {
            inspect::finder::fs_upload_unique(state, id, query, body_bytes)
        }
        (&Method::Post, &["api", "agent", id, "fs", "write"]) => {
            inspect::finder::fs_write(state, id, query, body_bytes)
        }
        (&Method::Post, &["api", "agent", id, "fs", "mkdir"]) => inspect::finder::fs_mkdir(state, id, query),
        (&Method::Post, &["api", "agent", id, "fs", "rename"]) => inspect::finder::fs_rename(state, id, query),
        (&Method::Post, &["api", "agent", id, "fs", "move"]) => inspect::finder::fs_move(state, id, body_bytes),
        (&Method::Post, &["api", "agent", id, "fs", "trash"]) => inspect::finder::fs_trash(state, id, body_bytes),
        (&Method::Post, &["api", "agent", id, "restart"]) => rest::restart_agent(state, id),
        (&Method::Post, &["api", "agent", id, "retire"]) => rest::retire_agent(state, id),
        (&Method::Post, &["api", "agent", id, "unretire"]) => rest::unretire_agent(state, id),
        (&Method::Post, &["api", "agent", id, "rename"]) => rest::rename_agent(state, id, body_bytes),
        (&Method::Post, &["api", "agent", id, "avatar"]) => rest::upload_avatar(state, id, body_bytes),
        (&Method::Delete, &["api", "agent", id, "avatar"]) => rest::delete_avatar(state, id),
        (&Method::Post, &["api", "fleet", "create"]) => rest::create_agent(state, body_bytes, auth_user),
        (&Method::Post, &["api", "ticket"]) => rest::mint_ticket(state, auth_user),

        // ── Release + update management — IT surface (can_manage_it) ──
        // One guard for every `/api/releases/*` and `/api/update/*` arm below
        // (update-policy §1 problem 2): a real caller without `can_manage_it`
        // (Admin+) is refused; `None` = access control off → god-mode (§13.10).
        (_, &["api", "releases" | "update", ..]) if auth_user.is_some_and(|u| !u.can_manage_it()) => {
            rest::HttpReply::error(403, "IT management access required")
        }
        // ── Auto-update (O5.1, update-policy §5.9) ──────────────────
        (&Method::Get, &["api", "update", "status"]) => rest::update_status(state),
        (&Method::Post, &["api", "update", "check"]) => rest::update_check(state),
        (&Method::Post, &["api", "update", "apply"]) => rest::update_apply(state),
        (&Method::Put, &["api", "update", "mode"]) => rest::update_set_mode(state, body_bytes),
        // Retired manual version-choice routes (T5.1.5): the Update pane owns
        // the flow now; these stay only as a break-glass hatch.
        (&Method::Post, &["api", "releases", "download"])
        | (&Method::Put, &["api", "releases", "select"])
        | (&Method::Delete, &["api", "releases", _])
            if !rest::releases_break_glass() =>
        {
            rest::HttpReply::error(410, "retired \u{2014} auto-update owns versions (set CP_RELEASES_BREAK_GLASS=1)")
        }
        (&Method::Get, &["api", "releases"]) => rest::list_releases(state),
        (&Method::Put, &["api", "releases", "arch"]) => rest::set_arch(state, body_bytes),
        (&Method::Post, &["api", "releases", "download"]) => rest::download_release(state, body_bytes),
        (&Method::Put, &["api", "releases", "select"]) => rest::select_release(state, body_bytes),
        (&Method::Post, &["api", "releases", "deploy"]) => rest::deploy_fleet(state, body_bytes),
        (&Method::Post, &["api", "releases", "restart-orchestrator"]) => rest::restart_orchestrator(state),
        (&Method::Delete, &["api", "releases", tag]) => rest::delete_release(state, tag),

        // ── Claude Code usage + login (OAuth) ───────────────────────
        (&Method::Get, &["api", "claude-usage"]) => rest::claude_usage(),
        (&Method::Get, &["api", "claude-login", "status"]) => rest::token_status(),
        (&Method::Post, &["api", "claude-login", "start"]) => rest::login_start(state),
        (&Method::Post, &["api", "claude-login", "complete"]) => rest::login_complete(state, body_bytes),
        (&Method::Post, &["api", "claude-login", "refresh"]) => rest::refresh_login(),

        // ── Claude multi-account token vault ────────────────────────
        (&Method::Get, &["api", "claude-accounts"]) => rest::list_accounts(),
        (&Method::Post, &["api", "claude-accounts", "store"]) => rest::store_account(),
        (&Method::Post, &["api", "claude-accounts", "switch"]) => rest::switch_account(body_bytes),
        (&Method::Delete, &["api", "claude-accounts", email]) => rest::delete_account(email),

        _ => rest::HttpReply { status: 404, body: "{\"error\":\"not found\"}".to_owned() },
    }
}
