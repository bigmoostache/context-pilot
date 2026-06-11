# P16 — Huge/exotic payloads through the channel

**Todo:** X573 · **Primary hazard:** OOM / truncation / mid-codepoint cut

## Objective
Extreme `extract`/`eval`/`screenshot` payloads are marshalled as `String` through
the worker channel vs the 30s timeout and the inline cap (`INLINE_CAP_BYTES=8000`).
Stress unicode, NUL bytes, multi-GB pages, infinite-growth DOM, and base64 blobs.

## Targeted hazard
The worker computes the full result string, then sends it through the channel; the
watcher delivers it. A multi-GB extract builds the whole `String` in worker memory
before the inline cap / file spill applies — OOM risk. `truncate_utf8` must cut on
a codepoint boundary. The 30s timeout may fire mid-marshal, producing a partial or
dropped payload.

## Subtasks

### [M] Medium
- **X878** Extract a 5MB page; through the channel inline-cap path.
- **X879** `eval` returns 1MB JSON; capped correctly.
- **X880** Unicode-heavy extract (CJK/emoji); no truncation mid-codepoint.
- **X881** full_page screenshot of a tall page; PNG bytes intact via the channel.
- **X882** Empty/whitespace extract; clean note, not an error.

### [H] Hard
- **X883** 50MB extract vs the 30s timeout; which wins.
- **X884** NUL bytes / control chars in extracted text.
- **X885** `truncate_utf8` at an exact multibyte boundary; no panic.
- **X886** `eval` returns a deeply nested object (stack depth).
- **X887** Infinite-growth DOM (appendChild loop) during snapshot.

### [V] Very hard
- **X888** base64 data-URL blob extract; size explosion.
- **X889** 200-eref snapshot `render_erefs` size through the channel.
- **X890** String marshalling cost for a 100MB payload; OOM risk.
- **X891** Invalid UTF-8 from the page (lossy?) survives the channel.
- **X892** Extract while the page mutates; consistent snapshot?

### [X] Extreme
- **X893** Multi-GB page extract; memory ceiling + graceful fail.
- **X894** Payload + timeout + reload combined; partial result.
- **X895** Fuzz extract output sizes 0..1GB; find the break point.
- **X896** Concurrent huge payloads on 2 ops; combined memory.
- **X897** Prove no unbounded memory via the channel under a payload storm.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| _none yet_ | | | | |

## Exit criterion
Payloads are bounded before marshalling (cap at source, not after building a giant
`String`); `truncate_utf8` never panics; combined-op memory has a proven ceiling.
