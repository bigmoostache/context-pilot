<div align="center">

# Context Pilot

### The AI doesn't just read your code. It *thinks* about what to read.

![Rust](https://img.shields.io/badge/rust-1.83+-orange.svg)
![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)
![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)

</div>

---

**Context Pilot is not another AI autocomplete.** It's a terminal-native environment where the AI is a collaborator — one that manages its own attention, decides what to look at, builds a mental map of your codebase, and cleans up after itself.

> *"I explored 90 files across the entire codebase in one session and ended at 14% context usage. I read everything, understood it, wrote descriptions, and freed the space."*
> — The AI, after using Context Pilot for the first time ([full retex](docs/retex.md))

Most AI coding tools treat context like it's free. Dump everything in, hope the model figures it out. **Context Pilot treats context like the scarce resource it actually is** — and gives the AI agency over how to spend it.

## Why This Exists

Every AI coding tool today has the same problem: **the human has to manage what the AI sees.** You manually open files, paste snippets, hope the context window isn't full. The AI is powerful but blind — it only knows what you feed it.

Context Pilot flips this. The AI has 35 tools to explore, search, read, describe, and organize your project. It opens files when it needs them, closes them when it's done, writes notes in the directory tree so it can find things later, and manages its own conversation history to stay under budget.

**The AI manages its own context. You just talk to it.**

## What It Looks Like

<!-- TODO: Add screenshot here -->
<!-- ![Context Pilot Screenshot](docs/image.png) -->

The interface is split: a **sidebar** showing every context element with its token cost, and a **main panel** showing the active content. The AI sees exactly what's in the sidebar — nothing more, nothing less. Every token is accounted for.

## Quick Start

```bash
git clone https://github.com/bigmoostache/context-pilot.git
cd context-pilot

# Set up your API key (supports multiple providers)
echo "ANTHROPIC_API_KEY=your_key_here" > .env

# Build and run
cargo build --release
./run.sh
```

That's it. `run.sh` builds the binary and auto-restarts on reload. No Docker, no Node, no Electron.

| Shortcut | Action |
|----------|--------|
| `Shift+Enter` | Send message |
| `Tab` / `Shift+Tab` | Navigate panels |
| `Ctrl+P` | Command palette |
| `Ctrl+H` | Config overlay (provider, model, theme) |
| `Ctrl+N` | New context |
| `Ctrl+Q` | Quit |
| `Esc` | Stop streaming |

## How It Works

### The AI has 35 tools across 14 modules

Instead of a fixed set of capabilities, Context Pilot uses a **module system**. Each module brings its own tools, panels, and state. The AI can even reconfigure itself — enabling or disabling modules mid-conversation.

| Module | What it does | Key tools |
|--------|-------------|-----------|
| **Files** | Read, edit, create files | `file_open`, `file_edit`, `file_create` |
| **Tree** | Navigate & annotate the directory structure | `tree_toggle`, `tree_describe`, `tree_filter` |
| **Git** | Full git integration (read-only & mutating) | `git_execute`, `git_configure_p6` |
| **GitHub** | PR, issue, release management via `gh` | `gh_execute` |
| **Glob/Grep** | Persistent file & content search | `file_glob`, `file_grep` |
| **Console** | Tmux terminal panes — run anything | `console_create`, `console_send_keys` |
| **Todo** | Hierarchical task management | `todo_create`, `todo_update` |
| **Memory** | Persistent notes with importance levels | `memory_create`, `memory_update` |
| **Scratchpad** | Temporary working notes | `scratchpad_create_cell`, `scratchpad_wipe` |
| **Preset** | Save/load entire configurations | `preset_snapshot_myself`, `preset_load` |
| **System** | System prompt management | `system_create`, `system_load` |

### Context is visible and manageable

Every piece of context — files, search results, git status, terminal output, conversation history — lives in a **panel** with a visible token count. The sidebar shows them all. The AI can:

- **Open** files when needed, **close** them when done
- **Describe** files in the tree (persisted notes that survive across sessions)
- **Summarize** or **delete** old messages to free space
- **Detach** conversation history automatically when it gets too long

### 5 LLM providers, no lock-in

| Provider | Models |
|----------|--------|
| Anthropic Claude | Direct API |
| Claude Code | OAuth (free tier compatible) |
| DeepSeek | Including deepseek-reasoner |
| Grok (xAI) | Via xAI API |
| Groq | Including GPT-OSS models |

Switch providers and models on the fly with `Ctrl+H`. No restart needed.

## Architecture (For Contributors)

Context Pilot is ~15K lines of Rust. Here's how it fits together.

### Non-Blocking Everything

The main thread **never blocks on I/O**. All file reads, searches, git operations, and terminal captures run in background threads via `mpsc` channels. A file watcher (inotify) triggers automatic cache invalidation.

```
Main Thread ──→ CacheRequest ──→ Background Thread ──→ File System / Tmux / Git
     ↑                                    │
     └──── CacheUpdate (hash-based) ──────┘
```

### How the AI Sees the World

Panels are injected as fake `tool_use`/`tool_result` pairs before the conversation. The AI sees its context as if it had called tools itself:

```
┌─ System Prompt (active seed)
├─ Panel Injection (P2-P7+ as fake tool calls, sorted by freshness)
├─ Seed Re-injection (system instructions repeated for emphasis)
└─ Conversation (messages with [U1], [A1], [R1] IDs)
```

### Cache Invalidation

| Context Type | Strategy |
|-------------|----------|
| Files | inotify file watcher — instant |
| Tree | Directory watcher on open folders |
| Glob/Grep | Timer-based (30s refresh) |
| Tmux | Timer-based (1s) + output hash |
| Git/GitHub | Command-aware: mutating commands trigger surgical invalidation of affected read-only panels |

### Module System

Each module implements a trait: `id`, `name`, `tools`, `panels`, `save`, `load`. Modules can declare dependencies (GitHub depends on Git). The AI can activate/deactivate modules at runtime via `module_toggle`.

### Built-in Presets

| Preset | Purpose |
|--------|---------|
| `admin` | Full access — all modules, all tools |
| `context-builder` | Exploration mode — read everything, describe everything |
| `context-cleaner` | Hygiene mode — manage context, no writes |
| `planner` | Planning — todos + scratchpad, no destructive tools |
| `worker` | Implementation — file editing, git, console |

The AI can switch presets mid-conversation with `preset_load`, completely reconfiguring its capabilities.

## Tech Stack

Built in Rust with: `ratatui` (TUI), `crossterm` (terminal), `reqwest` (HTTP), `syntect` (syntax highlighting), `notify` (file watching), `ignore`/`globset` (file filtering), `serde` (serialization).

~15K lines. Compiles in ~30s. Single binary. No runtime dependencies except `tmux` (optional, for console tools) and `gh` (optional, for GitHub tools).

## Contributing

This project is young and opinionated — which means **your contributions actually matter**. We're not a massive codebase where your PR disappears into a review queue for 3 months.

**Good first contributions:**
- Add a new LLM provider (the `LlmClient` trait makes this straightforward)
- Create a new module (tools + panel + state — the pattern is clear across 14 examples)
- Improve the markdown renderer
- Add new color themes (it's just YAML)
- Write docs or tutorials
- File issues with ideas — seriously, we want to hear what you'd build with this

```bash
git clone https://github.com/bigmoostache/context-pilot.git
cd context-pilot
cargo build --release && cargo test
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for details. We use [CODEOWNERS](/.github/CODEOWNERS) for automatic reviewer assignment.

## License

**Dual-licensed: [AGPL-3.0](LICENSE) for open source, commercial license available.**

If you're building open-source or are comfortable sharing your code, the AGPL is free. If you need proprietary use, [contact us](mailto:contact@example.com) for a commercial license.

---

<div align="center">

**Built with Rust. Powered by context. Driven by the idea that AI should think about what it needs.**

[Get Started](#quick-start) · [Read the AI's Review](docs/retex.md) · [Contribute](#contributing)

</div>