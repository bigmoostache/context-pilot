# P09 — Connection lifecycle under load

**Todo:** X566 · **Primary hazard:** reconnect storm / hung is_alive probe under lock

## Objective
Stress the idle self-close of the transport, reconnect-while-busy, and the
`is_alive()` probe (a CDP round-trip) executed while Chrome is hung — all while
holding the `conn` lock. Verify cached-vs-reconnect correctness under concurrency.

## Targeted hazard
`connect_shared` holds `conn`, checks `existing.is_alive()` (a real CDP round-trip),
and reconnects on failure. The headless_chrome transport self-closes after idle
(`open` AtomicBool flips false permanently). Under concurrency: multiple first-ops
can stampede the reconnect; `is_alive()` against a hung Chrome blocks while holding
`conn` (→ P08 close freeze, → P10 lock-hold).

## Subtasks

### [M] Medium
- **X738** Idle 60s+; transport self-closes; next op reconnects.
- **X739** `is_alive()` returns false on an idle-closed transport.
- **X740** Cached client reused when alive (no reconnect).
- **X741** Reconnect transparent to the LLM (op just works).
- **X742** `connect_shared` returns the cached `Arc` on the warm path.

### [H] Hard
- **X743** `is_alive()` while Chrome hung; probe blocks holding `conn`.
- **X744** Reconnect-while-busy: an op during another op's reconnect.
- **X745** Transport closes mid-op; single hard-fail then recover.
- **X746** `is_alive` false-positive (alive transport, dead tab).
- **X747** Reconnect to the same ws_url after idle; tab state preserved?

### [V] Very hard
- **X748** Idle-close + concurrent ops; thundering-herd reconnect.
- **X749** `connect_shared` double-connect race (both see `None`).
- **X750** `is_alive` cost measured; round-trip under the lock budget.
- **X751** Reconnect storm: 10 ops after idle; only one reconnects.
- **X752** Stale `Arc<Client>` lingering after reconnect; old one drops.

### [X] Extreme
- **X753** Chrome paused (`SIGSTOP`) mid-op; `is_alive` + timeout behavior.
- **X754** Network partition to the CDP socket; reconnect + timeout matrix.
- **X755** 1000 idle/reconnect cycles; connection-cache correctness.
- **X756** Prove exactly-one reconnect under concurrent first-ops.
- **X757** Reconnect + poison + timeout combined chaos.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H09-1 (suspected) | **S1/S3** | is_alive() on hung Chrome blocks holding conn → cascades to close-freeze | _to confirm_ | bounded is_alive timeout |

## Exit criterion
Exactly-one reconnect under concurrent first-ops; `is_alive` bounded so a hung
Chrome never blocks the lock indefinitely; cache correctness over 1000 cycles.
