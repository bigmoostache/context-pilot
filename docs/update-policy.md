# Update Policy — Fleet OTA Strategy

**Status:** Draft v2
**Author:** Context Pilot
**Date:** 2026-06-28

> v2 (2026-06-28): reworked §2/§5/§6 after a code audit. Two findings drive the
> design — (1) a release ships **two** binaries (the long-running orchestrator
> *and* the agent binary), so the orchestrator must replace *itself*; (2) procd
> `respawn 3600 5 5` means a crash-looping new binary bricks the box, so the
> rollback decision cannot live in the binary being swapped. The apply model is
> now **client-initiated** (an *Update* category in Settings): we choose the
> version, the client chooses the moment.

---

## §1 — Context & Problem

Context Pilot ships as an appliance (Photonicat 2 — OpenWrt / musl / procd) deployed
at client sites and reachable over a Tailscale/Headscale overlay. The runtime is a
single static binary plus the console server, with a `ReleaseStore`
(`crates/cp-orchestrator/src/services/releases/`) that already keeps N downloaded
versions under `~/.context-pilot/releases/{tag}/`.

Today the cockpit exposes a **Releases** pane (admin-only in the UI,
`web/src/components/shell/config/ReleasesPane.tsx`) that lets the client
download/select/delete release binaries directly. Two problems:

1. **Policy** — clients should not choose their own software versions. Version choice
   is ours; the client receives (cleanly) whatever the channel decides.

2. **Security gap (verified)** — the backend release routes
   (`transport/rest/releases.rs`, dispatched in `transport/mod.rs:337-341`) are the
   only mutating routes that do **not** receive `auth_user`. Compare the line just
   above them, `mint_ticket(state, auth_user)`, and `settings.rs` / `users.rs` which
   all take `auth_user` and check the role internally. The global `authenticate`
   gate (`transport/mod.rs:164`) *does* require a valid token, so the precise
   statement is: **any authenticated user, regardless of role, can call
   download/select/delete** by bypassing the frontend. Worse, the header comment in
   `releases.rs` claims *"All endpoints are admin-only — the router gates them"*,
   which is false (the router gates *authentication*, not *role*) and actively
   misleads the next reader. Both the gap and the comment must be fixed.

This document fixes the target update model.

---

## §2 — Decision

**The client never chooses a *version*; the client chooses *when* to apply.**

- **We choose the version** — published as a signed manifest per channel.
- **The box reads it** — polls the manifest, compares against its running version
  (egress to GitHub already exists via `ReleaseStore::fetch_remote_releases`).
- **The client triggers the apply** — from an *Update* category in Settings
  ("Update available: vY → [Update now]"). No automatic timer.

Apply is atomic, signature-verified, and **auto-rolls-back** on a failed health
check. This refines the "let the Admin pick a binary" behaviour of the current
Releases pane and is consistent with the fleet strategy frozen on 2026-06-27
(`updates push→pull`), with the relaxation that the client controls timing (the
"defer / maintenance window" allowance of §4.5).

---

## §3 — Beyond "push vs pull"

"The device pulls" vs "we push from a server" is a false dilemma. The dominant model
(Mender, balena, AWS IoT / Azure Device Update, Tesla, Android, k8s-at-the-edge)
separates two concerns:

- **Intent is centralised and declarative** — *we declare the desired state*:
  "channel `stable` is on version Y."
- **Transfer is device-initiated** — *the box pulls the artifact*: each appliance
  polls a manifest, compares against its current version, and converges (here, on
  client command rather than automatically).

This is GitOps / reconciliation applied to the edge. We keep control (we decide who
runs what) without the fragility of direct push.

**Why pull wins for a fleet:** direct push (SSH/Ansible) assumes the box is online at
exactly that moment, behind NAT, and handles partial failure poorly. Pull is
NAT-resilient and asynchronous. Direct push over Tailscale SSH stays excellent for
**day-0** and **break-glass**, not routine updates.

---

## §4 — The five state-of-the-art properties

1. **Centralised desired-state** — a signed manifest (channel → version). A signed
   JSON published on GitHub. *(See §5.3.)*

