# Context Pilot

A terminal-based AI coding assistant built in Rust that provides an interactive interface for AI-assisted development with full project context awareness. A terminal-based AI coding assistant built in Rust that provides an interactive interface for AI-assisted development with full project context awareness.

![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)

## Features

### 🤖 AI-Powered Assistance
- **Claude Integration** - Powered by Anthropic's Claude API with streaming responses
- **Context-Aware** - Automatically includes relevant project files, directory structure, and more
- **Tool Execution** - AI can directly interact with your codebase through built-in tools

### 📁 Smart Context Management
- **File Context** - Open files and keep them in context for reference
- **Directory Tree** - Filtered view of your project structure (gitignore-style filtering)
- **Glob Search** - Create persistent file searches that update automatically
- **Tmux Integration** - Create terminal panes, run commands, capture output
- **Todo Lists** - Hierarchical task management with status tracking

### 💬 Conversation Features
- **Message Summarization** - Automatic TL;DR generation for long messages
- **Context Control** - Mark messages as full, summarized, or forgotten
- **Token Tracking** - Visual token usage with 100K limit indicator
- **Persistent History** - Conversations saved and restored across sessions

### 🎨 Terminal UI
- **Modern Design** - Clean interface with warm color theme
- **Syntax Highlighting** - Code files displayed with proper highlighting
- **Mouse Support** - Click to select, scroll to navigate
- **Copy Mode** - Toggle mouse capture for text selection
- **Responsive Layout** - Adapts to terminal size

## Installation

### Prerequisites
- Rust 1.75 or later
- An Anthropic API key

### Build from Source

```bash
git clone https://github.com/yourusername/context-pilot.git
cd context-pilot
cargo build --release
```

### Configuration

Create a `.env` file in the project root:

```env
ANTHROPIC_API_KEY=your_api_key_here
```

## Usage

```bash
cargo run --release
```

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Shift+Enter` or `Alt+Enter` | Send message |
| `Ctrl+L` | Clear conversation |
| `Ctrl+Y` | Toggle copy mode |
| `Ctrl+Q` | Quit |
| `Esc` | Stop streaming / Exit copy mode |
| `PageUp/PageDown` | Scroll content |
| `p1`, `p2`, etc. | Quick switch context panels |

### Context Panel Navigation

Type `p1`, `p2`, `p3`, etc. in the input and press Enter/Space to quickly switch between context panels.

## Available Tools

The AI assistant can use these tools to interact with your project:

| Tool | Description |
|------|-------------|
| `open_file` | Open a file and add it to context |
| `create_file` | Create a new file |
| `edit_file` | Edit an existing file |
| `close_contexts` | Remove context elements |
| `glob` | Search for files matching a pattern |
| `grep` | Search file contents with regex pattern |
| `edit_tree_filter` | Modify directory tree filtering |
| `set_message_status` | Manage message context (full/summarized/forgotten) |
| `create_tmux_pane` | Create a new terminal pane |
| `tmux_send_keys` | Send commands to a tmux pane |
| `edit_tmux_config` | Configure tmux pane settings |
| `create_todos` | Create todo items |
| `update_todos` | Update or delete todo items |

## API Request Structure

This section documents exactly how the prompt sent to Claude is constructed.

### Top-Level Request

```
ApiRequest {
    model: "claude-sonnet-4-20250514"
    max_tokens: 8192
    system: ""                          # Empty for normal mode, custom for cleaner mode
    messages: Vec<ApiMessage>           # See below
    tools: [...]                        # Tool definitions JSON
    stream: true
}
```

### Message Construction

Messages are built by `messages_to_api()` in `src/api.rs`.

#### Step 1: Build Context Parts

Context is collected into `context_parts: Vec<String>` in this order:

```
1. Directory Tree (if non-empty)
   === Directory Tree ===
   {tree content}
   === End of Directory Tree ===

2. Todo List (if non-empty)
   === Todo List ===
   {todos}
   === End of Todo List ===

3. Memories (if non-empty and not "No memories")
   === Memories ===
   {memories}
   === End of Memories ===

4. Context Overview (if non-empty)
   === Context Overview ===
   {overview with token counts}
   === End of Context Overview ===

5. Open Files (for each file)
   === File: {path} ===
   {file content}
   === End of {path} ===

6. Glob Results (for each glob)
   === {glob name} ===
   {matching files}
   === End of {glob name} ===

7. Grep Results (for each grep)
   === grep:{pattern} ===
   {file:line:content matches}
   === End of grep:{pattern} ===

8. Tmux Panes (for each pane)
   === {pane header} ===
   {terminal output}
   === End of {pane header} ===
```

#### Step 2: Convert Messages to API Format

For each message in `state.messages` (skipping `status == Deleted`):

**TextMessage (U/A prefixes):**
```
role: "user" or "assistant"
content: [
    Text { text: "[U1]: {content}" }      # First user msg gets context prepended
]
```

First user message format:
```
{context_parts joined by \n\n}

[U1]: {user message}
```

**ToolCall (T prefix):**
- Only included if a ToolResult exists after it
- Appended to previous assistant message's content blocks:
```
content: [
    Text { text: "[A1]: {assistant text}" },
    ToolUse { id: "toolu_xxx", name: "open_file", input: {...} },
    ToolUse { id: "toolu_yyy", name: "edit_file", input: {...} }
]
```

**ToolResult (R prefix):**
```
role: "user"
content: [
    ToolResult { tool_use_id: "toolu_xxx", content: "[R1]: {result}" },
    ToolResult { tool_use_id: "toolu_yyy", content: "[R2]: {result}" }
]
```

#### Step 3: Final Assembly

```
messages: [
    { role: "user",      content: [Text { "[context...]\n\n[U1]: hello" }] },
    { role: "assistant", content: [Text { "[A1]: I'll help" }, ToolUse {...}] },
    { role: "user",      content: [ToolResult { "[R1]: file opened" }] },
    { role: "assistant", content: [Text { "[A2]: Done!" }] },
    { role: "user",      content: [Text { "[U2]: thanks" }] },
    ...
]
```

### Message ID Visibility to LLM

| ID Type | Prefix | Visible to LLM | How |
|---------|--------|----------------|-----|
| User message | U | Yes | `[U1]: {content}` |
| Assistant message | A | Yes | `[A1]: {content}` |
| Tool call | T | **No** | Only `tool_use` block sent (API constraint) |
| Tool result | R | Yes | `[R1]: {content}` in tool_result |

**Note:** T-block IDs cannot be exposed due to Claude API constraints - `tool_use` blocks cannot have text mixed in; they must be immediately followed by `tool_result` blocks.

### Summarized Messages

When `status == Summarized`:
- Uses `tl_dr` field instead of `content`
- Same format: `[A5]: {tl_dr text}`

### Tool Results Flow

When assistant calls tools:
1. Assistant message with `tool_use` blocks is sent
2. Tools execute locally, results collected
3. New request sent with `tool_result` blocks as user message
4. Assistant continues with access to results

### Cleaner Mode

Special mode triggered by `/clean` command:
- Custom system prompt from `context_cleaner.rs`
- Adds user message: `"Please clean up the context to reduce token usage:\n\n{cleaner_context}"`
- Cleaner context includes all message IDs, types, statuses, and token counts

## Dependencies

- **ratatui** - Terminal UI framework
- **crossterm** - Cross-platform terminal manipulation
- **reqwest** - HTTP client for API calls
- **serde/serde_json/serde_yaml** - Serialization
- **syntect** - Syntax highlighting
- **ignore** - Gitignore-style file filtering
- **globset** - Glob pattern matching

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

---

Built with ❤️ and Rust
