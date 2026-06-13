# Threads — Parallel Discussion & Work Topics

> Design document — `threads` branch  
> Status: **brainstorm / initial spec**

## Concept

Replace the single-conversation model with the ability to maintain **multiple parallel threads** — each a distinct discussion or work topic. The main conversation remains, but threads give structure to async back-and-forth on separate concerns.

---

## Core Model

### Thread

| Field | Type | Description |
|-------|------|-------------|
| `id` | `String` | Short unique identifier (e.g. `T1`, `T2`) |
| `name` | `String` | Free-text label |
| `status` | `enum` | `MY_TURN` / `THEIR_TURN` |
| `messages` | `Vec<ThreadMessage>` | Ordered list of messages |
| `created_at` | `u64` | Epoch ms |

### ThreadMessage

| Field | Type | Description |
|-------|------|-------------|
| `author` | `enum` | `User` / `Assistant` |
| `content` | `Option<String>` | Markdown text |
| `file_path` | `Option<String>` | Attached file reference |
| `question` | `Option<Question>` | Embedded question form (if any) |
| `timestamp` | `u64` | Epoch ms |

### Status Rules

| Event | Status becomes |
|-------|---------------|
| User sends a message (via `Send`) | `THEIR_TURN` |
| Assistant receives message / question answer | `MY_TURN` |
| Idle while `MY_TURN` | → **Spine notification** |

---

## Tools

### Option A: Separate Send + Ask

| Tool | Params | Description |
|------|--------|-------------|
| `Send` | `thread_id` (required), `markdown` (optional), `file_path` (optional) — at least one | Post a message to a thread. Sets status → `THEIR_TURN`. |
| `Ask_question` | `thread_id` (required) + current question params | Ask user a question within thread context. No more out-of-thread questions. |
| `Read` | `thread_id` (required), `count` (optional, default 10) | Read last `k` messages. Inline if short, creates panel if long. |

### Option B: Unified Send (leaning towards this)

Merge `Send` and `Ask_question` into a single `Send` tool:
- `thread_id` (required)
- `markdown` (optional) — message body
- `file_path` (optional) — file attachment
- `questions` (optional) — same structure as current `ask_user_question` questions array

At least one of `markdown`, `file_path`, or `questions` must be provided. Questions get rendered as an interactive form within the thread context.

### Read

- `thread_id` (required)
- `count` (optional, default 10)
- **Display logic**: if total content is short → inline in tool result. If long → creates a dynamic panel (same pattern as console output currently).

---

## Coucou Integration

### Thread-attached coucous

Coucou notifications are now **attached to a thread**:
- New param: `thread_id` (required)
- Notification message becomes: *"Coucou from thread `{name}` fired: {message}"*
- Functionally identical otherwise — just scoped.

### Recurrent scheduling (new)

Extend coucou beyond one-shot timers:
- New `recurrence` param: `once` (default, current behavior), `hourly`, `daily`, `weekly`, `custom`
- For `custom`: `interval` param (e.g. `"30m"`, `"2h"`, `"1d"`)
- Recurrent coucous keep firing until explicitly cancelled
- Cancel via a new tool or param (e.g. `coucou_cancel(id)`)
- Use case: periodic check-ins on a thread (*"Any update on the benchmark run?"* every 30min)

---

## Fixed Panel: Thread List

A fixed panel (like Todo, Memories, Spine) showing all threads:

```
======= Threads =======
T1 [MY_TURN]  "lint audit"        (3 messages)
  └─ [user 10:42] Can you check the new deny-level lints?
  └─ [asst 10:43] Found 4 issues in the workspace config...
  └─ [user 10:45] Fix them please

T2 [THEIR_TURN]  "benchmark run"  (7 messages)
  └─ [asst 11:02] Launched gpt2-codegolf on v0.2.10...
  └─ [asst 11:05] Container running, watching for completion.
  └─ [user 11:20] Any results yet?
  └─ [asst 11:21] Still running. Will notify when done.
  └─ [coucou 11:51] Scheduled check-in: benchmark status?
```

- Shows **last 5 messages** per thread (configurable?)
- Thread status badge: `[MY_TURN]` / `[THEIR_TURN]`
- Context output includes all threads for LLM awareness

---

## TUI: Threads View (Ctrl+V cycle)

Add a **4th view mode** to the Ctrl+V cycle:

| # | View | Description |
|---|------|-------------|
| 1 | Normal | Panel + conversation (current default) |
| 2 | Focus | Conversation only |
| 3 | Panels | Panels only |
| **4** | **Threads** | **Thread-centered view** |

### Threads view layout

