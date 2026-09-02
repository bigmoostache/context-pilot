# Migration: runtime artifacts → `~/.context-pilot/sync/<agent-id>/`

**Status:** proposal (T713). Exploration + plan only — no code changed yet.
**Decision (T713):** the sync plane moves to a **global, per-agent-namespaced**
directory in `$HOME`, not a per-realm subdir. Per-realm is kept below as the
alternative that was considered.

## 1. Goal

Move the five per-realm *runtime* artifacts out of the realm **root** and into a
single **global, per-agent** directory, `~/.context-pilot/sync/<agent-id>/`:

| Artifact       | Today (realm root)     | Proposed (global, per-agent)                        |
| -------------- | ---------------------- | --------------------------------------------------- |
| `bridge.lock`  | `<realm>/bridge.lock`  | `~/.context-pilot/sync/<id>/bridge.lock`            |
| `heartbeat`    | `<realm>/heartbeat`    | `~/.context-pilot/sync/<id>/heartbeat`              |
| `stream.sock`  | `<realm>/stream.sock`  | `~/.context-pilot/sync/<id>/stream.sock`            |
| `tee.sock`     | `<realm>/tee.sock`     | `~/.context-pilot/sync/<id>/tee.sock`               |
| `oplog/`       | `<realm>/oplog/`       | `~/.context-pilot/sync/<id>/oplog/`                 |

`<id>` is the agent's stable identifier — the **FNV-1a hash of the canonical
realm path** (`register::identity::folder_id`), the *same* id the registry
already uses for `~/.context-pilot/agents/<id>.json`. So the sync plane sits
right next to the discovery record it belongs to, and is consistent with what is
*already* global.

### Why per-agent namespacing is mandatory (not a flat shared dir)

One host runs **many** agents (one per realm). A single flat
`~/.context-pilot/sync/` would collide catastrophically:

- `bridge.lock` would become a whole-**host** gate instead of a per-realm gate —
  the second agent on the box could never boot. This breaks multi-agent entirely.
- Two agents cannot share one `oplog/`, one `heartbeat`, or one socket.

So the `<agent-id>/` level is load-bearing, not cosmetic.

### Why global beats per-realm

- ✅ **Eliminates the socket path-length hazard.** `sun_path` caps at ~104 bytes
  (macOS) / ~108 (Linux). A per-realm `<realm>/.context-pilot/sync/tee.sock`
  inherits the realm's (possibly deep) absolute path and can overflow. The global
  path `~/.context-pilot/sync/<16-hex>/tee.sock` (≈ 55 chars for a typical
  `$HOME`) is **bounded and short regardless of realm depth**. This was the top
  hazard of the per-realm plan; global retires it.
- ✅ **The user's project tree stays completely clean** — nothing is written into
  the realm folder at all.
- ✅ **One directory to tmpfs-mount / wipe** for the whole host's ephemeral
  coordination state.
- ✅ **Consistent with the existing global registry** (`~/.context-pilot/agents/`).

### The costs of global (both addressed below)

- ⚠️ **Orphaned sync dirs** when a realm is deleted (§4e — needs a reaper).
- ⚠️ **Forces the wire change** (`tee_socket_path`, §3) — the tee socket is no
  longer under `entry.folder`, so by-name reconstruction is impossible.

`oplog/bodies/` moves for free — it is always derived as `oplog_path.join("bodies")`
on both sides (`cp-mod-bridge/src/body.rs`, `cp-orchestrator/.../channel.rs`), so
moving the whole `oplog/` dir keeps the content-addressed body store intact.

## 2. Who owns / reads each path (verified, current state)

All five artifacts today live directly at `entry.folder` (the canonicalized agent
cwd = realm root).

### Agent side — `crates/cp-mod-bridge` (writer of all five)

