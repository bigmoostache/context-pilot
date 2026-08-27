# Design — Fleet-Shared Behaviours (home-dir storage)

> Status: **SHIPPED** (T651, built + reloaded, migration verified live). Functional
> requirements only — **no NFR** (multi-host semantics, locking, perf out of scope here).
>
> **Locked decisions (T651 form):** on a differing id collision the **already-shared
> file keeps the plain id** and the **incoming local file gets the `-<n>` suffix**;
> orchestrator + agents share the **same `$HOME`** (single-host mac-mini); emptied
> per-realm dirs are **left in place**; work landed on branch
> `t648-send-refreshes-threads-panel`. Migration verified: agents/skills/commands
> moved to `~/.context-pilot/behaviours/`, local realm dirs drained.

## 1. Intent

Today agents, skills, and commands are stored **per-realm** as `.md` files
under each agent's own folder (`<realm>/.context-pilot/{agents,skills,commands}/`).
They resolve there because `cp-mod-prompt/storage.rs::dir_for()` builds from the
**relative** `STORE_DIR = "./.context-pilot"` and the agent's cwd is its realm.

This rework moves **all three behaviour types to one fleet-shared home
directory** so every agent on the host sees the same behaviours:

```
~/.context-pilot/behaviours/agents/<id>.md
~/.context-pilot/behaviours/skills/<id>.md
~/.context-pilot/behaviours/commands/<id>.md
```

Built-ins (compiled from `yamls/library.yaml`) are unchanged — still merged at
load, disk overrides by id. Only the **disk location** moves, from per-realm to
shared-home.

## 2. Functional requirements (the mandate)

- **FR1 — Home-shared storage.** All agents/skills/commands live under
  `~/.context-pilot/behaviours/{agents,skills,commands}/<id>.md`, shared by the
  whole fleet on the host.
- **FR2 — One-shot migration.** On boot, read each per-realm behaviour file,
  **export it to the shared dir, then delete the local copy.**
- **FR3 — Collision handling.** If an id already exists in the shared dir:
  - **identical bytes** → do nothing, just delete the local copy;
  - **different bytes** → write the incoming local one under a **new suffixed
    id** (`<id>-1`, `<id>-2`, …) in the shared dir (the already-shared file
    keeps the plain id, untouched), then delete the local copy.
- **FR4 — Direct edits hit the shared file.** A tool-call edit (`Edit` on a
  behaviour `.md`, `Behaviour_create`, orchestrator library CRUD) operates
  **directly on the shared-home version**.
- **FR5 — Tool prompting points at the raw files.** Tool/param descriptions and
  the Library panel state where the raw files live
  (`~/.context-pilot/behaviours/…`) and that they are fleet-shared.

## 3. Where the change lands

### 3.1 One resolver in `cp-base` (shared by agent + orchestrator)

New in `cp-base/src/config/constants.rs`:

```rust
/// `~/.context-pilot/behaviours` — the fleet-shared behaviour root. Falls back
/// to `./.context-pilot/behaviours` when `$HOME` is unset (dev/test).
pub fn home_behaviours_dir() -> PathBuf
```

Resolves `$HOME` exactly like the registry's `default_agents_dir` (proven
single-host assumption: agent + orchestrator share `$HOME`, M-long-8). Both
crates already depend on `cp-base`, so no new dependency edge.

### 3.2 Agent side — one line flips everything

`cp-mod-prompt/storage.rs::dir_for(pt)` →
`home_behaviours_dir().join(subdir_for(pt))`. **Every** agent-side reader/writer
funnels through `dir_for()` / `load_prompts_for()`, so this single change
switches: `behaviour_create` (write), `agent_load`/`skill_load` (read),
`preflight_behaviour_create`, `is_prompt_file` (the `Edit`-guard — now matches
the home path, so FR4 for `Edit` works automatically), the Library panel
cheat-sheet paths, and every `src/` reader.

### 3.3 Orchestrator side — explicit path builders

Both build behaviour paths from `entry.folder` today; both switch to
`home_behaviours_dir()`:
- `transport/rest/create.rs` — `read_/upsert_/delete_library_agent`,
  `create_command` (4 handlers). `resolve_entry` stays (ACL), only the path
  source changes.
- `transport/inspect/panels.rs::library()` — the per-agent disk scan
  (`<folder>/.context-pilot/<subdir>`) becomes the shared scan. The listing is
  now fleet-wide identical, which is the point.

### 3.4 Migration — `storage::migrate_local_to_shared()`

Run once at agent boot (idempotent — after it runs, local dirs are empty, so
re-runs are no-ops). Per `<realm>/.context-pilot/<subdir>/*.md`:

1. target = `home/behaviours/<subdir>/<id>.md`.
2. target absent → write to home, delete local.
3. target present → compare **bytes** (no hash dep — bytewise equality *is* the
   "same hash" test):
   - equal → delete local only;
   - differ → allocate `<id>-{n}` (lowest free), write there, delete local.

Empty per-realm dirs are left in place (harmless; avoids dir-removal races).

### 3.5 Tool prompting (FR5)

`yamls/tools/prompt.yaml` (`Behaviour_create` + param descriptions) and the
Library panel note the shared-home location + fleet-shared semantics. The
panel's path lines already derive from `dir_for()`, so they update for free.

## 4. Dependency / cycle check

`home_behaviours_dir()` lives in `cp-base` (leaf). `cp-mod-prompt` and
`cp-orchestrator` already depend on `cp-base`. **No new edge, no cycle.**

## 5. Open questions (confirm before build)

1. **Suffix direction (FR3).** On a differing collision, the **incoming
   local** file takes the `-{n}` suffix and the **already-shared** file keeps
   the plain id. Confirm (vs. the reverse).
2. **Orchestrator HOME == agent HOME.** True on the single-host mac-mini setup
   (same as the registry's existing assumption). Confirm no split-HOME
   deployment is intended.
3. **Delete emptied local dirs?** Proposed: leave the now-empty per-realm
   `{agents,skills,commands}/` dirs (harmless). Confirm or request cleanup.

## 6. Surface touched (informational)

- `cp-base/config/constants.rs`: `home_behaviours_dir()`.
- `cp-mod-prompt/storage.rs`: `dir_for` → home; new `migrate_local_to_shared()`;
  called at boot (`seed::ensure_default_agent` or module init).
- `cp-mod-prompt/lib.rs`: `is_prompt_file` follows `dir_for` (no change needed).
- `cp-orchestrator/transport/rest/create.rs` + `inspect/panels.rs`: path source
  → `home_behaviours_dir()`.
- `yamls/tools/prompt.yaml` + Library panel copy: point at raw shared files.
