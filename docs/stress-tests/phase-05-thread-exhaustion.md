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
| H05-1 (suspected) | **S3** | N queued ops → N parked threads on op_lock | _to confirm_ | bounded pool, or reject/queue at dispatch |

## Exit criterion
Thread/FD/mem return to baseline after any burst; no unbounded growth; a
documented ceiling and (if needed) a bounded-pool mitigation.
