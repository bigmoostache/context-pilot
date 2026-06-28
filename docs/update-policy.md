# Update Policy — Fleet OTA Strategy

**Status:** Draft v1
**Author:** Context Pilot
**Date:** 2026-06-28

---

## §1 — Context & Problem

Context Pilot ships as an appliance (Photonicat 2 — OpenWrt / musl / procd) deployed
at client sites and reachable over a Tailscale/Headscale overlay. The runtime is a
single static binary (`cpilot`) plus the console server, with a `ReleaseStore`
(`crates/cp-orchestrator/src/services/releases/`) that already keeps N downloaded
versions under `~/.context-pilot/releases/{tag}/`.

Today the cockpit exposes a **Releases** pane (admin-only in the UI) that lets the
client download/select/delete release binaries directly. Two problems:

1. **Policy** — clients should not choose their own software versions. Version choice
   is ours; the client receives (cleanly) whatever the channel decides.
2. **Security gap** — the backend release routes
   (`transport/rest/releases.rs`) require a Bearer token but perform **no admin-role
   check** (no `auth_user` passed, unlike `settings.rs` / `users.rs`). Any
   authenticated user can currently call them by bypassing the frontend.

This document fixes the target update model.

---

## §2 — Decision

**The client never chooses a version.** Updates follow a centrally-declared,
device-pulled model with atomic apply and automatic rollback.

This supersedes the "let the Admin pick a binary" behaviour of the current Releases
pane. It is consistent with the fleet strategy already frozen on 2026-06-27
(`updates push→pull`).

---

## §3 — Beyond "push vs pull"

"The device pulls" vs "we push from a server" is a false dilemma. The dominant model
(Mender, balena, AWS IoT / Azure Device Update, Tesla, Android, k8s-at-the-edge)
separates two concerns:

- **Intent is centralised and declarative** — *we push the desired state*: "client X
  should be on version Y of the `stable` channel."
- **Transfer is device-initiated** — *the box pulls the artifact*: each appliance
  polls a manifest, compares against its current version, and converges.

This is GitOps / reconciliation applied to the edge. We keep control (we decide who
runs what) without the fragility of direct push.

**Why pull wins for a fleet:** direct push (SSH/Ansible) assumes the box is online at
exactly that moment, behind NAT, and handles partial failure poorly (50 boxes, 3
offline, 1 bricks). Pull is NAT-resilient, asynchronous, and scales. Direct push over
Tailscale SSH stays excellent for **day-0** and **break-glass**, not routine updates.

---

## §4 — The five state-of-the-art properties

1. **Centralised desired-state** — a signed manifest (channel → version, per-client /
   per-tag override). Can be as simple as a signed JSON in the GitHub release or a
   small endpoint on our server.

2. **Atomic apply + health-gated automatic rollback** — the #1 reliability property.
   The device applies, runs a self-test, and **reverts itself** if the health check
   fails. Without this, one bad update bricks the fleet remotely. The heavyweights use
   A/B partitions (RAUC, Mender, swupdate, OSTree); for a single binary we already
   have 90%: `ReleaseStore` keeps N versions → swap symlink → restart under procd →
   health check → revert symlink on failure.

3. **Phased rollout (canary / rings)** — never 100% at once. 1 pilot box → cohort →
   fleet, with a health gate between stages (hawkBit, Mender phased deployments).

4. **Signed artifacts** — signature verified *on the device* before swap, so a
   compromised update server cannot push malware. Formal reference: TUF / Uptane. At
   minimum: sign releases, the box verifies before activating.

5. **Visibility, not choice, for the client** — the Admin sees version/channel and may
   at most *defer* or schedule a maintenance window. They never pick an arbitrary
   version.

### Tooling landscape (for reference)

| Tool | Model | Relevance |
|------|-------|-----------|
| Mender / RAUC+hawkBit / swupdate | A/B image, pull | Embedded standard, but heavy — built to swap the whole OS image |
| balena | Containers, pull | Full platform, too much for a single binary |
| OpenWrt ASU / `owut` / sysupgrade | Pull, OS image | Relevant for the *OS* layer, not our app |
| TUF (go-tuf / theupdateframework) | Secure distribution spec | Reference for the signing layer |

Since we run a **single static binary** on OpenWrt (no OS image to swap), a full
RAUC/Mender is over-dimensioned. The existing `ReleaseStore` + symlink + procd restart
+ health check is the right weight. The gaps to fill are: **signing, central
desired-state manifest, health-gated rollback, phased rollout**.

---

## §5 — Target design for Context Pilot

1. **Remove version choice from the cockpit.** The Releases pane becomes read-only for
   the Admin (current version, channel, history). Drop arbitrary download/select/delete
   — or keep them behind an admin-only **break-glass** flag. This also closes the
   backend RBAC gap, since the mutating routes are no longer exposed to the client.

2. **On-box update agent** (procd timer, periodic poll): reads a signed
   `channel → version` manifest, compares, downloads, **verifies the signature**,
   atomically swaps, restarts, health-checks, reverts on failure. Reuses most of
   `services/releases/`.

3. **Minimal control plane on our side.** Initially a signed JSON per client/tag
   (published on GitHub or our server). Push stays over Tailscale **only for day-0 and
   break-glass** (existing Ansible).

4. **Ring-based rollout** later: a `ring` field in the manifest; promote to `stable`
   after validation on a pilot box.

**In one line:** desired-state declared by us, artifact pulled by the box, atomic apply
with rollback, mandatory signing. The client never chooses — it receives (cleanly)
whatever the channel decides.

---

## §6 — Implementation delta (to scope)

- [ ] Make Releases pane read-only (or break-glass gated); pass `auth_user` +
      `can_admin()` to the release routes regardless, to close the RBAC gap.
- [ ] Define the signed manifest format (channel, version, per-tag override, ring).
- [ ] On-box update agent: poll → verify signature → atomic swap → health check →
      rollback (procd timer).
- [ ] Release signing in CI + on-device public-key verification.
- [ ] Health-check contract (what "healthy after restart" means).
- [ ] Ring/canary promotion flow.

---

## §7 — Related

- `deploy/PROVISIONING.md` — canonical provisioning / fleet doc (Tailscale overlay,
  day-0 manual, push→pull updates).
- `docs/design-auth.md` — auth & RBAC (the `can_admin()` pattern referenced in §5).
