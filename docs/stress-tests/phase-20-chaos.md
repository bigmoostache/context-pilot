# P20 — Chaos / combinatorial fuzzing

**Todo:** X577 · **Primary hazard:** emergent failure modes · **DEPLOYMENT GATE**

## Objective
Everything at once: randomized concurrent multi-tool turns + reload + close +
guard-rail trips + Chrome faults + slow ops, run as a fuzz loop. The integration
crucible that surfaces emergent failure modes no single phase predicts. **This is
the deployment sign-off gate.**

## Targeted hazard
The combination of all prior hazards: a timeout zombie (P03) holding `op_lock`
(P02) while a close (P08) blocks and a reload (P06) orphans the pending tool_use
(P11 stale write lands afterward) — interactions that only appear under
randomized concurrency. Drive with a seeded RNG so every failure is reproducible
and shrinkable to a minimal repro.

## Subtasks

### [M] Medium
- **X958** Fuzz: random op sequence (goto/snapshot/click/eval) ×100.
- **X959** Random open/close interleaved with ops.
- **X960** Random reload points during the op stream.
- **X961** Mixed browser + file + git tools per turn fuzz.
- **X962** Random guard-rail limits set mid-fuzz.

### [H] Hard
- **X963** Concurrent main + 1 reverie issuing random ops.
- **X964** Fuzz + injected timeouts (slow pages) randomly.
- **X965** Fuzz + Chrome `SIGSTOP`/`SIGCONT` randomly.
- **X966** Fuzz + random panel open/close/resize.
- **X967** Fuzz + random Esc-cancel of streams.

### [V] Very hard
- **X968** Full chaos: ops+reload+close+guardrail+faults 1h loop.
- **X969** 2 reveries + main + chaos faults concurrently.
- **X970** Property assertions checked after each chaos step (invariants).
- **X971** Seeded-repro: every chaos run reproducible from a seed.
- **X972** Shrink a failing chaos seed to a minimal repro.

### [X] Extreme
- **X973** 24h chaos fuzz; zero crashes, zero orphan tool_use, zero leak.
- **X974** Chaos + all prior-phase fault injectors enabled at once.
- **X975** Differential: async vs a synchronous reference oracle.
- **X976** Catalog every distinct failure mode found; severity-rank.
- **X977** Sign-off gate: deployment blocked until chaos green 24h.

## Invariants checked after every chaos step
1. No orphan `tool_use` (every `tool_use` has a `tool_result`).
2. No main-loop tick > 100ms (responsiveness).
3. No deadlock (watchdog: all locks acquirable within budget).
4. `shared` never describes a superseded op (epoch guard).
5. Resource baseline holds (threads/Chrome/FD/mem).
6. No TUI crash / terminal corruption.

## Findings
| ID | Severity | Repro (seed) | Status | Fix / Issue |
|----|----------|--------------|--------|-------------|
| _none yet_ | | | | |

## Exit criterion (DEPLOYMENT GATE)
24h of seeded chaos fuzzing with **zero S0/S1/S2**; all six invariants hold at
every step; every prior-phase finding has a landed fix or an accepted-risk sign-off.