| Artifact      | Constant / site                                             |
| ------------- | ---------------------------------------------------------- |
| `bridge.lock` | `boot/lock.rs::LOCK_FILE`, `folder.join(LOCK_FILE)` in `acquire_lock` |
| `oplog/`      | `boot/mod.rs::OPLOG_DIR`, `canonical.join(OPLOG_DIR)`      |
| `stream.sock` | `boot/mod.rs::SOCKET_FILE`, `canonical.join(SOCKET_FILE)`  |
| `heartbeat`   | `boot/mod.rs::HEARTBEAT_FILE`, `canonical.join(HEARTBEAT_FILE)` |
| `tee.sock`    | `boot/activate.rs::TEE_SOCKET`, `Path::new(&entry.folder).join(TEE_SOCKET)` |

The boot sequence (`boot/mod.rs::start_inner`) computes
`id = folder_id(&canonical)` early (before the lock) and writes the registry
`Entry` with the **absolute** paths it chose (`socket_path`, `oplog_path`,
`heartbeat_path`). Because the id is already available before any resource is
acquired, building `~/.context-pilot/sync/<id>/` at boot needs no new plumbing.

### Orchestrator side — `crates/cp-orchestrator` (reader)

The orchestrator is **almost entirely path-agnostic**: it reads absolute paths
verbatim from the registry `Entry` and never reconstructs them by name.

| Path             | How the orchestrator gets it                | Path-agnostic? |
| ---------------- | ------------------------------------------- | -------------- |
| command socket   | `entry.socket_path` → `registry/channel.rs` connect | ✅ yes |
| oplog dir        | `entry.oplog_path` → tailer, `metrics.rs`, `runtime/driver.rs`, `stream/upgrade.rs` | ✅ yes |
| body store       | `Path::new(&entry.oplog_path).join("bodies")` → `channel.rs` | ✅ yes (follows oplog) |
| heartbeat        | `entry.heartbeat_path` → `registry/liveness.rs`, `registry/mod.rs`, `vitals/mod.rs` | ✅ yes |
| **tee socket**   | **`PathBuf::from(&entry.folder)` then `TeeReader::spawn` joins `"tee.sock"`** (`runtime/driver.rs:158-161` + `registry/tee_reader.rs::TEE_SOCKET`) | ❌ **NO — reconstructed by name** |

Because `socket_path` / `oplog_path` / `heartbeat_path` are advertised as
absolute paths, moving them into `~/.context-pilot/sync/<id>/` needs **zero
orchestrator change and no wire change** — the orchestrator follows automatically.

## 3. The one hard coupling: `tee.sock` → `tee_socket_path` is now MANDATORY

`bridge.lock` is not in the registry at all (agent-internal single-process gate) —
moving it is agent-only, zero orchestrator coordination.

`stream.sock`, `oplog/`, `heartbeat` are advertised as absolute paths — the agent
puts them under `sync/<id>/` and the orchestrator follows automatically.

`tee.sock` is the exception: it is advertised **nowhere** yet reconstructed by
**both** sides from `entry.folder + "tee.sock"`. With the global location the tee
socket is **no longer under `entry.folder` at all**, so the "Option B" lockstep
rename that was possible for a per-realm move **is not available** here. The tee
path *must* become a first-class advertised field:

### Advertise `tee_socket_path` in the registry `Entry` (required)

Add a 15th field `tee_socket_path: String` to `cp-wire`'s `registry::Entry`. The
agent writes the absolute tee path (like it already does for the other three);
the orchestrator reads it verbatim and drops the by-name reconstruction. This
**removes the last name-coupling for good** and makes the migration
deploy-order-tolerant.

- `cp-wire` is the N-1 compat boundary. `Entry` deserialization is already
  tolerant of unknown fields (`registry_extra_fields_ignored` test), so an **old
  orchestrator** reading a **new** entry just ignores `tee_socket_path`. It would
  then fall back to `entry.folder + "tee.sock"` — which now points at nothing
  (the agent no longer writes there). Consequence: **an old orchestrator loses
  the live token-stream plane for a migrated agent** (the durable oplog remains
  the safety net, so it degrades quietly, not crashes). This makes the rollout
  order (§6) load-bearing: **ship the orchestrator first.**
