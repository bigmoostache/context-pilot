# Implementation Plan — Auth v3 (Four-Role RBAC + Unified Cockpit)

**Branch:** `auth` (from `deploy`)
**Design source of truth:** `docs/design-auth.md` §13
**Date:** 2026-07-08

This plan is ordered so each chantier is independently reviewable and, where
possible, independently shippable. The backend capability refactor (C1–C2) lands
first because everything else depends on it. The `:9090` teardown (C5) lands last
because it is the riskiest and benefits from the new IT endpoints already existing.

---

## C0 — Foundations: the capability layer (backend)

The keystone. Nothing else should reference a role name after this.

- **`services/auth/types.rs`**
  - Extend `UserRole` with `Superadmin` and `Manager`. Add `as_str`/`from_sql`
    arms (`"superadmin"`, `"manager"`).
  - Derive/implement `Ord` on `UserRole` with `superadmin > admin > manager > user`
    (explicit `rank()` → `u8` is clearer than derive; derive ties to declaration
    order — if used, declare in ascending or descending order deliberately and
    comment it).
- **New `services/auth/capabilities.rs`** (or a `caps` mod on `User`):
  ```rust
  impl User {
      fn can_manage_all_agents(&self) -> bool { self.role >= UserRole::Manager }
      fn can_manage_users(&self)      -> bool { self.role >= UserRole::Manager }
      fn can_manage_it(&self)         -> bool { self.role >= UserRole::Admin }
      fn can_manage_secrets(&self)    -> bool { self.role == UserRole::Superadmin }
      /// Anti-escalation (FR-v3-03): may the caller assign `target` role?
      fn can_assign_role(&self, target: UserRole) -> bool { target < self.role }
      /// Vendor invisibility (FR-v3-05).
      fn can_see(&self, other_role: UserRole) -> bool {
          other_role != UserRole::Superadmin || self.role == UserRole::Superadmin
      }
  }
  ```
