# Design Document — Authentication & Authorization

**Status:** Draft v2  
**Author:** Context Pilot  
**Date:** 2026-06-23  
**Thread:** T341

---

## §1 — Context & Problem

Context Pilot is deployed as-a-service to companies — pre-installed on a VPS or
rented hardware (e.g. Photonicat appliance). A single orchestrator serves the
entire fleet of agents. Multiple employees connect to the same instance
simultaneously via the web frontend.

Today the orchestrator has **no authentication**: anyone who can reach port 7878
sees every agent, every thread, every file. The existing ticket mechanism
(`ticket.rs`) defends only against confused-deputy / DNS-rebind attacks from
malicious websites — it proves the *browser tab* is the legitimate frontend, not
that the *human* is a legitimate user.

Multi-tenancy requires three capabilities:

1. **Identity** — who is this human?
2. **Authentication** — prove it.
3. **Authorization** — what are they allowed to see and do?

---

## §2 — Functional Requirements

### Identity & Authentication

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-01 | Users authenticate via email + password | Must |
| FR-02 | User accounts stored in an orchestrator-managed SQLite database | Must |
| FR-03 | First user registration (zero existing users) creates an admin — no CLI tool or config file needed | Must |
| FR-04 | Admin can create, list, and delete user accounts | Must |
| FR-05 | Successful login returns a session token | Must |
| FR-06 | Users can log out; session is destroyed server-side | Must |
| FR-07 | Authenticated user can query their own identity | Must |

### Authorization & Access Control

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-08 | Each agent has an access control list — a set of authorized users | Must |
| FR-09 | Admins have implicit access to all agents (no ACL entry needed) | Must |
| FR-10 | Non-admin users see and interact only with agents they are explicitly granted | Must |
| FR-11 | Admins can grant and revoke per-agent access for any user | Must |
| FR-12 | Fleet listing is filtered per-user on the server side | Must |
| FR-13 | SSE streams are subject to the same per-agent authorization as REST endpoints | Must |
| FR-14a | Each ACL entry carries a per-agent role: `agent-admin` or `agent-user` | Must |
| FR-14b | `agent-admin` can invite/remove users and change per-agent roles on their agent — this is the **only** behavioral difference with `agent-user` | Must |
| FR-14c | System admin ≠ agent-admin: system admin has implicit god-mode on all agents; agent-admin has management rights on one specific agent only | Must |

### Session Management

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-14 | Sessions are server-side (not stateless client tokens like JWT) | Must |
| FR-15 | Sessions have a configurable TTL (default 30 days) | Should |
| FR-16 | Admin can revoke all sessions for a given user (force logout) | Should |
| FR-17 | Deleting a user cascades to all their sessions and ACL entries | Must |

### Backward Compatibility

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-18 | Auth is disabled by default via an environment variable | Must |
| FR-19 | When auth is disabled, the system behaves identically to today — no middleware, no login | Must |
| FR-20 | Any authenticated user can create agents (no admin restriction, no quotas) | Must |
| FR-21 | Passwords must be at least 8 characters — no further complexity rules | Must |

### Deferred (explicitly out of scope for v1)

| ID | Requirement | Rationale |
|----|-------------|-----------|
| FR-D1 | Password reset flow | Admin can delete + recreate user |
| FR-D2 | OAuth / SSO / SAML federation | Overkill for initial deployments |
| FR-D3 | ~~Per-agent role granularity~~ | **Promoted to FR-14a/b/c** — two per-agent roles (`agent-admin`, `agent-user`) ship in v0 |
| FR-D4 | Audit log (who did what, when) | Valuable but not blocking |

---

## §3 — Non-Functional Requirements

### Security