2. **Atomic apply + health-gated automatic rollback** — the #1 reliability property.
   The device applies, runs a self-test, and **reverts itself** if the health check
   fails. The heavyweights use A/B partitions (RAUC, Mender, swupdate, OSTree); for
   our two binaries we get the same effect with `ReleaseStore` (keeps N versions) +
   symlink swap + a **rollback supervisor that survives the swap** + a `/healthz`
   contract. *(See §5.2 — this is the part the v1 draft under-specified.)*

3. **Phased rollout (canary / rings)** — never 100% at once. **Deferred:** with a
   ~1-box fleet today, rings are premature; explicitly out of v1 (§5.7), not just
   "later".

4. **Signed artifacts** — signature verified *on the device* before swap, so a
   compromised update server cannot push malware. Formal reference: TUF / Uptane —
   over-dimensioned here. We keep the two protections that matter: signed manifest +
   **anti-rollback / freshness** (§5.6).

5. **Visibility + timing, not version choice, for the client** — the Admin sees
   version/channel, the release notes, and decides *when* to apply. They never pick
   an arbitrary version.

### Tooling landscape (for reference)

| Tool | Model | Relevance |
|------|-------|-----------|
| Mender / RAUC+hawkBit / swupdate | A/B image, pull | Embedded standard, but heavy — built to swap the whole OS image |
| balena | Containers, pull | Full platform, too much for two binaries |
| OpenWrt ASU / `owut` / sysupgrade | Pull, OS image | Relevant for the *OS* layer, not our app (out of scope, §5.7) |
| minisign | Detached signatures | Our signing layer — `minisign` CLI in CI, `minisign-verify` crate on-box |
| TUF / Uptane | Secure distribution spec | Reference; we borrow anti-rollback + freshness only |

Since we run **two static binaries** on OpenWrt (no OS image to swap), a full
RAUC/Mender is over-dimensioned. The existing `ReleaseStore` + symlink swap + procd
restart + health check is the right weight. The gaps to fill are: **a rollback
supervisor that survives self-replacement, signing, a central manifest, health-gated
rollback, and the DB-migration story**.

---

## §5 — Target design for Context Pilot

### §5.1 — On-disk layout (introduces a wrapper + symlinks)

A release tarball contains **two** binaries — the long-running console/orchestrator
process (`cp-console-server`, deployed as `bin/cp-orchestrator`, `PROG` in the procd
init) and the agent binary the supervisor spawns (`cpilot`, deployed as `bin/tui`,
`CP_AGENT_BINARY`). `ReleaseStore::select` today only repoints the *agent* binary, not
the running orchestrator — so updating the orchestrator is the real self-replacement
problem.

Make `PROG` swappable and rollbackable via a wrapper + symlinks:

```
/mnt/data/context-pilot/
  bin/
    run-orchestrator.sh        # NEW PROG (the rollback supervisor; posed day-0, ~never changes)
    cp-orchestrator            # symlink -> ../releases/<tag>/cp-console-server
    tui                        # symlink -> ../releases/<tag>/cpilot
  releases/<tag>/{cp-console-server, cpilot}   # managed by ReleaseStore (already)
  current   -> releases/<tag>                  # active tag
  previous  -> releases/<tag-1>                # rollback target
  update-state.json                            # {staged, pending_confirm, boot_deadline}
```

`config.json` (`active_tag`, already persisted) stays the logical source of truth; the
`current`/`previous` symlinks are what the wrapper follows (it can't parse JSON, it
follows the link).

### §5.2 — The rollback supervisor = the wrapper (keystone)

`PROG` becomes `run-orchestrator.sh`. **procd respawns the wrapper, not the binary**,
so the wrapper survives the swap and owns the rollback decision — which the binary
being replaced cannot, because procd (`respawn 3600 5 5`) would just respawn a
crash-looping new binary five times and then give up, bricking the box.

