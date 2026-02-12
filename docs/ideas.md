# Ideas — What's Missing

## Already Shipped (keeping for historical reference)

### ~~Log Journaling~~ ✓
Tree-structured log summarization with `log_summarize` and `log_toggle`. Chunked persistence. Shipped on `log-journaling` branch, merged PR #41.

### ~~Conversation History Management~~ ✓
`close_conversation_history` with log/memory extraction. Automatic detachment and summarization when context exceeds threshold. ConversationHistory panels with UIDs persist across reloads.

---

## High Priority — Would Transform the Tool

### 1. Build-Test-Fix Loop

The single biggest gap for autonomous operation. Right now the AI can edit code, send a build command to tmux, read the output, and fix errors — but it's ad-hoc every time. There should be a **structured verification cycle**:

1. Edit code
2. Build (detect success/failure automatically)
3. Run tests (parse results)
4. If failure → read error → fix → goto 2
5. If success → continue

This isn't just convenience — it's what makes the difference between "autonomous for 2 minutes" and "autonomous for 20 minutes." Without it, the AI drifts. SWE-bench harnesses proved that structured feedback loops are the #1 predictor of autonomous coding success.

Could be a module that wraps the console: you configure the build command and test command once, and the AI gets structured pass/fail/error feedback instead of raw terminal text.

---

### 2. Multi-Worker Parallel Execution

The state model already has `worker_id` and per-worker data. The infrastructure is 80% there. What's missing:

- Spawning workers from within a conversation
- Workers running in parallel on independent tasks
- A coordination layer (one worker finishes → notifies orchestrator)
- Branch-per-worker isolation (each worker on its own git branch)

The dream: "Refactor the auth module, update the API docs, and add tests for the parser" → three workers, three branches, three PRs. The orchestrator merges when all pass CI.

This is the feature that would put Context Pilot genuinely ahead of everything else in the space. Nobody does multi-agent parallel development well today.

---

### 3. Diff Review Before Commit — Self-Review

Before any commit, the AI should review its own changes against the original intent. Not just "do the tests pass" — a deliberate pause where it diffs what it actually changed vs. what was asked.

Key insight: **the AI that wrote the code is the worst reviewer of it.** Ideally this would be a separate worker with fresh context, reviewing the diff cold. Think: internal PR review before the human even sees it.

Could be triggered automatically by the spine when the AI calls `git commit`, or as a slash command (`/review`).

---

### 4. Code Intelligence Integration

No LSP, no type info, no go-to-definition, no find-all-references. The AI navigates by grepping raw text. For small projects this is fine; for 500+ file codebases it becomes a serious context budget drain.

Options:
- **rust-analyzer / LSP integration**: type-at-point, definition lookup, references
- **Tree-sitter**: lightweight structural parsing (function signatures, imports, class hierarchies) without a full language server
- **Symbol index**: background-built index of functions/types/modules, queryable as a tool

Even basic symbol awareness ("show me all functions that take a `&State`") would dramatically reduce wasted context on navigation.

---

### 5. Automated Workflow Hooks

Everything is conversation-driven. No way to say "on every commit, run clippy" or "when a file in `src/modules/` changes, re-run tests." The system is reactive (human asks → AI does) but never proactive.

A simple event system:
- File change → trigger action
- Git commit → trigger review
- Test failure → trigger fix attempt
- Timer → periodic health check

This turns Context Pilot from "powerful assistant you talk to" into "autonomous agent that watches and acts."

---

## Medium Priority — Would Significantly Improve UX

### 6. Conversation Forking

Can't branch conversations. Want to explore two approaches? Pick one. Should be able to fork at any point — same context, same history, divergent paths. Kill the loser.

Different from workers: workers are parallel tasks with isolated state. Forking is "what if I went a different direction from this exact point?"

---

### 7. Cost Tracking & Token Economics Dashboard

Every LLM call has a dollar cost. Users need:
- Cost per message, per conversation, per session, cumulative
- Which panels eat the most tokens relative to their usefulness
- Cache hit rates (prompt caching is implemented — is it saving money?)
- "This panel costs $0.02/message and you haven't referenced it in 5 turns — consider closing"

The spine tracks cost as a guard rail, but there's no visibility or optimization. Making economics visible would make users smarter.

---

### 8. Smart Context Management

Context management is powerful but entirely manual. Ideas for automation:
- **Auto-close stale panels**: "this file panel hasn't been referenced in 10 turns, close it"
- **Reference tracking**: which panels does the AI actually use in its responses?
- **Priority-based paging**: when approaching budget, page out least-recently-referenced panels
- **Panel cost annotations**: show cost-per-message impact of each panel in the sidebar

The goal: the context budget manages itself, and the human only intervenes for strategic decisions.

---

### 9. Project Onboarding — Automatic Context Discovery

Cold start friction. First time in a new repo, the AI should automatically:
- Read README, scan structure, identify language/framework
- Find entry point, build system, CI config
- Check recent git history for active areas
- Populate memories, tree descriptions, project summary scratchpad

The first 5 minutes of every new project shouldn't be "let me explain my codebase."

---

### 10. MCP Server Support

Model Context Protocol is becoming the standard for tool interoperability. Consuming MCP servers would give Context Pilot every tool anyone builds for MCP — database connectors, API clients, Kubernetes, Jira, Slack.

The module system is already close conceptually. The gap is the protocol layer.

---

## Lower Priority — Nice to Have

### 11. Session Replay & Teaching

Record entire sessions (messages, tool calls, file edits, panel state) and make them replayable. Scrub through a timeline seeing exactly what the AI saw at each step.

Teaching tool: senior devs record problem-solving sessions, juniors replay step-by-step. Teams audit AI usage. Debug bad AI behavior by replaying exact context.

---

### 12. Log Stream Observation Panel

Dedicated module for tailing log files / docker containers / `journalctl`. Passive observation (vs console's interactive terminals). Filtering, log level highlighting, auto-scroll.

Combined with workflow hooks, the AI could watch logs and re-engage on exceptions: "I see a panic in the service I just modified — let me check the stacktrace."

---

### 13. Checkpoint & Rollback

Lightweight snapshots before every file edit. "Undo the last 3 things the AI did" without messing with git history. A local undo stack specifically for AI-made changes.

The AI itself should be able to use this: "that approach didn't work, let me rollback and try differently."

---

### 14. Test Suite for Context Pilot Itself

The codebase is reaching the size where manual testing stops scaling. Priority targets:
- `file_edit` normalized whitespace matching (property tests)
- `log_summarize` / `log_toggle` tree manipulation
- Git/GitHub command classification
- Cache invalidation rules
- Persistence round-trips (save → load → verify)
- Panel token estimation accuracy

`cargo-mutants` on the module system would reveal a lot about test coverage gaps.

---

## The Killer Combo

**Workers + Build-Test-Fix Loop + Self-Review + Workflow Hooks:**

AI gets a task → spins up a worker → makes changes on a branch → build-test-fix loop until green → separate reviewer worker examines the diff → if good, opens a PR → notification alerts you → you approve with a one-liner.

Nobody does this well today. The infrastructure is 80% there. The module system, presets, git integration, persistence, spine, guard rails — all built. Workers and verification loops are the missing pieces, and self-review is the missing workflow.

The project that cracks **trustworthy autonomous multi-step development** — where the AI works independently for 20 minutes and comes back with a clean PR — that's the one that wins.
