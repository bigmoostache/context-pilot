# P17 — Error-propagation fidelity

**Todo:** X574 · **Primary hazard:** orphan / mislabelled / lost tool result

## Objective
Differentiate every failure channel and verify each yields a **correct,
non-crashing, well-labelled** tool result with exactly one `tool_result` per
`tool_use`:
- caught op panic (`catch_panic` → `Err`),
- uncaught thread panic (channel `Disconnected`),
- timeout,
- worker-thread spawn failure,
- poisoned locks.

## Targeted hazard
The async pipeline has many failure exits: `catch_panic` returns `Err` inline; a
panic *outside* `catch_panic` drops the channel sender → the watcher sees
`Disconnected`; the 30s timeout fires its own result; `spawn` can fail; a poisoned
lock returns a string error. Each must reach the LLM as a single, correctly
`is_error`-flagged `tool_result`. `ASYNC_ERROR_PREFIX` must strip cleanly and never
leak raw CDP `-32000`.

## Subtasks

### [M] Medium
- **X898** Caught op panic → `Err` content reaches the LLM, `is_error` set.
- **X899** Timeout → "Async tool timed out after Ns" message.
- **X900** Missing param → synchronous error, no worker spawned.
- **X901** No-browser-running → clean `op_ctx` error before spawn.
- **X902** Unknown ref → `resolve()` error, no worker spawned.

### [H] Hard
- **X903** Uncaught thread panic → channel `Disconnected` → error result.
- **X904** Thread-spawn failure → synchronous `ToolResult` error.
- **X905** Poisoned-lock error wording reaches the LLM clearly.
- **X906** `ASYNC_ERROR_PREFIX` correctly strips + sets `is_error`.
- **X907** `catch_panic` "connection lost" vs a real op error — distinguishable.

### [V] Very hard
- **X908** Every error path yields a **valid** `tool_result` (no orphan tool_use).
- **X909** Error during a multi-tool turn; subsequent tools unaffected.
- **X910** `preserves_tempo` correctness per error type.
- **X911** Worker panic in `note_nav` (after op) vs in op; both handled.
- **X912** Error message never leaks raw CDP `-32000` to the LLM.

### [X] Extreme
- **X913** Inject a failure at each line of `run_browser_op`; classify the result.
- **X914** Double-fault: panic inside the panic-handler path.
- **X915** Error storm: 100 failing ops; no crash, all paired.
- **X916** Verify `is_error` is never lost across the watcher→cleanup pipeline.
- **X917** Prove totality: every code path returns exactly one `tool_result`.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| _none yet_ | | | | |

## Exit criterion
Totality proven: every failure exit produces exactly one correctly-labelled
`tool_result`; no orphan `tool_use`; no raw CDP error string leaks.
