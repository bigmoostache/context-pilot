# Architecture Overview

Context Pilot is a Rust TUI application built around a **library + binary + plugin modules** pattern. The codebase is split across three layers: `crates/cp-base` (shared foundation), `crates/cp-mod-*` (feature modules), and `src/` (the binary application).

## Dependency Direction

The dependency graph flows one way. `cp-base` depends on nothing internal — only external crates like serde, ratatui, and sha2. Each `cp-mod-*` crate depends only on `cp-base`, implementing the `Module` and `Panel` traits it defines. The binary (`src/`) depends on everything — cp-base and all module crates — wiring them together into the running application. This means module crates are independently compilable: changing `cp-mod-todo` doesn't recompile `cp-mod-git`.

```
cp-mod-* crates ──depend on──▶ cp-base
src/ (binary)   ──depends on──▶ cp-base + all cp-mod-* crates
```

## cp-base: The Shared Foundation (~24k lines)

cp-base is the "lingua franca" of the codebase. It defines what things ARE (types) and what they CAN DO (traits), but never HOW things happen at the application level. No event loops, no UI rendering, no LLM API calls live here.

**State** (`state/runtime.rs`) — The central `State` struct holds all runtime data: messages, context panels, todos, memories, git status, model selection, token counts, and more (~150 fields). It lives in cp-base so every crate can read and write it via `&mut State`.

**Types** (`types/`) — Pure data types for each domain: `TodoItem`, `MemoryItem`, `LogEntry`, `Notification`, `ScratchpadCell`, `GitFileChange`, `PromptItem`, `TreeFileDescription`. No behavior, just structs and enums.

**Module trait** (`modules.rs`) — The core abstraction every module implements: `id()`, `tool_definitions()`, `execute_tool()`, `create_panel()`, `save_module_data()`, `load_module_data()`. This trait is in cp-base so module crates can implement it without depending on the binary.

**Panel trait** (`panels.rs`) — How panels render and generate LLM context: `title()`, `content()`, `refresh()`, `context()`, `refresh_cache()`, `apply_cache_update()`. Also provides utilities like `paginate_content()`, `update_if_changed()`, and `mark_panels_dirty()`.

**Tool definitions** (`tool_defs.rs`) — `ToolDefinition`, `ToolParam`, `ParamType`, `ToolCategory` — the schema system for defining tools that get sent to the LLM as JSON schema.

**Actions** (`actions.rs`) — The `Action` and `ActionResult` enums — the message-passing vocabulary between the event system, UI, and state mutations.

**Cache** (`cache.rs`) — `CacheRequest` and `CacheUpdate` enums for background panel refresh (file content, git status, tmux capture). Also `hash_content()` for change detection.

**Shared utilities** — Shell argument parsing and command classification (`classify.rs`), table rendering with Unicode box-drawing (`ui.rs`), shared constants and theme colors (`constants.rs`, `config.rs`).

## cp-mod-* Crates: Feature Modules (~10.5k lines)

Each `cp-mod-*` crate is a self-contained feature module that implements the `Module` and `Panel` traits from cp-base. There are 14 module crates: files, git, github, glob, grep, logs, memory, preset, prompt, scratchpad, spine, tmux, todo, and tree. Each depends only on cp-base and provides its own tool definitions, tool execution logic, and panel rendering.

## src/: The Binary Application (~15.5k lines)

The binary is where everything comes together. It owns the event loop, renders the UI, calls LLM APIs, and persists state.

**Event loop** (`core/app.rs`) — The heart of the application. `App::run()` polls for keyboard input, processes LLM stream events, manages the typewriter buffer for character-by-character display, handles the tool execution pipeline (tool call → execute → result → continue streaming), runs file watchers, timer-based cache deprecation, spine auto-continuation, and background persistence. Adaptive polling runs at 8ms during streaming and 50ms when idle.

**Context preparation** (`core/context.rs`) — `prepare_stream_context()` is called before every LLM API request. It detaches old conversation chunks, refreshes panel token counts, collects and sorts context items by freshness, tracks panel cache costs, and builds the final message list.

**LLM providers** (`llms/`) — Concrete streaming clients for Anthropic, Claude Code (OAuth), Grok, Groq, and DeepSeek. Each implements streaming, tool call parsing, and API health checks. Context panels are injected as fake tool call/result pairs.

**Module registry** (`modules/mod.rs`) — `all_modules()` creates instances of all modules. `dispatch_tool()` routes tool calls to the right module. `active_tool_definitions()` collects tools from active modules. The `module_toggle` tool and dependency validation live here.

**UI rendering** (`ui/`) — Ratatui-based terminal rendering: sidebar, markdown, syntax highlighting, input area, spinner animations, and the theme system.

**Persistence** (`persistence/`) — Loads and saves state (config.json, worker.json) and individual message files. Uses a background `PersistenceWriter` thread for non-blocking disk I/O. Handles multi-instance ownership checks.

**Other binary concerns** — Keyboard event mapping (`events.rs`), TL;DR background summarization (`background.rs`), file system watching (`watcher.rs`), syntax highlighting (`highlight.rs`, injected into State as a callback so module crates can use it), and the command palette (`help/`).
