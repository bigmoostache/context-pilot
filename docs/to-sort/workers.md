# Workers & Threading Architecture

This document describes all threads, workers, and concurrent processes in the TUI application.

## Overview

The application uses a **single-threaded main loop** with **background worker threads** for I/O-bound operations. Communication between the main thread and workers is done via `std::sync::mpsc` channels.

```
┌──────────────────────────────────────────────────────────────────────────┐
│                           MAIN THREAD                                    │
│                                                                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │   Event     │  │   Render    │  │   State     │  │   Action    │    │
│  │   Loop      │  │   UI        │  │   Mgmt      │  │   Handler   │    │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘    │
│         │                                                                │
│         ▼                                                                │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                 Channel Receivers (polled)                        │   │
│  │  • rx: StreamEvent         • tldr_rx: TlDrResult                 │   │
│  │  • cache_rx: CacheUpdate   • watcher.poll_events()               │   │
│  └──────────────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────────────┘
                                    ▲
                                    │ mpsc channels
                                    ▼
┌──────────────────────────────────────────────────────────────────────────┐
│                        BACKGROUND THREADS                                │
│                                                                          │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐         │
│  │  LLM Streaming  │  │  Cache Pool     │  │   TL;DR Worker  │         │
│  │  (1 at a time)  │  │  (6 workers)    │  │  (1 at a time)  │         │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘         │
│                                                                          │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐         │
│  │  Persistence    │  │  GH Watcher     │  │  File Watcher   │         │
│  │  Writer (1)     │  │  (1 poller)     │  │  (notify crate) │         │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘         │
└──────────────────────────────────────────────────────────────────────────┘
```

## Thread Spawning Locations

### 1. LLM Streaming (`src/llms/mod.rs`)

**Function:** `start_streaming()`

Spawns a thread for each LLM API call to stream responses.

```rust
std::thread::spawn(move || {
    let request = LlmRequest { model, messages, context_items, tools, ... };
    if let Err(e) = client.stream(request, tx.clone()) {
        let _ = tx.send(StreamEvent::Error(e.to_string()));
    }
});
```

- **Trigger:** User sends a message, tool execution continues, or spine auto-continuation fires
- **Lifetime:** Until stream completes, errors, or is cancelled
- **Channel:** `Sender<StreamEvent>` → `rx` in app.rs
- **Concurrency:** Only one active at a time (previous cancelled if new starts)

**Function:** `start_api_check()`

Spawns a thread to check API connectivity.

```rust
std::thread::spawn(move || {
    let result = client.check_api(&model);
    let _ = tx.send(result);
});
```

- **Trigger:** Config panel API check action
- **Lifetime:** Short-lived (single request)
- **Channel:** `Sender<ApiCheckResult>`

### 2. Cache Pool (`src/cache.rs`)

**Struct:** `CachePool` — bounded thread pool with `CACHE_POOL_SIZE` (6) workers.

Workers pull `(CacheRequest, Sender<CacheUpdate>)` pairs from a shared job channel.
Created once as a global `LazyLock<CachePool>` static.

```rust
pub struct CachePool {
    job_tx: Sender<(CacheRequest, Sender<CacheUpdate>)>,
}

impl CachePool {
    pub fn new() -> Self {
        let (job_tx, job_rx) = mpsc::channel();
        let job_rx = Arc::new(Mutex::new(job_rx));
        for i in 0..CACHE_POOL_SIZE {
            let rx = Arc::clone(&job_rx);
            thread::Builder::new()
                .name(format!("cache-worker-{}", i))
                .spawn(move || { /* pull jobs from rx, dispatch to panel.refresh_cache() */ })
                .ok();
        }
        Self { job_tx }
    }
}

static CACHE_POOL: LazyLock<CachePool> = LazyLock::new(CachePool::new);
```

**Function:** `process_cache_request()` submits to the pool:

```rust
pub fn process_cache_request(request: CacheRequest, tx: Sender<CacheUpdate>) {
    CACHE_POOL.submit(request, tx);
}
```

