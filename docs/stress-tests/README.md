# Browser Async Module — Stress-Testing Roadmap

> Target: the **off-main-thread browser module** (branch `browser`, commit `a6ed47a`).
> Goal: **find as many points of failure as possible BEFORE deployment.**
> Scope: 20 phases × 20 subtasks = **400 stress tests**, ~2–3 weeks of work for
> experienced stress-testers.

## What changed (the system under test)

Every slow CDP op now runs on a **worker thread** via `spawn_async_tool`
(`cp_base::tools::async_exec`); results return through the `ChannelWatcher`
pipeline. `BrowserState` shares three pieces of state across threads:

| Field | Type | Owner / role |
|-------|------|--------------|
| `meta` / `handle` | plain | **main thread** — Chrome process lifecycle + persistence |
| `conn` | `Arc<Mutex<Option<Arc<Client>>>>` | cached CDP connection, reused/reconnected by workers under the slot lock |
| `shared` | `Arc<Mutex<SharedBrowser>>` | worker-written runtime data: erefs, eref_selectors, snapshot_text, last_action, url, title |
| `op_lock` | `Arc<Mutex<()>>` | serializes CDP ops on the single transport |

This new concurrency surface is the primary attack target. Functional CDP
correctness (eval/click/type/extract semantics) was already swept in
scratchpad C19 / C20 — **these phases focus on the THREADING and ASYNC-DELIVERY
failure modes the refactor introduces**, plus their interaction with reload,
persistence, watchers, spine/guard-rails, panels, and multi-tool turns.

## Difficulty tiers (per phase: 5 each)

- **[M] Medium** — single-op, deterministic, observable by eye.
- **[H] Hard** — requires contention/timing or measurement instrumentation.
- **[V] Very hard** — multi-actor races, needs harness + injection + proof.
- **[X] Extreme** — soak/chaos/formal; long-running; CI sign-off gates.

## Top a-priori hazards (hypotheses to confirm/refute)

These are the failure modes suspected from a code read — each phase tries to
**prove or disprove** them:

1. **Timeout/zombie race (P03/P11):** an op exceeding the 30s `spawn_async_tool`
   timeout — the watcher fires a timeout result and self-removes, but the worker
   keeps running, still holds `op_lock`, and later writes `shared` (stale clobber).
2. **conn-slot poisoning (P04):** a panic while a worker holds the `conn` Mutex
   (even one caught by `catch_panic`) poisons it permanently;
   `clear_session`/`kill_chrome` use `if let Ok(...)` so they silently skip —
   **no in-session recovery, only reload.**
3. **close re-freeze (P08):** `browser_close` runs synchronously on the main
   thread and `clear_session` does a blocking `conn.lock()`. If a worker holds
   `conn` mid-connect, the main thread blocks — **reintroducing the freeze.**
4. **Same-turn ordering (P14):** `resolve()` reads `shared` erefs on the main
   thread at dispatch time; a `snapshot`+`click` in one turn can resolve `click`
   **before** the snapshot worker writes erefs → `Unknown ref`.
5. **Thread pile-up (P05):** `spawn_async_tool` spawns one **unpooled** thread per
   call; ops blocked on `op_lock` hold parked threads — burst → thread/FD growth.
6. **Reload-mid-op orphan (P06):** the `ChannelWatcher` is runtime-only; a reload
   drops it, leaving a `tool_use` with no `tool_result` → potential API-400.
7. **Artifact collision (P15):** `artifact_path` is `now_ms()`-named; two ops in
   the same millisecond overwrite each other.

## Severity scale (for Findings tables)

- **S0 Crash/abort** — TUI aborts, terminal corrupted, data loss.
- **S1 Hang/deadlock/freeze** — UI unresponsive, requires kill.
- **S2 Correctness** — wrong page acted on, stale state, lost result, API-400.
- **S3 Degraded** — slow, leak, recoverable error mislabelled.
- **S4 Cosmetic** — confusing message, minor UX.

## Phase index

| # | Phase | Primary hazard | Todo |
|---|-------|----------------|------|
| [P01](phase-01-baseline.md) | Baseline async responsiveness & invariants | freeze regression | X558 |
| [P02](phase-02-op-lock.md) | op_lock serialization & op ordering | reordering/starvation | X559 |
| [P03](phase-03-timeout-zombie.md) | Timeout vs slow-op zombie-worker race | op_lock held by zombie | X560 |
| [P04](phase-04-poison.md) | conn-slot Mutex poisoning & recovery | unrecoverable poison | X561 |
| [P05](phase-05-thread-exhaustion.md) | Worker-thread exhaustion & burst | thread/FD leak | X562 |
| [P06](phase-06-reload-midflight.md) | Reload mid-flight: orphaned tool_use | API-400 | X563 |
| [P07](phase-07-persistence-reconnect.md) | Persistence & reconnect races | stale ws_url / double-spawn | X564 |
| [P08](phase-08-close-refreeze.md) | close/kill during in-flight op | main-thread re-freeze | X565 |
| [P09](phase-09-connection-lifecycle.md) | Connection lifecycle under load | reconnect storm / hung probe | X566 |
| [P10](phase-10-deadlock.md) | Lock contention & deadlock hunting | lock-order inversion | X567 |
| [P11](phase-11-stale-write.md) | Stale-write race: zombie clobbers state | cross-op corruption | X568 |
| [P12](phase-12-render-contention.md) | Panel/render contention during ops | render stall / torn read | X569 |
| [P13](phase-13-spine-guardrails.md) | Spine / tempo / guard-rail interaction | stranded tool_use | X570 |
| [P14](phase-14-multitool-turn.md) | Multi-tool turn correctness | premature resolve() | X571 |
| [P15](phase-15-artifact-collision.md) | Artifact file-write collisions & disk | filename clobber | X572 |
| [P16](phase-16-huge-payloads.md) | Huge/exotic payloads through the channel | OOM / truncation | X573 |
| [P17](phase-17-error-fidelity.md) | Error-propagation fidelity | orphan/mislabelled result | X574 |
| [P18](phase-18-resource-leaks.md) | Resource leaks over long sessions | thread/Chrome/FD/mem leak | X575 |
| [P19](phase-19-chrome-faults.md) | Chrome-side adversarial faults | crash → TUI fault | X576 |
| [P20](phase-20-chaos.md) | Chaos / combinatorial fuzzing | emergent | X577 |

## Per-phase document structure

Each `phase-NN-*.md` contains:
1. **Objective** — what this approach tries to break.
2. **Targeted hazard** — the specific code path / invariant under attack.
3. **Setup / tooling** — harnesses, fault injectors, instrumentation needed.
4. **Subtasks** — the 20 tests, tagged `[M]/[H]/[V]/[X]`, each with *method* and
   *pass criterion / watch-for*.
5. **Findings** — a living table: `ID | severity | repro | status | fix/issue`.
6. **Exit criterion** — what "phase green" means.

## Working agreement

- One markdown doc per phase, **updated as findings land** (not at the end).
- Every confirmed break gets: minimal repro, severity, and a linked fix or issue.
- Prefer **deterministic harnesses + seeds** over manual repro for [V]/[X].
- Deployment is **gated** on P20 chaos green for 24h with zero S0/S1/S2.
