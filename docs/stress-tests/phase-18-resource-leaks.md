# P18 — Resource leaks over long sessions

**Todo:** X575 · **Primary hazard:** thread / Chrome / FD / memory leak

## Objective
Soak testing: thousands of ops, repeated open/close, and audit for leaked Chrome
processes, worker threads, file descriptors, memory growth in `shared`/`conn`, and
orphan daemon sessions (`browser_` prefix cleanup).

## Targeted hazard
Each op = one unpooled worker thread (P05); each `browser_open` = one Chrome child
under the console daemon; `conn` caches `Arc<Client>` (old clients must drop on
reconnect); artifacts and the stderr log grow unbounded. Over a long session these
can accumulate. `cleanup_orphans` (browser_-prefix scoped) must reap dead sessions
without killing live ones.

## Subtasks

### [M] Medium
- **X918** 100 open/goto/close cycles; all Chrome PIDs reaped.
- **X919** Baseline FD/thread/mem snapshot before soak.
- **X920** 500 sequential ops; memory delta measured.
- **X921** Orphan `browser_` daemon sessions cleaned on reload.
- **X922** No stray "New Tab" targets accumulate (adopt_initial_tab).

### [H] Hard
- **X923** 1h soak at 2 ops/s; memory slope ~flat.
- **X924** Repeated reload+reconnect; Chrome process count stable.
- **X925** `shared`/`conn` `Arc` strong-count audit; no leak of old `Client`s.
- **X926** Watcher registry drained; no accumulation after ops.
- **X927** Artifact files unbounded growth; disk accounting.

### [V] Very hard
- **X928** 10k ops overnight; full resource accounting.
- **X929** `leaks` (macOS) on the tui process post-soak.
- **X930** Profile dir (`.context-pilot/browser/profile`) growth over time.
- **X931** Daemon socket FD count over many sessions.
- **X932** Browser stderr log unbounded growth.

### [X] Extreme
- **X933** 48h continuous soak; leak slope must be zero.
- **X934** Open 50 browsers (multi-instance future); resource ceiling.
- **X935** `kill -9` Chrome repeatedly; respawn leak accounting.
- **X936** Prove baseline == post-soak for **all** resource classes.
- **X937** Continuous reverie+main ops 24h; combined leak audit.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| _none yet_ | | | | |

## Exit criterion
After a 48h soak, every resource class (threads, Chrome PIDs, FDs, memory) returns
to baseline; artifact/log growth is bounded or rotated.