```sh
while true; do
  target=$(readlink -f current)
  "$target/cp-console-server" &      # CHILD, not exec
  child=$!
  if wait_healthy "http://127.0.0.1:7878/healthz" 60; then
      wait "$child"; exit $?         # healthy -> transparent; procd respawns wrapper on exit
  else
      kill "$child"
      if [ -L previous ] && [ "$(readlink -f previous)" != "$target" ]; then
          ln -sfn "$(readlink previous)" current     # ROLLBACK
          logger -t context-pilot "update failed, rolled back"
          continue                                   # loop on the old version
      fi
      exit 1                          # already on previous -> let procd back off
  fi
done
```

This requires a **`/healthz` endpoint** on the orchestrator (200 once it has bound +
DB open + registry loaded). "Healthy" = `/healthz` answers OK within the deadline.
This is the health-check contract the v1 draft listed but never defined.

### §5.3 — Manifest format (signed)

Two files per channel, at a stable URL (e.g. a `channels` branch of the repo:
`raw.githubusercontent.com/<org>/<repo>/channels/stable.json`):

`stable.json`:
```json
{
  "schema": 1,
  "channel": "stable",
  "version": "v0.4.0-abc1234",
  "released_at": "2026-06-28T10:00:00Z",
  "expires_at":  "2026-09-28T00:00:00Z",
  "min_from":    "v0.2.0",
  "notes_url":   "https://github.com/<org>/<repo>/releases/tag/v0.4.0",
  "artifacts": {
    "linux-aarch64": { "url": ".../cpilot-linux-aarch64.tar.gz", "sha256": "…", "size": 12345678 },
    "linux-x86_64":  { "url": ".../cpilot-linux-x86_64.tar.gz",  "sha256": "…", "size": 12000000 }
  }
}
```
`stable.json.minisig`: detached **minisign** signature of the exact JSON bytes.

Chain of trust: the signature covers the manifest; the manifest pins a per-arch
`sha256` → trust extends to the actual tarball bits.

### §5.4 — Signing toolchain (musl-friendly)

- **CI (publish):** after building the tarballs, compute `sha256`, generate the JSON,
  sign with the `minisign` CLI (private key = GitHub Actions secret), push
  `stable.json` + `.minisig` to the `channels` branch.
- **Box (verify):** the **`minisign-verify`** crate (pure Rust, clean static musl
  build), with the **public key embedded as a `const`** in the orchestrator at build
  time. No TUF/Uptane — we keep only its anti-rollback + freshness ideas (§5.6).

### §5.5 — End-to-end steps

**Our side (publish a version):**
1. CI builds aarch64 + x86_64 tarballs (already in place).
2. CI computes sha256, generates `stable.json`, signs → `.minisig`.
3. CI publishes tarballs (GitHub release) + pushes manifest+sig to `channels`. **Done
   — nothing is pushed to the boxes.**

**Box side (check — read-only, continuous):**
1. On boot + every N hours + on "Check now": GET `stable.json` + `.minisig`.
2. Verify the signature (embedded public key). Fail → ignore, keep last-known state.
3. Verify **freshness**: `expires_at > now` (else a stale signed manifest is being
   replayed).
4. Verify **anti-rollback**: `version > current` (monotonic) and `current >=
   min_from`. `version == current` → "up to date".
5. Else → surface "Update available: vY" + `notes_url`. No download yet.

**Box side (apply — only on admin click):**
1. *PREPARE (reversible):* resolve the artifact for this arch, download into
   `releases/<newtag>/` (reuse `ReleaseStore::download`), **verify the sha256**
   against the manifest. KO → clean abort, nothing moved.
2. **Back up `auth.db` → `auth.db.bak-<oldtag>`** (see §5.8).
3. *APPLY:* `previous → (old current)`; `current → releases/<newtag>`; trigger the
   procd restart (orchestrator exits cleanly, the wrapper takes over).
4. The wrapper relaunches `cp-console-server` of the new tag → `/healthz`.
5. **Healthy within the deadline** → write `active_tag=<newtag>`, repoint the `tui`
   (agent) symlink; status "up to date on vY".
6. **Not healthy** → the wrapper already did `current → previous` and relaunched the
   old binary; on restart the orchestrator restores `auth.db.bak` if a migration had
   run; status "update failed, rolled back to vX".

### §5.6 — Anti-rollback / freshness (Uptane, light)

