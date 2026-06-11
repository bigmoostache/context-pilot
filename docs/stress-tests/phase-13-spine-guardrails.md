# P13 — Spine / tempo / guard-rail interaction

**Todo:** X570 · **Primary hazard:** stranded tool_use / mis-counted guard rails

## Objective
Async sentinels break tempo; in-flight workers run while the main loop continues.
Test guard rails (`max_duration`/`max_cost`/`max_messages`), auto-continuation, and
the notification flow with pending browser results.

## Targeted hazard
A browser tool returns `BLOCKING_TOOL_SENTINEL` with `preserves_tempo=false`, so
tempo breaks and the spine may evaluate continuation while a worker is still
pending. Guard rails accrue wall-time/cost during the op. The risk: the spine
fires a continuation or a guard-rail block that strands the pending browser
`tool_use`, or double-counts.

## Subtasks

### [M] Medium
- **X818** Async sentinel breaks tempo; confirm `preserves_tempo=false`.
- **X819** In-flight op + spine idle; the main loop keeps ticking.
- **X820** Watcher result creates a correct `tool_result`, not a notification.
- **X821** `max_messages` guard rail counts during a pending op.
- **X822** A browser op result doesn't double-fire continuation.

### [H] Hard
- **X823** `max_duration` trips while an op is in flight; clean block.
- **X824** `max_cost` accrues during a long op; blocks mid-flight.
- **X825** Auto-continuation fires while a worker is pending; ordering.
- **X826** Esc during a pending op vs the spine `user_stopped` flag.
- **X827** Pending op + a new user message; spine notification flow.

### [V] Very hard
- **X828** Guard-rail block + a zombie worker still running.
- **X829** `coucou` timer fires during an op; both delivered.
- **X830** `continue_until_todos_done` + browser ops (user bans it — verify it's off).
- **X831** Tempo break vs cache-breakpoint cost during ops.
- **X832** Multiple sentinels in one turn; tempo accounting.

### [X] Extreme
- **X833** Guard-rail storm + op storm; spine stability 1h.
- **X834** Reverie issues a browser op; guard rails per-agent vs global.
- **X835** Pending op across a guard-rail block + reload + resume.
- **X836** Notification dedup under an op-result storm.
- **X837** Prove the spine never strands a pending browser `tool_use`.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| _none yet_ | | | | |

## Exit criterion
The spine never strands a pending browser `tool_use`; guard rails count exactly
once; continuation never fires while a result is mid-delivery.
