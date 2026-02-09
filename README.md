<div align="center">

# Context Pilot

### The AI coding assistant that manages its own brain.

[![Stars](https://img.shields.io/github/stars/bigmoostache/context-pilot?style=social)](https://github.com/bigmoostache/context-pilot/stargazers)
[![CI](https://github.com/bigmoostache/context-pilot/actions/workflows/rust.yml/badge.svg)](https://github.com/bigmoostache/context-pilot/actions)
![Rust](https://img.shields.io/badge/rust-1.83+-orange.svg)
![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)
![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)

**35 tools · 5 LLM providers · Runs in your terminal · No Electron · No browser · No VS Code**

<img src="docs/image copy.png" alt="Context Pilot Screenshot" width="900"/>

</div>

---

## The Problem

You open Cursor, Copilot, or ChatGPT. You paste code. You explain the project. You paste more code. The AI forgets what you showed it 5 messages ago. You paste it again. The context fills up. You start a new chat. Repeat.

**Context Pilot is what happens when you let the AI manage its own brain.**

It explores your codebase on its own. It opens files, reads them, takes notes, closes them when it's done. It searches, greps, runs commands in the terminal. When the conversation gets long, it summarizes old messages and frees the space. It **never** runs out of context because it cleans up after itself.

> *"I explored 90 files in one session and ended at 14% context usage. I read everything, understood it, wrote descriptions, and freed the space."*
> — The AI, after its first full codebase review ([full writeup](docs/retex.md))

## Why Context Pilot?

| | Cursor / Copilot | Claude Code CLI | Aider | **Context Pilot** |
|---|---|---|---|---|
| Context management | Manual copy-paste | Automatic but opaque | File-level | **AI-driven: open, read, annotate, close, summarize** |
| Token visibility | Hidden | Hidden | Partial | **Real-time sidebar with per-element token counts** |
| Tool count | Limited | ~15 | ~10 | **35 tools across 14 modules** |
| Terminal integration | Embedded terminal | Spawns processes | No | **Full tmux pane management** |
| File exploration | File tree | Auto-read | Manual add | **Glob, grep, tree with annotations** |
| Git workflow | Basic | Good | Great | **Full git + GitHub CLI with cache invalidation** |
| Memory across turns | None | CLAUDE.md | None | **Persistent memories, todos, scratchpad, prompt library** |
| Multi-provider | No | Claude only | Many | **Claude, DeepSeek, Grok, Groq** |
| Architecture | Plugin / Cloud | CLI | Python | **Rust TUI, single binary, ~50ms frames** |

## What can it do?

### 🔍 Explore
Opens files, navigates your directory tree, searches with glob and regex. Annotates everything it finds so it remembers later — even after closing the file.

### 🛠️ Build
Edits files, creates new ones, runs terminal commands, manages git branches, opens pull requests. All from within the conversation.

### 🧠 Think
Keeps todo lists, scratchpad notes, persistent memories. Plans before it acts. Breaks down complex tasks into steps.

### ♻️ Stay Sharp
Tracks every token in real-time. When things get heavy, it summarizes old messages, closes files it doesn't need, archives conversation history. You never have to say "you're running out of context."

## Tools

<details>
<summary><b>35 tools across 9 categories</b> (click to expand)</summary>

| Category | Tools | Description |
|----------|-------|-------------|
| **Context** | `context_close`, `context_message_status`, `system_reload`, `tool_manage`, `module_toggle`, `panel_goto_page` | Manage what's in the AI's working memory |
| **System Prompts** | `system_create`, `system_edit`, `system_delete`, `system_load` | Create and switch between AI personalities |
| **Files** | `file_open`, `file_edit`, `file_write` | Full file system access with syntax highlighting |
| **Search** | `file_glob`, `file_grep` | Find files by pattern, search contents with regex |
| **Tree** | `tree_filter`, `tree_toggle`, `tree_describe` | Directory exploration with persistent annotations |
| **Git** | `git_execute`, `git_configure_p6` | Full git CLI with smart cache invalidation |
| **GitHub** | `gh_execute` | Full GitHub CLI — PRs, issues, releases, actions |
| **Console** | `console_create`, `console_edit`, `console_send_keys`, `console_sleep` | Tmux terminal management |
| **Notes** | `todo_create`, `todo_update`, `memory_create`, `memory_update`, `scratchpad_create_cell`, `scratchpad_edit_cell`, `scratchpad_wipe` | Persistent memory across the conversation |
| **Presets** | `preset_snapshot_myself`, `preset_load` | Save and restore complete workspace configurations |

</details>

## How It Works

```
┌─────────────────────────────────────────────────────┐
│                    Context Pilot                     │
│                                                      │
│  ┌─ Sidebar ──┐  ┌─ Main Panel ──────────────────┐  │
│  │ P0 System  │  │                                │  │
│  │ P1 Chat    │  │   Active panel content with    │  │
│  │ P2 Tree    │  │   syntax highlighting,         │  │
│  │ P3 Todos   │  │   markdown rendering,          │  │
│  │ P4 Memory  │  │   and live updates             │  │
│  │ P5 World   │  │                                │  │
│  │ P6 Git     │  │                                │  │
│  │ P7 Scratch │  │                                │  │
│  │ ────────── │  │                                │  │
│  │ P8 file.rs │  │                                │  │
│  │ P9 grep    │  │                                │  │
│  │ P10 git log│  │                                │  │
│  │ P11 tmux   │  │                                │  │
│  │            │  │                                │  │
│  │ 6547/200K  │  │                                │  │
│  │ ████░░ 3%  │  │                                │  │
│  └────────────┘  └────────────────────────────────┘  │
│  ┌─ Status Bar ──────────────────────────────────┐   │
│  │ Claude 3.5 Sonnet │ master +3/-1 │ 142 chars  │   │
│  └───────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘

Each panel is a live context element with its own token count.
The AI opens and closes panels as needed to stay within budget.
```

Every context element — files, search results, terminal output, notes — lives as a **panel** with a real-time token count. The AI sees exactly what's consuming its context and can close anything it doesn't need anymore.

## Get Started

### Prerequisites
- Rust 1.83+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- tmux (`apt install tmux` / `brew install tmux`)
- At least one API key (Anthropic, xAI, Groq, or DeepSeek)

### Install

```bash
git clone https://github.com/bigmoostache/context-pilot.git
cd context-pilot

# Add your API key(s) to .env
cat > .env << 'EOF'
ANTHROPIC_API_KEY=your_key_here
# Optional: add more providers
# XAI_API_KEY=your_key
# GROQ_API_KEY=your_key
# DEEPSEEK_API_KEY=your_key
# GITHUB_TOKEN=your_token
EOF

cargo build --release
./run.sh
```

That's it. Talk to it. Ask it to explore your codebase.

### First things to try

```
> explore this codebase and describe what you find
> find all TODO comments in the project
> create a new feature branch and implement X
> review the recent git history and summarize changes
```

## Architecture

Built in **Rust** with [Ratatui](https://github.com/ratatui/ratatui) for the terminal UI. Single-threaded event loop with background workers for caching, file watching, and LLM streaming.

- **14 modules** — core, files, git, github, glob, grep, memory, preset, prompt, scratchpad, tmux, todo, tree, plus a module system for adding your own
- **5 LLM providers** — Anthropic (direct API), Claude Code (OAuth), DeepSeek, Grok (xAI), Groq
- **Smart caching** — SHA-256 hash-based change detection, background refresh, inotify file watching
- **Git-aware** — Regex-based cache invalidation (mutating git commands invalidate affected read-only panels)
- **Preset system** — Save and load complete workspace configurations (modules, tools, panel states)

## Contribute

This project is young and moving fast. Your PR won't sit in a queue for 3 months.

**Ideas for contributions:**
- 🆕 New LLM provider (OpenAI, Gemini, local models via Ollama)
- 🎨 New color themes (it's just YAML — see `yamls/themes.yaml`)
- 📖 Better markdown rendering
- 🧪 Test coverage
- 📝 Tutorials and guides
- 🐛 Bug reports and feature requests

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## License

[AGPL-3.0](LICENSE) for open source. Commercial license available — [open an issue](https://github.com/bigmoostache/context-pilot/issues/new) to discuss.

---

<div align="center">

**Built with Rust. Runs in your terminal. The AI manages its own context.**

⭐ **If this is useful to you, star the repo** — it helps others find it.

[Get Started](#get-started) · [Read the AI's Self-Review](docs/retex.md) · [Contribute](#contribute)

</div>