- Signature mandatory (§5.4).
- **Monotonic version** — refuse `version <= current` outside a break-glass downgrade.
- **`expires_at`** — refuse a stale replayed manifest. *Depends on a correct clock*:
  the Photonicat is LTE → NTP should be fine; if the clock is unreliable, the
  monotonic check stays the primary guard and expiry becomes advisory.

### §5.7 — Out of scope (state it explicitly)

The manifest governs **only** the app binaries (`cp-console-server` + `cpilot`). The
wrapper, Caddy, Tailscale, the procd init script, and the OpenWrt OS (`sysupgrade`)
are handled via day-0 / Ansible / break-glass. Rings/canary are **out of v1** (fleet
too small to matter). Keeping these out prevents scope creep.

### §5.8 — The subtle trap: DB schema migrations

If the new version migrates `auth.db` (forward-only) **before** it crashes, the old
binary may no longer read it on rollback → the rollback fails too. This is the
subtlest risk. Mitigation: **back up `auth.db` before apply** (§5.5 step 2),
**auto-restore on rollback**, and an engineering rule that **migrations stay
additive / backward-compatible across one version step**.

### §5.9 — Cockpit UX (the *Update* category)

The existing `releases` category (`ConfigPanes.tsx`, already `adminOnly`) becomes
**Update**:

- **Read** for everyone: current version + channel.
- **Admin:** "Up to date" *or* "Update available: vY" + notes + **[Update now]** +
  **[Check now]**.
- During apply: the API drops for a few seconds (restart). The frontend shows
  "Applying… the console is restarting" and **polls `/version`** until it returns —
  back on vY → success; back on vX → "failed, rolled back".
- **Remove** arbitrary download/select/delete (or hide behind a break-glass admin
  flag), which also shrinks the RBAC attack surface.

**In one line:** desired-state declared by us, artifact pulled and applied by the box
*on the client's command*, atomic apply with a swap-surviving rollback supervisor,
mandatory signing, DB backed up around the swap. The client picks the moment, never
the version.

---

## §6 — Implementation delta (to scope)

Two independent tracks. The first is a security fix that should ship **now**,
decoupled from the OTA epic.

**Track A — security (ship immediately):**
- [ ] Pass `auth_user` + `can_admin()` to the five release routes
      (`transport/mod.rs:337-341`), closing the RBAC gap.
- [ ] Fix the misleading "admin-only — the router gates them" header comment in
      `releases.rs`.

**Track B — OTA:**
- [ ] `/healthz` on the orchestrator (bind + DB + registry).
- [ ] `run-orchestrator.sh` rollback supervisor; switch `PROG`; lay down
      `current`/`previous` symlinks in day-0 (Ansible).
- [ ] `updater` module: fetch manifest, `minisign-verify`, freshness + anti-rollback
      checks, download + sha256, symlink swap, restart, `update-state.json`.
- [ ] `auth.db` backup/restore around apply; additive-migration rule documented.
- [ ] CI: generate + `minisign`-sign the manifest, push to `channels`.
- [ ] REST: `GET /api/update/status`, `POST /api/update/check`, `POST
      /api/update/apply` — all with `auth_user` + `can_admin()`.
- [ ] Cockpit *Update* pane (rename `releases`), drop version choice.

### Open decisions (before freezing)

- **A. Rollback owner:** wrapper script (recommended — atomic, survives the swap
  natively) vs. a separate procd-timer watchdog on a `confirmed/pending` flag (more
  OpenWrt-idiomatic, more moving parts).
- **B. Manifest hosting:** `channels` branch raw file (simplest to sign/push in CI)
  vs. an asset on a fixed GitHub release `channel-stable` (cleaner, but recreate the
  release on each promotion).

---

## §7 — Related

- `deploy/PROVISIONING.md` — canonical provisioning / fleet doc (Tailscale overlay,
  day-0 manual, push→pull updates).
- `docs/design-auth.md` — auth & RBAC (the `can_admin()` pattern referenced in §1/§6).
- `deploy/photonicat/init.d/context-pilot.init` — the procd service (`PROG`,
  `CP_AGENT_BINARY`, `respawn 3600 5 5`) the wrapper plugs into.
