# P09 — Connection lifecycle under load

**Todo:** X566 · **Primary hazard:** reconnect storm / hung is_alive probe under lock

## Objective
Stress the idle self-close of the transport, reconnect-while-busy, and the
`is_alive()` probe (a CDP round-trip) executed while Chrome is hung — all while
holding the `conn` lock. Verify cached-vs-reconnect correctness under concurrency.

## Targeted hazard
`connect_shared` holds `conn`, checks `existing.is_alive()` (a real CDP round-trip),
and reconnects on failure. The headless_chrome transport self-closes after idle
(`open` AtomicBool flips false permanently). Under concurrency: multiple first-ops
can stampede the reconnect; `is_alive()` against a hung Chrome blocks while holding
`conn` (→ P08 close freeze, → P10 lock-hold).

## Subtasks

### [M] Medium
- **X738** Idle 60s+; transport self-closes; next op reconnects.
- **X739** `is_alive()` returns false on an idle-closed transport.
- **X740** Cached client reused when alive (no reconnect).
- **X741** Reconnect transparent to the LLM (op just works).
- **X742** `connect_shared` returns the cached `Arc` on the warm path.

### [H] Hard
- **X743** `is_alive()` while Chrome hung; probe blocks holding `conn`.
- **X744** Reconnect-while-busy: an op during another op's reconnect.
- **X745** Transport closes mid-op; single hard-fail then recover.
- **X746** `is_alive` false-positive (alive transport, dead tab).
- **X747** Reconnect to the same ws_url after idle; tab state preserved?

### [V] Very hard
- **X748** Idle-close + concurrent ops; thundering-herd reconnect.
- **X749** `connect_shared` double-connect race (both see `None`).
- **X750** `is_alive` cost measured; round-trip under the lock budget.
- **X751** Reconnect storm: 10 ops after idle; only one reconnects.
- **X752** Stale `Arc<Client>` lingering after reconnect; old one drops.

### [X] Extreme
- **X753** Chrome paused (`SIGSTOP`) mid-op; `is_alive` + timeout behavior.
- **X754** Network partition to the CDP socket; reconnect + timeout matrix.
- **X755** 1000 idle/reconnect cycles; connection-cache correctness.
- **X756** Prove exactly-one reconnect under concurrent first-ops.
- **X757** Reconnect + poison + timeout combined chaos.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H09-1 (suspected) | ~~S1~~ → **S3** | is_alive() on hung Chrome blocks holding conn → cascades to close-freeze | **CONFIRMED but RECLASSIFIED S1→S3 (source)** — 2026-06-11. `is_alive()` (`client.rs`) = `tab.evaluate("1", false)` — a CDP round-trip — and `connect_shared` calls it **while holding the `conn` slot lock**. BUT: (1) connect_shared runs only on the **worker thread** (inside `run_browser_op`, under `op_lock`); the **main thread never calls it** — the only main-thread conn-toucher is `clear_session` via **`try_lock`** (P08 fix), which can't block. So a hung `is_alive` blocks the *worker*, not the main loop — **no UI freeze**. (2) It's **bounded**: `Client::connect` sets `tab.set_default_timeout(OP_TIMEOUT=8s)`, so `evaluate` fails (→ `false`) within ~8s and connect_shared reconnects; the worker's 30s watcher also bounds it. So it degrades to the **same throughput-stall already tracked as P03 H03-1** (a worker holding `op_lock` stalls subsequent workers), not a new freeze. | **NO NEW FIX** — subsumed by P03 H03-1 residual (bounded op timeout < watcher timeout). The conn-hold itself is benign (P10 H10-2): conn is a lock leaf, inter-worker conn contention is impossible because op_lock serializes workers (only one in connect_shared at a time). |
| H09-2 | **none (PASS)** | Exactly-one reconnect under concurrent first-ops (X749/X751/X756). | **PROVEN (source)** — 2026-06-11. | Two first-ops can't both see `conn=None`: `op_lock` serializes workers, so only ONE worker is ever in `connect_shared` at a time. The first connects + caches into the slot; the second (after op_lock releases) finds the cached `Arc<Client>`, `is_alive()`→true, and **reuses** it. No thundering-herd reconnect, no double-connect. Stale old `Arc<Client>` is dropped when the slot is overwritten on a miss-path reconnect. |

## Exit criterion
Exactly-one reconnect under concurrent first-ops; `is_alive` bounded so a hung
Chrome never blocks the lock indefinitely; cache correctness over 1000 cycles.

**Status (source):** core invariants MET. Exactly-one reconnect is guaranteed by
`op_lock` serializing workers (only one in `connect_shared`). `is_alive` is
bounded by the tab's 8s default timeout (and the 30s watcher), so a hung Chrome
never blocks indefinitely — and it blocks only the worker, never the main loop.
The residual worker-side throughput-stall is the P03 H03-1 item. The 1000-cycle
soak (X755) and live SIGSTOP matrix (X753) remain as optional live confirmation.
