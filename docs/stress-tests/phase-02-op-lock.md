# P02 — op_lock serialization & op ordering

**Todo:** X559 · **Primary hazard:** reordering / starvation / weakened ordering guarantee

## Objective
Attack the single `op_lock: Arc<Mutex<()>>` that serializes all CDP ops onto the
one transport. Verify ops execute one-at-a-time, in emission order, with no
starvation, and that holding the lock across a slow op (nav settle ~3.5s) doesn't
break correctness.

## Targeted hazard
`tools.rs::run_browser_op` worker: `let _op = ctx.op_lock.lock();` held for the
whole op. `std::sync::Mutex` provides **no fairness guarantee** — under
contention, ordering/fairness is OS-dependent. The async refactor moved ordering
from "main-loop sequential" to "whoever wins the lock," which may reorder ops the
LLM emitted in sequence.

## Setup / tooling
- Per-op enter/exit timestamps logged from inside the worker (around `_op`).
- A multi-op turn generator (emit N browser tools in one assistant turn).
- Overlap detector: assert no two ops' [enter,exit] intervals intersect.

## Subtasks

### [M] Medium
- **X598** Two sequential `goto`; second waits for first (ordered).
- **X599** Confirm op_lock serializes: logged timestamps are non-overlapping.
- **X600** `snapshot` then `click` same turn; correct order observed.
- **X601** `goto` + `eval` same turn; eval sees the navigated page.
- **X602** Three ops queued; all complete in emission order.

### [H] Hard
- **X603** Slow `goto` blocks a fast `eval`; eval waits, no error.
- **X604** FIFO fairness: 5 ops, verify no reordering.
- **X605** Op B starts only after Op A releases op_lock (instrument).
- **X606** Lock held across full nav settle (3.5s); next op queues cleanly.
- **X607** Interleave ops from a main turn + a reverie; serialized globally.

### [V] Very hard
- **X608** 20 ops back-to-back; assert strict serialization.
- **X609** Starvation: 1 long op + many short; shorts not starved forever.
- **X610** op_lock fairness under `std::sync::Mutex` (**no fairness guarantee** — quantify reorder rate).
- **X611** Verify ordering survives a mid-batch error (one op fails).
- **X612** Two reveries + main all issuing ops; global serialization holds.

### [X] Extreme
- **X613** 100 concurrent op spawns; verify exactly-one-at-a-time.
- **X614** Prove no two CDP messages interleave on the transport (CDP frame trace).
- **X615** op_lock + timeout: lock released exactly once (no double-unlock / leak).
- **X616** Adversarial ordering: rapid snapshot/click/goto permutations.
- **X617** Formal ordering model vs observed (TLA-style checklist).

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| _none yet_ | | | | |

## Exit criterion
Non-overlap proven across 100 ops; reorder rate quantified; no starvation over a
long-op + short-op-storm; ordering survives errors and timeouts.