- A **new orchestrator** reading an **old** entry (no field) must fall back to
  `entry.folder.join("tee.sock")`. Implement `tee_socket_path` handling as
  "use the field if non-empty, else reconstruct from folder" so a new
  orchestrator still serves un-migrated agents.
- `Entry` carries `#[expect(clippy::exhaustive_structs)]`; the field count in the
  justification comment (currently "14-field") must be updated to 15, and the
  round-trip + sample-entry tests extended.

## 4. Hazards

### 4a. Unix socket path length — RESOLVED by the global location ✅

This was the top risk of the per-realm plan (deep realm → `bind` `ENAMETOOLONG`
→ bridge boots OFF). The global path
`~/.context-pilot/sync/<16-hex-id>/{stream,tee}.sock` has a bounded length that
does **not** grow with realm depth, so the hazard is retired. (A pathologically
long `$HOME` is the only residual, and is not realistic; a cheap length preflight
on bind can still log a clear error rather than a raw errno.)

### 4b. Existing on-disk oplog must be migrated (now possibly cross-device) ⚠️

An existing agent has a populated `<realm>/oplog/` (durable rev history, `seen`
dedup set, body store). Pointing at the new location without moving it starts a
**fresh** oplog: command dedup resets, and the message chokepoint could re-emit a
`MessageCreated` backlog (double bubbles) until memos reseed.

Boot must perform a **one-time move**: if `~/.context-pilot/sync/<id>/oplog/` is
absent AND the old `<realm>/oplog/` exists, relocate it before opening.

- **`rename` is atomic and cheap only *within one filesystem*.** With the global
  location the source (realm, possibly on an external/removable drive or a
  different mount) and the destination (`$HOME`) can be on **different devices** →
  `rename` fails with `EXDEV`. Boot must therefore implement a **copy + fsync +
  remove fallback** for the cross-device case (walk the segments + `bodies/`,
  copy each, fsync, then remove the source). This is more prominent for the
  global plan than it was for the per-realm plan (where source and dest were
  always the same fs).
- Nothing else needs migration: the lock, sockets, and heartbeat are recreated
  fresh every boot.

### 4c. Directory creation + stale cleanup ordering

Boot must `create_dir_all(~/.context-pilot/sync/<id>)` **before** acquiring the
lock / binding sockets / opening the oplog. The existing stale-socket unlink
(`fs::remove_file` before `bind`) must target the new paths. `$HOME` must resolve
(reuse the same failure path the registry already has when `$HOME` is unset).

### 4d. Deploy trap (M177)

Ship the **orchestrator first** (it must understand `tee_socket_path` before any
agent starts advertising it — see §3 / §6). After the deploy, verify the live
stream plane (open a thread, confirm tokens stream) — a broken tee plane is
silent because the oplog masks it.

### 4e. Orphaned sync dirs — needs a reaper (new, global-only) ⚠️

Per-realm, deleting a realm folder deletes its sync artifacts with it. Global,
the `~/.context-pilot/sync/<id>/` dir **survives** the realm's deletion and
accumulates in `$HOME` with nobody to clean it.

Add a **reaper**: during a registry scan, a `sync/<id>/` directory whose `<id>`
has **no live registry entry** and **no live process** (and is older than a grace
period) is removed. There is a natural home for this next to the registry's
existing crash-orphan collector (`reap_tmp` in `registry/mod.rs`). Guard it with a
grace window so a booting agent that has created its sync dir but not yet written
its registry record is never reaped mid-boot.

## 5. Exact change set

**`cp-mod-bridge` (agent):**
- `register/identity.rs` (or `boot/mod.rs`): add a `sync_dir(id)` helper resolving
  `~/.context-pilot/sync/<id>` (reuse the `$HOME` resolution behind
  `registry::default_agents_dir` — e.g. a sibling `default_sync_dir(id)` that
  returns `<home>/.context-pilot/sync/<id>`).