| Request Type | Operation | External Processes |
|---|---|---|
| `RefreshFile` | Read file, compute SHA-256 hash | None |
| `RefreshTree` | Generate directory tree string (gitignore-filtered) | None |
| `RefreshGlob` | Compute glob pattern matches (WalkBuilder) | None |
| `RefreshGrep` | Search file contents with regex (WalkBuilder) | None |
| `RefreshTmux` | Capture terminal pane content | `tmux capture-pane` |
| `RefreshGitStatus` | Full git repository status | `git status`, `git branch`, `git diff`, etc. |
| `RefreshGitResult` | Re-execute read-only git command | `git <command>` via `run_with_timeout` |
| `RefreshGithubResult` | Re-execute read-only gh command | `gh <command>` via `run_with_timeout` |

- **Trigger:** Timer-based deprecation, file watcher events, tool actions
- **Lifetime:** Pool threads live for entire application lifetime
- **Channel:** Each job carries its own `Sender<CacheUpdate>` → `cache_rx` in app.rs
- **Concurrency:** Up to 6 jobs in parallel; excess queued in channel

### 3. TL;DR Generation (`src/background.rs`)

**Function:** `generate_tldr()`

Spawns a thread for each message summarization request. Short messages
(< `tldr_min_tokens()`) are used directly; longer ones are summarized via
a blocking Anthropic API call.

```rust
pub fn generate_tldr(message_id: String, content: String, tx: Sender<TlDrResult>) {
    thread::spawn(move || {
        let token_count = estimate_tokens(&content);
        if token_count < prompts::tldr_min_tokens() {
            tx.send(TlDrResult { message_id, tl_dr: content, token_count });
            return;
        }
        match summarize_content(&content) {
            Ok(summary) => tx.send(TlDrResult { message_id, tl_dr: summary, ... }),
            Err(_) => tx.send(TlDrResult { message_id, tl_dr: truncated, ... }),
        }
    });
}
```

- **Trigger:** After each user/assistant message is finalized
- **Lifetime:** Until API call completes
- **Channel:** `Sender<TlDrResult>` → `tldr_rx` in app.rs
- **Concurrency:** Multiple can run in parallel (tracked by `state.pending_tldrs` counter)

### 4. File Watcher (`src/watcher.rs`)

**Struct:** `FileWatcher` using `notify::RecommendedWatcher`

The `notify` crate internally spawns a background thread for filesystem monitoring.
Supports file watching, directory watching (non-recursive and recursive).
Canonicalizes paths for deduplication. Supports `rewatch_file()` for atomic
rename recovery (editors like vim/vscode save via rename, invalidating the watch).

```rust
impl FileWatcher {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let watcher = RecommendedWatcher::new(
            move |res| { /* classify as FileChanged/DirChanged, send via tx */ },
            Config::default()
        );
        // ...
    }
}
```

Watched paths (set up in `App::setup_file_watchers()`):
- **Files:** Each File panel's path (`watch_file`)
- **Directories:** Each open tree folder (`watch_dir`)
- **Git paths:** `.git/HEAD`, `.git/index`, `.git/refs/heads/` (recursive), etc.

- **Trigger:** Created at app startup
- **Lifetime:** Entire application lifetime
- **Channel:** Internal `mpsc::channel()` polled via `poll_events()`
- **OS APIs:** inotify (Linux), FSEvents (macOS), ReadDirectoryChangesW (Windows)

### 5. Persistence Writer (`src/persistence/writer.rs`)

**Struct:** `PersistenceWriter` — dedicated background thread for file I/O.

Offloads all file writes from the main thread. The main thread serializes
state into `WriteBatch` structs (CPU work), the writer thread does the actual
disk I/O. Debounces rapid state saves within 50ms to reduce disk churn.

```rust
pub struct PersistenceWriter {
    tx: Sender<WriterMsg>,
    flush_sync: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

enum WriterMsg {
    Batch(WriteBatch),    // State save (debounced — coalesced within 50ms)
    Message(WriteOp),     // Single message write (not debounced)
    Flush,                // Synchronous flush (blocks caller via Condvar)
    Shutdown,             // Graceful shutdown
}
```

- **Trigger:** `save_state_async()` on any state change, `save_message_async()` on message creation
- **Lifetime:** Entire application lifetime (created in `App::new()`)
- **Channel:** `Sender<WriterMsg>` (internal, not polled by main loop)
- **Flush:** Synchronous flush via `Condvar` on app exit (5s timeout)
- **Concurrency:** Single writer thread; batches coalesce (only latest state matters)

