# Prompt-Building Audit for Cache Hit Optimization

## Executive Summary

There are **7 sources of prefix instability** that break prefix caching (especially DeepSeek's). Every API call generates a different token sequence even when nothing meaningful changed.

---

## Complete Message Array Structure

Each API call sends this structure (in order):

```
Position  Content                                      Stability
────────  ───────────────────────────────────────────   ─────────
[system]  System prompt (from active seed)              STABLE ✓

[msg 0]   assistant: header + timestamp for panel #1    UNSTABLE ✗
[msg 1]   user: tool_result with panel #1 content       DEPENDS
[msg 2]   assistant: timestamp for panel #2             UNSTABLE ✗
[msg 3]   user: tool_result with panel #2 content       DEPENDS
...       (repeat for each panel)
[msg N]   assistant: footer with ALL message timestamps  UNSTABLE ✗
[msg N+1] user: tool_result "Panel display complete"    STABLE ✓

[msg N+2] user: seed re-injection                       STABLE ✓
[msg N+3] assistant: "Understood."                      STABLE ✓

[msg N+4] user: [U1]: <actual conversation>             STABLE ✓
[msg N+5] assistant: [A1]: <response>                   STABLE ✓
...       (actual conversation continues)
```

---

## Source 1: Panel Timestamp Text (CRITICAL)

**Location:** `src/llms/mod.rs:459-478`

Every panel's injected assistant message contains:
```
Panel automatically generated at 2026-02-09T08:28:45Z (18 minutes ago)
```

The `(18 minutes ago)` part changes **every second** because `current_ms = now_ms()` is called fresh in `messages_to_api()` (line 351 in anthropic.rs, line 438 in deepseek.rs).

**Impact:** Every panel injection message differs between calls. Since panels are injected at **positions 0-N** (before conversation), this invalidates the entire prefix from position 0 onward.

**Frequency:** Changes on **every single API call**, guaranteed.

---

## Source 2: Panel Footer with Message Timestamps (CRITICAL)

**Location:** `src/llms/mod.rs:481-519`

The footer (injected after all panels) contains:
```
Last message datetimes:
  - [U1] user: 2026-02-09T08:28:45Z (18 minutes ago)
  - [A1] assistant: 2026-02-09T08:29:13Z (17 minutes ago)
Current datetime: 2026-02-09T08:47:13Z
```

Every `{time_delta}` and the `Current datetime` change between calls.

**Impact:** Even if somehow panel timestamps were stable, the footer would still break the prefix.

**Frequency:** Changes on **every single API call**, guaranteed.

---

## Source 3: Panel Sort Order by `last_refresh_ms` (HIGH)

**Location:** `src/llms/mod.rs:534`

```rust
filtered.sort_by_key(|item| item.last_refresh_ms);
```

Panels are sorted ascending by their last refresh timestamp. When any panel refreshes (e.g., Git status refreshes every 2s, file watchers trigger instantly), its `last_refresh_ms` changes, potentially reordering it relative to others.

**Example:** If panels are `[P2(t=100), P3(t=200), P6(t=150)]`, they sort as `[P2, P6, P3]`. If P6 refreshes at t=250, order becomes `[P2, P3, P6]` — completely different message positions.

**Frequency:** Every time **any** panel's cache refreshes, which happens:
- Git (P6): every 2 seconds via timer
- File panels: instantly on file change (inotify)
- GitResult: on `.git/` changes
- GithubResult: every 60s via polling
- Overview (P5): regenerated on **every** `refresh_all_panels()` call (every API call!)

---

## Source 4: Overview Panel (P5) Regeneration (HIGH)

**Location:** `src/modules/core/overview_panel.rs:44-52`

```rust
fn refresh(&self, state: &mut State) {
    let content = self.generate_context_content(state);
    ...
    ctx.cached_content = Some(content);
}
```

`refresh()` is called by `refresh_all_panels()` which runs before **every** API call. The Overview panel's `generate_context_content()` includes:
- Token counts for every panel (change whenever any panel's content changes)
- Message count statistics (changes after every assistant response)
- Todo status counts
- Memory counts

This means P5's content changes on virtually every API call, changing both:
1. Its content in the message array
2. Its `last_refresh_ms`, affecting sort order (Source 3)

---

## Source 5: Git Panel (P6) 2-Second Timer (MEDIUM)

**Location:** `src/modules/git/mod.rs:8` — `GIT_STATUS_REFRESH_MS = 2000`

The Git status panel refreshes every 2 seconds via the timer check in `check_timer_based_deprecation`. Even if the actual git status hasn't changed, the `last_refresh_ms` timestamp updates, which:
1. Changes the panel's timestamp text (Source 1)
2. May change sort order (Source 3)

**Frequency:** Every 2 seconds if git is active.

---

## Source 6: Conversation Context Token Count (LOW)

**Location:** `src/core/context.rs:16`

```rust
refresh_conversation_context(state);
```

Called before every API call. Recalculates the Conversation panel's token count based on current messages. While P1 (Conversation) is filtered from panel injection, it does affect the Overview panel's content (Source 4).

---

## Source 7: `collect_all_context` Iteration Order (LOW)

**Location:** `src/core/panels.rs:239-255`

```rust
let context_types: Vec<ContextType> = state.context.iter()
    .map(|c| c.context_type)
    .filter(|ct| seen.insert(*ct))
    .collect();
```

Context types are collected in the order they appear in `state.context`. While this is usually stable, adding/removing dynamic panels (File, Glob, Grep, GitResult, GithubResult) changes which `ContextType`s are seen first. This affects the order panels call `.context()`, though ultimately `prepare_panel_messages` re-sorts by `last_refresh_ms` anyway.

---

## How Prefix Caching Works (DeepSeek Specifically)

DeepSeek caches the **longest prefix** of tokens that matches a previous request. If even a single token changes at position X, everything from X onward is a cache miss.

Current structure means:
```
[system prompt]          ← CACHEABLE (stable)
[panel 0: assistant msg] ← MISS (timestamp changed)
[panel 0: content]       ← MISS (follows a miss)
[panel 1: assistant msg] ← MISS (timestamp changed)
...everything else...    ← ALL MISS
```

**Result:** Only the system prompt gets cache hits. Everything else (panels, conversation) is always a cache miss.

---

## What Changes Between Two Consecutive Tool-Loop API Calls

Given a tool-call loop where the LLM calls a tool and we send the result + restart:

| What | Changes? | Why |
|------|----------|-----|
| System prompt | No | Static seed content |
| Panel timestamps | **Yes** | `now_ms()` recalculated |
| Panel sort order | **Maybe** | If any panel refreshed between calls |
| Panel content | **Maybe** | If file/git changed, if overview recalculated |
| Footer timestamps | **Yes** | `now_ms()` recalculated, new messages added |
| Footer current datetime | **Yes** | Always changes |
| Seed re-injection | No | Static |
| Conversation messages | **Yes** | New tool call + result messages appended |

---

## Panels and Their Refresh Triggers

| Panel | Type | Refresh Trigger | Timer | `last_refresh_ms` Stability |
|-------|------|-----------------|-------|----------------------------|
| P0 System | Fixed | Never (static) | - | Stable |
| P1 Conversation | Fixed | Token count only | - | N/A (filtered out) |
| P2 Tree | Fixed | Dir watcher | - | Stable unless dir changes |
| P3 Todo | Fixed | Tool execution | - | Stable unless todos modified |
| P4 Memory | Fixed | Tool execution | - | Stable unless memories modified |
| P5 Overview | Fixed | **Every API call** | - | **UNSTABLE** — regenerated every call |
| P6 Git | Fixed | Watcher + **2s timer** | 2000ms | **UNSTABLE** — refreshes constantly |
| P7 Scratchpad | Fixed | Tool execution | - | Stable unless scratchpad modified |
| P8+ File | Dynamic | File watcher | - | Changes on file save |
| P8+ Glob | Dynamic | Tool execution | - | Stable until re-searched |
| P8+ Grep | Dynamic | Tool execution | - | Stable until re-searched |
| P8+ Tmux | Dynamic | Tool execution | - | Stable until re-captured |
| P8+ GitResult | Dynamic | `.git/` watcher | - | Changes on git operations |
| P8+ GithubResult | Dynamic | ETag poll | 60s | Changes on GH changes |

---

## Key Files Reference

| File | Lines | What |
|------|-------|------|
| `src/core/context.rs` | 14-51 | `prepare_stream_context()` — orchestrates everything |
| `src/core/panels.rs` | 226-255 | `refresh_all_panels()` + `collect_all_context()` |
| `src/llms/mod.rs` | 459-478 | `panel_timestamp_text()` — dynamic timestamps |
| `src/llms/mod.rs` | 481-519 | `panel_footer_text()` — dynamic footer |
| `src/llms/mod.rs` | 525-544 | `prepare_panel_messages()` — sort by `last_refresh_ms` |
| `src/llms/anthropic.rs` | 344-535 | `messages_to_api()` — Anthropic message building |
| `src/llms/deepseek.rs` | 408-611 | `messages_to_ds()` — DeepSeek message building |
| `src/modules/core/overview_panel.rs` | 44-52, 628-755 | Overview `refresh()` + `generate_context_content()` |
| `yamls/prompts.yaml` | 96-107 | Panel prompt templates |
