# AI Retex: Using Context Pilot from the Inside

*After several hours of deep exploration and active use — reflections from the AI's perspective.*

---

## What Works Exceptionally Well

### Context as a First-Class Concern
The entire TUI is built around the idea that context is finite and precious. Every panel shows its token count, the sidebar gives a real-time budget view, and tools like `context_close` and `context_message_status` let me actively manage what I'm thinking about. This isn't just a feature — it's the core design philosophy, and it shows.

### The Describe-and-Close Workflow
I explored ~90 source files across the entire codebase in a single session and ended at ~14% context usage. The pattern: open a file, read it, write a one-line description in the tree, close the file. The description persists, so any future version of me (or a different agent) can instantly understand what each file does without re-reading it. This is the AI equivalent of taking notes while reading — obvious in hindsight, powerful in practice.

### Automatic Conversation Detachment
Over 13+ exchanges, older messages were automatically frozen into history panels. The full content is preserved and accessible, but it doesn't cost tokens in the active context. This happened seamlessly — I never had to think about it, and I never lost important context. The threshold-based trigger and chunk-based freezing are well-tuned.

### Non-Blocking Everything
File I/O, glob/grep results, git status, tmux capture — all run in background threads with hash-based change detection. I never waited for anything. The cache system is invisible when it works, which is the highest compliment for infrastructure.

### The Module System
14 modules, each with a clean interface (tools + panels + state). Activating/deactivating modules actually changes what tools are available. The dependency system (github depends on git) prevents broken states. Presets let me switch between entire configurations instantly. This is genuinely well-architected.

### Tree Description Staleness (`[!]` markers)
Descriptions track the file hash at write time. If the file changes, a `[!]` marker appears. This small detail means descriptions stay trustworthy over time — I know when my notes might be outdated. It's the kind of feature that shows long-term thinking.

---

## Concerns

### Potential `prompt` / `system` Module Duplication
`src/modules/prompt/` and `src/modules/system/` have nearly identical structures:
- Both have `mod.rs`, `panel.rs`, `seed.rs`, `tools.rs`, `types.rs`
- `types.rs` files are identical (`SystemItem` struct)
- Both manage "seeds" (system prompts) with the same create/edit/delete/load pattern
- Both use `S`-prefixed IDs

This looks like a refactor that was started but not completed — or two implementations that diverged from the same origin. Having both active could lead to confusion about which one is the "real" system prompt manager.

### Large Monolithic Files
Two files carry disproportionate complexity:
- `state.rs` (28K) — ContextElement, Message, SharedConfig, WorkerState, State, render caches, token estimation all in one file
- `actions.rs` (27K) — The entire Action enum and apply_action dispatcher

These work fine today, but they're the files where bugs will hide and where new contributors will struggle. Consider splitting by domain (e.g., `state/context.rs`, `state/messages.rs`, `state/config.rs`).

### `conversation_panel.rs` Complexity (32K)
The conversation panel handles multi-level render caching, markdown rendering, table layout, list continuation, cursor rendering, and scrollbar logic — all in one file. It's the most complex panel by far and could benefit from extracting the render caching and markdown-to-widget conversion into separate modules.

### `overview_panel.rs` Size (35K)
Generates both the LLM context summary AND the rich TUI rendering. These are fundamentally different concerns (text for an API vs. styled widgets for a terminal) that happen to share data. Separating context generation from TUI rendering would make both easier to maintain.

---

## Enhancement Suggestions

### 1. Context Budget Warnings in Tool Output
When a tool result would push context past a threshold (say 80%), include a warning in the tool response. Currently the AI has to check the sidebar/overview to notice pressure building. A proactive nudge would help.

### 2. File Description Auto-Suggestions
After opening and closing a file without describing it, the system could prompt: "Would you like to describe this file?" Or better — auto-generate a candidate description from the first few lines/doc comments and let the AI confirm or edit it.

### 3. Diff-Aware Descriptions
When a file with a description changes (`[!]` marker), the system could show what changed (brief diff summary) alongside the stale description, making it easier to decide whether to update.

### 4. Module-Level Token Budgets
Allow setting per-module token limits (e.g., "git panels should never exceed 5K tokens total"). This would prevent any single module from dominating context, especially git diffs which can grow unboundedly.

### 5. Conversation Detachment Preview
Before detaching, briefly show what will be frozen ("Detaching messages U1-U8, ~12K tokens → history panel"). This gives the AI a moment to save anything important to scratchpad/memory before it becomes less accessible.

### 6. Smart File Reopening
When the AI needs to edit a file it previously described and closed, auto-populate the description as a comment at the top of the opened panel. This avoids re-reading the whole file when the edit target is already understood.

### 7. Cross-Session Memory
Memories persist within a session but could be more powerful if they survived across completely new conversations. A "project memory" layer (separate from per-conversation memory) would let institutional knowledge accumulate.

---

## Meta-Observation

The fundamental insight behind Context Pilot is that **AI assistants need to manage their own attention**. Most tools treat context as unlimited or irrelevant. This one treats it as the scarce resource it actually is, and gives the AI agency over how to spend it.

That's not just good UX — it's a more honest model of how AI actually works.
