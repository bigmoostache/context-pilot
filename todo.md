# TODO — Auth v3 Implementation (Four-Role RBAC + Unified Cockpit)

**Branch:** `auth`
**Design source of truth:** `docs/design-auth.md` §13 (authoritative). Plan narrative: `docs/auth-v3-plan.md`.
**Scope:** implement §13 in six milestones. Each milestone is an independently reviewable unit.

---

## Global rules (non-negotiable)

1. **Linters are authoritative and MUST NOT be overridden.** The CI check scripts
   below are the sole arbiters of "green". No `#[allow(...)]`, `// eslint-disable`,
   `@ts-ignore`, `#[cfg(...)]`-hiding, or lint-exception registry entry may be added
   to make a milestone pass. The check scripts already enforce this: the Rust lints
   script runs a **lint-exception registry** audit and a **vault-bypass** audit; the
   TS lints script runs a **suppressions** audit + **rule-census** + **knip**
   dead-code. If a lint fails, fix the code — never the lint.
2. **Code review by a neutral, context-free agent.** Every milestone closes with a
   review by a freshly-spawned agent that has **no access to this conversation**. It
   receives only: the milestone diff, `docs/design-auth.md` §13, and that milestone's
   acceptance criteria from this file. It must (a) run every check script itself and
   (b) independently confirm each acceptance criterion. See **Code-review gate** below.
3. **Structure limits hold.** Every file ≤ 500 lines, every folder ≤ 8 files
   (`check-structure.sh`). Split modules proactively (the existing `auth/` split is
   the template).
4. **Backward compatibility.** With access control OFF (the default), behaviour is
   identical to today at every milestone (design §13.10, FR-19).

### Authoritative validation commands

| # | Command | Gates |
|---|---------|-------|
| L1 | `.github/checks/check-rust-lints.sh --ci` | fmt · clippy · cargo check · lint-exception registry · vault-bypass |
| L2 | `.github/checks/check-rust-tests.sh --ci` | cargo build --release · cargo test (workspace) |
| L3 | `.github/checks/check-structure.sh --ci` | file ≤500 lines · folder ≤8 files · hash chain |
| L4 | `.github/checks/check-api-contract.sh --ci` | OpenAPI ↔ TS client codegen drift · route exhaustiveness · manual-fetch audit |
| L5 | `.github/checks/check-ts-lints.sh --ci` | eslint · prettier · stylelint · tsc · type-coverage (≥99.8%) · suppressions · rule-census · knip |
| L6 | `.github/checks/check-ts-tests.sh --ci` | Playwright e2e (stub until stack provisioned) |

> **DoD for every milestone:** L1–L6 relevant to the touched surface pass **with zero
> new suppressions**, all milestone acceptance criteria are verified by the neutral
> reviewer, and the diff is merged via its own PR.

### Code-review gate (per milestone)

Spawn a context-free agent with this exact brief:

> You are reviewing a single PR in isolation. You have NO prior context. Inputs:
> (1) the diff, (2) `docs/design-auth.md` §13, (3) milestone `<Mx>` acceptance
> criteria from `todo.md`. Do this and only this: run every command in the
> "Authoritative validation commands" table and paste raw output; for each acceptance
> criterion, state PASS/FAIL with the exact evidence (test name, grep result, HTTP
> status); flag any new lint suppression, `#[allow]`, `@ts-ignore`, or registry entry
> as an automatic FAIL. Do not fix anything. Return a verdict: MERGEABLE or
> CHANGES-REQUIRED with a numbered list.

A milestone is **Done** only when the neutral reviewer returns MERGEABLE.

---

## M0 — Capability foundation + access-control flag (backend)

Keystone. After this, no enforcement site references a role name directly.

### Objective O0.1 — Four ordered roles
- [ ] Extend `UserRole` (`services/auth/types.rs`) with `Superadmin`, `Manager`.
  - [ ] `as_str`/`from_sql` arms: `"superadmin"`, `"manager"`; unknown → `User`.
  - [ ] Implement a total order via an explicit `rank()` → `u8`, `impl Ord`, with a comment fixing `superadmin > admin > manager > user`.
