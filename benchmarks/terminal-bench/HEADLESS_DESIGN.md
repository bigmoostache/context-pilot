# Headless Mode — Design (X520)

Status: **DESIGN VALIDATED — D1–D4 confirmed by captain (2026-06-11).**
No implementation yet. D5–D9 are low-controversy defaults, flagged for review.

Goal: `tui --headless "<instruction>"` runs the full Context Pilot agent loop
autonomously inside a Terminal-Bench container — no terminal rendering, exits
when the task is complete, writes a trajectory for Harbor artifact collection.

---

## Current architecture (what we're reusing)

```
main.rs                 terminal setup → phased boot (5 phases) → App::new → app.run() → cleanup
src/app/run/lifecycle.rs  run(): ONE loop interleaving three concerns:
  [input]    event::poll → palette / autocomplete / question form / actions   ← terminal-only
  [orchestr] ~25 calls: process_stream_events, handle_retry, process_typewriter,
             cache updates, watchers, tool checks, handle_tool_execution,
             finalize_stream, check_spine, reverie ×4, reload check, ownership ← terminal-FREE
  [render]   terminal.draw(...)                                              ← terminal-only
```

Key observation: **the boot phases and the entire background-orchestration
block never touch the terminal.** Only boot-screen rendering, input handling,
and `terminal.draw` do. The seam is clean.

---

## D1 — Loop seam: shared `background_tick()` ★ RECOMMENDED

Extract the orchestration block of `run()` into a single method:

```rust
impl App {
    /// One iteration of terminal-free orchestration.
    /// Called by BOTH the interactive loop and the headless loop.
    fn background_tick(&mut self, ch: &EventChannels<'_>) -> TickStatus { ... }
}
```

- `run()` (interactive) becomes: input handling + `background_tick()` + render.
- `run_headless()` becomes: `background_tick()` + quiescence check + trajectory writes.

**Alternatives rejected:**
- *(b) Duplicated headless loop* — zero risk to the TUI path, but the
  orchestration **order** is subtle (tools → finalize → spine; reverie after
  main tools) and two copies will drift. Worse trade.
- *(c) Drive the TUI with synthetic key events* — fragile, slow, benchmarks the
  input layer instead of the agent.

Structure budget: `run/` is at 7 entries → `headless.rs` makes 8 (at cap, OK).
`lifecycle.rs` (372) shrinks; `headless.rs` est. ~250 lines.

## D2 — Completion: quiescence detection + todos

CP has no explicit "task done" signal. Headless exits 0 on **quiescence**:

```
stream phase == Idle
AND typewriter drained AND no pending tools / deferred sleeps / blocking watchers
AND no unprocessed spine notifications
AND check_spine() == Idle
… sustained for a settle window (~2–3 s of consecutive idle ticks)
```

The settle window lets async watchers/callbacks/coucou fire before we declare done.

**Validated:** `continue_until_todos_done` **ON in headless only** — it is
*exactly* benchmark semantics (persist until the plan is done), and guard
rails (D3) backstop runaway loops. Captain confirms autocontinuation works
reliably now (former M18 concern deleted 2026-06-11).

## D3 — Guard rails = the safety harness

Headless must never hang (Harbor hard-kills at task timeout → wasted trial).
CLI-tunable limits mapped onto the **existing** spine guard rails:

| Flag | Default | Guard rail |
|---|---|---|
| `--max-cost` | $5.00 / task | MaxCost |
| `--max-messages` | 150 | MaxMessages |
| `--max-duration-secs` | off (defer to Harbor's task timeout) | MaxDuration |
| (fixed) | high (e.g. 100) | MaxAutoRetries |

On guard-rail stop: write final trajectory event (`status: "guard_rail"`),
**exit 2**. Fatal boot/stream errors: **exit 1**. Quiescent done: **exit 0**.

## D4 — Interactive tools: disable `ask_user_question` only

- `ask_user_question` — would block forever on the question form. **Remove from
  tool definitions** in headless (agent never sees it). Rejected alternative:
  auto-answer "use your best judgment" — wastes a roundtrip, teaches the agent
  the tool is safe to call.
- `system_reload` — **KEPT** (captain ruling 2026-06-11). The self-exec reload
  path in `main.rs` re-execs the same binary with the same args, so `--headless`
  + flags survive a reload and `--resume-stream` picks the loop back up. The
  headless loop must take the same reload-exit path as the interactive one.
- Everything else stays — consoles, callbacks, search, entities, web —
  full-capability agent per the bundled-daemons decision (web tools must avoid
  tbench.ai / TB GitHub per leaderboard rules; covered by instruction-level
  guardrail note, see D7).

## D5 — Typewriter bypass

The typewriter buffer is a cosmetic streaming animation; headless drains it
immediately each tick (or `AppendChars` applies text wholesale). Otherwise
every turn is artificially slowed by the animation pacing.

## D6 — Trajectory: JSONL, incrementally flushed

Written by the headless loop itself (separate from PersistenceWriter's
state-restore YAML). `--trajectory <path>`, default
`.context-pilot/trajectory.jsonl`. One JSON object per line:

```jsonl
{"ts":…,"event":"start","instruction":"…","model":"claude-sonnet-4-6","provider":"…"}
{"ts":…,"event":"assistant","text":"…"}
{"ts":…,"event":"tool_call","name":"Edit","intent":"…"}
{"ts":…,"event":"tool_result","name":"Edit","is_error":false,"tldr":"…"}
{"ts":…,"event":"usage","in":…,"out":…,"cache_hit":…,"cache_miss":…,"cost_usd":…}
{"ts":…,"event":"final","status":"done|guard_rail|error","turns":…,"total_cost_usd":…,"duration_secs":…}
```

Flushed after every event → crash-safe artifacts. Condensed one-line progress
also goes to **stdout** (Harbor/docker logs capture it). The Python adapter's
`populate_context_post_run()` parses this file.

## D7 — CLI surface

```
tui --headless "<instruction>"          # instruction as arg (Harbor shlex-quotes)
    [--instruction-file <path>]         # fallback for very large instructions
    [--trajectory <path>]
    [--provider claude-code-api-key]    # default; plain ANTHROPIC_API_KEY, no OAuth/Keychain
    [--model claude-sonnet-4-6]         # default
    [--max-cost 5.0] [--max-messages 150] [--max-duration-secs N]
```

The instruction is submitted as the first user message, verbatim. (Benchmark
integrity: no augmentation beyond what any harness prompt template adds.)

## D8 — Boot: reuse phased boot, skip terminal

- No raw mode / alternate screen / bracketed paste / boot screen.
- Same `boot_*` functions; the per-module progress callback prints a plain
  stdout line instead of rendering.
- Panic hook: keep panic.log write, drop terminal-restore calls.
- Fresh `.context-pilot/` in the task cwd = the existing fresh-start path.
- `pre_start_daemons()` already spawns Meilisearch + console-server in
  parallel; finding the bundled binaries in the container is **X521**'s scope.
- Reload self-exec: works in headless too (D4) — `main.rs` re-execs with the
  original args, so `--headless` + flags + `--resume-stream` carry over.

## D9 — Reverie stays on

The context cleaner auto-trigger matters for long tasks (token budget) and is
part of the agent being benchmarked. No change; noted for completeness.

---

## Decision summary

| # | Decision | Resolution |
|---|---|---|
| D1 | Loop seam | ✅ Extract shared `background_tick()` |
| D2 | Completion | ✅ Quiescence + settle window; `continue_until_todos_done` ON (headless only). M18 deleted — autocontinuation works now. |
| D3 | Guard rails | ✅ --max-cost $5 / --max-messages 150 defaults; exit codes 0/1/2 |
| D4 | Interactive tools | ✅ Disable `ask_user_question` only; `system_reload` KEPT (self-exec preserves --headless args) |
| D5 | Typewriter | Instant drain |
| D6 | Trajectory | JSONL, incremental flush, + stdout progress |
| D7 | CLI | `--headless` + flags above; instruction verbatim |
| D8 | Boot | Reuse phased boot, no terminal; reload = no-op |
| D9 | Reverie | Unchanged (on) |
