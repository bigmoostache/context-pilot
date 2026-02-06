# Contributing to Context Pilot

Thank you for your interest in contributing to Context Pilot! We welcome contributions from the community and are grateful for any help you can provide.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [How to Contribute](#how-to-contribute)
- [Pull Request Process](#pull-request-process)
- [Coding Guidelines](#coding-guidelines)
- [Architecture Overview](#architecture-overview)
- [Testing](#testing)
- [Documentation](#documentation)
- [Community](#community)

## Code of Conduct

By participating in this project, you agree to abide by our Code of Conduct. Please be respectful, inclusive, and constructive in all interactions.

## Getting Started

1. **Fork the repository** on GitHub
2. **Clone your fork** locally:
   ```bash
   git clone https://github.com/YOUR_USERNAME/tui.git
   cd tui
   ```
3. **Add the upstream remote**:
   ```bash
   git remote add upstream https://github.com/contextpilot/tui.git
   ```

## Development Setup

### Prerequisites

- **Rust 1.75+** - Install via [rustup](https://rustup.rs/)
- **tmux** - For terminal pane features
- **An LLM API key** - Anthropic, Grok, or Groq

### Building

```bash
# Debug build (faster compilation)
cargo build

# Release build (optimized)
cargo build --release

# Run
cargo run --release
```

### Configuration

Create a `.env` file in the project root:

```env
ANTHROPIC_API_KEY=your_key_here
# Optional:
XAI_API_KEY=your_grok_key
GROQ_API_KEY=your_groq_key
```

## How to Contribute

### Reporting Bugs

- Check existing issues to avoid duplicates
- Use the [Bug Report template](.github/ISSUE_TEMPLATE/bug_report.md)
- Include environment details, steps to reproduce, and logs

### Suggesting Features

- Use the [Feature Request template](.github/ISSUE_TEMPLATE/feature_request.md)
- Explain the problem you're solving
- Describe your proposed solution

### Submitting Code

1. **Create a branch** for your feature/fix:
   ```bash
   git checkout -b feature/your-feature-name
   # or
   git checkout -b fix/issue-description
   ```

2. **Make your changes** following our coding guidelines

3. **Test your changes**:
   ```bash
   cargo build --release
   cargo test
   # Manual testing with the TUI
   ```

4. **Commit with clear messages**:
   ```bash
   git commit -m "feat: add cool new feature"
   # or
   git commit -m "fix: resolve issue with file caching"
   ```

5. **Push and create a PR**:
   ```bash
   git push origin feature/your-feature-name
   ```

## Pull Request Process

1. **Fill out the PR template** completely
2. **Link related issues** using `Fixes #123` or `Closes #123`
3. **Ensure CI passes** - builds must succeed
4. **Request review** from maintainers
5. **Address feedback** promptly
6. **Squash commits** if requested before merge

### Commit Message Format

We follow conventional commits:

```
type(scope): description

[optional body]

[optional footer]
```

Types:
- `feat` - New feature
- `fix` - Bug fix
- `docs` - Documentation only
- `style` - Code style (formatting, no logic change)
- `refactor` - Code refactoring
- `perf` - Performance improvement
- `test` - Adding/updating tests
- `chore` - Maintenance tasks

## Coding Guidelines

### Rust Style

- Follow standard Rust conventions (`cargo fmt`)
- Use `cargo clippy` and address all warnings
- Prefer explicit types over complex inference
- Document public APIs with doc comments

### Architecture Patterns

- **State**: All mutable state lives in `State` struct
- **Actions**: State changes go through `Action` enum and `apply_action()`
- **Panels**: UI panels implement the `Panel` trait
- **Tools**: LLM tools go in `src/tools/` with matching entry in `tool_defs.rs`
- **Caching**: Heavy I/O goes through background cache system

### File Organization

```
src/
├── core/          # App loop, initialization
├── tools/         # Tool implementations
├── panels/        # UI panel renderers
├── persistence/   # State persistence
├── ui/            # UI components
├── llms/          # LLM provider integrations
└── help/          # Command palette
```

### Adding a New Tool

1. Create `src/tools/your_tool.rs`
2. Add tool definition in `src/tool_defs.rs`
3. Add dispatch case in `src/tools/mod.rs`
4. Update documentation

### Adding a New Panel

1. Create `src/panels/your_panel.rs`
2. Implement the `Panel` trait
3. Add to `ContextType` enum in `src/state.rs`
4. Add dispatch in `src/panels/mod.rs`

## Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture
```

### Manual Testing Checklist

- [ ] Basic chat functionality works
- [ ] File operations (open, edit, create) work
- [ ] Panel switching works
- [ ] Streaming responses display correctly
- [ ] Reload preserves state
- [ ] No UI glitches or rendering issues

## Documentation

- Update README.md for user-facing changes
- Add doc comments (`///`) for public APIs
- Update tool descriptions in `tool_defs.rs`
- Add tree descriptions for new files

## Community

- **Issues**: For bugs and feature requests
- **Discussions**: For questions and ideas
- **Pull Requests**: For code contributions

### Getting Help

If you're stuck or have questions:

1. Check existing issues and discussions
2. Read the codebase - it's well-documented
3. Open a discussion for general questions
4. Reach out to maintainers

---

Thank you for contributing to Context Pilot! 🚀