- **Validation (assertable):**
  - V0.1a Test `role_ordering` asserts `Superadmin > Admin > Manager > User` (all 6 pairwise) — passes under L2.
  - V0.1b Test `role_sql_roundtrip` asserts `from_sql(r.as_str()) == r` for all four, and `from_sql("bogus") == User`.

### Objective O0.2 — Capability predicates
- [ ] New `services/auth/capabilities.rs` (or `impl User`): `can_manage_all_agents`, `can_manage_users` (≥ Manager), `can_manage_it` (≥ Admin), `can_manage_secrets` (== Superadmin), `can_assign_role(target)` (`target < self.role`), `can_see(other_role)` (superadmin hidden from non-superadmin).
- **Validation (assertable):**
  - V0.2a Test matrix `capabilities_by_role` asserts the exact truth table of design §13.3 for all 4 roles × 5 capabilities (20 assertions) — passes under L2.
  - V0.2b Test `anti_escalation` asserts `can_assign_role` is false for target ≥ self and true for target < self, per role.
  - V0.2c Test `vendor_invisibility` asserts `can_see(Superadmin)` is true only when caller is Superadmin.

### Objective O0.3 — Role-column migration
- [ ] Widen the `users.role` CHECK to `('superadmin','admin','manager','user')` in `init_schema` (`store.rs`), idempotent, via the 12-step table rebuild.
  - [ ] On rebuild of a non-empty legacy DB, map `admin` → `superadmin`, `user` → `user`.
- **Validation (assertable):**
  - V0.3a Test `migration_widens_check` seeds a DB with the old CHECK + one `admin` + one `user` row, runs `init_schema`, asserts the `admin` row now reads `superadmin` and inserting a `manager` row succeeds.
  - V0.3b Test `migration_idempotent` runs `init_schema` twice; second run is a no-op (row count + roles unchanged).

### Objective O0.4 — Access-control master flag
- [ ] Add central setting `access_control` (shared global config, same store as `onboarding_completed`; **NOT** localStorage), default `false`.
  - [ ] Enforcement reads the flag: OFF ⇒ pipeline supplies `auth_user == None` (or synthetic superadmin) ⇒ existing full-access fast-path. No new parallel code path.
  - [ ] Toggle endpoint: **enable** allowed for anyone; **disable** requires `can_manage_secrets`. Mark the disable branch `// DEV-PHASE (design §13.10)`.
- **Validation (assertable):**
  - V0.4a Test `flag_default_off` asserts `access_control` reads `false` on a fresh DB.
  - V0.4b Test `flag_off_is_god_mode` asserts that with the flag OFF, a request with no token resolves to full access on an agent-scoped route (mirrors current auth-disabled behaviour).
  - V0.4c Test `disable_requires_superadmin` asserts the disable call returns 403 for `manager`/`admin` tokens and 200 for `superadmin`; enable returns 200 for any caller.

**M0 Done when:** L1, L2, L3 green with zero new suppressions; neutral reviewer MERGEABLE.

---

## M1 — Rewire enforcement to capabilities (backend)

Mechanical, per design §13.7. No behaviour change except the intended re-gating.

