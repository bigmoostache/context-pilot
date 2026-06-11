# P11 — Stale-write race: zombie clobbers fresh state

**Todo:** X568 · **Primary hazard:** cross-op state corruption (wrong-page e-refs)

## Objective
After a timed-out/abandoned worker (P03) eventually completes, it writes
`url`/`title`/`erefs` into `shared` that a **newer** op already overwrote. Detect
the resulting cross-op state corruption and wrong-page e-refs.

## Targeted hazard
`shared` writes carry **no generation/epoch token**. The worker writes whatever it
computed whenever it finishes. Sequence: op A (snapshot) times out → user/LLM
issues op B (goto new page) → A's zombie finally writes A's erefs into `shared`,
which now describe the *old* page → a subsequent `click` by ref acts on the wrong
page. Last-writer-wins with no ordering = silent corruption.

## Subtasks

### [M] Medium
- **X778** Trigger one timeout; observe a later zombie write to `shared`.
- **X779** Snapshot times out; later writes stale erefs; click hits the wrong element.
- **X780** goto times out; zombie sets `url` to the old page after a new nav.
- **X781** `last_action` shows the zombie's action after a newer op.
- **X782** Panel digest shows stale `url`/`title` from a zombie write.

### [H] Hard
- **X783** Zombie erefs collide with the new page; click hits the wrong target.
- **X784** Two timeouts, both zombies write; last-writer-wins corruption.
- **X785** Fresh op writes `shared`, zombie overwrites it a microsecond later.
- **X786** `resolve()` reads erefs mid zombie-write; torn lookup.
- **X787** Detect: **no generation/epoch guard** on `shared` writes (root cause).

### [V] Very hard
- **X788** Add + test an op-epoch token; reject stale-worker writes.
- **X789** Zombie write after `browser_close`; writes to a cleared `shared`.
- **X790** Zombie write after reload; the new session sees ghost erefs.
- **X791** Quantify the stale-write window per op type.
- **X792** Snapshot/click/goto zombie matrix; which corrupts what.

### [X] Extreme
- **X793** Deterministic zombie harness; reproduce the clobber 100%.
- **X794** Prove correctness needs an epoch guard (counter-example).
- **X795** Cross-op corruption under a sustained timeout storm.
- **X796** Wrong-page action causing a real side effect (form submit).
- **X797** Formal model: `shared` writes are not op-ordered (proof).

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H11-1 | **S2** | `SharedBrowser` (`types.rs`) carries **no generation/epoch token** — `set_erefs`/`note_nav` just overwrite `erefs`/`eref_selectors`/`url`/`title`/`last_action`. A timed-out/abandoned worker (P03 H03-1: detached, never cancelled) finishes later and writes its (now-stale) results into `shared`, clobbering whatever a newer op wrote. A subsequent click-by-ref then resolves against the wrong page's e-refs. Last-writer-wins with no op ordering on `shared`. | **CONFIRMED then FIXED+VERIFIED** — 2026-06-11. | **FIXED** via **timeout-tied cooperative cancellation** (NOT epoch-at-dispatch, which would break the P14 same-turn fix). cp-base: `spawn_async_tool_cancellable` hands the worker an `Arc<AtomicBool>` cancel flag; `ChannelWatcher::check_timeout` flips it `true` the instant the watcher fires the 30s timeout (`watchers.rs`). Browser (`tools.rs`): all `shared` mutations route through a new `OpSink` whose `note_shared`/`note_nav`/`write_snapshot` early-return when `cancel` is set. The flag is THIS op's own — only the timed-out (abandoned/zombie) worker is gated, so normal ops AND same-turn pending ops (not yet timed out) write freely → P14 preserved. `resolve` is a READ, never gated. Live-verified: same-turn `snapshot`+`click(e1)` → `Clicked '#b1'`, title `clicked-b1` (correct element), no regression. |

## Exit criterion
A late/zombie worker can never mutate `shared` for a superseded op. **MET** via
timeout-tied cooperative cancellation (`OpSink` cancel-gate): once a worker's 30s
watcher fires, its flag flips and every subsequent `shared` write no-ops. Normal
and same-turn-pending ops are unaffected (flag only set on timeout), so the P14
fix is preserved. Live-verified non-regressive.
