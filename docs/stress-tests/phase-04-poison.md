# P04 — conn-slot Mutex poisoning & recovery

**Todo:** X561 · **Primary hazard:** unrecoverable Mutex poison (no in-session recovery)

## Objective
Induce a panic while a worker holds the `conn` `ConnSlot` lock and determine
whether the Mutex poisons **permanently**, locking out the browser until a reload.

## Targeted hazard
`client::connect_shared` does `conn.lock()` then calls `Client::connect(ws_url)` /
`is_alive()` while holding the guard. It runs inside `catch_panic` (in
`run_browser_op`), so a panic is *caught* — but the guard unwinds, which **poisons
the Mutex**. Recovery paths use `if let Ok(...) = self.conn.lock()`
(`BrowserState::clear_session`) → they **silently skip on poison**, so
`kill_chrome`/`browser_open` never clear it. Hypothesis: **poison is terminal
without reload.** `op_lock` is *not* poisoned (catch_panic boundary is inside it).

## Subtasks

### [M] Medium
- **X638** Force `Client::connect` to panic (inject); observe the caught `Err`.
- **X639** After the caught panic, retry an op; check if `conn` is poisoned.
- **X640** Verify the `conn.lock().map_err` path returns the "poisoned" message.
- **X641** Poisoned `conn`: does `browser_open` recover? *(expect no)*
- **X642** Poisoned `conn`: does `browser_close` + reopen recover? *(expect no)*

### [H] Hard
- **X643** Confirm a *caught* panic still poisons the Mutex (guard unwound).
- **X644** `clear_session` on a poisoned `conn` silently skips (`if let Ok`).
- **X645** `kill_chrome` path leaves `conn` permanently poisoned.
- **X646** Only reload clears poison; verify no in-session recovery exists.
- **X647** Panic inside `is_alive()` while holding `conn`.

### [V] Very hard
- **X648** `shared` Mutex poisoning via a panic in the `note_nav` path.
- **X649** `op_lock` **not** poisoned (catch_panic boundary) — verify.
- **X650** Propose + test fix: `PoisonError::into_inner()` recovery in `connect_shared`.
- **X651** Poison during a reverie op; is the main path also dead?
- **X652** Poison persists across save/load? *(should not — runtime-only)*

### [X] Extreme
- **X653** Deterministic poison-injection harness + full recovery matrix.
- **X654** Poison `conn` + poison `shared` simultaneously; full lockout.
- **X655** Verify every `lock()` uses `unwrap_or_else(into_inner)` or document why not.
- **X656** Audit all 3 locks for poison-recovery completeness.
- **X657** Long session with periodic panics; cumulative poison damage.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H04-1 (suspected) | **S1** | panic while holding conn → permanent poison, browser dead until reload | _to confirm_ | use `lock().unwrap_or_else(PoisonError::into_inner)` everywhere |

## Exit criterion
Either prove poison is unreachable, or convert all `conn`/`shared` lock sites to
poison-tolerant access so a single caught panic never bricks the browser.
