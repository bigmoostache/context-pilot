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
| H10-1 | **none (PASS)** | Full lock-site audit (grep of every `.lock()`/`.try_lock()` on `op_lock`/`conn`/`shared` in `cp-mod-browser`). Acquisition sites: **op_lock** — `tools.rs:194` only (worker, outermost, held whole op). **conn** — `client.rs:79` in `connect_shared` (acquires, `is_alive`/`Client::connect`, then `drop(slot)` / returns → guard released BEFORE the op runs) + `types.rs:174` `clear_session` (main thread, `try_lock`, dropped immediately). **shared** — `tools.rs` OpSink `note_shared`/`note_nav`/`write_snapshot`/`resolve` (worker, under op_lock, shared-only) + `note_shared_main` (main, shared-only) + `panel.rs:50` `build_content` (render, shared-only) + `types.rs:177` `clear_session` (main, try_lock, sequential AFTER conn — not nested). | **PROVEN ACYCLIC (source)** — 2026-06-11. | **NO FIX NEEDED.** The decisive fact: `connect_shared` releases `conn` (hit-path return drops the guard; miss-path explicit `drop(slot)`) **before** returning the `Arc<Client>`, so by the time the op runs and OpSink locks `shared`, `conn` is no longer held → **conn and shared are NEVER nested.** The only nesting edges are `op_lock→conn` and `op_lock→shared`, with op_lock always outermost and acquired alone. No `shared→conn` inversion exists (no shared-holder ever calls `connect_shared`). Lattice: `op_lock → {conn, shared}`, conn/shared are leaves → a DAG, no cycle → **deadlock impossible** among the three locks. |
| H10-2 | **S4 (note)** | `connect_shared` holds `conn` across the blocking `Client::connect` handshake + `is_alive` CDP round-trip. | **BENIGN (source)** — 2026-06-11. | Not a deadlock: `conn` is a leaf (above only headless_chrome's internal `get_tabs` mutex). Inter-worker `conn` contention is impossible because `op_lock` serializes workers — only one worker is ever in `connect_shared` at a time. The only other `conn` toucher is `clear_session` via **`try_lock`** (P08 fix), which can't block. So the long conn-hold never freezes anything. Render liveness: workers hold `shared` only for a memcpy-class critical section (no I/O under `shared`), so render-thread `build_content` wait is bounded (X776 priority-inversion safe). |

## Exit criterion
A proven-acyclic lock-order lattice; no deadlock under 1h chaos; documented
lock-hold budgets; render never starved beyond one op's `shared` write window.
