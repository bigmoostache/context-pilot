# P01 — Baseline async responsiveness & invariants

**Todo:** X558 · **Primary hazard:** freeze regression / lost responsiveness

## Objective
Establish and *measure* the core invariant the whole refactor exists to provide:
**the TUI keeps rendering and accepting input during every CDP op.** This is the
control group every later phase is measured against. If we can't prove
responsiveness here with instrumentation, no later result is trustworthy.

## Targeted hazard
`run_browser_op` → `spawn_async_tool` must move all blocking CDP work off the
main event loop (`src/app/run/lifecycle.rs::run`). A regression where any op
still blocks the loop (e.g. `op_ctx()` doing I/O, or `resolve()` blocking on a
contended `shared` lock) reintroduces the freeze.

## Setup / tooling
- Flame instrumentation (`./run.sh --telemetry`, `CP_FLAMEGRAPH=1`) — inspect the
  `loop` span; assert no single tick > 100ms.
- A keypress logger (timestamped) to detect input gaps during an op.
- A slow-page server (`/sleep/N`) to widen the op window.
- FPS/tick counter from the perf overlay (Ctrl+ overlay) for visual confirmation.

## Subtasks

### [M] Medium
- **X578** Single `goto`: confirm TUI redraws the spinner mid-op. *Watch:* spinner animates throughout.
- **X579** Type a message while `goto` in flight; keystrokes register. *Pass:* input box updates live.
- **X580** Scroll a panel during `extract`; scroll is smooth. *Pass:* no scroll lag.
- **X581** Verify the `⏳ browser_*` watcher appears in the Spine panel during op. *Pass:* watcher listed, removed on completion.
- **X582** Sentinel returned, real result lands via watcher. *Pass:* tool_result content ≠ sentinel.

### [H] Hard
- **X583** Measure main-loop tick latency during op (< 50ms). *Method:* flame `loop` span histogram.
- **X584** Open command palette (Ctrl+P) mid-op; responsive. *Pass:* palette opens < 1 frame.
- **X585** Resize terminal repeatedly during op; no stall. *Pass:* reflow each resize.
- **X586** Esc-cancel the stream while a browser op is pending; clean state. *Watch:* pending tool flushed as interrupted, no orphan.
- **X587** Render FPS stays ~28fps throughout op. *Method:* perf overlay frame budget.

### [V] Very hard
- **X588** Instrument: assert no main-thread block > 100ms (flame gate).
- **X589** 10 rapid sequential ops; UI never freezes between. *Method:* keypress-gap detector.
- **X590** Op during an active reverie stream; both progress independently.
- **X591** Keypress histogram during a 30s slow op; no gaps > 60ms.
- **X592** Verify a worker thread named `async-tool-browser_*` exists during op (`ps -M`).

### [X] Extreme
- **X593** Continuous input flood + ops for 10 min; zero dropped keys.
- **X594** CPU profile: confirm CDP work runs off the main thread (`perf.log` / sampling).
- **X595** Compare pre/post-refactor freeze duration — **regression gate** (sync baseline vs async).
- **X596** Assert invariant holds under 4 concurrent reveries all issuing ops.
- **X597** Automated freeze-detector harness wired as a CI gate.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| _none yet_ | | | | |

## Exit criterion
Flame-proven: no main-loop tick > 100ms during any op; keypress-gap detector
clean over 10 min; regression gate shows async freeze ≈ 0 vs sync seconds.
