# P10 — Lock contention & deadlock hunting

**Todo:** X567 · **Primary hazard:** lock-order inversion / nested-hold deadlock

## Objective
Audit and stress every lock-acquisition path (`op_lock`, `conn`, `shared`) for
ordering inversions and nested-hold deadlocks. `connect_shared` holds `conn`
across blocking I/O inside `op_lock`. Hunt for any `shared → conn` inversion.

## Targeted hazard
Known acquisition orders:
- worker op: `op_lock` → (`conn` inside `connect_shared`) → (`shared` via `note_nav`)
  = `op_lock > conn > shared` (outer→inner).
- `resolve()` (main thread): `shared` only.
- `clear_session`: `conn` then `shared` **sequentially** (not nested).
- panel `build_content`: `shared` only.

No path currently locks `shared` then `conn` (which would invert against the
worker). This phase **proves** that absence (or finds the inversion).

## Subtasks

### [M] Medium
- **X758** Audit: enumerate every `lock()` of op_lock/conn/shared.
- **X759** Map the lock-acquisition order per call path.
- **X760** Confirm worker order is `op_lock > conn > shared` (outer→inner).
- **X761** `resolve()` locks `shared` only (main thread); verify.
- **X762** `clear_session` locks `conn` then `shared` sequentially (not nested).

### [H] Hard
- **X763** Hunt any `shared → conn` inversion across the crate.
- **X764** `connect_shared` holds `conn` across blocking `Client::connect`.
- **X765** Panel `build_content(shared)` vs worker `(shared)` convoy.
- **X766** Main-thread `resolve(shared)` vs worker `note_nav(shared)` contention.
- **X767** `close(conn)` vs worker `connect_shared(conn)` ordering.

### [V] Very hard
- **X768** Two workers: A holds op_lock+conn, B waits op_lock; no deadlock.
- **X769** Render thread + worker + main all touch `shared`; liveness.
- **X770** Lock-hold-time profiling for each lock under load.
- **X771** Inject an artificial slow-path to widen race windows.
- **X772** ThreadSanitizer / loom-style model of the 3 locks.

### [X] Extreme
- **X773** Construct a deadlock attempt; prove impossible or find it.
- **X774** Stress all paths concurrently 1h; watchdog for stalls.
- **X775** Formal lock-order lattice; prove acyclic.
- **X776** Priority-inversion: render starved by a worker holding `shared`.
- **X777** Chaos lock fuzz: random hold-times + ops + render.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| _none yet_ | | | | |

## Exit criterion
A proven-acyclic lock-order lattice; no deadlock under 1h chaos; documented
lock-hold budgets; render never starved beyond one op's `shared` write window.
