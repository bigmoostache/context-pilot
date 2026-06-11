# P07 — Persistence & reconnect races after reload

**Todo:** X564 · **Primary hazard:** stale ws_url / double-spawn / lost erefs

## Objective
Attack `save_module_data` / `load_module_data` + `reconnect_chrome` interplay with
the **fresh (empty) `conn`/`shared`** after reload. Stale ws_url, dead Chrome,
about:blank tab, lost erefs, and concurrent first-op reconnect.

## Targeted hazard
`lib.rs::save_module_data` persists `meta` + `next_session_id` only; `conn`/`shared`
are runtime and start empty after `init_state`. `load_module_data` →
`reconnect_chrome` re-attaches to the persisted `ws_url`. The **first op** after
reload calls `connect_shared`, which connects fresh into the empty slot. Races:
two first-ops both see `None` and both connect; reconnect to a dead/stale ws_url;
orphan cleanup (`browser_` prefix) killing a live session.

## Subtasks

### [M] Medium
- **X698** Reload with Chrome alive; `reconnect_chrome` succeeds.
- **X699** After reload, `conn` slot empty; first op connects fresh.
- **X700** After reload, tab is about:blank; goto needed before snapshot.
- **X701** `save_module_data` persists `meta` + `next_session_id` only.
- **X702** `shared` (erefs) **not** persisted; empty after reload.

### [H] Hard
- **X703** Reload with Chrome **dead**; reconnect fails, clean error path.
- **X704** Stale ws_url in `meta`; first-op reconnect detects + errors.
- **X705** Two rapid ops after reload race to populate the `conn` slot.
- **X706** Orphan cleanup (`browser_` prefix) doesn't kill a live session.
- **X707** `reconnect_chrome` + concurrent `browser_open`; no double-spawn.

### [V] Very hard
- **X708** Corrupt `meta` JSON on disk; `load_module_data` resilience.
- **X709** `next_session_id` monotonic across many reloads.
- **X710** Panel dropped when reconnect fails; `retain()` correctness.
- **X711** Reconnect + first-op `connect_shared` share one ws_url.
- **X712** Reload ×20 with live Chrome; connection stable each time.

### [X] Extreme
- **X713** Concurrent reconnect of N orphaned `browser_` sessions.
- **X714** Disk `meta` vs live daemon state divergence injection.
- **X715** Reconnect to the wrong Chrome (port reuse) detection.
- **X716** Persistence fuzz: random `meta` mutations + reconnect.
- **X717** Prove no Chrome leak across 100 reload cycles.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| _none yet_ | | | | |

## Exit criterion
Reconnect is exactly-once under concurrent first-ops; dead/stale Chrome yields a
clean error not a hang; no double-spawn; no Chrome leak across 100 reloads.
