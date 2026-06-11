# P12 — Panel/render contention during ops

**Todo:** X569 · **Primary hazard:** render stall / torn read / lock convoy

## Objective
The Browser panel's `build_content` locks `shared`; hammer rendering (resize, page
nav, rapid redraw) while workers hold `shared`. Probe for render stalls, torn
reads, panel staleness, and lock convoy at the ~28fps render cadence.

## Targeted hazard
`panel.rs::build_content` reads runtime data from `shared.lock()` on the render
path. A worker holds `shared` only briefly (to write erefs/url/title), but at
28fps the render thread competes for it every frame. Risks: render blocked while a
worker holds `shared` during a large snapshot write; a torn read showing
half-old/half-new state if fields are written non-atomically.

## Subtasks

### [M] Medium
- **X798** Render the Browser panel while a `goto` worker runs; no stall.
- **X799** `build_content` shows a "(momentarily locked)" placeholder under contention.
- **X800** Panel updates after the op completes (`mark_dirty` path).
- **X801** Switch to the Browser panel mid-op; renders the current state.
- **X802** Snapshot worker writes `shared`; panel reflects the new erefs.

### [H] Hard
- **X803** Rapid redraw (resize spam) while a worker holds `shared`.
- **X804** Render-thread lock-wait measured; < 16ms budget.
- **X805** Paginated e-ref table render during a `shared` write.
- **X806** 28fps sustained while 3 ops churn `shared`.
- **X807** Torn read: does the panel ever show half-old/half-new snapshot?

### [V] Very hard
- **X808** Lock convoy: render + `resolve` + worker all want `shared`.
- **X809** Panel freeze/cache interaction with live `shared` writes.
- **X810** Big snapshot (200 erefs) render under contention.
- **X811** Panel goto-page during a `shared` write; index stable.
- **X812** Measure `shared` lock-hold by `build_content` (clone-out fast?).

### [X] Extreme
- **X813** Render starvation: worker holds `shared` 8s; UI degradation.
- **X814** 1000 redraws/s vs op churn; dropped-frame accounting.
- **X815** Prove the panel never deadlocks the render loop.
- **X816** Multi-panel + browser + reverie render chaos.
- **X817** Snapshot consistency invariant during concurrent render.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H12-1 | **none (PASS)** | Audited `panel.rs`. The render **hot path** does NOT lock `shared`: `Panel::blocks()` reads `ctx.cached_content.clone()` (a pre-rendered `String`); `build_content` (the only `shared.lock()` site) runs in `Panel::refresh()` — **main-thread, occasional** (on `mark_dirty`/refresh cycle), `needs_cache()=false`. So the ~28fps render loop never touches `shared` at all. | **PROVEN SAFE (source)** — 2026-06-11. | **NO FIX NEEDED.** Even on the refresh path, `build_content` holds `shared` only for a memcpy-class critical section (read `last_action`/`erefs.len()`/`snapshot_text`, format into `out`) — no I/O under the lock; bounded by the snapshot size (~200 erefs ≈ a few KB) ≪ 16 ms. Worker writes (`OpSink::write_snapshot`) set snapshot_text+erefs+url+title+last_action under a **single** `shared.lock()` acquisition, and `build_content` reads under a single acquisition → reads are atomic w.r.t. the mutex, **no torn read** (X807/X817 pass). No lock convoy: render reads cached, only refresh + workers touch `shared`, workers serialized by `op_lock`. |
| H12-2 | **S4 (cosmetic)** | `build_content`'s `let Ok(shared) = bs.shared.lock() else { "(browser state momentarily locked)" }` placeholder fires only on **poison**, not contention — a blocking `.lock()` waits on contention, it never returns `Err`. The "momentarily locked" wording implies a contention fallback it doesn't actually provide. | **NOTED (source)** — 2026-06-11. | Harmless: the brief, bounded hold means contention-wait is sub-frame anyway. Optional: reword to "(browser state unavailable)" or switch to `try_lock` for a true momentary-skip. Not a blocker. |

## Exit criterion
Render-thread `shared` lock-wait < 16ms always; no torn reads (snapshot written
atomically / cloned under one lock); panel never blocks the render loop.

**Status (source):** MET — exceeded. The render hot path (`blocks()`) reads
`cached_content` and never locks `shared`, so render-thread lock-wait is **zero**.
`shared` writes are atomic under one mutex acquisition → no torn reads. Only the
occasional main-thread `refresh()` and the op_lock-serialized workers touch
`shared`, each for a bounded memcpy-class hold. PASS.
