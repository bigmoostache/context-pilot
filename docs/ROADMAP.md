# Roadmap

Context Pilot is under active development. Here's where we're heading.

## 🎯 Near Term (v0.2)

### New LLM Providers
- [ ] OpenAI GPT-4o / o1 / o3-mini
- [ ] Google Gemini 2.0
- [ ] Local models via Ollama / llama.cpp
- [ ] AWS Bedrock

### Context Intelligence
- [ ] Auto-summarization — automatically compress old messages when approaching threshold
- [ ] Smart panel prioritization — rank panels by relevance to current task
- [ ] Context budgeting per task — allocate token budgets to sub-tasks

### Developer Experience
- [ ] `cargo install context-pilot` — publish to crates.io
- [ ] Homebrew formula
- [ ] Nix flake
- [ ] Docker image for quick try-out
- [ ] Demo GIF / asciinema recording in README

## 🔭 Medium Term (v0.3)

### Multi-Agent
- [ ] Multiple workers running simultaneously on different tasks
- [ ] Worker-to-worker communication
- [ ] Supervisor agent that delegates and coordinates
- [ ] Shared memory between workers

### IDE Integration
- [ ] VS Code extension (connect to running Context Pilot instance)
- [ ] Neovim plugin
- [ ] LSP-like protocol for editor communication

### Knowledge Base
- [ ] Persistent codebase index (embeddings + search)
- [ ] Cross-session memory — remember project context between restarts
- [ ] Automatic documentation generation from tree descriptions

## 🌟 Long Term

### Plugin System
- [ ] User-defined modules in Lua or WASM
- [ ] Plugin marketplace / registry
- [ ] Custom panel types

### Collaboration
- [ ] Multi-user sessions
- [ ] Shared workspaces
- [ ] Audit trail / conversation replay

### Advanced Capabilities
- [ ] Image understanding (screenshots, diagrams)
- [ ] Voice input
- [ ] Web browsing tool
- [ ] Database query tool
- [ ] Cloud deployment monitoring

---

**Want to work on something?** Check [issues labeled `good first issue`](https://github.com/bigmoostache/context-pilot/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22) or [open a discussion](https://github.com/bigmoostache/context-pilot/discussions).
