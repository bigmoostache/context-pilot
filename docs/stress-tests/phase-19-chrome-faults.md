# P19 — Chrome-side adversarial faults

**Todo:** X576 · **Primary hazard:** Chrome misbehaviour → TUI fault

## Objective
Drive the async path while Chrome itself misbehaves: tab crash (sad tab), renderer
OOM, navigation storms, target detaches, devtools disconnect, port reuse, multiple
windows/targets, and alert/beforeunload dialogs.

## Targeted hazard
The worker holds `op_lock` (+ `conn`) across a CDP op that Chrome may never
complete (sad tab, hung renderer, modal dialog). `catch_panic` guards the
register_missing_tabs `unwrap` panic, but new fault modes (target detach,
disconnect mid-op, dialog block) must each resolve to a clean `Err` and a healthy
reconnect — never a TUI crash and never an indefinitely-held lock.

## Subtasks

### [M] Medium
- **X938** Tab crash (`chrome://crash`) mid-op; clean error.
- **X939** Navigate to `chrome://kill`; op recovers or errors.
- **X940** `alert()` dialog blocks the page; op times out cleanly.
- **X941** `beforeunload` prompt on goto; nav handled.
- **X942** Multiple windows opened by the page; the driven tab is correct.

### [H] Hard
- **X943** Renderer OOM (huge allocation); op error, not crash.
- **X944** Target detaches mid-op; `is_alive` + reconnect path.
- **X945** DevTools disconnect mid-op; transport-closed handling.
- **X946** Navigation storm (location loop); `settle_after_nav` behavior.
- **X947** `target=_blank` popups; orphan-target accumulation.

### [V] Very hard
- **X948** Chrome killed externally mid-op; worker error + recovery.
- **X949** Port reuse: a new Chrome on the old port; wrong-target detect.
- **X950** Sad-tab (GPU crash) then op; reconnect or error.
- **X951** Infinite redirect chain; nav settle bounded.
- **X952** Page spawns 100 iframes; snapshot scope correct.

### [X] Extreme
- **X953** Random Chrome `SIGKILL` during an op storm; full recovery matrix.
- **X954** Renderer hang + timeout + reconnect combined.
- **X955** Adversarial-site fuzz (crashy JS) across all ops.
- **X956** Chrome version mismatch / CDP protocol drift.
- **X957** Prove no TUI crash under any Chrome-side fault.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| _none yet_ | | | | |

## Exit criterion
No Chrome-side fault crashes the TUI or holds a lock indefinitely; every fault →
clean `Err` + healthy reconnect on the next op; no orphan targets accumulate.