- **Migration** (`services/auth/store.rs::init_schema`): widen the `role` CHECK to
  `('superadmin','admin','manager','user')`. For existing non-empty DBs use the
  12-step table rebuild (SQLite can't alter a CHECK). Map legacy `admin` →
  `superadmin` during the copy (design §13.6). Add a guarded idempotent block akin
  to the existing line-98 ALTER.
- **Access-control master flag (design §13.10).** RBAC is gated behind one runtime
  central setting, **off by default** (`access_control` key in the shared global
  config — same store as `onboarding_completed`, **not** localStorage).
  - Off ⇒ effective role = superadmin for everyone, no login. Reuse the existing
    "no authenticated user ⇒ full access" fast-path; do **not** fork a new code
    path.
  - The capability helpers take the effective user; when access control is off the
    pipeline supplies `auth_user == None` (or a synthetic superadmin), so the
    existing short-circuits apply unchanged.
  - Toggle gating: **enable** = anyone; **disable** = `can_manage_secrets`
    (superadmin) — asymmetric, FR-v3-10/11.
  - Bootstrap invariant: Ansible always seeds ≥1 superadmin, so no
    create-first-superadmin-on-enable flow is needed.
  - ⚠️ Mark the disable path **DEV-PHASE** in code comments (design §13.10) — a
    global god-mode off-switch is a known temporary escalation surface, to harden
    before prod.
- **Tests:** `Ord` ordering; each capability at each role; `can_assign_role`
  anti-escalation; legacy `admin`→`superadmin` migration on a seeded old DB;
  access-control off ⇒ all-pass; disable gated to superadmin.

**Exit:** builds, all existing auth tests green, new capability tests green.

---

## C1 — Rewire enforcement to capabilities (backend)

Mechanical, guided by design §13.7. No behavior change for existing 2-role
deployments *except* the intended re-gating.

- `transport/auth/acl.rs`: `authorize_agent`, `can_manage_acl`, `filter_fleet` →
  `can_manage_all_agents()`.
- `transport/rest/config/env_keys.rs` (lines 26/64/89): `role != Admin` →
  `!can_manage_secrets()`. **This intentionally removes API-key access from client
  admins.**
- `transport/auth/users.rs`: admin-gate → `can_manage_users()`; add
  anti-escalation on create/update-role (FR-v3-03) and vendor invisibility on
  list/get/delete (FR-v3-05, `can_see`).
- `transport/rest/config/settings.rs`: audit its `Admin` check; map to the correct
  capability (likely `can_manage_it` or product-config gate — decide per key).
- `runtime/seed.rs`: seed a `superadmin` (env `CP_SEED_SUPERADMIN_*`) and,
  optionally, a first `admin` (`CP_SEED_ADMIN_*`). Keep `must_change_password`.
- Grep gate: `rg 'UserRole::Admin' crates/` must return **only** capability
  internals + tests afterwards.

**Exit:** `cargo test --workspace` green; manual matrix check per design §13.2.

---

## C2 — User management: 4 roles + anti-escalation (full-stack)

- Backend `users.rs`: role param accepts the 4 values; enforce `can_assign_role`;
  filter superadmins from responses for non-superadmin callers.
- OpenAPI: regenerate contract for the widened role enum (`openapi.json` →
  `web/src/lib/api/generated/`).
- Frontend `web/src/components/auth/UsersDialog.tsx`: role `useState<"admin"|"user">`
  → the 4-role union; the role `<select>` lists only roles the current user
  `can_assign` (below own rank); hide superadmin rows unless viewer is superadmin.
- `RoleBadge` gains `manager`/`superadmin` variants.

**Exit:** a manager can create users only; an admin can create managers+users; a
superadmin sees/creates everything; nobody escalates. Verified in the running app.

---

## C3 — Settings: Secrets section + Access-control toggle (superadmin) (frontend)

- Surface provider **API keys** (`envKeys.ts`) and **Claude OAuth**
  (`claude_oauth.rs` flow) as a Settings section in `shell/config/*`.
- Gate render on `can_manage_secrets` (client-side cosmetic; server already
  enforces via C1). Section absent for admin/manager/user.
- Add the **Access control** toggle as a `ToggleRow` next to Developer mode /
  Show Overlay (`ConfigPanes.tsx`), but backed by the **server** central setting
  (via `settings.rs`), not `localStorage`. Enable = anyone; the switch is only
  *shown* to superadmins, and the disable call is server-gated to
  `can_manage_secrets` (C0). Reflects design §13.10.

**Exit:** superadmin can view/rotate API keys + drive OAuth from Settings; admin
gets 403 from the API and no UI entry point.

---

## C4 — Settings: IT section (admin) (full-stack) — precedes the teardown

Re-home the maintenance backend into product REST *before* deleting the plane, so
the capability exists on `:443` first.

- New product routes under `can_manage_it`, reusing the retained modules
  (`ca.rs`, `identity.rs`, `caddy.rs`, `state.rs`, `crypto.rs`):
  - `GET  /api/it/ca.crt`, `GET /api/it/ca/fingerprint` (CA download/trust)
  - `GET/POST /api/it/identity` (name/IP; POST regenerates cert + reloads Caddy)
  - provisioning-flag read (from `state.rs`)
- Frontend IT Settings section (migrate the useful bits of `maint/*` steps into
  `shell/config/`), gated on `can_manage_it`.

**Exit:** an admin can download the CA cert and change the IP from cockpit Settings
over `:443`; manager gets 403 + no UI.

---

## C5 — Day-0 on :80 and teardown of the `:9090` plane

The riskiest, last. Depends on C4 (IT endpoints live) and the day-0 flow.

- **Caddy** (`deploy/photonicat/Caddyfile`): serve the cockpit on `:80` pre-finalize;
  after finalize, `:80` → 308 redirect to `:443`. Remove the `:9090`→`:9191`
  maintenance vhost.
- **Backend `transport/mod.rs`**: delete the `Plane` split and the dual-socket;
  single cockpit pipeline. Day-0 identity entry becomes a normal `can_manage_it`
  REST call served over `:80` until `:443` exists.
- **`AuthGuard` / `next_action`**: add day-0 states (post-login: force password →
  set identity → download CA → done) as `next_action` values, replacing the
  standalone `MaintWizard`.
- **Delete**: `transport/maint/mod.rs`; `web/src/components/auth/maint/*`;
  `web/src/lib/api/maint.ts`; `probeMaintPlane` branch in `App.tsx`.
- **Ansible** (`deploy/ansible/`): stop provisioning identity; seed superadmin +
  first admin only; update `admin-sheet.txt.j2` to reflect the two seeded accounts
  and the `:80` day-0 URL.

**Exit:** fresh box boots serving cockpit on `:80`; admin completes identity on
`:80`; `:443` comes up; `:80` redirects; no `:9090` anywhere. End-to-end verified.

---

## Cross-cutting

- **Docs:** keep `deploy/PROVISIONING.md` and `docs/design-auth.md` §13 in sync as
  reality lands.
- **Backward-compat:** `CP_AUTH_ENABLED=false` must still behave as today at every
  chantier (NFR-19/13). The capability helpers all short-circuit when `auth` is
  `None`.
- **Verify gate:** run `/verify` (or the project verify skill) after C2, C4, C5 —
  drive the real flow, not just tests.

## Suggested commit/PR granularity

C0+C1 in one PR (atomic: the refactor is only safe as a unit). C2, C3, C4 each its
own PR. C5 its own PR (teardown), reviewed carefully.