- `boot/mod.rs::start_inner`: after computing `id`, `create_dir_all(sync_dir)`
  first; build `OPLOG_DIR`, `SOCKET_FILE`, `HEARTBEAT_FILE` **under `sync_dir`**;
  add the one-time oplog move with the cross-device copy fallback (§4b). The
  registry `Entry` then advertises the new absolute paths automatically.
- `boot/lock.rs`: `acquire_lock` takes/joins the lock under `sync_dir` (pass the
  sync dir instead of the realm folder).
- `boot/activate.rs`: `setup_tee` binds `tee.sock` under `sync_dir`; set
  `entry.tee_socket_path`.
- Update the module-doc path narration in `boot/mod.rs` + `lib.rs`.
- Update the affected unit tests (they assert `folder.join("stream.sock")` etc. —
  they'll now assert under a temp `sync_dir`; the tests already pass an explicit
  `agents_dir` tempdir, so give them an explicit `sync_dir` tempdir the same way).

**`cp-wire`:**
- `types/registry.rs`: add `tee_socket_path: String`; update the exhaustive-struct
  justification (14→15); extend the round-trip + sample-entry tests.

**`cp-orchestrator`:**
- `runtime/driver.rs` + `registry/tee_reader.rs`: use `entry.tee_socket_path` when
  non-empty, else fall back to `entry.folder.join("tee.sock")` (serves un-migrated
  agents). `TeeReader::spawn` takes the full socket path instead of a folder.
- Add the **orphan reaper** (§4e) near `registry/mod.rs::reap_tmp`.
- All other consumers (`channel.rs`, `metrics.rs`, `liveness.rs`, `vitals/mod.rs`,
  `stream/upgrade.rs`) need **no change** — they read absolute paths from the
  registry.

**Repo hygiene:**
- `.gitignore`: the realm-root entries for `bridge.lock`, `stream.sock`,
  `tee.sock`, `heartbeat`, `oplog` become unnecessary (nothing is written into the
  realm anymore) — they can be removed once the migration ships. `~/.context-pilot`
  is outside any repo, so no ignore rule is needed for the new location.
- `crates/cp-mod-tree/src/lib.rs:323` lists `"oplog"` in a tree-filter default —
  it can be dropped once the oplog no longer lives in the realm.

## 6. Rollout order

1. Ship the **orchestrator** first: it reads `tee_socket_path` when present and
   falls back to `entry.folder + "tee.sock"` otherwise. It still fully serves old
   (un-migrated) agents, and is ready for new ones.
2. Ship the **agent** second: it writes artifacts under
   `~/.context-pilot/sync/<id>/`, advertises the new absolute paths +
   `tee_socket_path`, and runs the one-time oplog move.
3. Verify: registry entry shows `sync/<id>/` paths; heartbeat/liveness green; open
   a thread and confirm live token streaming (tee plane); confirm command intake
   (send a message) still connects; confirm the realm folder no longer gains any
   runtime artifacts.

## 7. Open questions for the user

1. **Oplog migration (§4b):** auto-move the existing `oplog/` on first boot (with
   the cross-device copy fallback), or accept a fresh oplog on the new path
   (simpler, but resets dedup/rev history and risks a one-time `MessageCreated`
   backlog)?
2. **Reaper grace period (§4e):** what grace window before an entry-less
   `sync/<id>/` is reaped (e.g. only reap dirs with no registry entry AND
   untouched for > 24h)?

## Appendix — alternative considered: per-realm `<realm>/.context-pilot/sync/`

The original plan grouped the artifacts under the realm's existing
`.context-pilot/` directory. It is **self-contained** (deleting the realm deletes
everything, no reaper needed) and needs **no wire change** if `tee.sock` is moved
in lockstep on both sides. It was rejected because it **inherits the realm's
absolute path length** and so risks `bind` `ENAMETOOLONG` on deep realm folders —
the exact hazard the global location eliminates.
