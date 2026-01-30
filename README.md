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
| `edit_tree_filter` | Modify directory tree filtering |
| `set_message_status` | Manage message context (full/summarized/forgotten) |
| `create_tmux_pane` | Create a new terminal pane |
| `tmux_send_keys` | Send commands to a tmux pane |
| `edit_tmux_config` | Configure tmux pane settings |
| `create_todos` | Create todo items |
| `update_todos` | Update or delete todo items |

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