### 6. GitHub Watcher (`src/gh_watcher.rs`)

**Struct:** `GhWatcher` — background polling thread for GithubResult panels.

Polls GitHub CLI commands for changes. Uses HTTP ETags for `gh api` commands
(conditional requests, respects `X-Poll-Interval` header) and SHA-256 output
hashing for other `gh` commands.

```rust
pub struct GhWatcher {
    watches: Arc<Mutex<HashMap<String, GhWatch>>>,
    _thread: JoinHandle<()>,
}
```

Sends `CacheUpdate::GithubResultContent` directly through `cache_tx` (bypasses
the two-phase deprecate→refresh pattern because polling inherently produces
the new content as a byproduct of change detection).

- **Trigger:** Created at app startup; watches synced via `sync_watches()` every 5s
- **Lifetime:** Entire application lifetime
- **Channel:** Sends directly to `cache_tx` (shared with cache pool)
- **Polling:** Wakes every 5s, polls individual watches per their interval (default 60s)
- **Concurrency:** Single thread, sequential polling

## External Process Spawning

### `run_with_timeout` (`src/modules/mod.rs`)

Spawns a child process with a timeout guard thread. Used by git and github
tools when executing commands in cache worker threads.

```rust
pub fn run_with_timeout(mut cmd: Command, timeout_secs: u64) -> io::Result<Output> {
    let child = cmd.spawn()?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || { let _ = tx.send(child.wait_with_output()); });
    match rx.recv_timeout(Duration::from_secs(timeout_secs)) {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, ...)),
    }
}
```

### In Cache Worker Threads (non-blocking to main thread)

| Location | Process | Purpose |
|----------|---------|---------|
| Cache pool (RefreshTmux) | `tmux capture-pane` | Capture terminal output |
| Cache pool (RefreshGitStatus) | `git status`, `git branch`, `git diff`, `git show` | Git status panel |
| Cache pool (RefreshGitResult) | `git <command>` via `run_with_timeout` | Read-only git command panels |
| Cache pool (RefreshGithubResult) | `gh <command>` via `run_with_timeout` | Read-only github CLI panels |
| GH Watcher thread | `gh api -i`, `gh <command>` | Polling for changes |

### Synchronous (tool execution on main thread)

| Location | Process | Purpose |
|----------|---------|---------|
| `modules/tmux/tools.rs` | `tmux send-keys`, `tmux new-window`, `tmux list-panes` | Console management |
| `modules/git/tools.rs` | `git <mutating command>` via `run_with_timeout` | Mutating git operations |
| `modules/github/tools.rs` | `gh <mutating command>` via `run_with_timeout` | Mutating github operations |
| `modules/core/tools/close_context.rs` | `tmux kill-pane` | Close console panels |

## Thread-Safe Shared State

### Global Statics (`LazyLock`)

| Location | Static | Type | Purpose |
|----------|--------|------|---------|
| `src/config.rs` | `PROMPTS`, `LIBRARY`, `UI`, `THEMES` | `LazyLock<*Config>` | Parsed YAML configs |
| `src/perf.rs` | `PERF` | `LazyLock<PerfMetrics>` | In-memory performance metrics |
| `src/cache.rs` | `CACHE_POOL` | `LazyLock<CachePool>` | Bounded thread pool for cache ops |
| `src/highlight.rs` | `SYNTAX_SET` | `LazyLock<SyntaxSet>` | Syntax highlighting definitions |
| `src/highlight.rs` | `THEME_SET` | `LazyLock<ThemeSet>` | Highlighting theme definitions |
| `src/highlight.rs` | `HIGHLIGHT_CACHE` | `LazyLock<Mutex<HashMap>>` | Highlighted file cache (50 entries) |

### Atomic / Lock-Based