| ID | Requirement |
|----|-------------|
| NFR-01 | Passwords hashed with Argon2id (current OWASP recommendation) |
| NFR-02 | Session tokens are 256-bit cryptographically random (system entropy) |
| NFR-03 | Password verification is constant-time (Argon2 provides this natively) |
| NFR-04 | Bearer token model — immune to CSRF by design (browser never auto-sends Authorization headers) |
| NFR-05 | ACL checked on every request, server-authoritative — never cached or trusted client-side |
| NFR-06 | Fail-closed: corrupted or unreachable auth database blocks all authenticated requests, never silently bypasses |

### Performance

| ID | Requirement |
|----|-------------|
| NFR-07 | Session validation adds < 1 ms latency per request (single indexed SQLite lookup) |
| NFR-08 | Expired sessions swept lazily on validation calls — no background timer, no extra thread |
| NFR-09 | Auth middleware is a no-op pass-through when auth is disabled (zero overhead) |

### Compatibility

| ID | Requirement |
|----|-------------|
| NFR-10 | Works with the existing `Access-Control-Allow-Origin: *` CORS policy |
| NFR-11 | Works over Tailscale WireGuard tunnel and direct HTTPS |
| NFR-12 | No change to the TUI agent codebase — the agent has no auth concept; the orchestrator is the sole gateway |
| NFR-13 | All four migration phases are individually non-breaking (auth disabled = status quo) |

### Maintainability

| ID | Requirement |
|----|-------------|
| NFR-14 | Auth is a single service — one struct, one SQLite connection, no ORM, no framework |
| NFR-15 | All auth-specific routes grouped under a `/api/auth/*` prefix |
| NFR-16 | Auth middleware is a single function inserted at the top of the request handler — not scattered |
| NFR-17 | Configuration via environment variables only (consistent with all other orchestrator config) |
| NFR-18 | No CLI tool needed for initial setup — first-user bootstrap via the API |

### Durability

| ID | Requirement |
|----|-------------|
| NFR-19 | Rolling backup of auth.db every ~5 minutes (overwrites previous rolling copy) |
| NFR-20 | Two permanent daily snapshots of auth.db (e.g. midnight + noon) — never overwritten by the rolling cycle |
| NFR-21 | Auth database stored at orchestrator level (`~/.context-pilot/orchestrator/auth.db`), not inside agents_dir |

---

## §4 — Architecture Decisions

| # | Decision | Alternatives considered | Rationale |
|---|----------|------------------------|-----------|
| D1 | Server-side sessions | JWT | Instant revocation; small user count (5–50); no distributed validation needed; JWT revocation requires a blacklist that ends up being a session store anyway |
| D2 | Bearer token (not cookie) | httpOnly cookie | Works with `*` CORS origin; matches the existing architecture; simpler than SameSite/Secure/Domain cookie config; Tailscale tunnel provides transport encryption |
| D3 | ACL with per-agent roles | Single binary ACL (no roles) | Agent-admin enables delegated management without system admin involvement; keeps system admin as god-mode |
| D4 | Auth disabled by default | Always-on | Non-breaking migration; local development stays frictionless; opt-in via `CP_AUTH_ENABLED=true` |
| D5 | Argon2id for password hashing | bcrypt, scrypt | Modern OWASP recommendation; pure-Rust crate available; memory-hard (resists GPU attacks) |
| D6 | First-user bootstrap | CLI seed command | Zero-friction: first `register` call with zero users creates admin; no extra tooling |
| D7 | Single SQLite file for auth at orchestrator level | Separate DB per concern, or inside agents_dir | Users, sessions, and ACL are tightly related; joins are useful; one file to back up. Auth is an orchestrator concern — lives alongside orchestrator config, not agent data |

---

## §5 — Data Model

Three entities, one SQLite database at `~/.context-pilot/orchestrator/auth.db` (orchestrator-level — not in agents_dir).

### Users

Each user has a unique ID (UUID v4), a unique email, a name, an Argon2id password hash (PHC string format), a role (`admin` or `user`), and creation/update timestamps.

### Sessions

