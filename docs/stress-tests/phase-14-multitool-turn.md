# P14 — Multi-tool turn correctness

**Todo:** X571 · **Primary hazard:** premature `resolve()` → stale/Unknown e-refs

## Objective
The LLM emits `snapshot`+`click` (or `goto`+`snapshot`+`click`) in **one assistant
turn**. With async, `click`'s `resolve()` can read `shared` e-refs **before** the
snapshot worker writes them. Map every interleaving that produces stale or
`Unknown ref` outcomes.

## Targeted hazard
`tools.rs` `resolve()` reads e-refs from `shared` on the **main thread at dispatch
time**, then spawns the worker. In a same-turn `snapshot`+`click`: snapshot returns
the sentinel (worker still running, erefs **not yet written**); the pipeline then
dispatches `click`, whose `resolve()` reads the *old/empty* erefs → `Unknown ref`
or a stale-page ref. The synchronous version guaranteed snapshot completed first;
**async breaks that contract.** This is the most likely real-world LLM failure.

## Subtasks

### [M] Medium
- **X838** `snapshot`+`click` same turn; click resolves **before** snapshot writes.
- **X839** Reproduce `Unknown ref` from a premature `resolve()`.
- **X840** `goto`+`snapshot` same turn; snapshot sees the pre-nav page.
- **X841** `goto`+`snapshot`+`click` triple in one turn; ordering.
- **X842** Confirm `resolve()` runs at dispatch, not at worker time.

### [H] Hard
- **X843** Does `op_lock` serialize *tools* or just *workers*? (dispatch timing).
- **X844** click after snapshot: ref empty because `shared` not yet written.
- **X845** type after snapshot same turn; same stale-ref hazard.
- **X846** Pipeline order: are same-turn tools dispatched sequentially or batched?
- **X847** Map every 2-tool browser combo for ordering bugs.

### [V] Very hard
- **X848** 3+ browser tools per turn; full interleaving matrix.
- **X849** click resolves a stale ref from the **previous** page's snapshot.
- **X850** Mixed browser + non-browser tools same turn; interplay.
- **X851** Propose fix: run `resolve()` **inside** the worker after `op_lock`.
- **X852** Propose fix: snapshot result carries erefs forward to click.

### [X] Extreme
- **X853** Adversarial turn: 10 interdependent browser tools.
- **X854** Prove the snapshot→act contract is broken by async (counter-example).
- **X855** Real-world flow (login form) breaks under async ordering.
- **X856** Same-turn ordering under reverie + main concurrency.
- **X857** Formal happens-before model for same-turn browser tools.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H14-1 (suspected) | **S2** | same-turn snapshot+click → click resolves before snapshot writes erefs → Unknown ref | _to confirm_ | resolve refs inside the worker under op_lock, OR make same-turn browser tools await prior result |

## Exit criterion
A same-turn `snapshot`+`click` always acts on the snapshot's e-refs (resolve moved
under `op_lock`, or per-turn ordering enforced); 0 spurious `Unknown ref`.
