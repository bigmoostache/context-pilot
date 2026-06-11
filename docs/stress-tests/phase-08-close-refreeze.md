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
| H08-1 (suspected) | **S1** | close while worker holds conn mid-connect → main thread freezes | _to confirm_ | make close async, or `try_lock` + deferred teardown |

## Exit criterion
`browser_close` never blocks the main loop > 50ms regardless of in-flight ops;
Chrome always dies; a mitigation (async close or non-blocking teardown) verified.