Each session has a primary key token (256-bit random hex), a foreign key to the user, creation and expiry timestamps, and an optional user-agent string for auditing. Sessions are deleted on cascade when the owning user is deleted. An index on `user_id` supports bulk revocation; an index on `expires_at` supports lazy sweep.

### Agent ACL

A join table: `(agent_id, user_id)` is the composite primary key. Each row carries a **per-agent role** (`agent-admin` or `agent-user`) that determines whether the user can manage access on that specific agent. A `granted_at` timestamp and a `granted_by` foreign key (nullable) record provenance. Entries cascade-delete with the user.

**Two per-agent roles (FR-14a/b/c):**
- `agent-admin` — can invite/remove users and change per-agent roles on this agent.
- `agent-user` — can interact with the agent but cannot manage access.

System admins need no row — they have implicit access and management rights on all agents. The first user granted access to an agent is typically made `agent-admin`.

---

## §6 — API Surface

### Public routes (no auth required)

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/api/auth/login` | Authenticate with email + password, returns session token + user profile |
| POST | `/api/auth/register` | Bootstrap-only (zero users → creates admin) or admin-only (creates user) |
| GET | `/api/health` | Health check (unchanged) |

### Protected routes (session required)

All existing routes become protected when auth is enabled. New auth-specific routes:

| Method | Path | Access | Purpose |
|--------|------|--------|---------|
| GET | `/api/auth/me` | Any authenticated user | Current user profile |
| POST | `/api/auth/logout` | Any authenticated user | Destroy current session |
| GET | `/api/auth/users` | Admin only | List all users |
| POST | `/api/auth/users` | Admin only | Create a new user |
| DELETE | `/api/auth/users/{id}` | Admin only | Delete user (cascades sessions + ACL) |
| GET | `/api/agent/{id}/acl` | Admin or agent-admin | List users with access to this agent |
| POST | `/api/agent/{id}/acl` | Admin or agent-admin | Grant a user access to this agent (with role) |
| PATCH | `/api/agent/{id}/acl/{userId}` | Admin or agent-admin | Change a user's per-agent role |
| DELETE | `/api/agent/{id}/acl/{userId}` | Admin or agent-admin | Revoke a user's access to this agent |

### Error semantics

| Code | Meaning |
|------|---------|
| 401 | Missing, malformed, or expired session token |
| 403 | Valid session but insufficient permissions (wrong role or no ACL entry) |

---

## §7 — Integration Points

### Transport middleware

The auth gate inserts between the existing CORS preflight handling and the route dispatch. Three sequential checks:

1. **Is auth enabled?** If not, pass through — system behaves as today.
2. **Is this a public route?** Login, register, health → skip auth.
3. **Extract and validate session.** Read the `Authorization: Bearer <token>` header, look up the session in SQLite, reject if missing/expired.
4. **Per-agent authorization.** If the route targets a specific agent (any URL containing an agent ID), verify the user has access (admin bypass or ACL row).

### SSE stream

The existing ticket mechanism (mint → redeem single-use token for SSE upgrade) is extended: tickets are enriched with the minting user's ID. On SSE connection, the redeemed ticket's user is checked against the requested agent's ACL. This layers cleanly — no change to the SSE wire protocol.

### Frontend

- The shared `request()` function injects the Bearer token from `localStorage` on every call.
- A 401 response triggers token removal + redirect to a login page.
- A React auth context provides `user`, `token`, `login()`, `logout()` to the component tree.
- Fleet listing is server-filtered — the frontend renders whatever the backend returns, no client-side filtering needed.

---

## §8 — Configuration

| Variable | Default | Purpose |
|----------|---------|---------|
| `CP_AUTH_ENABLED` | `false` | Master switch — enables the auth middleware and login requirement |
| `CP_SESSION_TTL_SECS` | `2592000` (30 days) | Session lifetime before expiry |
| `CP_AUTH_DB` | `~/.context-pilot/orchestrator/auth.db` | Path to the auth SQLite database (orchestrator-level, NOT in agents_dir) |

---

## §9 — Migration Strategy

Four phases, each independently deployable and non-breaking.

| Phase | Scope | What changes |
|-------|-------|-------------|
| **1 — Backend auth** | AuthStore service + middleware + login/register/logout endpoints | Backend gains the capability but it's inert when `CP_AUTH_ENABLED=false` |
| **2 — Frontend auth** | Login page + auth context + Bearer injection + 401 redirect | Frontend adapts but only activates when the backend starts returning 401s |
| **3 — ACL enforcement** | Fleet filtering + per-agent checks + ACL CRUD endpoints | Authorization layer; admin grants access after user creation |
| **4 — Admin UI** | User management page + per-agent access grant UI | Last-mile UX; functional without this (admin uses API directly) |

---

## §10 — New Dependencies

| Crate | Purpose | Notes |
|-------|---------|-------|
| `rusqlite` | User / session / ACL store | Already in the workspace (cp-mod-entities uses it) — may share the dep |
| `argon2` | Password hashing (Argon2id) | Pure Rust, ~200 KB |
| `password-hash` | PHC string parsing | Companion to argon2 |

---

## §11 — Resolved Questions

All 12 open questions have been decided.

| # | Question | Decision |
|---|----------|----------|
| Q1 | **Auto-grant on agent creation** | Yes, as `agent-admin`. Creator can immediately invite others without system admin intervention. |
| Q2 | **Discovered agents** (pre-existing, booted externally) | Admin-only. System admin must explicitly grant access. |
| Q3 | **Concurrent users on the same agent** | Non-issue. Multiple users accessing the same agent simultaneously is the expected operational mode. No locking, no "primary operator" concept. |
| Q4 | **Token storage** | `localStorage`. Simple, sufficient. Deployment is behind Tailscale / private network. |
| Q5 | **Rate limiting on login** | Not needed. Deployment is not in a hostile network environment. |
| Q6 | **Session TTL model** | Absolute (30 days from creation). Stolen token cannot be kept alive indefinitely. |
| Q7 | **Multi-device sessions** | No per-user session limit. Admin can force-logout (FR-16) as a blunt tool. |
| Q8 | **Agent creation permission** | Any authenticated user can create agents, no quotas. |
| Q9 | **Auth database location** | **Orchestrator-level, NOT in agents_dir.** Auth is an orchestrator concern, not agent data. Default: `~/.context-pilot/orchestrator/auth.db`. Mirrors how agents store their own databases, but at the orchestrator level. |
| Q10 | **Password policy** | Basic enforcement: minimum 8 characters. No complexity rules. |
| Q11 | **Multi-orchestrator** | Never. There will never be multiple orchestrators on the same machine. Single orchestrator, single auth.db. |
| Q12 | **Backup strategy** | Rolling backup every ~5 minutes (overwrites previous). Plus 2 permanent daily snapshots. Fail-closed on corruption (500 until restart or restore). |

---

## §12 — Estimated Scope

| Area | Estimate | Files |
|------|----------|-------|
| Backend — AuthStore service | ~300 lines | `services/auth.rs` (new) |
| Backend — Auth transport handlers | ~200 lines | `transport/auth.rs` (new) |
| Backend — Middleware integration | ~50 lines | `transport/mod.rs` (edit), `ticket.rs` (edit), `rest/mod.rs` (edit) |
| Frontend — Login page | ~100 lines | `components/auth/LoginPage.tsx` (new) |
| Frontend — Auth context + guard | ~130 lines | `lib/auth.tsx` (new), `shell/AuthGuard.tsx` (new) |
| Frontend — Token injection + 401 handling | ~20 lines | `lib/api/client.ts` (edit), `App.tsx` (edit) |
| **Total** | **~800 lines** | **4 new files + 4 edits** |
