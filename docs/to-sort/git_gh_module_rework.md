# Git & GitHub Module Rework — FR/NFR Specification

> **Issue:** #5
> **Branch:** `feature/github-tools-rework`
> **PR:** #20
> **Status:** Design phase
> **Last updated:** 2026-02-08

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Glossary](#2-glossary)
3. [Current State (Baseline)](#3-current-state-baseline)
4. [Target Architecture](#4-target-architecture)
5. [Module: `git` (Local)](#5-module-git-local)
6. [Module: `github` (Remote)](#6-module-github-remote)
7. [Command Classification System](#7-command-classification-system)
8. [Panel Types & Lifecycle](#8-panel-types--lifecycle)
9. [Caching & Refresh Strategies](#9-caching--refresh-strategies)
10. [Authentication & Token Management](#10-authentication--token-management)
11. [State Changes](#11-state-changes)
12. [Persistence](#12-persistence)
13. [Non-Functional Requirements](#13-non-functional-requirements)
14. [Migration & Backwards Compatibility](#14-migration--backwards-compatibility)
15. [Removed Components](#15-removed-components)
16. [File Inventory](#16-file-inventory)

---

## 1. Executive Summary

Replace the current monolithic `git` module (9 dedicated tools, 1 fixed panel) with two focused modules:

| Module | Scope | Tools | Panel Types |
|--------|-------|-------|-------------|
| **`git`** (local) | Local git operations | `git_execute`, `git_configure_p6` | P6 fixed panel (unchanged) + `GitResult` dynamic panels |
| **`github`** (remote) | GitHub API operations via `gh` CLI | `gh_execute` | `GithubResult` dynamic panels |

**Key design principles:**
- Raw command strings — the LLM passes `git log --oneline -20` or `gh pr list --json number,title` directly
- Hardcoded lookup table classifies every command as read-only vs. mutating — no LLM trust required for classification
- Read-only commands spawn/reuse dynamic result panels; mutating commands execute and return output
- Git refresh: local `.git/` filesystem heuristics (free, no network)
- GitHub refresh: HTTP ETag conditional requests (304 = free, no rate limit cost)
- GitHub token required for all `gh` commands — zero reliance on local git credential configuration
- P6 git status panel preserved as-is with enhanced diff base configuration
- All 9 legacy git tools removed; replaced by 2 tools (`git_execute` + `git_configure_p6`)

---

## 2. Glossary

| Term | Definition |
|------|-----------|
| **P6** | The fixed Git panel (position 6 in `FIXED_PANEL_ORDER`), showing local working tree status, diffs, and logs |
| **Dynamic panel** | A panel created at runtime (P8+), not part of the fixed panel order. Has a `uid` for persistence |
| **`is_pure_description`** | A command that only reads data and has no side effects (safe to auto-refresh, safe to retry) |
| **ETag** | HTTP header for conditional requests. Server returns `304 Not Modified` if content unchanged |
| **`X-Poll-Interval`** | GitHub header specifying minimum seconds between poll requests (typically 60s, can increase under load) |
| **Cache deprecated** | Flag on `ContextElement` indicating cached content is stale and needs regeneration |
| **Lookup table** | Hardcoded `HashMap` mapping command patterns to read-only/mutating classification |

---

## 3. Current State (Baseline)

### 3.1 Current Module: `git`

**File:** `src/modules/git/mod.rs`

- **Module ID:** `"git"`
- **Module name:** `"Git"`
- **`is_global()`:** `false` (worker module, persisted to `worker.json`)
- **Fixed panel:** `ContextType::Git` → P6, name `"Changes"`, `cache_deprecated: false`
- **Persisted data:** `git_show_diffs: bool` (saved to config.json via `save_module_data`)

### 3.2 Current Tools (9 total, all `ToolCategory::Git`, all `enabled: true`)

| # | Tool ID | Params | Required | Description |
|---|---------|--------|----------|-------------|
| 1 | `git_toggle_details` | `show: Boolean` | no | Toggle showing full diff content vs summary |
| 2 | `git_toggle_logs` | `show: Boolean`, `args: String` | no | Toggle git log display with custom args |
| 3 | `git_commit` | `message: String`, `files: Array<String>` | message=yes | Stage files and create commit |
| 4 | `git_branch_create` | `name: String` | yes | Create and switch to new branch |
| 5 | `git_branch_switch` | `branch: String` | yes | Switch to existing branch (checks for uncommitted changes) |
| 6 | `git_merge` | `branch: String` | yes | Merge branch, auto-delete on success |
| 7 | `git_pull` | (none) | — | Pull from remote (`GIT_TERMINAL_PROMPT=0`) |
| 8 | `git_push` | (none) | — | Push to remote (`GIT_TERMINAL_PROMPT=0`) |
| 9 | `git_fetch` | (none) | — | Fetch from remote (`GIT_TERMINAL_PROMPT=0`) |

### 3.3 Current State Fields (`State` struct)

```rust
// All runtime-only (not persisted via serde), except git_show_diffs via module data
pub git_branch: Option<String>,
pub git_branches: Vec<(String, bool)>,       // (name, is_current)
pub git_is_repo: bool,
pub git_file_changes: Vec<GitFileChange>,
pub git_show_diffs: bool,                    // persisted via module save/load
pub git_status_hash: Option<String>,
pub git_show_logs: bool,
pub git_log_args: Option<String>,
pub git_log_content: Option<String>,
```

### 3.4 Current Cache Variants

```rust
// CacheRequest
RefreshGitStatus {
    show_diffs: bool,
    current_hash: Option<String>,
}

// CacheUpdate
GitStatus {
    branch: Option<String>,
    is_repo: bool,
    file_changes: Vec<(String, i32, i32, GitChangeType, String)>,
    branches: Vec<(String, bool)>,
    formatted_content: String,
    token_count: usize,
    status_hash: String,
}
GitStatusUnchanged
```

### 3.5 Current Types

```rust
// src/modules/git/types.rs
pub struct GitFileChange {
    pub path: String,
    pub additions: i32,
    pub deletions: i32,
    pub change_type: GitChangeType,
    pub diff_content: String,
}

pub enum GitChangeType {
    Modified,
    Added,
    Untracked,
    Deleted,
    Renamed,
}
```

### 3.6 Current P6 Panel Behavior

- **Refresh interval:** 2000ms (`GIT_STATUS_REFRESH_MS`)
- **Change detection:** Hash of `git status --porcelain -uall` + branch name. If hash matches, returns `GitStatusUnchanged` (skips expensive diff operations)
- **Background operations:** `git status --porcelain -uall`, `git rev-parse --abbrev-ref HEAD`, `git diff --cached --numstat`, `git diff --numstat`, `git diff HEAD` (conditional on `show_diffs`), `git branch --format=%(refname:short)`
- **LLM context format:** Markdown table (`| File | Type | + | - | Net |`) with optional unified diff blocks
- **UI rendering:** Styled branch name, branch list, file table with colors, diff syntax highlighting (green/red/bold/muted)
- **Diff base:** Always `HEAD` (working tree vs HEAD). No configurable comparison point.

---

## 4. Target Architecture

### 4.1 Module Split

```
Before:                          After:
┌──────────────────────┐        ┌──────────────────────┐  ┌─────────────────────────┐
│     git module       │        │     git module        │  │    github module         │
│                      │        │                       │  │                          │
│  9 tools             │   →    │  git_execute          │  │  gh_execute              │
│  P6 panel            │        │  git_configure_p6     │  │  GithubResult panels     │
│  GitFileChange type  │        │  P6 panel (enhanced)  │  │  ETag cache              │
│                      │        │  GitResult panels     │  │  Token management        │
└──────────────────────┘        └──────────────────────┘  └─────────────────────────┘
```

### 4.2 New ContextType Variants

```rust
pub enum ContextType {
    // ... existing variants unchanged ...
    Git,           // P6 fixed panel (unchanged)
    GitResult,     // NEW: dynamic panel for git read-only command results
    GithubResult,  // NEW: dynamic panel for gh read-only command results
}
```

### 4.3 New ToolCategory Variant

```rust
pub enum ToolCategory {
    // ... existing variants unchanged ...
    Git,      // for git_execute, git_configure_p6
    Github,   // NEW: for gh_execute
}
```

### 4.4 Tool Surface Summary

| Tool | Module | Category | Params | Purpose |
|------|--------|----------|--------|---------|
| `git_execute` | git | Git | `command: String` (required) | Execute any `git` command |
| `git_configure_p6` | git | Git | `show_diffs: Boolean`, `show_logs: Boolean`, `log_args: String`, `diff_base: String` (all optional) | Configure P6 panel display |
| `gh_execute` | github | Github | `command: String` (required) | Execute any `gh` command |

---

## 5. Module: `git` (Local)

### 5.1 Module Definition

```rust
// src/modules/git/mod.rs
impl Module for GitModule {
    fn id(&self) -> &'static str { "git" }
    fn name(&self) -> &'static str { "Git" }
    fn description(&self) -> &'static str { "Local git operations and working tree status panel" }
    fn is_global(&self) -> bool { false }  // worker module

    fn fixed_panel_types(&self) -> Vec<ContextType> { vec![ContextType::Git] }
    fn dynamic_panel_types(&self) -> Vec<ContextType> { vec![ContextType::GitResult] }
    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::Git, "Changes", false)]
    }
}
```

### 5.2 Tool: `git_execute`

#### 5.2.1 Definition

```rust
ToolDefinition {
    id: "git_execute".to_string(),
    name: "Git Execute".to_string(),
    short_desc: "Run a git command".to_string(),
    description: "Execute a git CLI command. Read-only commands (log, diff, show, blame, etc.) \
                  open a result panel. Mutating commands (commit, push, merge, etc.) execute \
                  and return the output. The command string must start with 'git '.".to_string(),
    params: vec![
        ToolParam {
            name: "command".to_string(),
            param_type: ParamType::String,
            description: Some("The full git command to execute (e.g., 'git log --oneline -20')".to_string()),
            required: true,
            enum_values: None,
            default: None,
        },
    ],
    enabled: true,
    category: ToolCategory::Git,
}
```

#### 5.2.2 Execution Flow

```
LLM calls git_execute(command: "git log --oneline -20")
         │
         ▼
┌─ Validate command ─────────────────────────┐
│  1. Must start with "git "                 │
│  2. Strip "git " prefix → "log --oneline"  │
│  3. Extract subcommand → "log"             │
│  4. Reject dangerous patterns:             │
│     - Shell operators: |, ;, &&, ||, `, $( │
│     - Redirects: >, <, >>                  │
│     - Newlines                             │
└────────────────────────────────────────────┘
         │
         ▼
┌─ Classify via lookup table ────────────────┐
│  "log" → is_pure_description = true        │
│  Result: READ_ONLY or MUTATING             │
└────────────────────────────────────────────┘
         │
         ├── READ_ONLY ──────────────────────────────────────────┐
         │                                                        │
         │   1. Compute panel key = sha256(command_string)        │
         │   2. Search state.context for existing GitResult       │
         │      panel with matching key                           │
         │   3a. If found: reuse panel                            │
         │       - Execute command in background                  │
         │       - Update cached_content                          │
         │       - Return "Panel updated: {command}"              │
         │   3b. If not found: create new GitResult panel         │
         │       - Assign next dynamic ID (P8, P9, ...)           │
         │       - Generate UID                                   │
         │       - Execute command                                │
         │       - Store output as cached_content                 │
         │       - Set panel name = truncated command (≤40 chars) │
         │       - Return "Panel created: {panel_id} {command}"   │
         │                                                        │
         ├── MUTATING ───────────────────────────────────────────┐
         │                                                        │
         │   1. Execute command via std::process::Command          │
         │      - Working dir: repo root                          │
         │      - env: GIT_TERMINAL_PROMPT=0                      │
         │      - stdin: null                                     │
         │   2. Capture stdout + stderr                           │
         │   3. Mark P6 as cache_deprecated = true                │
         │   4. Mark all GitResult panels as cache_deprecated     │
         │   5. Return command output (stdout + stderr)           │
         │   6. If exit code != 0: set is_error = true            │
         │                                                        │
         └────────────────────────────────────────────────────────┘
```

#### 5.2.3 Command Validation Rules

| Rule | Action |
|------|--------|
| Command does not start with `"git "` | Return error: `"Command must start with 'git '"` |
| Command contains `\|`, `;`, `&&`, `\|\|`, `` ` ``, `$(` | Return error: `"Shell operators are not allowed. Pass a single git command."` |
| Command contains `>`, `<`, `>>` | Return error: `"Redirects are not allowed."` |
| Command contains newline characters | Return error: `"Multi-line commands are not allowed."` |
| Subcommand not found in lookup table | Treat as MUTATING (safe default — no panel created, just executes) |
| Empty command after `"git "` | Return error: `"Empty git command."` |

#### 5.2.4 Environment Variables for Execution

All `git` commands executed by `git_execute` MUST set:

```rust
Command::new("git")
    .args(parsed_args)
    .current_dir(&state.working_dir)  // repo root
    .env("GIT_TERMINAL_PROMPT", "0")  // prevent interactive auth prompts
    .stdin(Stdio::null())             // prevent stdin reads
```

No GitHub token is injected for local git commands. Authentication for push/pull/fetch relies on whatever the user has configured in their git environment (SSH keys, credential helpers, etc.). This is the one exception to the "no local config" rule — it applies only to GitHub module, not local git.

### 5.3 Tool: `git_configure_p6`

#### 5.3.1 Definition

```rust
ToolDefinition {
    id: "git_configure_p6".to_string(),
    name: "Configure Git Panel".to_string(),
    short_desc: "Configure the P6 git status panel".to_string(),
    description: "Configure the fixed git status panel (P6). All parameters are optional — \
                  only provided parameters are updated. \
                  diff_base sets the comparison base for diffs (default: 'HEAD', e.g., \
                  'HEAD~5', 'main', 'abc1234', a branch name, or a tag).".to_string(),
    params: vec![
        ToolParam {
            name: "show_diffs".to_string(),
            param_type: ParamType::Boolean,
            description: Some("Show full diff content in the panel (true/false)".to_string()),
            required: false,
            enum_values: None,
            default: None,
        },
        ToolParam {
            name: "show_logs".to_string(),
            param_type: ParamType::Boolean,
            description: Some("Show git log output in the panel (true/false)".to_string()),
            required: false,
            enum_values: None,
            default: None,
        },
        ToolParam {
            name: "log_args".to_string(),
            param_type: ParamType::String,
            description: Some("Custom git log arguments (e.g., '-10 --oneline'). Defaults to '-10 --oneline'.".to_string()),
            required: false,
            enum_values: None,
            default: None,
        },
        ToolParam {
            name: "diff_base".to_string(),
            param_type: ParamType::String,
            description: Some("Comparison base for diffs. Any valid git ref: commit SHA, branch name, tag, or relative ref like 'HEAD~3'. Default: 'HEAD'.".to_string()),
            required: false,
            enum_values: None,
            default: None,
        },
    ],
    enabled: true,
    category: ToolCategory::Git,
}
```

#### 5.3.2 Execution Flow

```rust
pub fn execute_configure_p6(tool: &ToolUse, state: &mut State) -> ToolResult {
    let mut changes = Vec::new();

    if let Some(v) = tool.input.get("show_diffs").and_then(|v| v.as_bool()) {
        state.git_show_diffs = v;
        changes.push(format!("show_diffs={}", v));
    }
    if let Some(v) = tool.input.get("show_logs").and_then(|v| v.as_bool()) {
        state.git_show_logs = v;
        changes.push(format!("show_logs={}", v));
    }
    if let Some(v) = tool.input.get("log_args").and_then(|v| v.as_str()) {
        state.git_log_args = Some(v.to_string());
        changes.push(format!("log_args='{}'", v));
    }
    if let Some(v) = tool.input.get("diff_base").and_then(|v| v.as_str()) {
        // Validate: run `git rev-parse --verify {v}` to check ref exists
        // If invalid, return is_error = true with message
        state.git_diff_base = Some(v.to_string());
        changes.push(format!("diff_base='{}'", v));
    }

    // Mark P6 as cache_deprecated to force refresh with new settings
    // ... (find Git context element, set cache_deprecated = true)

    // If show_logs was enabled, fetch log content immediately
    // ... (run git log with args, store in git_log_content)

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("P6 configured: {}", changes.join(", ")),
        is_error: false,
    }
}
```

#### 5.3.3 `diff_base` Validation

Before accepting a `diff_base` value, run:
```bash
git rev-parse --verify <diff_base>
```
- Exit code 0 → valid ref, accept
- Exit code != 0 → return error: `"Invalid git ref: '{diff_base}'. Must be a valid commit, branch, tag, or relative ref."`

#### 5.3.4 `diff_base` Effect on P6

When `git_diff_base` is `Some(ref)`:
- Replace `git diff HEAD` with `git diff {ref}`
- Replace `git diff --cached --numstat` + `git diff --numstat` with `git diff {ref} --numstat`
- Display in P6 title: `"Git (main) [vs {ref}]"` instead of `"Git (main)"`
- Display in LLM context header: `"Branch: main (comparing against {ref})"`

When `git_diff_base` is `None` (default):
- Behave exactly as current implementation (diff HEAD = working tree vs HEAD)

### 5.4 Persisted Module Data

```rust
fn save_module_data(&self, state: &State) -> serde_json::Value {
    json!({
        "git_show_diffs": state.git_show_diffs,
        "git_diff_base": state.git_diff_base,
    })
}

fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
    if let Some(v) = data.get("git_show_diffs").and_then(|v| v.as_bool()) {
        state.git_show_diffs = v;
    }
    if let Some(v) = data.get("git_diff_base").and_then(|v| v.as_str()) {
        state.git_diff_base = Some(v.to_string());
    }
}
```

---

## 6. Module: `github` (Remote)

### 6.1 Module Definition

```rust
// src/modules/github/mod.rs
impl Module for GithubModule {
    fn id(&self) -> &'static str { "github" }
    fn name(&self) -> &'static str { "GitHub" }
    fn description(&self) -> &'static str { "GitHub API operations via gh CLI" }
    fn is_global(&self) -> bool { false }  // worker module
    fn dependencies(&self) -> &[&'static str] { &["git"] }  // depends on git module for repo context

    fn fixed_panel_types(&self) -> Vec<ContextType> { vec![] }  // no fixed panels
    fn dynamic_panel_types(&self) -> Vec<ContextType> { vec![ContextType::GithubResult] }
    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> { vec![] }
}
```

### 6.2 Tool: `gh_execute`

#### 6.2.1 Definition

```rust
ToolDefinition {
    id: "gh_execute".to_string(),
    name: "GitHub Execute".to_string(),
    short_desc: "Run a gh CLI command".to_string(),
    description: "Execute a GitHub CLI (gh) command. Read-only commands (list, view, status, \
                  diff, checks, etc.) open a result panel that auto-refreshes via ETag polling. \
                  Mutating commands (create, merge, close, comment, etc.) execute and return \
                  the output. The command string must start with 'gh '. All commands are \
                  authenticated via the configured GITHUB_TOKEN.".to_string(),
    params: vec![
        ToolParam {
            name: "command".to_string(),
            param_type: ParamType::String,
            description: Some("The full gh command to execute (e.g., 'gh pr list --state open')".to_string()),
            required: true,
            enum_values: None,
            default: None,
        },
    ],
    enabled: true,
    category: ToolCategory::Github,
}
```

#### 6.2.2 Execution Flow

```
LLM calls gh_execute(command: "gh pr list --state open")
         │
         ▼
┌─ Check token ──────────────────────────────┐
│  If state.github_token is None:            │
│    Return error: "GITHUB_TOKEN not set.    │
│    Add GITHUB_TOKEN=ghp_... to your .env   │
│    file and restart."                      │
└────────────────────────────────────────────┘
         │
         ▼
┌─ Validate command ─────────────────────────┐
│  1. Must start with "gh "                  │
│  2. Strip "gh " prefix → "pr list ..."    │
│  3. Extract subcommand group → "pr"        │
│  4. Extract action → "list"                │
│  5. Reject shell operators (same as git)   │
└────────────────────────────────────────────┘
         │
         ▼
┌─ Classify via lookup table ────────────────┐
│  ("pr", "list") → is_pure_description      │
│  Result: READ_ONLY or MUTATING             │
└────────────────────────────────────────────┘
         │
         ├── READ_ONLY ──────────────────────────────────────────┐
         │                                                        │
         │   1. Compute panel key = sha256(command_string)        │
         │   2. Search state.context for existing GithubResult    │
         │      panel with matching key                           │
         │   3a. If found: reuse panel                            │
         │       - Execute command                                │
         │       - Update cached_content + etag                   │
         │       - Return "Panel updated: {command}"              │
         │   3b. If not found: create new GithubResult panel      │
         │       - Assign next dynamic ID (P8, P9, ...)           │
         │       - Generate UID                                   │
         │       - Execute command (with -i flag for ETag)        │
         │       - Parse ETag from response headers               │
         │       - Store output as cached_content                 │
         │       - Store ETag in panel metadata                   │
         │       - Set panel name = truncated command (≤40 chars) │
         │       - Return "Panel created: {panel_id} {command}"   │
         │                                                        │
         ├── MUTATING ───────────────────────────────────────────┐
         │                                                        │
         │   1. Execute command via std::process::Command          │
         │      - env: GITHUB_TOKEN={token}                       │
         │      - env: GH_TOKEN={token}                           │
         │      - env: GH_PROMPT_DISABLED=1                       │
         │      - stdin: null                                     │
         │   2. Capture stdout + stderr                           │
         │   3. Mark ALL GithubResult panels cache_deprecated     │
         │   4. Mark P6 as cache_deprecated (mutations may        │
         │      affect local state, e.g. gh pr checkout)          │
         │   5. Return command output                             │
         │   6. If exit code != 0: set is_error = true            │
         │                                                        │
         └────────────────────────────────────────────────────────┘
```

#### 6.2.3 Environment Variables for All `gh` Commands

```rust
Command::new("gh")
    .args(parsed_args)
    .current_dir(&state.working_dir)
    .env("GITHUB_TOKEN", &token)      // Primary auth
    .env("GH_TOKEN", &token)          // gh CLI also checks this
    .env("GH_PROMPT_DISABLED", "1")   // Prevent interactive prompts
    .env("NO_COLOR", "1")             // Prevent ANSI color codes in output
    .stdin(Stdio::null())
```

**Critical:** Both `GITHUB_TOKEN` and `GH_TOKEN` are set. The `gh` CLI checks `GH_TOKEN` first, then `GITHUB_TOKEN`. Setting both ensures compatibility regardless of `gh` version.

#### 6.2.4 ETag Extraction for Read-Only Commands

For read-only `gh api` equivalent commands, we need the ETag from the response. Two strategies depending on command type:

**Strategy A — Commands that map to a single API endpoint** (e.g., `gh pr list`, `gh issue view 42`):

Under the hood, convert the `gh` command to `gh api` with `-i` flag to get response headers:

```rust
// Instead of: gh pr list --json number,title
// Execute:    gh api /repos/{owner}/{repo}/pulls -H "Accept: application/vnd.github+json" -i
// Parse ETag from response headers
```

However, this adds complexity. **Preferred approach:** Execute the `gh` command as-is for the initial call, then use `gh api` with ETag for subsequent refresh polls. Store the API endpoint mapping alongside the panel.

**Strategy B — Simple approach (recommended for v1):**

Execute the `gh` command normally. For refresh, re-execute the same command. No ETag on first implementation. Add ETag optimization in a follow-up iteration.

**Strategy C — Hybrid (recommended for v2):**

For commands that we can map to API endpoints, use `gh api` with ETag caching. For commands we can't map, re-execute with a long interval.

> **Decision:** Start with Strategy B (re-execute the command). Plan for Strategy C as optimization. The lookup table can include an optional `api_endpoint` field for future ETag support.

### 6.3 ETag Caching (Future Optimization — v2)

When implemented, the ETag system works as follows:

#### 6.3.1 Initial Request

```bash
gh api /repos/{owner}/{repo}/pulls -i
```

Response includes:
```
HTTP/2.0 200 OK
Etag: W/"55b867737538c231740bfec5b088fd6917f8b08d9a7818a2e025f905b7db579c"
X-Ratelimit-Remaining: 4972
X-Ratelimit-Used: 28
```

Store the `Etag` value in the `ContextElement` metadata.

#### 6.3.2 Subsequent Polls

```bash
gh api /repos/{owner}/{repo}/pulls -i -H "If-None-Match: W/\"55b867...\""
```

**If unchanged (304):**
- Exit code: `1`
- Stderr: `"gh: HTTP 304"`
- Body: empty
- Rate limit: **not consumed**
- Action: keep existing `cached_content`, update `last_refresh_ms`

**If changed (200):**
- Exit code: `0`
- Body: new JSON data
- New `Etag` header in response
- Action: update `cached_content`, store new ETag, update `last_refresh_ms`

#### 6.3.3 Detecting 304 in Rust

```rust
let output = Command::new("gh").args(args).output()?;

if output.status.code() == Some(1) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("HTTP 304") {
        return None;  // Not modified — no CacheUpdate needed
    }
    // Actual error — return error CacheUpdate
}
// Success — parse new content and ETag
```

#### 6.3.4 ETag Storage

Per `ContextElement`, stored in a new field:

```rust
// In ContextElement
#[serde(skip)]
pub gh_etag: Option<String>,

// In ContextElement
#[serde(skip)]
pub gh_api_endpoint: Option<String>,  // for ETag refresh
```

#### 6.3.5 Rate Limit Awareness

Every `gh api` response includes rate limit headers. Parse and store:

```rust
// In State (global, not per-panel)
pub github_rate_limit_remaining: Option<u32>,
pub github_rate_limit_reset: Option<u64>,  // Unix epoch
```

If `github_rate_limit_remaining < 100`, increase poll intervals by 4x. If `< 10`, stop all auto-refresh.

### 6.4 No-Token Behavior

When `state.github_token` is `None`:
- Module loads normally
- `gh_execute` tool is registered and visible
- On any `gh_execute` call, return:
  ```
  ToolResult {
      content: "GitHub token not configured.\n\n\
                To use GitHub features, add your token to .env:\n\
                  GITHUB_TOKEN=ghp_your_token_here\n\n\
                Then restart the TUI.",
      is_error: true,
  }
  ```
- No GithubResult panels are created
- P6 git panel works normally (no token needed)

---

## 7. Command Classification System

### 7.1 Lookup Table Structure

```rust
use std::collections::HashMap;

/// Classification of a CLI command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    /// Read-only: safe to auto-refresh, creates/reuses result panel
    ReadOnly,
    /// Mutating: executes once, returns output, invalidates caches
    Mutating,
}

/// Entry in the classification lookup table
pub struct CommandEntry {
    pub class: CommandClass,
    /// Optional: API endpoint for ETag polling (v2)
    pub api_endpoint: Option<&'static str>,
}
```

### 7.2 Git Command Lookup Table

Classification based on `git <subcommand>`:

| Subcommand | Classification | Notes |
|------------|---------------|-------|
| `log` | ReadOnly | |
| `diff` | ReadOnly | |
| `show` | ReadOnly | |
| `status` | ReadOnly | |
| `blame` | ReadOnly | |
| `branch` (no args or `-l`, `-a`, `-r`, `-v`, `-vv`, `--list`, `--merged`, `--no-merged`, `--contains`, `--show-current`, `--sort`, `--format`) | ReadOnly | When listing/querying only |
| `branch` (with `-d`, `-D`, `-m`, `-M`, `-c`, `-u`, `--unset-upstream`) | Mutating | When modifying |
| `tag` (no args or `-l`, `-n`, `--contains`, `--no-contains`, `--points-at`) | ReadOnly | When listing only |
| `tag` (with creation/deletion args) | Mutating | |
| `remote` (no args or `-v`, `show`, `get-url`) | ReadOnly | |
| `remote` (with `add`, `remove`, `rename`, `set-url`, `prune`, `update`) | Mutating | |
| `stash` with `list`, `show` | ReadOnly | |
| `stash` (other: `push`, `pop`, `apply`, `drop`, `clear`, `branch`) | Mutating | |
| `rev-parse` | ReadOnly | |
| `rev-list` | ReadOnly | |
| `ls-tree` | ReadOnly | |
| `ls-files` | ReadOnly | |
| `ls-remote` | ReadOnly | |
| `cat-file` | ReadOnly | |
| `for-each-ref` | ReadOnly | |
| `describe` | ReadOnly | |
| `shortlog` | ReadOnly | |
| `reflog` (no args or `show`) | ReadOnly | |
| `count-objects` | ReadOnly | |
| `fsck` | ReadOnly | |
| `check-ignore` | ReadOnly | |
| `check-attr` | ReadOnly | |
| `hash-object` (without `-w`) | ReadOnly | |
| `name-rev` | ReadOnly | |
| `symbolic-ref` (query, not set) | ReadOnly | |
| `grep` | ReadOnly | |
| `bisect` with `log`, `visualize` | ReadOnly | |
| `archive` | ReadOnly | |
| `bundle` with `verify`, `list-heads` | ReadOnly | |
| `format-patch` | ReadOnly | |
| `config` with `--list`, `--get`, `--get-regexp`, `--show-origin`, `--show-scope` | ReadOnly | |
| `diff --stat`, `diff --shortstat`, `diff --name-only`, `diff --name-status`, `diff --check` | ReadOnly | |
| `apply --stat`, `apply --check` | ReadOnly | |
| `commit` | Mutating | |
| `push` | Mutating | |
| `pull` | Mutating | |
| `fetch` | Mutating | Modifies refs, but non-destructive |
| `merge` | Mutating | |
| `rebase` | Mutating | |
| `cherry-pick` | Mutating | |
| `revert` | Mutating | |
| `reset` | Mutating | |
| `checkout` | Mutating | |
| `switch` | Mutating | |
| `add` | Mutating | |
| `rm` | Mutating | |
| `mv` | Mutating | |
| `restore` | Mutating | |
| `clean` | Mutating | |
| `init` | Mutating | |
| `clone` | Mutating | |
| `apply` (without `--stat`/`--check`) | Mutating | |
| `am` | Mutating | |
| `gc` | Mutating | |
| `prune` | Mutating | |
| `repack` | Mutating | |
| `update-index` | Mutating | |
| `filter-branch` | Mutating | |
| `filter-repo` | Mutating | |
| `replace` | Mutating | |
| `notes` (with `add`, `edit`, `remove`, `merge`) | Mutating | |
| `notes` (with `show`, `list`) | ReadOnly | |
| `worktree` with `list` | ReadOnly | |
| `worktree` (other) | Mutating | |
| `submodule` with `status`, `summary` | ReadOnly | |
| `submodule` (other) | Mutating | |
| `sparse-checkout` with `list` | ReadOnly | |
| `sparse-checkout` (other) | Mutating | |
| `lfs` with `ls-files`, `status`, `env`, `logs`, `track` (no args) | ReadOnly | |
| `lfs` (other) | Mutating | |
| `maintenance` with `start`, `stop`, `run` | Mutating | |
| `config` with `set`, `--unset`, `--edit`, `--remove-section` | Mutating | |
| _Unknown subcommand_ | **Mutating** (safe default) | |

**Implementation note:** The lookup table uses subcommand as primary key. For subcommands that can be both read-only and mutating (e.g., `branch`, `tag`, `stash`, `remote`, `config`), the table entry is a function that inspects the arguments to determine classification.

```rust
fn classify_git(args: &[&str]) -> CommandClass {
    let subcmd = args.first().map(|s| *s).unwrap_or("");
    match subcmd {
        // Always read-only
        "log" | "diff" | "show" | "status" | "blame" | "rev-parse" | "rev-list"
        | "ls-tree" | "ls-files" | "ls-remote" | "cat-file" | "for-each-ref"
        | "describe" | "shortlog" | "count-objects" | "fsck" | "check-ignore"
        | "check-attr" | "name-rev" | "grep" => CommandClass::ReadOnly,

        // Context-dependent
        "branch" => {
            if args.len() == 1 || args.iter().any(|a| matches!(*a,
                "-l" | "--list" | "-a" | "-r" | "-v" | "-vv" | "--merged"
                | "--no-merged" | "--contains" | "--no-contains" | "--show-current"
                | "--sort" | "--format"
            )) {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "stash" => {
            if args.get(1).map(|s| *s) == Some("list") || args.get(1).map(|s| *s) == Some("show") {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        // ... similar for tag, remote, config, notes, etc.

        // Always mutating
        "commit" | "push" | "pull" | "fetch" | "merge" | "rebase" | "cherry-pick"
        | "revert" | "reset" | "checkout" | "switch" | "add" | "rm" | "mv"
        | "restore" | "clean" | "init" | "clone" | "am" | "gc" | "prune"
        | "repack" | "update-index" | "filter-branch" | "filter-repo" => CommandClass::Mutating,

        // Unknown: default to mutating (safe)
        _ => CommandClass::Mutating,
    }
}
```

### 7.3 GitHub (`gh`) Command Lookup Table

Classification based on `gh <group> <action>`:

| Group | Action | Classification | Notes |
|-------|--------|---------------|-------|
| **pr** | `list` | ReadOnly | |
| **pr** | `view` | ReadOnly | |
| **pr** | `status` | ReadOnly | |
| **pr** | `diff` | ReadOnly | |
| **pr** | `checks` | ReadOnly | |
| **pr** | `create` | Mutating | |
| **pr** | `merge` | Mutating | |
| **pr** | `close` | Mutating | |
| **pr** | `reopen` | Mutating | |
| **pr** | `ready` | Mutating | |
| **pr** | `review` | Mutating | |
| **pr** | `comment` | Mutating | |
| **pr** | `edit` | Mutating | |
| **pr** | `checkout` | Mutating | |
| **pr** | `lock` | Mutating | |
| **pr** | `unlock` | Mutating | |
| **pr** | `update-branch` | Mutating | |
| **issue** | `list` | ReadOnly | |
| **issue** | `view` | ReadOnly | |
| **issue** | `status` | ReadOnly | |
| **issue** | `create` | Mutating | |
| **issue** | `close` | Mutating | |
| **issue** | `reopen` | Mutating | |
| **issue** | `comment` | Mutating | |
| **issue** | `edit` | Mutating | |
| **issue** | `delete` | Mutating | |
| **issue** | `transfer` | Mutating | |
| **issue** | `pin` | Mutating | |
| **issue** | `unpin` | Mutating | |
| **issue** | `lock` | Mutating | |
| **issue** | `unlock` | Mutating | |
| **issue** | `develop` | Mutating | |
| **repo** | `view` | ReadOnly | |
| **repo** | `list` | ReadOnly | |
| **repo** | `clone` | Mutating | |
| **repo** | `create` | Mutating | |
| **repo** | `fork` | Mutating | |
| **repo** | `edit` | Mutating | |
| **repo** | `rename` | Mutating | |
| **repo** | `delete` | Mutating | |
| **repo** | `archive` | Mutating | |
| **repo** | `unarchive` | Mutating | |
| **repo** | `sync` | Mutating | |
| **repo** | `set-default` | Mutating | |
| **repo** | `deploy-key list` | ReadOnly | |
| **repo** | `deploy-key add/delete` | Mutating | |
| **run** | `list` | ReadOnly | |
| **run** | `view` | ReadOnly | |
| **run** | `watch` | ReadOnly | |
| **run** | `download` | ReadOnly | (downloads artifacts, but read-only on GitHub) |
| **run** | `rerun` | Mutating | |
| **run** | `cancel` | Mutating | |
| **run** | `delete` | Mutating | |
| **workflow** | `list` | ReadOnly | |
| **workflow** | `view` | ReadOnly | |
| **workflow** | `run` | Mutating | |
| **workflow** | `enable` | Mutating | |
| **workflow** | `disable` | Mutating | |
| **release** | `list` | ReadOnly | |
| **release** | `view` | ReadOnly | |
| **release** | `download` | ReadOnly | |
| **release** | `create` | Mutating | |
| **release** | `edit` | Mutating | |
| **release** | `upload` | Mutating | |
| **release** | `delete` | Mutating | |
| **label** | `list` | ReadOnly | |
| **label** | `create` | Mutating | |
| **label** | `edit` | Mutating | |
| **label** | `delete` | Mutating | |
| **label** | `clone` | Mutating | |
| **secret** | `list` | ReadOnly | |
| **secret** | `set` | Mutating | |
| **secret** | `delete` | Mutating | |
| **variable** | `list` | ReadOnly | |
| **variable** | `get` | ReadOnly | |
| **variable** | `set` | Mutating | |
| **variable** | `delete` | Mutating | |
| **cache** | `list` | ReadOnly | |
| **cache** | `delete` | Mutating | |
| **gist** | `list` | ReadOnly | |
| **gist** | `view` | ReadOnly | |
| **gist** | `create` | Mutating | |
| **gist** | `edit` | Mutating | |
| **gist** | `clone` | Mutating | |
| **gist** | `rename` | Mutating | |
| **gist** | `delete` | Mutating | |
| **project** | `list` | ReadOnly | |
| **project** | `view` | ReadOnly | |
| **project** | `field-list` | ReadOnly | |
| **project** | `item-list` | ReadOnly | |
| **project** | `create` | Mutating | |
| **project** | `edit` | Mutating | |
| **project** | `close` | Mutating | |
| **project** | `delete` | Mutating | |
| **project** | `copy` | Mutating | |
| **project** | `link` | Mutating | |
| **project** | `unlink` | Mutating | |
| **project** | `field-create` | Mutating | |
| **project** | `field-delete` | Mutating | |
| **project** | `item-create` | Mutating | |
| **project** | `item-add` | Mutating | |
| **project** | `item-edit` | Mutating | |
| **project** | `item-archive` | Mutating | |
| **project** | `item-delete` | Mutating | |
| **project** | `mark-template` | Mutating | |
| **search** | `repos` | ReadOnly | |
| **search** | `issues` | ReadOnly | |
| **search** | `prs` | ReadOnly | |
| **search** | `commits` | ReadOnly | |
| **search** | `code` | ReadOnly | |
| **browse** | (any) | ReadOnly | |
| **status** | (any) | ReadOnly | |
| **ssh-key** | `list` | ReadOnly | |
| **ssh-key** | `add` | Mutating | |
| **ssh-key** | `delete` | Mutating | |
| **gpg-key** | `list` | ReadOnly | |
| **gpg-key** | `add` | Mutating | |
| **gpg-key** | `delete` | Mutating | |
| **ruleset** | `list` | ReadOnly | |
| **ruleset** | `view` | ReadOnly | |
| **ruleset** | `check` | ReadOnly | |
| **attestation** | `verify` | ReadOnly | |
| **attestation** | `download` | ReadOnly | |
| **org** | `list` | ReadOnly | |
| **extension** | `list` | ReadOnly | |
| **extension** | `search` | ReadOnly | |
| **extension** | `browse` | ReadOnly | |
| **extension** | `install` | Mutating | |
| **extension** | `upgrade` | Mutating | |
| **extension** | `remove` | Mutating | |
| **extension** | `create` | Mutating | |
| **alias** | `list` | ReadOnly | |
| **alias** | `set` | Mutating | |
| **alias** | `delete` | Mutating | |
| **alias** | `import` | Mutating | |
| **api** | (any GET) | ReadOnly | Default; `--method POST/PUT/PATCH/DELETE` → Mutating |
| **auth** | `status` | ReadOnly | |
| **auth** | `token` | ReadOnly | |
| **auth** | (other) | Mutating | |
| **config** | `get` | ReadOnly | |
| **config** | `list` | ReadOnly | |
| **config** | (other) | Mutating | |
| **completion** | (any) | ReadOnly | |
| **help** | (any) | ReadOnly | |
| **version** | (any) | ReadOnly | |
| **codespace** | `list`, `view`, `ssh`, `code`, `jupyter`, `logs`, `ports` | ReadOnly | |
| **codespace** | (other) | Mutating | |
| _Unknown group/action_ | | **Mutating** (safe default) | |

**Special case — `gh api`:** Default is ReadOnly (GET). If `--method` flag is present and value is `POST`, `PUT`, `PATCH`, or `DELETE`, classify as Mutating.

```rust
fn classify_gh(args: &[&str]) -> CommandClass {
    let group = args.first().map(|s| *s).unwrap_or("");
    let action = args.get(1).map(|s| *s).unwrap_or("");

    match group {
        "api" => {
            // Check for --method flag
            if let Some(pos) = args.iter().position(|a| *a == "--method" || *a == "-X") {
                match args.get(pos + 1).map(|s| s.to_uppercase()).as_deref() {
                    Some("POST") | Some("PUT") | Some("PATCH") | Some("DELETE") => CommandClass::Mutating,
                    _ => CommandClass::ReadOnly,
                }
            } else {
                CommandClass::ReadOnly  // default is GET
            }
        }
        "pr" => match action {
            "list" | "view" | "status" | "diff" | "checks" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        "issue" => match action {
            "list" | "view" | "status" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        "run" => match action {
            "list" | "view" | "watch" | "download" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        // ... (full table above)
        _ => CommandClass::Mutating,  // unknown = safe default
    }
}
```

---

## 8. Panel Types & Lifecycle

### 8.1 `GitResult` Dynamic Panel

| Property | Value |
|----------|-------|
| `ContextType` | `GitResult` |
| Fixed/Dynamic | Dynamic (P8+) |
| Has UID | Yes (persisted) |
| `needs_cache()` | Yes |
| Panel name | Truncated command, max 40 chars. E.g., `"git log --oneline -20"` |
| LLM context header | `"Git Result: {command}"` |
| Reuse key | SHA-256 hash of full command string, stored in `ContextElement.command_hash` |

#### 8.1.1 ContextElement Fields Used

```rust
ContextElement {
    context_type: ContextType::GitResult,
    name: "git log --oneline -20",        // truncated command
    cached_content: Some("abc1234 Fix...\n..."),
    git_result_command: Some("git log --oneline -20"),  // NEW field: full command for re-execution
    git_result_command_hash: Some("a1b2c3..."),         // NEW field: for panel reuse lookup
    cache_deprecated: false,
    last_refresh_ms: 1707350400000,
    // ... other fields default
}
```

#### 8.1.2 Refresh Strategy

- **Trigger:** Local git filesystem watcher detects changes in:
  - `.git/HEAD` (branch switch, commit)
  - `.git/refs/` (new branches, tags, remote refs after fetch)
  - `.git/index` (staging area changes)
  - `.git/MERGE_HEAD`, `.git/REBASE_HEAD` (merge/rebase state)
- **On trigger:** Mark all `GitResult` panels as `cache_deprecated = true`
- **Refresh interval:** No timer-based refresh. Purely event-driven via file watcher.
- **Re-execution:** When `cache_deprecated`, background thread re-runs the stored command and updates `cached_content`

#### 8.1.3 Panel Reuse Logic

```rust
fn find_or_create_git_result_panel(
    command: &str,
    command_hash: &str,
    state: &mut State,
) -> (String, bool) {  // returns (panel_id, is_new)
    // Search for existing panel with matching hash
    if let Some(ctx) = state.context.iter().find(|c|
        c.context_type == ContextType::GitResult
        && c.git_result_command_hash.as_deref() == Some(command_hash)
    ) {
        return (ctx.id.clone(), false);  // reuse
    }
    // Create new panel
    let id = state.next_dynamic_panel_id();  // P8, P9, ...
    let uid = generate_uid();
    let ctx = ContextElement {
        id,
        uid: Some(uid),
        context_type: ContextType::GitResult,
        name: truncate(command, 40),
        git_result_command: Some(command.to_string()),
        git_result_command_hash: Some(command_hash.to_string()),
        cache_deprecated: false,
        // ...
    };
    state.context.push(ctx);
    (id, true)
}
```

### 8.2 `GithubResult` Dynamic Panel

| Property | Value |
|----------|-------|
| `ContextType` | `GithubResult` |
| Fixed/Dynamic | Dynamic (P8+) |
| Has UID | Yes (persisted) |
| `needs_cache()` | Yes |
| Panel name | Truncated command, max 40 chars. E.g., `"gh pr list --state open"` |
| LLM context header | `"GitHub Result: {command}"` |
| Reuse key | SHA-256 hash of full command string |

#### 8.2.1 ContextElement Fields Used

```rust
ContextElement {
    context_type: ContextType::GithubResult,
    name: "gh pr list --state open",
    cached_content: Some("#123 Fix bug\n#124 Add feature\n..."),
    gh_result_command: Some("gh pr list --state open"),    // NEW field
    gh_result_command_hash: Some("d4e5f6..."),             // NEW field
    gh_etag: None,                                         // NEW field (v2: ETag for conditional refresh)
    gh_api_endpoint: None,                                 // NEW field (v2: mapped API endpoint)
    cache_deprecated: false,
    last_refresh_ms: 1707350400000,
}
```

#### 8.2.2 Refresh Strategy (v1 — Simple)

- **Interval:** 120 seconds (`GH_RESULT_REFRESH_MS = 120_000`)
- **Method:** Re-execute the stored `gh` command
- **Optimization:** Only refresh if panel is "active" (was accessed in the last LLM context generation, or is currently selected in UI)

#### 8.2.3 Refresh Strategy (v2 — ETag)

- **Interval:** Respect `X-Poll-Interval` header (typically 60s), minimum 60s
- **Method:** `gh api` with `If-None-Match: {stored_etag}`
- **304 detection:** Exit code 1 + stderr contains `"HTTP 304"`
- **Rate limit awareness:** If `X-Ratelimit-Remaining < 100`, multiply interval by 4. If `< 10`, stop auto-refresh.
- **Fallback:** If no API endpoint mapping exists for the command, fall back to v1 (re-execute)

#### 8.2.4 Panel Reuse Logic

Identical to `GitResult` but searches `ContextType::GithubResult` and uses `gh_result_command_hash`.

### 8.3 Panel UI Rendering

Both `GitResult` and `GithubResult` panels render their `cached_content` as plain text with basic formatting:

- Lines starting with `#` → styled as headers (bold)
- Lines starting with `+` → green (if diff-like output)
- Lines starting with `-` → red (if diff-like output)
- Lines starting with `@@` → blue/muted (diff hunk headers)
- Otherwise → default text color

The UI should detect if the content looks like a diff (contains `+++`, `---`, `@@` patterns) and apply diff syntax highlighting automatically. Otherwise, render as plain monospaced text.

### 8.4 Panel Closing

Both panel types can be closed via the existing `context_close` tool. Closing a result panel:
1. Removes from `state.context`
2. Removes from `panel_uid_to_local_id`
3. Deletes persisted `panels/{uid}.json`
4. Stops any auto-refresh for that panel

---

## 9. Caching & Refresh Strategies

### 9.1 P6 Git Panel (Unchanged + `diff_base` Enhancement)

| Aspect | Value |
|--------|-------|
| Refresh interval | 2000ms (`GIT_STATUS_REFRESH_MS`) |
| Change detection | Hash of `git status --porcelain -uall` + branch name |
| Background commands | `git status`, `git rev-parse`, `git diff [base] --numstat`, `git diff [base]` (conditional) |
| Deprecation triggers | `git_execute` mutating command, `git_configure_p6`, `gh_execute` mutating (e.g., `gh pr checkout`) |

**Enhancement:** When `state.git_diff_base` is `Some(ref)`:
- `CacheRequest::RefreshGitStatus` gains a new field: `diff_base: Option<String>`
- Background thread uses `git diff {ref}` instead of `git diff HEAD`
- numstat commands: `git diff {ref} --numstat` instead of `git diff --cached --numstat` + `git diff --numstat`
- P6 title: `"Git (branch) [vs ref]"`

### 9.2 GitResult Panels

| Aspect | Value |
|--------|-------|
| Refresh interval | None (event-driven) |
| Change detection | `.git/` filesystem watcher events |
| Background commands | Re-execute stored git command |
| Deprecation triggers | `.git/HEAD` change, `.git/refs/` change, `.git/index` change, any `git_execute` mutating command |

**File watcher targets:**
```
.git/HEAD
.git/refs/heads/      (recursive)
.git/refs/tags/       (recursive)
.git/refs/remotes/    (recursive)
.git/index
.git/MERGE_HEAD
.git/REBASE_HEAD
.git/CHERRY_PICK_HEAD
```

**Implementation:** Use the existing `notify` crate (already a dependency for file watching). Add watchers for the above paths when the git module initializes. On any event, iterate all `ContextType::GitResult` elements and set `cache_deprecated = true`.

### 9.3 GithubResult Panels

| Aspect | Value |
|--------|-------|
| Refresh interval (v1) | 120s (`GH_RESULT_REFRESH_MS`) |
| Refresh interval (v2) | `X-Poll-Interval` header value (min 60s) |
| Change detection (v1) | Content comparison (hash of output) |
| Change detection (v2) | ETag / 304 response |
| Background commands | Re-execute stored gh command (or `gh api` with ETag in v2) |
| Deprecation triggers | Any `gh_execute` mutating command |

### 9.4 Cache Variants (New)

```rust
// New CacheRequest variants
pub enum CacheRequest {
    // ... existing variants ...

    /// Refresh a GitResult panel by re-executing the stored command
    RefreshGitResult {
        context_id: String,
        command: String,
    },

    /// Refresh a GithubResult panel by re-executing the stored command
    RefreshGithubResult {
        context_id: String,
        command: String,
        github_token: String,
        etag: Option<String>,           // v2: for conditional request
        api_endpoint: Option<String>,   // v2: for ETag-based refresh
    },
}

// New CacheUpdate variants
pub enum CacheUpdate {
    // ... existing variants ...

    /// Git command result
    GitResultContent {
        context_id: String,
        content: String,
        token_count: usize,
        is_error: bool,
    },

    /// GitHub command result
    GithubResultContent {
        context_id: String,
        content: String,
        token_count: usize,
        is_error: bool,
        new_etag: Option<String>,       // v2: updated ETag from response
    },

    /// GitHub result unchanged (ETag matched, 304)
    GithubResultUnchanged {
        context_id: String,
    },
}
```

---

## 10. Authentication & Token Management

### 10.1 Token Source

The GitHub token is loaded from the `.env` file in the project root:

```
GITHUB_TOKEN=ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

### 10.2 Token Loading

On TUI startup (or module load):
1. Read `.env` file from working directory
2. Look for `GITHUB_TOKEN=...` line
3. Store value in `state.github_token: Option<String>`
4. If not found, also check environment variable `GITHUB_TOKEN`
5. If neither found, `state.github_token = None` (module loads with warning)

### 10.3 Token Usage

**Every** `gh` command execution MUST inject:
```rust
.env("GITHUB_TOKEN", &token)
.env("GH_TOKEN", &token)
```

This overrides any local `gh auth` configuration. The TUI does not rely on `gh auth login` or any credential stored in `~/.config/gh/`.

### 10.4 Token Security

- Token is stored in `State` at runtime only (`#[serde(skip)]` — never persisted)
- Token is never logged, never included in error messages, never sent to LLM
- Token is never displayed in any panel content
- If a command's output accidentally contains the token, it should be redacted before storing in `cached_content`

### 10.5 Token Refresh

No automatic refresh. If the token expires or is revoked:
- `gh` commands will fail with authentication errors
- The error message is returned to the LLM
- User must update `.env` and restart the TUI

---

## 11. State Changes

### 11.1 New Fields in `State`

```rust
// === GitHub (runtime-only) ===
/// GitHub token loaded from .env (never persisted, never logged)
#[serde(skip)]
pub github_token: Option<String>,

/// GitHub API rate limit remaining (updated from response headers)
#[serde(skip)]
pub github_rate_limit_remaining: Option<u32>,

/// GitHub API rate limit reset time (Unix epoch, from response headers)
#[serde(skip)]
pub github_rate_limit_reset: Option<u64>,

// === Git P6 configuration (persisted via module data) ===
/// Diff comparison base for P6 panel. None = HEAD (default behavior).
/// Can be a commit SHA, branch name, tag, or relative ref.
pub git_diff_base: Option<String>,
```

### 11.2 New Fields in `ContextElement`

```rust
// === GitResult / GithubResult panel fields ===
/// Full command string for re-execution (e.g., "git log --oneline -20")
#[serde(default, skip_serializing_if = "Option::is_none")]
pub result_command: Option<String>,

/// SHA-256 hash of command string for panel reuse lookup
#[serde(default, skip_serializing_if = "Option::is_none")]
pub result_command_hash: Option<String>,

/// ETag from last GitHub API response (for conditional refresh, v2)
#[serde(skip)]
pub gh_etag: Option<String>,

/// Mapped API endpoint for ETag refresh (v2)
#[serde(skip)]
pub gh_api_endpoint: Option<String>,
```

### 11.3 Removed Fields from `State`

None removed. All existing `git_*` fields remain (P6 panel still uses them).

### 11.4 Changed Defaults

| Field | Old Default | New Default | Reason |
|-------|-------------|-------------|--------|
| `git_show_diffs` | `true` | `true` | Unchanged |
| `git_diff_base` | (didn't exist) | `None` | New field, None = HEAD |

---

## 12. Persistence

### 12.1 GitResult Panels

Persisted to `panels/{uid}.json` like other dynamic panels. Key fields:
- `panel_type: "git_result"`
- `name: "git log --oneline -20"`
- `result_command: "git log --oneline -20"`
- `result_command_hash: "a1b2c3..."`

On load: `cache_deprecated = true` (will re-execute command to refresh content).

### 12.2 GithubResult Panels

Persisted to `panels/{uid}.json`. Key fields:
- `panel_type: "github_result"`
- `name: "gh pr list --state open"`
- `result_command: "gh pr list --state open"`
- `result_command_hash: "d4e5f6..."`

On load: `cache_deprecated = true`. ETag is NOT persisted (always re-fetches on startup).

### 12.3 Module Data

**Git module** (`config.json`):
```json
{
  "git": {
    "git_show_diffs": true,
    "git_diff_base": null
  }
}
```

**GitHub module** (`config.json`):
```json
{
  "github": {}
}
```

No GitHub module data is persisted (token comes from `.env`, rate limit is runtime-only).

### 12.4 Worker State

`ImportantPanelUids` gains no new entries (GitResult and GithubResult are dynamic, stored in `panel_uid_to_local_id`).

---

## 13. Non-Functional Requirements

### 13.1 Performance

| Requirement | Target |
|-------------|--------|
| P6 refresh latency | <500ms for repos with <1000 changed files |
| `git_execute` read-only response | <2s for typical commands (log, diff, show) |
| `gh_execute` read-only response | <5s (network-dependent) |
| `gh_execute` mutating response | <10s (network-dependent) |
| Panel reuse lookup | O(n) where n = number of open panels (typically <20) |
| Memory per GitResult panel | <1MB cached_content (truncate if larger) |
| Memory per GithubResult panel | <1MB cached_content (truncate if larger) |

### 13.2 Rate Limiting

| Requirement | Target |
|-------------|--------|
| GitHub API budget | <100 requests/hour for auto-refresh (v1) |
| Manual operations | No limit (user-initiated) |
| Rate limit monitoring | Parse `X-Ratelimit-*` headers, back off when low |
| Polling interval | Minimum 60s between auto-refresh for same panel |

### 13.3 Security

| Requirement | Implementation |
|-------------|---------------|
| No shell injection | Validate command strings, reject shell operators |
| Token isolation | Token only in env vars, never in args, logs, or panel content |
| No local config trust | All `gh` commands use explicit `GITHUB_TOKEN`/`GH_TOKEN` env vars |
| Command sandboxing | `stdin: null`, `GIT_TERMINAL_PROMPT=0`, `GH_PROMPT_DISABLED=1` |
| Output sanitization | Scan `cached_content` for token substring, redact if found |

### 13.4 Reliability

| Requirement | Implementation |
|-------------|---------------|
| Command timeout | 30s for git commands, 60s for gh commands |
| Graceful degradation | If git not installed → P6 shows "git not found", git_execute returns error |
| Graceful degradation | If gh not installed → gh_execute returns "gh CLI not found. Install: https://cli.github.com" |
| Error propagation | Non-zero exit code → `is_error: true` in ToolResult, stderr included in content |
| Crash resistance | Command execution wrapped in catch_unwind / timeout |

### 13.5 Observability

| Requirement | Implementation |
|-------------|---------------|
| Command logging | Log all executed commands (without token) to `swallowed-errors.log` on failure |
| Rate limit visibility | Display remaining rate limit in status bar (when <500) |
| Panel staleness | Show `[refreshing...]` prefix in LLM context when `cache_deprecated = true` |

### 13.6 Compatibility

| Requirement | Target |
|-------------|--------|
| `git` version | 2.25+ (for `git switch`, `git restore`) |
| `gh` version | 2.0+ |
| GitHub API | REST v3 (no GraphQL dependency) |
| Token types | Classic PAT (`ghp_`), Fine-grained PAT (`github_pat_`), OAuth tokens |

---

## 14. Migration & Backwards Compatibility

### 14.1 Config Migration

On first load with the new code:
1. Read existing `config.json` → `git.git_show_diffs` field migrates as-is
2. `git_diff_base` defaults to `None` (no existing data)
3. No `github` section yet → created empty

### 14.2 Tool Migration

Old tool IDs are no longer recognized. If persisted tool state references `git_commit`, `git_push`, etc., they are silently ignored (tools are re-registered from module definitions on startup).

### 14.3 Panel Migration

Existing P6 panel (`ContextType::Git`) is unchanged. No migration needed.

New panel types (`GitResult`, `GithubResult`) don't exist in old data. No migration needed.

### 14.4 Deserialization Compatibility

New `ContextType` variants (`GitResult`, `GithubResult`) must handle missing entries in old `panels/{uid}.json` gracefully. The `#[serde(rename_all = "snake_case")]` attribute on the enum ensures clean serialization as `"git_result"` and `"github_result"`.

---

## 15. Removed Components

### 15.1 Removed Tools (9)

| Tool ID | Replacement |
|---------|-------------|
| `git_toggle_details` | `git_configure_p6(show_diffs=true/false)` |
| `git_toggle_logs` | `git_configure_p6(show_logs=true/false, log_args="...")` |
| `git_commit` | `git_execute(command="git commit -m '...'")` or `git_execute(command="git add file && git commit -m '...'")` — note: shell operators blocked, so staging must be a separate call: first `git_execute(command="git add file.rs")` then `git_execute(command="git commit -m 'message'")` |
| `git_branch_create` | `git_execute(command="git checkout -b branch-name")` |
| `git_branch_switch` | `git_execute(command="git checkout branch-name")` |
| `git_merge` | `git_execute(command="git merge branch-name")` |
| `git_pull` | `git_execute(command="git pull")` |
| `git_push` | `git_execute(command="git push")` |
| `git_fetch` | `git_execute(command="git fetch")` |

### 15.2 Removed Code Files

| File | Disposition |
|------|-------------|
| `src/modules/git/tools.rs` | **Rewrite entirely.** Replace 9 tool functions with `execute_git_command()` and `execute_configure_p6()` |

### 15.3 Removed Cache Variants

None removed. `RefreshGitStatus`, `GitStatus`, `GitStatusUnchanged` remain for P6.

---

## 16. File Inventory

### 16.1 New Files

| File | Purpose |
|------|---------|
| `src/modules/github/mod.rs` | GitHub module definition, `gh_execute` tool |
| `src/modules/github/panel.rs` | `GithubResultPanel` implementing Panel trait |
| `src/modules/github/types.rs` | GitHub-specific types (if needed) |
| `src/modules/github/classify.rs` | `gh` command classification lookup table |
| `src/modules/git/classify.rs` | `git` command classification lookup table |

### 16.2 Modified Files

| File | Changes |
|------|---------|
| `src/modules/git/mod.rs` | Replace 9 tool defs with 2 (`git_execute`, `git_configure_p6`). Add `dynamic_panel_types()`. Update `execute_tool()` dispatch. |
| `src/modules/git/tools.rs` | Rewrite: remove 9 old functions, add `execute_git_command()` + `execute_configure_p6()` |
| `src/modules/git/panel.rs` | Add `GitResultPanel`. Modify `GitPanel` to use `git_diff_base`. Add `.git/` file watchers. |
| `src/modules/git/types.rs` | Add `CommandClass` enum (or put in `classify.rs`) |
| `src/modules/mod.rs` | Register `GithubModule` in `all_modules()`. Add `ContextType::GitResult`, `ContextType::GithubResult` handling. |
| `src/state.rs` | Add `ContextType::GitResult`, `ContextType::GithubResult` to enum. Add `github_token`, `github_rate_limit_*`, `git_diff_base` to `State`. Add `result_command`, `result_command_hash`, `gh_etag`, `gh_api_endpoint` to `ContextElement`. |
| `src/cache.rs` | Add `RefreshGitResult`, `RefreshGithubResult` request variants. Add `GitResultContent`, `GithubResultContent`, `GithubResultUnchanged` update variants. Wire up dispatch for new context types. |
| `src/tool_defs.rs` | Add `ToolCategory::Github` variant. |
| `src/constants.rs` | Add `GH_RESULT_REFRESH_MS: u64 = 120_000`. |
| `src/persistence/mod.rs` | Handle `GitResult` and `GithubResult` in `panel_to_context()`. Persist/restore `result_command` and `result_command_hash`. |
| `src/persistence/panel.rs` | Add serialization for new `ContextElement` fields. |
| `src/modules/core/tools/manage_tools.rs` | No changes needed (new tools are not protected). |

### 16.3 Unchanged Files

| File | Reason |
|------|--------|
| `src/modules/core/mod.rs` | No changes (panel_goto_page stays as-is) |
| `src/modules/{tree,glob,grep,tmux,todo,memory,scratchpad}/*` | Unaffected by this rework |
| `src/llms/*` | Unaffected |
| `src/ui/*` | Unaffected (panel rendering dispatches via Panel trait) |
| `src/core/context.rs` | Unaffected (dynamic panels already collected by `collect_all_context()`) |

---

## Appendix A: LLM Tool Description (System Prompt)

The following text is what the LLM sees in its tool definitions:

### `git_execute`
> Execute a git CLI command. Read-only commands (log, diff, show, blame, etc.) open a result panel with the output. Mutating commands (commit, push, merge, etc.) execute and return the output directly. The command must start with 'git '. Shell operators (|, ;, &&, ||) and redirects are not allowed — pass a single git command.

### `git_configure_p6`
> Configure the fixed git status panel (P6). All parameters are optional — only provided parameters are updated. `diff_base` sets the comparison base for diffs (default: HEAD). Examples: 'HEAD~5', 'main', 'abc1234', a tag name.

### `gh_execute`
> Execute a GitHub CLI (gh) command. Read-only commands (list, view, status, checks, etc.) open a result panel that auto-refreshes. Mutating commands (create, merge, close, etc.) execute and return the output. The command must start with 'gh '. All commands are authenticated via the configured GITHUB_TOKEN. Shell operators are not allowed.

---

## Appendix B: Example Flows

### B.1 LLM Lists Open PRs

```
LLM → gh_execute(command: "gh pr list --state open --json number,title,author")
  1. Validate: starts with "gh ", no shell operators ✓
  2. Classify: gh > pr > list → ReadOnly
  3. No existing GithubResult panel with this command hash
  4. Execute: gh pr list --state open --json number,title,author
     env: GITHUB_TOKEN=ghp_xxx, GH_TOKEN=ghp_xxx, GH_PROMPT_DISABLED=1, NO_COLOR=1
  5. Create GithubResult panel P8, name="gh pr list --state open --j..."
  6. Store output as cached_content
  7. Return: "Panel created: P8 — gh pr list --state open --json number,title,author"
  8. Panel auto-refreshes every 120s
```

### B.2 LLM Commits and Pushes

```
LLM → git_execute(command: "git add src/main.rs src/lib.rs")
  1. Classify: git > add → Mutating
  2. Execute: git add src/main.rs src/lib.rs
  3. Mark P6 + all GitResult panels as cache_deprecated
  4. Return: stdout (empty on success)

LLM → git_execute(command: "git commit -m 'feat: add new feature'")
  1. Classify: git > commit → Mutating
  2. Execute: git commit -m 'feat: add new feature'
  3. Mark P6 + all GitResult panels as cache_deprecated
  4. Return: "[main abc1234] feat: add new feature\n 2 files changed, ..."

LLM → git_execute(command: "git push")
  1. Classify: git > push → Mutating
  2. Execute: git push (env: GIT_TERMINAL_PROMPT=0)
  3. Mark P6 + all GitResult/GithubResult panels as cache_deprecated
  4. Return: "To github.com:user/repo.git\n   abc1234..def5678  main -> main"
```

### B.3 LLM Views Git Log (Reuse)

```
LLM → git_execute(command: "git log --oneline -20")
  1. Classify: git > log → ReadOnly
  2. hash("git log --oneline -20") = "abc..."
  3. No existing panel → create P9 GitResult
  4. Return: "Panel created: P9 — git log --oneline -20"

... later ...

LLM → git_execute(command: "git log --oneline -20")
  1. Classify: git > log → ReadOnly
  2. hash("git log --oneline -20") = "abc..."
  3. Found existing P9 → reuse, re-execute, update content
  4. Return: "Panel updated: P9 — git log --oneline -20"
```

### B.4 LLM Configures P6 Diff Base

```
LLM → git_configure_p6(diff_base: "HEAD~5", show_diffs: true)
  1. Validate "HEAD~5": git rev-parse --verify HEAD~5 → success
  2. Set state.git_diff_base = Some("HEAD~5")
  3. Set state.git_show_diffs = true
  4. Mark P6 cache_deprecated = true
  5. Return: "P6 configured: show_diffs=true, diff_base='HEAD~5'"
  6. Next P6 refresh: git diff HEAD~5, git diff HEAD~5 --numstat
  7. P6 title: "Git (main) [vs HEAD~5]"
```