### Objective O1.1 — Replace every role-name check with a capability
- [ ] `transport/auth/acl.rs`: `authorize_agent`, `can_manage_acl`, `filter_fleet` → `can_manage_all_agents()`.
- [ ] `transport/rest/config/env_keys.rs` (3 sites): `role != Admin` → `!can_manage_secrets()`.
- [ ] `transport/auth/users.rs`: admin-gate → `can_manage_users()` + `can_assign_role` on create/update + `can_see` filtering on list/get/delete.
- [ ] `transport/rest/config/settings.rs`: map its `Admin` check to the correct capability per key.
- [ ] `runtime/seed.rs`: seed ≥1 `superadmin` + optional first `admin`, keep `must_change_password`.
- **Validation (assertable):**
  - V1.1a `grep -rn 'UserRole::Admin' crates/cp-orchestrator/src` returns matches **only** in `capabilities.rs`, `types.rs`, and `*/tests.rs` (0 matches in any `transport/` handler). Assert exact file set.
  - V1.1b Integration test `secrets_regated`: with flag ON, `GET /api/env-keys/{name}` returns 403 for an `admin` token, 200 for a `superadmin` token. (Intentional removal of API-key access from client admins.)
  - V1.1c Integration test `all_agents_manager`: a `manager` token can `GET` any agent it has no ACL row for (200); a `user` token cannot (403).
  - V1.1d Integration test `users_anti_escalation`: `manager` creating an `admin` → 403; `admin` creating a `manager` → 200; any non-superadmin listing users never returns a superadmin row.

**M1 Done when:** L1, L2, L3 green, zero new suppressions; V1.1a grep assertion holds; neutral reviewer MERGEABLE.

---

## M2 — User management: 4 roles + anti-escalation (full-stack)

### Objective O2.1 — Backend accepts and enforces 4 roles
- [ ] `users.rs` create/update accept the 4 role values; enforce `can_assign_role`; filter superadmins for non-superadmin callers.
- **Validation:** V2.1a Test `create_all_roles` asserts a superadmin can create each of the 4 roles; V2.1b re-asserts V1.1d end-to-end via the HTTP handler.

### Objective O2.2 — Contract regenerated
- [ ] Regenerate `openapi.json` for the widened role enum; regenerate `web/src/lib/api/generated/`.
- **Validation:** V2.2a `check-api-contract.sh --ci` (L4) passes with **zero drift** (generated client matches spec; no manual edit).

### Objective O2.3 — Frontend user UI
- [ ] `UsersDialog.tsx`: role state → 4-role union; the role `<select>` lists only roles `can_assign` (strictly below current user); hide superadmin rows unless viewer is superadmin.
- [ ] `RoleBadge` gains `manager` + `superadmin` variants.
- **Validation:**
  - V2.3a L5 green (eslint · tsc · type-coverage ≥99.8% · prettier · knip) with zero new suppressions.
  - V2.3b Manual/Playwright assertion (documented in PR): logged in as `manager`, the role selector offers only `user`; as `admin`, offers `{manager,user}`; as `superadmin`, offers all four; no superadmin row is visible to a non-superadmin.

**M2 Done when:** L1–L5 relevant green, zero new suppressions; neutral reviewer MERGEABLE.

---

## M3 — Settings: Secrets section + Access-control toggle (superadmin) (frontend)

### Objective O3.1 — Secrets section
- [ ] Surface provider API keys (`envKeys.ts`) + Claude OAuth flow (`claude_oauth.rs`) as a Settings section in `shell/config/*`, rendered iff `can_manage_secrets`.
- **Validation:**
  - V3.1a With flag ON as `superadmin`: the Secrets section renders and a key rotate round-trips (PUT then GET reflects change). As `admin`: section absent **and** the API returns 403 (server-authoritative, re-confirms V1.1b).

### Objective O3.2 — Access-control toggle in Settings
- [ ] Add an **Access control** `ToggleRow` (`ConfigPanes.tsx`) backed by the server central setting (not localStorage); shown only to superadmins; disable path server-gated (M0/O0.4).
- **Validation:**
  - V3.2a Flipping the toggle persists across a full page reload **and** across browsers (proves server-side, not localStorage).
  - V3.2b L5 green, zero new suppressions.

**M3 Done when:** L4 (if contract touched), L5 green; V3.1a + V3.2a verified; neutral reviewer MERGEABLE.

---

## M4 — Settings: IT section (admin), backend re-homed on :443 (full-stack)

Re-home the maintenance backend into product REST **before** deleting the plane (M5).

