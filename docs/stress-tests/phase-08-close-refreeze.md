# P08 — close/kill during in-flight op (re-freeze)

**Todo:** X565 · **Primary hazard:** main-thread re-freeze (the bug we removed)

## Objective
`browser_close` runs **synchronously on the main thread** and
`kill_chrome → clear_session` does a blocking `conn.lock()`. If a worker holds
`conn` mid-connect, the main thread blocks — reintroducing the very freeze the
refactor eliminated. Reproduce and quantify it.

## Targeted hazard
`tools.rs::close` is not async; it calls `lifecycle::kill_chrome(bs)` →
`BrowserState::clear_session()` → `self.conn.lock()`. A worker in `connect_shared`
holds `conn` across `Client::connect` (a CDP handshake that can take seconds or
hang). `close`'s `conn.lock()` then blocks the main loop. `clear_session` uses
`if let Ok` (won't deadlock on poison) but **does** block on a held lock.

## Subtasks

### [M] Medium
- **X718** `browser_close` with no op in flight; clean + fast.
- **X719** `browser_close` after an op completes; `conn` cleared.
- **X720** Time `browser_close` latency baseline (< 50ms).
- **X721** close then immediate reopen; fresh session.
- **X722** close locks `conn` via `clear_session`; confirm the path.

### [H] Hard
- **X723** close **while** a `goto` worker holds `conn` mid-connect; main blocks. *Reproduce.*
- **X724** Measure the re-freeze duration during a contended close.
- **X725** close during a slow `connect_shared` (Chrome hung 8s).
- **X726** `kill_chrome` `take(handle)` vs worker still using `conn`.
- **X727** close mid-op: does the worker error "connection lost" cleanly?

### [V] Very hard
- **X728** close + op_lock contention; ordering of teardown.
- **X729** Panel removed mid-op; worker writes `shared` to a dropped panel.
- **X730** close during a 30s-timeout zombie; lock interplay.
- **X731** Rapid close/open while ops queued; consistency.
- **X732** `on_close_context` (panel close) path vs the `browser_close` tool.

### [X] Extreme
- **X733** Worst-case close freeze: hung connect + close; quantify.
- **X734** Propose `try_lock`/timeout fix for close to avoid re-freeze.
- **X735** close + reload + reopen interleaving fuzz.
- **X736** Verify Chrome process actually dies under every close race.
- **X737** Double-close + op in flight; no panic, no leak.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H08-1 | **S1** | `tools.rs::close` runs synchronously on the main thread → `lifecycle::kill_chrome` → `BrowserState::clear_session` (`types.rs`) did a **blocking** `self.conn.lock()` (`if let Ok(mut slot) = self.conn.lock()` — the `if let Ok` guards POISON, not contention; `.lock()` still blocks on a held lock). A worker in `connect_shared` holds `conn` across `Client::connect` / `is_alive` (CDP round-trip, up to the 8s op timeout, unbounded if Chrome is hung) → the main loop blocks for that window. **The exact freeze the refactor removed, reachable via close during a worker's connect window.** | **CONFIRMED then FIXED+VERIFIED** — 2026-06-11. | **FIXED** (`types.rs::clear_session`): switched both `conn` and `shared` to `try_lock`. `try_lock` cannot block → the main loop is never stalled by a worker holding `conn`. On contention the connection drop is abandoned (safe: `kill_chrome` is killing Chrome anyway, the worker's op fails out and releases `conn`, and the stale `Arc<Client>` is replaced on the next op's `connect_shared` via `is_alive`→false→reconnect). Live-verified no regression: close clean+fast, reopen spawns fresh. The S1 freeze is eliminated by construction. |

## Exit criterion
`browser_close` never blocks the main loop > 50ms regardless of in-flight ops;
Chrome always dies; a mitigation (async close or non-blocking teardown) verified.
