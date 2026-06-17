# Context Pilot — Live Web Client

React + TypeScript frontend for the Context Pilot orchestration backend.
Renders real-time agent state: fleet overview, threads with live messaging,
cockpit panels (memory, todos, spine, tree, etc.), and a confined file finder.

## Architecture

```
Browser (web/)  ──REST──▶  Orchestrator (cp-orchestrator, :7878)  ──reads──▶  Agent state files
                ◀──SSE───                                          ──UDS───▶  Agent bridge (TUI)
```

- **REST** — read agent state (threads, panels, finder, meta) + submit commands
- **SSE** — real-time oplog deltas + stream hints (token-by-token)
- **Commands** — `SendMessage`, `CreateThread`, `ArchiveThread`, `Stop`, etc.

The frozen design maquette lives in `../ui/` for visual comparison.

## Quick start

```bash
# Full stack (backend + web + TUI with bridge)
./web/run-stack.sh

# Backend + web only (run TUI separately with CP_BRIDGE=1)
./web/run-stack.sh --no-tui
```

### Manual start

```bash
# 1. Backend
cargo build --release -p cp-orchestrator
./target/release/cp-orchestrator          # serves :7878

# 2. Web dev server
cd web && pnpm install && pnpm dev        # serves :5174

# 3. TUI with bridge activation
CP_BRIDGE=1 cargo run --release           # self-registers with backend
```

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `CP_ORCH_PORT` | `7878` | Orchestrator HTTP port |
| `VITE_API_URL` | `http://localhost:7878` | Backend URL (web client) |
| `CP_BRIDGE` | unset | Set to `1` to activate agent bridge in TUI |

## Data flow

The web client uses **two data channels**:

1. **Inspection plane** (read-only) — backend reads the agent's `.context-pilot/`
   tier-② state files and reshapes them to JSON. Zero agent writes, zero oplog bloat.
2. **Orchestration plane** (durable) — oplog → SSE deltas for liveness, phase,
   cost, stream hints, and command effects.

## Key directories

```
web/src/lib/
  api.ts      — Typed REST client (22 endpoints)
  sse.ts      — Reconnecting SSE with Last-Event-ID replay
  live.ts     — React hooks: useFleet, useThreads, usePanels, etc.
                (fetch + SSE-invalidate + 5s poll backstop)

web/src/components/
  agents/     — Fleet dashboard, prompts library
  threads/    — Thread list, conversation, composer (real commands)
  panels/     — 13 cockpit panel components
  finder/     — Confined file browser
  shell/      — TopBar, StatusBar, LeftRail, Config, etc.
```

## Tech stack

- React 19 + TypeScript + Vite 8
- Tailwind CSS v4 + shadcn/ui (Base UI)
- No async runtime — all data via fetch + EventSource
