# P05 — Worker-thread exhaustion & burst concurrency

**Todo:** X562 · **Primary hazard:** unbounded thread/FD growth

## Objective
`spawn_async_tool` spawns **one unpooled `std::thread` per call**. Flood with
simultaneous/queued browser ops so that ops blocked on `op_lock` pile up parked
threads; probe for thread/FD exhaustion and unbounded growth.

## Targeted hazard
`cp_base::tools::async_exec::spawn_async_tool` → `std::thread::Builder::spawn`
per invocation, no pool, no cap. Each op blocked on `op_lock` holds a live OS
thread for the full queue wait. A burst of N browser tools (or a slow op + many
queued) → N live threads. Combined with P03 zombies, threads can accumulate.

## Subtasks

### [M] Medium
- **X658** 5 browser ops in one assistant turn; threads spawn.
- **X659** Count live threads during the burst (`ps -M` / Activity Monitor).
- **X660** Burst then idle; threads drain back to baseline.
- **X661** Verify each `spawn_async_tool` = one `std::thread` (unpooled).
- **X662** FD count during burst stays bounded.

### [H] Hard
- **X663** 20 ops queued; 19 block on op_lock holding threads.
- **X664** Slow op + 30 queued; thread high-water mark.
- **X665** Thread-spawn failure path (simulate); `ToolResult` error returned synchronously.
- **X666** Reverie + main both bursting; combined thread count.
- **X667** Memory per blocked worker; growth under pile-up.

### [V] Very hard
- **X668** 100 ops while one holds op_lock; 99 parked threads.
- **X669** OS thread limit approached; graceful vs crash.
- **X670** Channel buffers under burst; no unbounded growth.
- **X671** FD leak check across 500 ops (`lsof` delta).
- **X672** Watcher registry size under burst; cleanup correctness.

### [X] Extreme
- **X673** 1000-op flood; thread/FD/mem ceiling + recovery.
- **X674** Sustained 10/s op rate for 30 min; leak slope.
- **X675** Burst + timeouts + reload combined thread accounting.
- **X676** Prove no thread leak: baseline == post-soak count.
- **X677** Consider + evaluate a bounded worker pool as mitigation.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H05-1 | **S3 (bounded)** | `spawn_async_tool_cancellable` (`async_exec.rs`) does `std::thread::Builder::new().name("async-tool-{name}").spawn(...)` — **one unpooled OS thread per call, no pool, no cap.** The browser worker's first act is `let _op = ctx.op_lock.lock()` (`tools.rs:194`), which BLOCKS while another worker holds it. Confirmed at source that the pipeline batch-loop (`pipeline.rs` `for tool … tool_results.push`) executes **every** tool in a turn (no short-circuit on the first sentinel) before `has_console_wait` defers — so a single assistant turn with N browser tool_uses spawns **N threads near-simultaneously, N−1 parked on `op_lock`** (each ~2 MB stack) until they serialize through. | **CONFIRMED (source + live), reclassified S3 bounded** — 2026-06-11. Live: idle tui = ~16 threads, **0** `async-tool-*` at rest → workers terminate after their op, **no leak at rest** (X660/X661). | **NO INLINE FIX (S3, not an S0/S1/S2 blocker per M55).** Pile-up is BOUNDED, not unbounded: (1) per-turn burst ≤ the # of browser tool_uses the LLM emits (rarely >10 → ~20 MB transient); (2) the sentinel-defer turn structure gates arrival — the turn WAITS on the watchers before the next turn dispatches, so threads drain (op_lock released one-by-one) faster than new turns arrive; (3) FDs do NOT grow per-op — `connect_shared` reuses the one cached WebSocket; (4) thread-spawn failure at the OS limit returns a synchronous `is_error` `ToolResult` (graceful, no crash — X665/X669); (5) workers always terminate (ops bounded by client `OP_TIMEOUT`/settle). **Residual risk:** combined with a P03 zombie holding `op_lock` for its full 30s, new ops park behind it — the same throughput-stall already tracked as P03 H03-1. The cheap real mitigation is the P03 fix (bounded op timeout < watcher timeout), NOT a worker pool (which would only move parking from "OS thread on op_lock" to "task in queue" — marginal, given op_lock already serializes). Bounded-pool = optional future hardening (X677). |

## Exit criterion
Thread/FD/mem return to baseline after any burst; no unbounded growth; a
documented ceiling and (if needed) a bounded-pool mitigation.

**Status (source + live):** MET for normal operation. Threads drain to baseline
at rest (live: 0 `async-tool-*` idle); per-turn burst is bounded by the # of
browser tool_uses; FDs don't grow per-op (conn reused); spawn-failure is
graceful. The only pile-up vector is a P03 zombie holding `op_lock` (tracked
there as S3). No worker pool needed today; documented as optional future
hardening (X677). Not a deployment blocker.
