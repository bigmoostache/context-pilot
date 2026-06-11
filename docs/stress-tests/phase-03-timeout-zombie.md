# P03 — Timeout vs slow-op zombie-worker race

**Todo:** X560 · **Primary hazard:** zombie worker holds op_lock + later writes shared

## Objective
Force a CDP op to exceed the **30s `spawn_async_tool` ChannelWatcher timeout**
(`OP_TIMEOUT_SECS`). When the watcher fires its timeout result and removes itself,
the worker thread **keeps running** — still holding `op_lock`, eventually writing
`shared`, and `send()`-ing on a channel nobody listens to. This is the headline
architectural hazard of the refactor.

## Targeted hazard
`cp_base::state::watchers::ChannelWatcher::check_timeout` returns a result and the
watcher is dropped from the registry, but the spawned thread in `spawn_async_tool`
is **never joined or cancelled**. Meanwhile `tools.rs` worker still owns
`ctx.op_lock` and will call `note_nav`/`set_erefs` on `shared` whenever it finally
returns. Consequences to confirm:
1. Every subsequent op blocks on op_lock until the zombie finishes (effective hang).
2. The zombie's late `shared` write clobbers newer state (→ P11).
3. The zombie's `tx.send` fails silently (channel receiver dropped) — must not panic.

## Setup / tooling
- Slow-page server with controllable delay (`/sleep/35`, `/hang`).
- Worker enter/exit + lock-hold logging.
- A second op issued right after the timeout to measure the post-timeout block.

## Subtasks

### [M] Medium
- **X618** `goto` a page that sleeps 35s; observe the 30s timeout result.
- **X619** Confirm the timeout message is well-formed and `is_error` set.
- **X620** After timeout, the next op still works (eventually).
- **X621** `eval` infinite loop; bounded by timeout, not an infinite hang.
- **X622** Verify the watcher is removed from the registry after timeout.

### [H] Hard
- **X623** Timed-out worker **still holds op_lock**; next op blocks until the zombie ends. *Confirm + quantify.*
- **X624** Measure how long the post-timeout block lasts (= remaining zombie runtime).
- **X625** Zombie completes and `send`s on the dead channel; **no panic**.
- **X626** Timeout then reload; zombie thread cleanup (dies with process?).
- **X627** Two ops both exceed timeout; both zombies + op_lock contention.

### [V] Very hard
- **X628** Zombie writes `shared` after timeout; stale erefs detected (→ P11).
- **X629** Timeout during `connect_shared` (Chrome hung); recovery path after.
- **X630** Verify the zombie's `ToolOutput` is silently dropped (send err path).
- **X631** 30s timeout vs nav-settle budget; tune / confirm no false timeouts on slow-but-ok pages.
- **X632** Guard-rail `max_duration` trips while the zombie runs.

### [X] Extreme
- **X633** N zombies accumulate holding op_lock; total deadlock-by-starvation.
- **X634** Zombie + fresh op race to write `shared`; interleave corruption.
- **X635** Chrome killed mid-zombie; thread join / unwind behavior.
- **X636** Prove the worker thread eventually terminates (no perpetual leak).
- **X637** Sustained timeout storm; thread/lock accounting over 1h.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H03-1 (suspected) | **S1** | op > 30s → next op blocks until zombie finishes | _to confirm_ | needs cancellation or op_lock try_lock |
| H03-2 (suspected) | **S2** | zombie late-writes shared → stale state | _to confirm_ | needs op-epoch guard (see P11) |

## Exit criterion
Either (a) prove zombies can't hold op_lock / clobber shared, or (b) ship a
mitigation: cooperative cancellation, op-epoch guard on shared writes, and a
bounded op timeout shorter than the watcher timeout. No S1/S2 remaining.
