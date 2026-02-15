# Claude Code Complete Features Reference
### Comprehensive Documentation of All Existing Capabilities

---

## Table of Contents
1. [Built-in Tools](#1-built-in-tools)
2. [Slash Commands (Built-in)](#2-slash-commands-built-in)
3. [CLI Commands & Flags](#3-cli-commands--flags)
4. [Keyboard Shortcuts](#4-keyboard-shortcuts)
5. [Input Modes & Quick Commands](#5-input-modes--quick-commands)
6. [Permission Modes](#6-permission-modes)
7. [Thinking Modes](#7-thinking-modes)
8. [Hooks System](#8-hooks-system)
9. [MCP (Model Context Protocol)](#9-mcp-model-context-protocol)
10. [Skills System](#10-skills-system)
11. [Subagents](#11-subagents)
12. [Configuration Files](#12-configuration-files)
13. [Environment Variables](#13-environment-variables)
14. [IDE Integrations](#14-ide-integrations)
15. [CLAUDE.md Memory System](#15-claudemd-memory-system)
16. [Session Management](#16-session-management)
17. [Output Formats](#17-output-formats)
18. [Plugins & Marketplaces](#18-plugins--marketplaces)

---

## 1. Built-in Tools

Core tools Claude Code uses to interact with your codebase and system.

| Tool | Description | Key Parameters | Use Case |
|------|-------------|----------------|----------|
| **Read** | Read file contents | `file_path`, `line_range` (optional) | View source code, configs, docs |
| **Write** | Create new files | `file_path`, `content` | Generate new files from scratch |
| **Edit** | Modify existing files | `file_path`, `old_text`, `new_text` | Make targeted changes to files |
| **MultiEdit** | Batch file editing | `edits[]` array | Multiple changes across files |
| **Bash** | Execute shell commands | `command`, `description` | Run tests, git, npm, docker, etc. |
| **Grep** | Search file contents (ripgrep) | `pattern`, `glob`, `output_mode` | Find code patterns, text search |
| **Glob** | Find files by pattern | `pattern` | Locate files by name/extension |
| **LS** | List directory contents | `path`, `depth` | Explore project structure |
| **Task** | Spawn sub-agents | `subagent_type`, `prompt` | Delegate complex research tasks |
| **WebFetch** | Fetch web content | `url` | Read documentation, APIs |
| **WebSearch** | Search the web | `query` | Find current information |
| **TodoRead** | Read task list | - | Check current tasks |
| **TodoWrite** | Create/update tasks | `todos[]` array | Track multi-step work |
| **NotebookRead** | Read Jupyter notebooks | `file_path` | View notebook cells |
| **NotebookEdit** | Edit Jupyter notebooks | `file_path`, `cell_changes` | Modify notebook content |

### Tool Permission Syntax

| Permission Pattern | Description | Example |
|--------------------|-------------|---------|
| `Read` | Allow all file reads | Full codebase access |
| `Write(src/**)` | Write only in src/ | Scoped write permissions |
| `Bash(git *)` | Only git commands | `git status`, `git commit` |
| `Bash(npm:*)` | Only npm commands | `npm install`, `npm test` |
| `mcp__server__tool` | MCP tool access | `mcp__github__create_issue` |

---

## 2. Slash Commands (Built-in)

Commands invoked with `/` during interactive sessions.

### Core Session Commands

| Command | Description | Notes |
|---------|-------------|-------|
| `/help` | Show all available commands | Lists built-in + custom commands |
| `/clear` | Clear conversation history | Fresh start, saves tokens |
| `/compact` | Compress/summarize context | Preserves essential info |
| `/exit` | Exit Claude Code | Proper session cleanup |
| `/status` | Show version and connectivity | Troubleshooting info |

### Context & Memory Commands

| Command | Description | Notes |
|---------|-------------|-------|
| `/context` | Visualize context usage | Shows token consumption grid |
| `/memory` | Edit CLAUDE.md file | Add persistent instructions |
| `/init` | Generate CLAUDE.md | Auto-creates project memory |
| `/add-dir` | Add directory to context | Access additional folders |

### Configuration Commands

| Command | Description | Notes |
|---------|-------------|-------|
| `/config` | Open settings panel | Tab-based interface |
| `/model` | Switch AI model | Interactive model picker |
| `/permissions` | Manage tool permissions | Add/remove allowed tools |
| `/hooks` | Configure hooks interactively | Set up automation |
| `/mcp` | Manage MCP servers | View/configure connections |
| `/agents` | Manage subagents | Configure specialized agents |
| `/terminal-setup` | Configure terminal bindings | Fix Shift+Enter, etc. |

### Workflow Commands

| Command | Description | Notes |
|---------|-------------|-------|
| `/resume` | Resume previous session | Conversation picker |
| `/review` | Code review current changes | Auto-review workflow |
| `/doctor` | Check installation health | Diagnose issues |
| `/cost` | Show token usage/costs | Session expense tracking |
| `/export` | Export conversation | Save for documentation |
| `/todos` | Show task list | View tracked tasks |
| `/ide` | Connect to IDE | Link external terminal |
| `/login` | Authenticate account | OAuth login flow |
| `/logout` | Sign out | End authentication |
| `/bug` | Report a bug | Submit issue to Anthropic |

### GitHub Integration

| Command | Description | Notes |
|---------|-------------|-------|
| `/install-github-app` | Set up PR reviews | Auto-review PRs |

---

## 3. CLI Commands & Flags

Commands run from your terminal outside of interactive sessions.

### Basic CLI Usage

| Command | Description | Example |
|---------|-------------|---------|
| `claude` | Start interactive REPL | Opens chat interface |
| `claude "prompt"` | Start with initial prompt | `claude "fix the bug"` |
| `claude -p "prompt"` | Print mode (headless) | Non-interactive, outputs result |
| `claude -c` | Continue last session | Resume most recent chat |
| `claude --continue` | Continue last session | Same as `-c` |
| `claude --resume` | Resume specific session | Opens session picker |
| `claude --resume <id>` | Resume by session ID | Direct session restore |
| `claude --version` | Show version | Check installed version |
| `claude update` | Update to latest | Self-update mechanism |

### Model Selection Flags

| Flag | Description | Example |
|------|-------------|---------|
| `--model <name>` | Specify model | `--model opus` |
| `--model sonnet` | Use Sonnet model | Balanced performance |
| `--model opus` | Use Opus model | Maximum capability |
| `--model haiku` | Use Haiku model | Fast, lightweight |

### Output & Format Flags

| Flag | Description | Example |
|------|-------------|---------|
| `--output-format json` | JSON output | For scripting/parsing |
| `--output-format text` | Plain text output | Human readable |
| `--output-format stream-json` | Streaming JSON | Real-time processing |
| `--verbose` | Enable verbose logging | Debug information |
| `--debug` | Enable debug mode | `--debug "api,mcp"` |

### Permission Flags

| Flag | Description | Example |
|------|-------------|---------|
| `--allowedTools` | Whitelist tools | `--allowedTools "Read,Write"` |
| `--disallowedTools` | Blacklist tools | `--disallowedTools "Bash(rm:*)"` |
| `--permission-mode` | Set permission mode | `--permission-mode plan` |
| `--dangerously-skip-permissions` | Skip all prompts | ⚠️ Use with caution |

### Context & Directory Flags

| Flag | Description | Example |
|------|-------------|---------|
| `--add-dir <path>` | Add working directory | `--add-dir ../shared` |
| `--max-turns <n>` | Limit conversation turns | `--max-turns 3` |

### System Prompt Flags

| Flag | Description | Use Case |
|------|-------------|----------|
| `--system-prompt` | Replace entire system prompt | Full control |
| `--system-prompt-file` | Load prompt from file | Version controlled |
| `--append-system-prompt` | Add to default prompt | Safest option |
| `--append-system-prompt-file` | Append from file | Team consistency |

### Agent Configuration Flags

| Flag | Description | Example |
|------|-------------|---------|
| `--agents` | Define subagents (JSON) | Complex agent setup |

### MCP Commands

| Command | Description | Example |
|---------|-------------|---------|
| `claude mcp add` | Add MCP server | `claude mcp add github` |
| `claude mcp add-json` | Add server with JSON | Complex config |
| `claude mcp list` | List configured servers | View all connections |
| `claude mcp remove` | Remove server | `claude mcp remove github` |
| `claude mcp get` | Test server connection | Verify setup |

### Config Commands

| Command | Description | Example |
|---------|-------------|---------|
| `claude config set` | Set configuration value | `claude config set model opus` |
| `claude config set -g` | Set global config | User-wide setting |

---

## 4. Keyboard Shortcuts

### General Controls

| Shortcut | Action | Notes |
|----------|--------|-------|
| `Esc` | Stop/cancel current action | Interrupt Claude |
| `Esc Esc` (double) | Rewind conversation picker | Jump to previous messages |
| `Ctrl+C` | Cancel operation | First press stops, second exits |
| `Ctrl+C Ctrl+C` | Hard exit | Force quit |
| `Ctrl+D` | Exit Claude Code | Clean exit |

### Mode Toggles

| Shortcut | Action | Notes |
|----------|--------|-------|
| `Tab` | Toggle thinking mode | Enable/disable extended thinking |
| `Shift+Tab` | Cycle permission modes | Normal → Auto-accept → Plan |
| `Ctrl+O` | Toggle verbose mode | Show Claude's thinking |

### Text Editing (Bash-style)

| Shortcut | Action | Notes |
|----------|--------|-------|
| `Ctrl+A` | Jump to start of line | Beginning of input |
| `Ctrl+E` | Jump to end of line | End of input |
| `Option/Alt+F` | Move forward one word | Mac: requires Meta config |
| `Option/Alt+B` | Move backward one word | Mac: requires Meta config |
| `Ctrl+W` | Delete previous word | Quick editing |
| `Ctrl+U` | Clear line | Start fresh |

### Input & Navigation

| Shortcut | Action | Notes |
|----------|--------|-------|
| `Shift+Enter` | New line in prompt | Multi-line input |
| `Up Arrow` | Previous command/history | Navigate history |
| `Down Arrow` | Next command/history | Navigate history |
| `Ctrl+R` | Reverse search history | Find past commands |
| `Ctrl+V` | Paste images | Works when Cmd+V fails |
| `?` | Show help/shortcuts | View available shortcuts |

### Task & Background

| Shortcut | Action | Notes |
|----------|--------|-------|
| `Ctrl+T` | Toggle task list view | Show/hide todos |
| `Ctrl+B` | Send task to background | Continue working |
| `Ctrl+G` | Open plan in editor | Edit plan externally |

### VS Code Extension Specific

| Shortcut | Action | Notes |
|----------|--------|-------|
| `Cmd/Ctrl+Esc` | Toggle between editor/Claude | Focus switching |
| `Cmd/Ctrl+N` | New conversation | Fresh chat |
| `Cmd/Ctrl+Option+K` | Insert file reference | Quick @file insertion |

---

## 5. Input Modes & Quick Commands

### Quick Prefixes

| Prefix | Function | Example |
|--------|----------|---------|
| `@` | Reference file/directory | `@src/index.js` |
| `@file#L1-50` | Reference file with lines | `@utils.js#L10-25` |
| `@terminal:name` | Reference terminal output | `@terminal:dev-server` |
| `#` | Add to CLAUDE.md memory | `# Always use TypeScript` |
| `!` | Execute bash directly | `!git status` |
| `!` `command` | Bash bypass mode | Saves tokens vs conversational |

### Thinking Trigger Words

| Word/Phrase | Effect | Token Budget |
|-------------|--------|--------------|
| `think` | Enable thinking | Standard budget |
| `think hard` | Deep thinking | Higher budget |
| `ultrathink` | Maximum thinking | Maximum budget |
| `think more` | Extended reasoning | Increased budget |

---

## 6. Permission Modes

Modes controlling how Claude handles edits and executions.

| Mode | Indicator | Behavior | Toggle |
|------|-----------|----------|--------|
| **Normal/Edit** | Default | Asks permission for each action | Default mode |
| **Auto-Accept** | `⏵⏵ accept edits on` | Auto-approves file edits | `Shift+Tab` once |
| **Plan Mode** | `⏸ plan mode on` | Plans without executing | `Shift+Tab` twice |

### Starting in Specific Modes

| Method | Command |
|--------|---------|
| CLI flag | `claude --permission-mode plan` |
| Headless plan | `claude -p --permission-mode plan "prompt"` |

---

## 7. Thinking Modes

Extended thinking capabilities for complex reasoning.

| Feature | Description | Control |
|---------|-------------|---------|
| **Extended Thinking** | Deep reasoning before responding | Enabled by default |
| **Thinking Toggle** | Enable/disable thinking | `Tab` key |
| **Verbose Thinking** | View Claude's reasoning | `Ctrl+O` toggle |
| **Thinking Budget** | Token limit for thinking | `MAX_THINKING_TOKENS` env |

### Configuration

| Setting | Description | Values |
|---------|-------------|--------|
| Default budget | Up to 31,999 tokens | Automatic allocation |
| Minimum budget | Lowest possible | 1,024 tokens |
| Disable thinking | Turn off entirely | `/config` → thinking toggle |

---

## 8. Hooks System

Automated shell commands triggered at specific lifecycle events.

### Hook Events

| Event | Trigger Point | Can Block? | Use Case |
|-------|--------------|------------|----------|
| **PreToolUse** | Before tool execution | Yes (exit 2) | Validate, block dangerous ops |
| **PostToolUse** | After tool completion | No | Format code, run tests |
| **PermissionRequest** | On permission dialog | Yes | Auto-approve/deny |
| **UserPromptSubmit** | Before prompt processed | Yes | Validate, inject context |
| **Notification** | When Claude notifies | No | Custom alerts, TTS |
| **Stop** | When Claude finishes | Yes | Final checks, reports |
| **SubagentStop** | When subagent finishes | Yes | Ensure completion |
| **PreCompact** | Before compaction | No | Backup transcripts |
| **SessionStart** | Session begins/resumes | No | Load context, setup |
| **SessionEnd** | Session ends | No | Cleanup, logging |
| **Setup** | Initial setup phase | No | Environment configuration |

### Hook Configuration Structure

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "prettier --write $CLAUDE_FILE_PATHS"
          }
        ]
      }
    ]
  }
}
```

### Common Matchers

| Matcher | Matches |
|---------|---------|
| `Bash` | Any bash command |
| `Write` | File write operations |
| `Edit` | File edit operations |
| `Edit\|Write` | Either edit or write |
| `Task` | Subagent tasks |
| `WebFetch` | Web fetches |
| `*` | All tools |

### Hook Environment Variables

| Variable | Description |
|----------|-------------|
| `$CLAUDE_FILE_PATHS` | Files being operated on |
| `$CLAUDE_TOOL_NAME` | Current tool name |
| `$CLAUDE_WORKING_DIR` | Working directory |
| `$CLAUDE_SESSION_ID` | Session identifier |
| `$CLAUDE_PROJECT_DIR` | Project root path |
| `$CLAUDE_CODE_REMOTE` | Remote environment flag |

### Exit Codes

| Code | Effect |
|------|--------|
| `0` | Success, continue |
| `1` | Error (logged but continues) |
| `2` | Block action, show error to Claude |

---

## 9. MCP (Model Context Protocol)

Connect Claude Code to external tools, databases, and APIs.

### Transport Types

| Type | Use Case | Example |
|------|----------|---------|
| `stdio` | Local processes | npm packages |
| `http` | Cloud HTTP servers | REST APIs |
| `sse` | Server-sent events | Real-time connections |

### Configuration Locations

| Location | Scope | Version Control |
|----------|-------|-----------------|
| `.mcp.json` | Project | ✓ Committed |
| `.claude/settings.local.json` | Project | ✗ Gitignored |
| `~/.claude/settings.local.json` | User | ✗ Personal |

### MCP Server Configuration Example

```json
{
  "mcpServers": {
    "github": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_xxx"
      }
    }
  }
}
```

### Popular MCP Servers

| Server | Purpose | Features |
|--------|---------|----------|
| GitHub | Repository management | PRs, issues, CI/CD |
| Slack | Team communication | Post messages, read channels |
| Jira | Issue tracking | Create/update tickets |
| Figma | Design access | Read designs, assets |
| Sentry | Error monitoring | View/analyze errors |
| Postgres | Database access | Query, schema inspection |
| Perplexity | AI search | Research, documentation |
| Sequential Thinking | Reasoning | Complex problem breakdown |
| Context7 | Documentation | Real-time library docs |
| Playwright | Browser automation | UI testing |

### MCP Tool Search

| Setting | Description | Default |
|---------|-------------|---------|
| Auto mode | Activates when tools > 10% context | Enabled |
| Threshold | Context percentage trigger | 10% |
| `ENABLE_TOOL_SEARCH` | Control behavior | `auto`, `auto:N`, `false` |

---

## 10. Skills System

Extended capabilities through SKILL.md files.

### Skill Structure

```markdown
---
name: skill-name
description: When to use this skill
allowed-tools: Read, Write, Bash
disable-model-invocation: false
user-invocable: true
---

# Skill Instructions

Detailed instructions for Claude...
```

### Skill Frontmatter Options

| Field | Description | Values |
|-------|-------------|--------|
| `name` | Skill name (becomes /command) | String |
| `description` | When to invoke | String |
| `allowed-tools` | Tools skill can use | Comma-separated |
| `disable-model-invocation` | Prevent auto-invocation | `true`/`false` |
| `user-invocable` | Show in menu | `true`/`false` |
| `argument-hint` | Argument description | `[arg1] [arg2]` |
| `model` | Model to use | `sonnet`, `opus` |

### Skill Locations

| Location | Scope |
|----------|-------|
| `.claude/skills/skill-name/SKILL.md` | Project |
| `~/.claude/skills/skill-name/SKILL.md` | Global |

### Anthropic Official Skills

| Skill | Purpose |
|-------|---------|
| `pdf` | PDF processing |
| `docx` | Word document creation/editing |
| `pptx` | PowerPoint creation |
| `xlsx` | Excel spreadsheet handling |

---

## 11. Subagents

Specialized Claude instances for delegated tasks.

### Built-in Agent Types

| Type | Description | Tools Available |
|------|-------------|-----------------|
| `general-purpose` | Research, exploration | All tools |
| `statusline-setup` | Configure status line | Read, Edit |
| `output-style-setup` | Create output styles | Read, Write, Edit, Glob, LS, Grep |

### Custom Subagent Configuration

```markdown
---
name: code-reviewer
description: Expert code reviewer for quality and security
tools: Read, Grep, Glob
model: sonnet
---

You are a senior code reviewer...
```

### Subagent Locations

| Location | Scope |
|----------|-------|
| `.claude/agents/agent-name.md` | Project |
| `~/.claude/agents/agent-name.md` | Global |

### Agent Tool Categories

| Category | Tools | Use Case |
|----------|-------|----------|
| Read-only | Read, Grep, Glob | Reviewers, auditors |
| Research | Read, Grep, Glob, WebFetch, WebSearch | Analysts |
| Code writers | Read, Write, Edit, Bash, Glob, Grep | Developers |
| Documentation | Read, Write, Edit, Glob, Grep, WebFetch | Writers |

---

## 12. Configuration Files

### File Hierarchy (Precedence: Top to Bottom)

| File | Location | Scope | Shared |
|------|----------|-------|--------|
| Managed policy | Enterprise | Organization | ✓ |
| User settings | `~/.claude/settings.json` | All projects | ✗ |
| User local | `~/.claude/settings.local.json` | All projects | ✗ |
| Project settings | `.claude/settings.json` | Project | ✓ Git |
| Project local | `.claude/settings.local.json` | Project | ✗ |

### Settings.json Structure

```json
{
  "model": "claude-sonnet-4-5-20250929",
  "maxTokens": 4096,
  "permissions": {
    "allowedTools": ["Read", "Write", "Bash(git *)"],
    "deny": ["Read(.env)", "Bash(rm *)"]
  },
  "hooks": { ... },
  "env": {
    "ANTHROPIC_MODEL": "opus"
  }
}
```

### Global Configuration File

| File | Purpose |
|------|---------|
| `~/.claude/claude.json` | Legacy config (being deprecated) |
| `~/.claude.json` | User preferences, onboarding state |

---

## 13. Environment Variables

### Authentication

| Variable | Description |
|----------|-------------|
| `ANTHROPIC_API_KEY` | API key for authentication |
| `ANTHROPIC_AUTH_TOKEN` | Alternative auth token |
| `ANTHROPIC_BASE_URL` | Custom API endpoint |

### Model Configuration

| Variable | Description |
|----------|-------------|
| `ANTHROPIC_MODEL` | Default model |
| `ANTHROPIC_DEFAULT_OPUS_MODEL` | Model for complex tasks |
| `ANTHROPIC_DEFAULT_SONNET_MODEL` | Model for daily tasks |
| `ANTHROPIC_DEFAULT_HAIKU_MODEL` | Model for simple tasks |

### Behavior Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `MAX_THINKING_TOKENS` | Thinking budget limit | Up to 31,999 |
| `MAX_MCP_OUTPUT_TOKENS` | MCP output limit | 25,000 |
| `BASH_DEFAULT_TIMEOUT_MS` | Bash command timeout | 30,000 |
| `MCP_TIMEOUT` | MCP server timeout | - |
| `CLAUDE_CODE_MAX_OUTPUT_TOKENS` | Max output tokens | 16,384 |
| `ENABLE_TOOL_SEARCH` | MCP tool search | `auto` |
| `SLASH_COMMAND_TOOL_CHAR_BUDGET` | Skill description budget | 15,000 |
| `CLAUDE_CODE_TASK_LIST_ID` | Named task list | - |

### Environment Setup

| Variable | Description |
|----------|-------------|
| `CLAUDE_ENV_FILE` | Source file before each Bash |
| `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR` | Reset cwd after each command |

### Debugging & Telemetry

| Variable | Description |
|----------|-------------|
| `ANTHROPIC_LOG` | Enable logging (`debug`) |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | Disable telemetry |
| `DISABLE_AUTOUPDATER` | Disable auto-updates |
| `DISABLE_TELEMETRY` | Disable telemetry |
| `DISABLE_ERROR_REPORTING` | Disable error reports |
| `DISABLE_BUG_COMMAND` | Disable /bug command |

### Third-Party Providers

| Variable | Description |
|----------|-------------|
| `CLAUDE_CODE_USE_BEDROCK` | Use AWS Bedrock |
| `AWS_REGION` | AWS region |
| `CLAUDE_CODE_USE_VERTEX` | Use Google Vertex AI |
| `HTTPS_PROXY` | Corporate proxy |

---

## 14. IDE Integrations

### VS Code Extension (Recommended)

| Feature | Description |
|---------|-------------|
| Native GUI | Graphical interface in sidebar |
| Inline diffs | See changes with accept/reject |
| Plan mode | Review plans before execution |
| Auto-accept | Enable edit auto-approval |
| Checkpoints | Rewind to previous states |
| @-mentions | Reference files with line ranges |
| Multiple tabs | Separate conversations |

### Extension Shortcuts

| Shortcut | Action |
|----------|--------|
| `Cmd/Ctrl+Esc` | Toggle Claude/editor focus |
| `Cmd/Ctrl+N` | New conversation |
| `Cmd/Ctrl+Option+K` | Insert file reference |

### Legacy CLI Integration

| Feature | Description |
|---------|-------------|
| Selection context | Share current selection |
| Diff viewing | VS Code diff viewer |
| Diagnostic sharing | Auto-share lint errors |
| `/ide` command | Connect external terminal |

### Supported IDEs

| IDE | Support Level |
|-----|---------------|
| VS Code | Full (extension + CLI) |
| Cursor | Full (extension + CLI) |
| Windsurf | Full (extension + CLI) |
| VSCodium | Full (extension + CLI) |
| JetBrains (IntelliJ, PyCharm, etc.) | Plugin available |

### Extension Settings

| Setting | Description |
|---------|-------  ------|
| `claudeCode.autoConnect` | Auto-connect to IDE |
| `claudeCode.disableLoginPrompt` | Skip login prompts |
| `claudeCode.contextWindow` | Context sharing mode |

---

## 15. CLAUDE.md Memory System

Persistent project/user instructions loaded into context.

### File Locations (All Loaded)

| Location | Scope | Use Case |
|----------|-------|----------|
| `~/.claude/CLAUDE.md` | Global | Personal preferences |
| `./CLAUDE.md` | Project root | Project-wide instructions |
| `./CLAUDE.local.md` | Project (gitignored) | Personal project prefs |
| `./subdir/CLAUDE.md` | Subdirectory | Module-specific rules |

### Best Practices

| Guideline | Description |
|-----------|-------------|
| Keep concise | Avoid excessive content |
| Use `#` prefix | Quick additions during session |
| Include in commits | Share with team |
| Document commands | List build/test commands |
| Style guidelines | Coding conventions |

### Content Suggestions

- Project structure overview
- Build and test commands
- Coding conventions
- File naming patterns
- Git workflow preferences
- Framework-specific patterns

---

## 16. Session Management

### Session Persistence

| Feature | Description |
|---------|-------------|
| Auto-save | All conversations saved |
| Per-directory | Sessions tied to project |
| History navigation | Up arrow for past commands |
| Resume by ID | `--resume <session-id>` |

### Session Commands

| Action | Command |
|--------|---------|
| Continue last | `claude -c` or `claude --continue` |
| Resume picker | `claude --resume` |
| Resume specific | `claude --resume <id>` |
| Clear session | `/clear` |
| Export session | `/export` |

### Checkpointing (VS Code)

| Feature | Description |
|---------|-------------|
| Fork conversation | Branch from any point |
| Rewind code | Revert files to checkpoint |
| Fork + rewind | Both actions combined |

---

## 17. Output Formats

### CLI Output Modes

| Format | Flag | Use Case |
|--------|------|----------|
| Interactive | (default) | Normal usage |
| Text | `--output-format text` | Human readable output |
| JSON | `--output-format json` | Scripting, parsing |
| Stream JSON | `--output-format stream-json` | Real-time processing |

### Piping & Scripting

| Pattern | Example |
|---------|---------|
| Pipe input | `cat file.log \| claude -p "analyze"` |
| Chain commands | `git diff \| claude -p "review"` |
| Batch processing | `claude -p --max-turns 1 "query"` |

---

## 18. Plugins & Marketplaces

### Plugin System (December 2025+)

| Feature | Description |
|---------|-------------|
| Marketplace | Official Anthropic plugins |
| Community marketplaces | Third-party collections |
| Auto-updates | Per-marketplace setting |

### Plugin Commands

| Command | Action |
|---------|--------|
| `/plugins` | Browse marketplace |
| `/plugins install <name>` | Install plugin |
| `/plugins list` | Show installed |
| `/plugins update` | Update all |

### Plugin Components

Plugins can bundle:
- Skills (SKILL.md files)
- Subagents
- Slash commands
- Hooks
- MCP server configurations

---

## Summary Statistics

| Category | Count |
|----------|-------|
| Built-in Tools | 15+ |
| Built-in Slash Commands | 25+ |
| CLI Flags | 30+ |
| Keyboard Shortcuts | 25+ |
| Hook Events | 11 |
| Environment Variables | 25+ |
| Configuration Files | 5 scopes |

---

*Document generated: January 2026*
*Based on Claude Code v2.x documentation and community resources*