### Objective O4.1 — IT REST endpoints under `can_manage_it`
- [ ] New product routes reusing `ca.rs`/`identity.rs`/`caddy.rs`/`state.rs`/`crypto.rs`: `GET /api/it/ca.crt`, `GET /api/it/ca/fingerprint`, `GET/POST /api/it/identity` (POST regenerates cert + reloads Caddy), provisioning-flag read.
- **Validation:**
  - V4.1a Integration test `it_gated`: with flag ON, every `/api/it/*` route returns 403 for `manager`/`user`, 200 for `admin`/`superadmin`.
  - V4.1b Test `it_identity_roundtrip`: `POST /api/it/identity` with a valid name/IP then `GET` reflects it; invalid name/IP → 400 (reuses existing `validate_name`/`validate_ip`).
  - V4.1c L4 passes: the new routes are in `openapi.json` and the generated TS client (no manual-fetch audit failure).

### Objective O4.2 — IT Settings UI
- [ ] Migrate the useful bits of `maint/*` steps into a `shell/config/` IT section, gated on `can_manage_it`.
- **Validation:** V4.2a As `admin`: CA cert downloads and IP change succeeds from cockpit over `:443`; as `manager`: section absent + API 403. V4.2b L5 green, zero new suppressions.

**M4 Done when:** L1–L5 relevant green; V4.1a–c + V4.2a verified; neutral reviewer MERGEABLE.

---

## M5 — Day-0 on :80 + teardown of the :9090 plane

Riskiest, last. Depends on M4 (IT endpoints live on :443).

### Objective O5.1 — Cockpit served on :80 pre-finalize, redirect post-finalize
- [ ] `deploy/photonicat/Caddyfile`: serve cockpit on `:80` while unprovisioned; after finalize, `:80` → 308 redirect to `:443`. Remove the `:9090`→`:9191` vhost.
- **Validation:**
  - V5.1a On a fresh (unprovisioned) box: `curl -s http://<ip>/` returns the cockpit HTML (200). After finalize: `curl -sI http://<ip>/` returns `308` with `Location: https://…`.
  - V5.1b `curl -sI http://<ip>:9090/` fails to connect (port closed).

### Objective O5.2 — Day-0 flow folded into AuthGuard
- [ ] Add day-0 `next_action` states (force password → set identity → download CA → done) replacing `MaintWizard`.
- **Validation:** V5.2a Integration test on `/api/auth/me`: a seeded admin with `must_change_password` + unprovisioned box returns `next_action` walking password → identity → ready in order.

### Objective O5.3 — Delete the plane
- [ ] Delete `transport/maint/mod.rs`, the `Plane` split + dual-socket in `transport/mod.rs`, `web/src/components/auth/maint/*`, `lib/api/maint.ts`, the `probeMaintPlane` branch in `App.tsx`.
- **Validation:**
  - V5.3a `git ls-files` shows none of: `crates/cp-orchestrator/src/transport/maint/mod.rs`, `web/src/components/auth/maint/`, `web/src/lib/api/maint.ts`.
  - V5.3b `grep -rn 'probeMaintPlane\|:9090\|:9191\|Plane::' crates web/src` returns 0 matches (excluding docs).
  - V5.3c L1–L5 green after deletion (no dangling refs; knip reports no orphaned exports).

### Objective O5.4 — Ansible seeds accounts only
- [ ] `deploy/ansible/`: stop provisioning identity; seed superadmin + first admin only; update `admin-sheet.txt.j2` (two accounts + `:80` day-0 URL).
- **Validation:** V5.4a `grep -rn 'identity\|finalize' deploy/ansible/` shows no identity/finalize provisioning task remains; the sheet template references both seeded accounts and the `:80` URL.

**M5 Done when:** L1–L6 green; V5.1a–V5.4a verified on a real fresh-box run (end-to-end, not just tests); neutral reviewer MERGEABLE.

---

## Suggested PR granularity

- **PR-1:** M0 + M1 (atomic — the capability refactor is only safe as a unit).
- **PR-2:** M2 · **PR-3:** M3 · **PR-4:** M4 · **PR-5:** M5.

Each PR: its own neutral-agent review returning MERGEABLE before merge.