- **No panels displayed at all**
- **Sidebar**: list of threads (name + status badge + unread indicator)
- **Main area**: selected thread's full message history (scrollable)
- **Navigation**: `Tab` / `Shift+Tab` to switch between threads
- **Input**: typing in this view sends to the selected thread
- Completely different rendering — not panel-centered

---

## Locked Decisions

1. **Thread creation**: user-only (TUI), not via LLM tool. *Note: revisit adding an LLM-facing create tool later if needed.*
2. **Thread lifecycle**: archive only (hidden from panel, still searchable). Archive is user-only (TUI), not via LLM tool.
3. **Main conversation**: stays separate. It becomes the LLM's "consciousness" — long-term, should not be user-facing. Backend/Rust: **no changes** to existing conversation system. Threads are 90% addition, 10% modification (mostly ask_question → threads).
4. **LLM context**: only `MY_TURN` threads displayed in the fixed panel. Per-thread content capped (KB limit). `max_freeze` of 3 on the panel.
5. **Read scope**: Read on any thread returns content. Only sets focus when thread is `MY_TURN`.
6. **Focus bootstrap**: focus rules only enforced when at least one `MY_TURN` thread exists. Zero threads or all `THEIR_TURN` → rules deactivated, tools work normally.
7. **Thread priority**: AI chooses freely which `MY_TURN` thread to focus on. No FIFO enforcement.
8. **Dangling expiry**: soft escalation with increasing severity messages — never a hard block. Tools keep executing but the LLM gets yelled at with escalating intensity.
9. **Persistence**: shared across workers. All workers see all threads. Focus is per-worker.
10. **Panel order**: Threads sits between Todo and Spine in sidebar.
11. **Thread view input**: direct to thread, bypasses main conversation entirely.
12. **File attachments**: path reference only (no content snapshot). 
13. **Standalone ask_user_question**: **removed**. All questions go through `Send(thread_id, questions=...)`. No threadless questions.
14. **Questions are non-blocking**: Send with questions sets `THEIR_TURN` and AI moves on. Multiple threads can have pending questions simultaneously.
15. **Question rendering**: Threads view = inline in thread message area. Normal view = overlay popup tagged with thread name. Multiple pending = stacked/queued overlays.
16. **Subworkers**: orthogonal. All workers see all threads, focus is per-worker. No special interaction needed.

---

## Focus System

The LLM must **always be focused on exactly one thread** while working. There is no explicit `Focus` tool — focus is implicit.

### Focus Rules

| Event | Effect on focus |
|-------|----------------|
| `Read(thread_id)` | Sets focus → `thread_id` |
| `Send(thread_id)` | Clears focus → `null` |
| Conversation start / no threads | Focus = `null` (see bootstrap below) |

### Read & Focus Interaction

- `Read(thread_id)` on a **MY_TURN** thread → sets focus to that thread.
- `Read(thread_id)` on a **THEIR_TURN** thread → returns content but **does not change focus**. This allows peeking without accidentally claiming a thread.

### Enforcement

- **When focus is set**: all tools work normally. Tool calls operate in the context of the focused thread.
- **When focus is null AND a MY_TURN thread exists**: all tools **return errors** except `Think` and `Read`. Forces AI to pick a `MY_TURN` thread via `Read` before doing anything.
- **When focus is null AND no MY_TURN thread exists**: focus rules **deactivated** — all tools work normally. AI is idle, waiting for user to create/reply to a thread. Same applies at conversation start with zero threads.
- **AI chooses freely**: when multiple threads are `MY_TURN`, the AI picks whichever it wants based on context / spine notifications. No FIFO enforcement.

### Dangling Phase

After `Send` clears focus, allow **5 tool calls** for cleanup (closing panels, updating tree descriptions, committing, etc).

- **Exempt from countdown**: `Think`, `Queue_execute`, and individual tool calls within a queue flush.
- Each tool result in the dangling phase gets a countdown appended (e.g. `⚠️ Dangling phase: 3 remaining`).
- **After the 5 calls expire**: does NOT hard-block. Instead, escalating-severity messages are appended to every subsequent tool result:
  - Level 0–5: polite reminder — *"Please focus on an available thread using the Read tool."*
  - Level 6–15: firm — *"You MUST focus on a thread. Use Read(thread_id) now."*
  - Level 16–29: aggressive — *"STOP. Focus on a thread immediately."*
  - Level 30+: nuclear — escalating profanity. The system gets genuinely angry.
- This is a **soft escalation** — tools still execute, but the pressure becomes unbearable.

### Bootstrap

When no `MY_TURN` threads exist (conversation start, all threads `THEIR_TURN`, or zero threads), focus rules are **deactivated**. All tools work normally. A spine notification fires to signal "waiting for work" if threads exist but none are `MY_TURN`.

---

## Open Questions

*None — all design questions resolved. Ready for implementation.*