| Location | Primitive | Purpose |
|----------|-----------|---------|
| `src/config.rs` | `AtomicPtr<Theme>` (`CACHED_THEME`) | Zero-cost active theme access |
| `src/perf.rs` | `AtomicBool` | Perf monitoring enabled/disabled toggle |
| `src/perf.rs` | `AtomicU32`, `AtomicU64` | CPU usage, memory, frame counts, op stats |
| `src/perf.rs` | `RwLock<HashMap>`, `RwLock<RingBuffer>` | Per-op stats, frame time samples |
| `src/highlight.rs` | `Mutex<HashMap<String, Arc<...>>>` | Syntax highlighting result cache |
| `src/watcher.rs` | `Arc<Mutex<HashMap>>` (×2) | Watched files/dirs path tracking |
| `src/gh_watcher.rs` | `Arc<Mutex<HashMap>>` | Per-panel watch state (etags, hashes, intervals) |
| `src/persistence/writer.rs` | `Arc<(Mutex<bool>, Condvar)>` | Flush synchronization |

## Channel Summary

### Main Thread Channels (created in `main.rs`)

| Channel | Sender Location | Receiver Location | Message Type |
|---------|-----------------|-------------------|--------------|
| LLM Stream | `src/llms/mod.rs` | `src/core/app.rs` (`rx`) | `StreamEvent` |
| TL;DR Result | `src/background.rs` | `src/core/app.rs` (`tldr_rx`) | `TlDrResult` |
| Cache Update | `src/cache.rs` workers + `src/gh_watcher.rs` | `src/core/app.rs` (`cache_rx`) | `CacheUpdate` |

### Internal Channels (not polled by main loop)

| Channel | Location | Purpose |
|---------|----------|---------|
| Cache job queue | `src/cache.rs` (`CachePool`) | Dispatches `(CacheRequest, Sender)` to worker threads |
| Writer messages | `src/persistence/writer.rs` | `WriterMsg` (Batch/Message/Flush/Shutdown) to I/O thread |
| File watch events | `src/watcher.rs` | `WatchEvent` from notify callback to `poll_events()` |
| Timeout guard | `src/modules/mod.rs` | `run_with_timeout()` per-command output channel |

## Main Loop Polling

The main event loop in `src/core/app.rs` uses adaptive polling:

```rust
// Simplified main loop structure
loop {
    // === INPUT FIRST: minimal latency ===
    if event::poll(Duration::ZERO)? {
        let evt = event::read()?;
        // Handle command palette or normal input
        // Render immediately after input for instant feedback
    }

    // === BACKGROUND PROCESSING ===
    process_stream_events(&rx);       // LLM streaming chunks/tools/done/error
    handle_retry(&tx);                // Auto-retry on transient errors
    process_typewriter();             // Animated text output
    process_tldr_results(&tldr_rx);   // Background summarization results
    process_cache_updates(&cache_rx); // Panel content refreshes
    process_watcher_events();         // File/dir change notifications
    check_waiting_for_panels(&tx);    // Wait for panels before continuing stream
    check_deferred_sleep(&tx);        // Timer-based sleep (console_sleep/send_keys)
    sync_gh_watches();                // Throttled every 5s
    check_timer_based_deprecation();  // Interval-based cache refresh scheduling
    handle_tool_execution(&tx);       // Execute pending tool calls
    finalize_stream(&tldr_tx);        // Complete stream when all tools done
    check_spine(&tx, &tldr_tx);       // Auto-continuation decisions

    // Ownership check (every 1s)
    // Spinner animation (every 100ms)

    // Render if dirty (throttled to ~28fps via RENDER_THROTTLE_MS)
    if state.dirty && enough_time_passed {
        terminal.draw(|f| render(f, &state))?;
    }

    // Adaptive sleep: 8ms when streaming/dirty, 50ms when idle
    event::poll(Duration::from_millis(poll_ms))?;
}
```

## Files With No Threading/Concurrency

The following files contain no worker threads, channels, or concurrency primitives:

- `src/api.rs` — Re-export module
- `src/constants.rs` — Static constants
- `src/events.rs` — Event mapping logic
- `src/state/` — Data structures (all submodules)
- `src/tool_defs.rs` — Tool definitions
- `src/typewriter.rs` — Text animation buffer
- `src/core/context.rs` — Stream context preparation
- `src/core/init.rs` — State initialization
- `src/core/mod.rs` — Module exports
- `src/core/wait.rs` — Panel readiness checks
- `src/help/` — Command palette UI
- `src/modules/` — Module implementations (sync process spawning via `run_with_timeout` only)
- `src/ui/` — UI components
- `src/actions/` — Action handlers
