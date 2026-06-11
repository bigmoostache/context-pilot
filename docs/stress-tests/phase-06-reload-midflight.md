# P06 — Reload mid-flight: orphaned tool_use recovery

**Todo:** X563 · **Primary hazard:** orphaned tool_use → API-400

## Objective
Trigger `system_reload` (and hard crash) while a browser worker is in flight. The
`ChannelWatcher` is runtime-only and is dropped on reload, so the pending
`tool_use` loses its `tool_result`. Test conversation recovery and API-400
avoidance after resume.

## Targeted hazard
On reload, `src/app/run/lifecycle.rs` re-execs; the `WatcherRegistry` is **not
persisted**, so an in-flight async tool's `ChannelWatcher` vanishes. The
conversation now holds a `tool_use` with no matching `tool_result` → the next
stream can 400. `flush_pending_tool_results_as_interrupted` runs on Esc — does it
run on the reload path? This phase determines whether reload-mid-op is safe.

## Subtasks

### [M] Medium
- **X678** Reload while `goto` in flight; observe resume behavior.
- **X679** After reload, check the conversation has no orphan tool_use.
- **X680** Pending browser tool_use gets a paired tool_result post-reload.
- **X681** No API-400 on the first stream after reload-mid-op.
- **X682** Watcher gone after reload (runtime-only); verify.

### [H] Hard
- **X683** Reload mid-`snapshot`; erefs lost, click-by-ref errors cleanly.
- **X684** Does `flush_pending_tool_results_as_interrupted` fire on reload?
- **X685** Hard `SIGKILL` mid-op; restart; state consistency.
- **X686** Reload during the 30s timeout window; double-resume.
- **X687** Two ops in flight at reload; both orphaned correctly.

### [V] Very hard
- **X688** Reload-mid-op then immediate user message; ordering sane.
- **X689** `resume_stream` + browser sentinel interaction.
- **X690** Worker thread killed by reload; Chrome process survives.
- **X691** Reload mid-op 10× rapid; cumulative orphan check.
- **X692** Pending result lost: does the spine create a dangling notification?

### [X] Extreme
- **X693** Crash-injection harness at every await point of an op.
- **X694** Reload during `connect_shared` handshake; partial-connect state.
- **X695** Verify no zombie Chrome after a reload-mid-op storm.
- **X696** Conversation-integrity invariant proof across 50 reloads.
- **X697** Power-loss simulation (`kill -9` daemon + tui) mid-op recovery.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H06-1 (suspected) | **S2** | reload mid-op → orphan tool_use → API-400 | _to confirm_ | flush pending async tool_uses as interrupted on reload |

## Exit criterion
50 reloads-mid-op produce zero orphan tool_use and zero API-400; pending browser
tool_uses are always paired (interrupted) on reload; no zombie Chrome.
