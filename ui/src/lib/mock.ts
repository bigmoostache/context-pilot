import type {
  ContextPanel,
  ChatMessage,
  Thread,
  ThreadDetail,
  SpineNotif,
  StatRow,
  StatusModel,
  Agent,
  FsNode,
  LibraryItem,
  UsagePoint,
  UsageRates,
  MemoryCard,
  RadarAnchor,
  RadarResult,
  TodoItem,
  EntityTable,
  ToolGroup,
  CallbackRow,
  QueueAction,
  ScratchCell,
  TreeRow,
} from "./types"

// ── Mock data approximating a live Context Pilot session ──

export const PROJECT = {
  name: "context-pilot",
  path: "~/context-pilot",
  branch: "maquette",
}

export const tokenBudget = {
  used: 74156,
  threshold: 170000,
  budget: 200000,
}

export const panels: ContextPanel[] = [
  { id: "P5", kind: "tree", name: "Directory Tree", tokens: 19517, costUsd: 1.75, cached: false, frozen: 5, misses: 4, fixed: true },
  { id: "P6", kind: "memory", name: "Memories", tokens: 12416, costUsd: 0.76, cached: false, frozen: null, misses: 1, fixed: true },
  { id: "P10", kind: "radar", name: "Context Radar", tokens: 5974, costUsd: 0.37, cached: false, frozen: 19, misses: 6, fixed: false },
  { id: "P1", kind: "todo", name: "Todo List", tokens: 4541, costUsd: 0.28, cached: false, frozen: null, misses: 1, fixed: true },
  { id: "P7", kind: "threads", name: "Threads", tokens: 2061, costUsd: 0.13, cached: false, frozen: null, misses: 1, fixed: true },
  { id: "P3", kind: "stats", name: "Statistics", tokens: 1228, costUsd: 0.11, cached: false, frozen: 22, misses: 20, fixed: true },
  { id: "P4", kind: "tools", name: "Tools", tokens: 1031, costUsd: 0.06, cached: false, frozen: null, misses: 1, fixed: true },
  { id: "P9", kind: "entities", name: "Entities", tokens: 430, costUsd: 0.02, cached: true, frozen: null, misses: 1, fixed: true },
  { id: "P8", kind: "spine", name: "Spine", tokens: 366, costUsd: 0.05, cached: false, frozen: 13, misses: 8, fixed: true },
  { id: "P11", kind: "callback", name: "Callbacks", tokens: 202, costUsd: 0.01, cached: false, frozen: null, misses: 1, fixed: true },
  { id: "P13", kind: "queue", name: "Queue", tokens: 52, costUsd: 0.0, cached: false, frozen: 5, misses: 2, fixed: true },
  { id: "P12", kind: "scratchpad", name: "Scratchpad", tokens: 6, costUsd: 0.0, cached: false, frozen: null, misses: 1, fixed: true },
]

export const cacheStats = {
  hit: 41822,
  miss: 12416,
  out: 4063,
  costUsd: 5.41,
  uncached: 7,
}

export const messages: ChatMessage[] = [
  {
    id: "m1",
    role: "user",
    ts: "17:14",
    text: "create a 'maquette branch' and build a frontend design of the TUI — js/ts/vite/shadcn, design only, no backend.",
  },
  {
    id: "m2",
    role: "assistant",
    ts: "17:14",
    text: "On it. I'll scaffold a Vite + React + TypeScript app under `ui/`, wire Tailwind v4 and shadcn, then build a styled maquette with mock data. Committing to a **phosphor cockpit** aesthetic — flight-instrument-at-night.",
  },
  {
    id: "m3",
    role: "tool",
    ts: "17:15",
    tool: {
      name: "console_create",
      intent: "Scaffold Vite React app",
      verb: "Scaffolding",
      params: { command: "pnpm create vite ui --template react-ts" },
      result: "Scaffolding project in ./ui …\n✔ Done. Now run: pnpm install",
    },
  },
  {
    id: "m4",
    role: "tool",
    ts: "17:20",
    tool: {
      name: "Write",
      intent: "Apply phosphor cockpit theme",
      verb: "Theming",
      params: { file_path: "ui/src/index.css" },
      result: "Wrote 'ui/src/index.css' (214 lines, 1942 tokens)",
    },
  },
  {
    id: "m5",
    role: "assistant",
    ts: "17:24",
    streaming: true,
    text: "Theme is in. Now assembling the shell: a left **context navigator** with live token bars, the conversation surface in the center, and a switchable inspector on the right. Mirroring how the real CP exposes its panels and cache economics…",
  },
]

export const threads: Thread[] = [
  { id: "T7", name: "Test Thread", status: "MY_TURN", messages: 8, unread: 1, last: "show me the meter, hide the carburetor." },
  { id: "T4", name: "hello 4", status: "THEIR_TURN", messages: 25, unread: 0, last: "Acknowledged — standing by." },
  { id: "T2", name: "release v0.2.10", status: "THEIR_TURN", messages: 12, unread: 0, last: "Deadman fix tagged and shipped." },
]

export const spine: SpineNotif[] = [
  { id: "N3408", kind: "custom", time: "17:24", text: "Please think more. Thinking is cheap and sharpens you.", processed: true },
  { id: "N3404", kind: "user", time: "17:18", text: "which is the best long-run option?", processed: true },
  { id: "N3402", kind: "user", time: "17:16", text: "Ok. the purpose of this branch is a frontend maquette…", processed: true },
  { id: "N3400", kind: "user", time: "16:47", text: "clean your context entirely", processed: true },
]

export const stats: StatRow[] = [
  { label: "Context", value: "74.2K / 200K", accent: "signal" },
  { label: "Messages", value: "62 (19u · 43a)" },
  { label: "Indexed", value: "5853 chunks · 1211 files", accent: "interactive" },
  { label: "Entities", value: "3 tables · 1143 rows" },
  { label: "Memories", value: "50" },
  { label: "Todos", value: "140 / 142", accent: "ok" },
  { label: "Session cost", value: "$5.41", accent: "warn" },
]

export const status: StatusModel = {
  phase: "streaming",
  agent: "threaded-consciousness",
  skills: ["frontend-design"],
  branch: "maquette",
  queue: 0,
  think: -2,
  reverie: false,
  autoContinue: false,
  costUsd: 5.41,
}

export const threadDetails: ThreadDetail[] = [
  {
    id: "T8",
    name: "Frontend Maquette",
    status: "MY_TURN",
    agentId: "a-cp",
    agent: "threaded-consciousness",
    createdAt: "26m ago",
    lastActivity: "just now",
    unread: 1,
    log: [
      {
        id: "t8-1",
        author: "user",
        ts: "17:17",
        text: "create a 'maquette branch' and build a frontend design of the TUI — js/ts/vite/shadcn, design only, no backend.",
      },
      {
        id: "t8-2",
        author: "assistant",
        ts: "17:17",
        text: "On it. Scaffolding **Vite + React + TS** under `ui/`, wiring Tailwind v4 + shadcn, then a styled maquette with mock data. Committing to a **phosphor cockpit** aesthetic.",
      },
      {
        id: "t8-3",
        author: "assistant",
        ts: "17:32",
        tool: {
          name: "console_create",
          intent: "Build the maquette",
          verb: "Building",
          params: { command: "pnpm run build" },
          result: "✓ built in 159ms\ndist/assets/index-BLczxz8v.js  297.01 kB │ gzip: 94.97 kB",
        },
      },
      {
        id: "t8-4",
        author: "assistant",
        ts: "17:37",
        text: "🛩️ It's live at **http://localhost:5173/**. Top bar, left context-rail, center conversation, right inspector, status bar. Tell me what catches your eye.",
      },
      {
        id: "t8-5",
        author: "user",
        ts: "17:40",
        text: "this is really good. you coded the panel-centered view of one agent. now, code the thread-centered view, much more classic.",
      },
    ],
  },
  {
    id: "T2",
    name: "Lint hash-chain audit",
    status: "MY_TURN",
    agentId: "a-cp",
    agent: "context-builder",
    createdAt: "2h ago",
    lastActivity: "8m ago",
    unread: 1,
    log: [
      {
        id: "t2-1",
        author: "user",
        ts: "15:10",
        text: "audit the protected lint files for drift since the last chain entry.",
      },
      {
        id: "t2-2",
        author: "assistant",
        ts: "15:24",
        text: "Walked the chain — 12 protected files, all hashes verify. One file changed content but the chain wasn't updated. I need a decision before I touch a protected file:",
      },
      {
        id: "t2-3",
        author: "assistant",
        ts: "15:25",
        questions: [
          {
            prompt: "The CI Rust pin (1.93) is stale — matrix-sdk is gone. How should I resolve the toolchain skew?",
            options: [
              "Bump CI toolchain to 1.95 (re-sign the chain)",
              "Prune the ~18 too-new lints from Cargo.toml",
              "Leave it — defer to a human reviewer",
            ],
            allowOther: true,
          },
        ],
      },
    ],
  },
  {
    id: "T7",
    name: "Release v0.2.10",
    status: "ACTIVE",
    agentId: "a-cp",
    agent: "worker",
    createdAt: "5h ago",
    lastActivity: "41m ago",
    unread: 0,
    log: [
      {
        id: "t7-1",
        author: "user",
        ts: "12:02",
        text: "ship the deadman-aware-of-retry fix. tag v0.2.10.",
      },
      {
        id: "t7-2",
        author: "assistant",
        ts: "12:40",
        text: "Done. `LAST_PROGRESS_MS` now bumps on stream-retry activity, so the deadman stops fighting the in-process retry budget. Built green, reloaded.",
      },
      {
        id: "t7-3",
        author: "assistant",
        ts: "12:41",
        tool: {
          name: "git_execute",
          intent: "Tag the release",
          verb: "Tagging",
          params: { command: "git tag v0.2.10 && git push --tags" },
          result: "To github.com:bigmoostache/context-pilot\n * [new tag]  v0.2.10 -> v0.2.10",
        },
      },
      {
        id: "t7-4",
        author: "assistant",
        ts: "12:42",
        text: "Tagged and pushed — release CI is building the linux bundle now. I'll report when `releases/latest` returns HTTP 200.",
      },
    ],
  },
  {
    id: "T4",
    name: "Terminal-Bench sweep",
    status: "THEIR_TURN",
    agentId: "a-opio",
    agent: "worker",
    createdAt: "1d ago",
    lastActivity: "3h ago",
    unread: 0,
    log: [
      {
        id: "t4-1",
        author: "user",
        ts: "Mon",
        text: "run the scientific-computing category, one task at a time.",
      },
      {
        id: "t4-2",
        author: "assistant",
        ts: "Mon",
        tool: {
          name: "console_create",
          intent: "Launch harbor run",
          verb: "Running",
          params: { command: "harbor run -d terminal-bench@2.0 -i adaptive-rejection-sampler" },
          result: "container adaptive-rejection-sampler__utdgsz9 … reward.txt = 1.0",
        },
      },
      {
        id: "t4-3",
        author: "assistant",
        ts: "Mon",
        text: "First task passed (**reward 1.0**). Standing by — launching `bn-fit-modify` next once OAuth utilisation drops below 40%.",
      },
    ],
  },
  {
    id: "T1",
    name: "Docs: threads.md",
    status: "THEIR_TURN",
    agentId: "a-lean",
    agent: "planner",
    createdAt: "1d ago",
    lastActivity: "Mon",
    unread: 0,
    log: [
      {
        id: "t1-1",
        author: "user",
        ts: "Mon",
        text: "write the design doc for the threads feature.",
      },
      {
        id: "t1-2",
        author: "assistant",
        ts: "Mon",
        text: "Wrote `docs/threads.md` — core model, Send/Read tools, coucou integration, the fixed panel, and the Ctrl+V threads view. Committed on the `threads` branch.",
        fileRef: "docs/threads.md",
      },
    ],
  },
  {
    id: "T6",
    name: "Spike: WASM preview",
    status: "THEIR_TURN",
    agentId: "a-cp",
    agent: "worker",
    createdAt: "3d ago",
    lastActivity: "2d ago",
    unread: 0,
    archived: true,
    log: [
      {
        id: "t6-1",
        author: "user",
        ts: "Sat",
        text: "explore compiling the highlighter to WASM for the finder previews.",
      },
      {
        id: "t6-2",
        author: "assistant",
        ts: "Sat",
        text: "Prototyped it — the WASM bundle added 300KB for a 4ms parse win. Not worth it at current file sizes. Parked the spike; archiving this thread.",
      },
    ],
  },
]

// ── Agents / workspaces — one agent per folder ────────────────────

export const agents: Agent[] = [
  {
    id: "a-cp",
    name: "context-pilot",
    folder: "~/code/context-pilot",
    branch: "maquette",
    model: "claude-opus-4-8",
    status: "needs-you",
    costUsd: 5.41,
    task: "Building the frontend maquette — Apple-grade Finder polish, awaiting design direction.",
    threads: 5,
    lastActivity: "just now",
    accent: "signal",
  },
  {
    id: "a-opio",
    name: "opio-rag",
    folder: "~/code/opio-rag",
    branch: "main",
    model: "claude-sonnet-4-6",
    status: "working",
    costUsd: 1.92,
    task: "Running the scientific-computing Terminal-Bench sweep, one task at a time.",
    threads: 2,
    lastActivity: "3m ago",
    accent: "interactive",
  },
  {
    id: "a-lean",
    name: "lean-proofs",
    folder: "~/code/maths/lean-proofs",
    branch: "q6a-wip",
    model: "claude-opus-4-8",
    status: "idle",
    costUsd: 0.34,
    task: "Proving Q6a in Lean — paused on the linear_combination step over ℂ.",
    threads: 1,
    lastActivity: "2h ago",
    accent: "ok",
  },
]

/** id of the agent the workspace is currently focused on. */
export const activeAgentId = "a-cp"

/**
 * Mock filesystem for the workspace browser. Folders carrying an `agentId`
 * already host an agent; plain folders can have one initialized in them.
 */
export const fileTree: FsNode = {
  name: "code",
  path: "~/code",
  kind: "dir",
  children: [
    {
      name: "context-pilot",
      path: "~/code/context-pilot",
      kind: "dir",
      agentId: "a-cp",
      children: [
        { name: "crates", path: "~/code/context-pilot/crates", kind: "dir", children: [
          { name: "cp-base", path: "~/code/context-pilot/crates/cp-base", kind: "dir", children: [] },
          { name: "cp-mod-threads", path: "~/code/context-pilot/crates/cp-mod-threads", kind: "dir", children: [] },
        ] },
        { name: "ui", path: "~/code/context-pilot/ui", kind: "dir", children: [
          { name: "src", path: "~/code/context-pilot/ui/src", kind: "dir", children: [] },
          { name: "package.json", path: "~/code/context-pilot/ui/package.json", kind: "file" },
        ] },
        { name: "Cargo.toml", path: "~/code/context-pilot/Cargo.toml", kind: "file" },
        { name: "README.md", path: "~/code/context-pilot/README.md", kind: "file" },
      ],
    },
    {
      name: "opio-rag",
      path: "~/code/opio-rag",
      kind: "dir",
      agentId: "a-opio",
      children: [
        { name: "src", path: "~/code/opio-rag/src", kind: "dir", children: [] },
        { name: "pyproject.toml", path: "~/code/opio-rag/pyproject.toml", kind: "file" },
      ],
    },
    {
      name: "maths",
      path: "~/code/maths",
      kind: "dir",
      children: [
        {
          name: "lean-proofs",
          path: "~/code/maths/lean-proofs",
          kind: "dir",
          agentId: "a-lean",
          children: [
            { name: "Q6a.lean", path: "~/code/maths/lean-proofs/Q6a.lean", kind: "file" },
          ],
        },
        {
          name: "scratch-notes",
          path: "~/code/maths/scratch-notes",
          kind: "dir",
          children: [
            { name: "ideas.md", path: "~/code/maths/scratch-notes/ideas.md", kind: "file" },
          ],
        },
      ],
    },
    {
      name: "website",
      path: "~/code/website",
      kind: "dir",
      children: [
        { name: "index.html", path: "~/code/website/index.html", kind: "file" },
        { name: "styles.css", path: "~/code/website/styles.css", kind: "file" },
      ],
    },
    {
      name: "experiments",
      path: "~/code/experiments",
      kind: "dir",
      children: [
        { name: "wasm-spike", path: "~/code/experiments/wasm-spike", kind: "dir", children: [] },
        { name: "notes.txt", path: "~/code/experiments/notes.txt", kind: "file" },
      ],
    },
  ],
}

// ── Global prompt library (Prompts page) ──────────────────────────
// Drawn from the TUI's prompt library; presented as if it were already global
// (the captain's intent), shared across every agent.

export const library: LibraryItem[] = [
  // agents
  { id: "threaded-consciousness", name: "Threaded Consciousness", kind: "agent", description: "Two-surface model — private reasoning loop, polished thread replies.", meta: "active on 1 agent", active: true },
  { id: "default", name: "Default", kind: "agent", description: "General-purpose coding assistant.", meta: "built-in", builtin: true },
  { id: "worker", name: "Worker", kind: "agent", description: "Focused implementation & testing — heads-down execution.", meta: "built-in", builtin: true },
  { id: "planner", name: "Planner", kind: "agent", description: "Task planning and breakdown before any code is touched.", meta: "built-in", builtin: true },
  { id: "context-builder", name: "Context Builder", kind: "agent", description: "Explores an unfamiliar codebase and maps its structure.", meta: "built-in", builtin: true },
  { id: "context-cleaner", name: "Context Cleaner", kind: "agent", description: "Trims and reshapes context for hygiene.", meta: "built-in", builtin: true },
  { id: "cartographer", name: "Cartographer", kind: "agent", description: "Background agent that describes files & folders in the tree.", meta: "reverie", builtin: true },
  { id: "pirate-coder", name: "Pirate Coder", kind: "agent", description: "A salty buccaneer who loves the sea and clean diffs.", meta: "custom" },

  // skills
  { id: "frontend", name: "frontend-design", kind: "skill", description: "Distinctive, production-grade frontend interfaces — avoids generic AI slop.", meta: "loaded", active: true },
  { id: "egui", name: "egui", kind: "skill", description: "egui immediate-mode GUI framework knowledge & patterns.", meta: "—" },
  { id: "brave-goggles", name: "Brave Goggles", kind: "skill", description: "Curated Brave Search goggles for domain re-ranking.", meta: "—" },
  { id: "setup-guides", name: "Setup Guides", kind: "skill", description: "How to wire Telegram, Discord, Slack, Brave, Firecrawl, GitHub.", meta: "—" },

  // commands
  { id: "boss-hunt", name: "/boss-hunt", kind: "command", description: "Slow, methodical lint & quality sweep.", meta: "/boss-hunt" },
  { id: "clean", name: "/clean", kind: "command", description: "Launch a reverie cleaner, then resume work in progress.", meta: "/clean" },
  { id: "hello", name: "/hello", kind: "command", description: "A simple greeting — handy for smoke-testing.", meta: "/hello", builtin: true },
]

// ── Usage / cost analytics (Usage page) ───────────────────────────
// Per-section pricing (USD per token). Cache reads are an order of magnitude
// cheaper than input, output an order of magnitude dearer — so the token and
// dollar lenses tell genuinely different stories (tokens dominated by cache
// hits, dollars dominated by output).
export const USAGE_RATES: UsageRates = {
  hit: 0.3 / 1_000_000,
  miss: 3 / 1_000_000,
  output: 15 / 1_000_000,
}

/**
 * Deterministic 12-month usage history (Jul 2025 → Jun 2026), 3 agents.
 * A gentle upward trend + per-agent seasonal wiggle; the final month (Jun 2026)
 * is partial (half elapsed) so the Usage page can forecast it. No randomness at
 * module-eval time — values are stable across renders.
 */
function genUsage(): UsagePoint[] {
  // 12 calendar months ending at the current (partial) month, Jun 2026.
  const months: { key: string; idx: number }[] = []
  let y = 2025
  let m = 7 // July 2025
  for (let i = 0; i < 12; i++) {
    months.push({ key: `${y}-${String(m).padStart(2, "0")}`, idx: i })
    m += 1
    if (m > 12) {
      m = 1
      y += 1
    }
  }

  // miss-token base per agent (input/uncached), with a distinct phase.
  const seeds = [
    { agentId: "a-cp", base: 1_650_000, phase: 0.0 },
    { agentId: "a-opio", base: 690_000, phase: 1.7 },
    { agentId: "a-lean", base: 168_000, phase: 3.1 },
  ]

  const out: UsagePoint[] = []
  for (const s of seeds) {
    for (const mo of months) {
      const i = mo.idx
      const trend = 1 + i * 0.058 // ~1.64× growth over the year
      const wiggle = 1 + 0.14 * Math.sin(i * 1.25 + s.phase)
      const partial = i === months.length - 1
      const mtd = partial ? 0.5 : 1 // current month is half-elapsed
      const missTokens = Math.round(s.base * trend * wiggle * mtd)
      const hitTokens = Math.round(missTokens * (8.5 + 1.5 * Math.sin(i + s.phase)))
      const outputTokens = Math.round(missTokens * (0.092 + 0.012 * Math.cos(i + s.phase)))
      out.push({
        agentId: s.agentId,
        month: mo.key,
        hitTokens,
        missTokens,
        outputTokens,
        ...(partial ? { partial: true, elapsed: 0.5 } : {}),
      })
    }
  }
  return out
}

export const usagePoints: UsagePoint[] = genUsage()

// ── Cockpit panel maquette data ───────────────────────────────────
// Realistic, design-only payloads for the panel-centered cockpit view.
// Mirror what the real Context Pilot TUI panels surface.

/** Directory Tree panel — a flattened slice of the project tree. */
export const treeRows: TreeRow[] = [
  { depth: 0, name: "context-pilot", kind: "dir", open: true },
  { depth: 1, name: "crates", kind: "dir", open: true, desc: "21 workspace module crates" },
  { depth: 2, name: "cp-base", kind: "dir", size: "—", desc: "Foundation crate: State, Module trait, config, tools, panels" },
  { depth: 2, name: "cp-mod-threads", kind: "dir", size: "—", desc: "Threads module: Send/Read, MY_TURN/THEIR_TURN, focus enforcement" },
  { depth: 2, name: "cp-render", kind: "dir", size: "—", desc: "IR rendering crate — semantic styling, Block enum, serializable" },
  { depth: 1, name: "src", kind: "dir", open: true, desc: "Main binary — app loop, llms, modules, state, ui" },
  { depth: 2, name: "app", kind: "dir", desc: "Event loop, actions, context, reverie, run pipeline" },
  { depth: 2, name: "llms", kind: "dir", desc: "7 LLM providers + cache breakpoint engine", changed: true },
  { depth: 2, name: "main.rs", kind: "file", size: "1.2K", desc: "Entry point — phased boot, FD limit, panic hook" },
  { depth: 1, name: "ui", kind: "dir", open: true, desc: "Frontend design maquette (this app)" },
  { depth: 2, name: "src", kind: "dir", size: "—" },
  { depth: 2, name: "package.json", kind: "file", size: "0.4K" },
  { depth: 1, name: "Cargo.toml", kind: "file", size: "2.1K", desc: "Workspace root — 21 members, 980 forbid-level lints" },
  { depth: 1, name: "README.md", kind: "file", size: "5.3K", desc: "Manifesto + flame graph telemetry docs" },
]

/** Memories panel — long-term recall cards. */
export const memoryCards: MemoryCard[] = [
  { id: "M8", tldr: "Context Pilot is a SIDE PROJECT. In <3 months: #1 tool for all of work + personal life, 2-4 simultaneous projects.", importance: "critical", labels: ["talk", "wow", "side-project"] },
  { id: "M45", tldr: "FEATURE COMPLETION SEQUENCE: build → reload → commit+push (with memories/tree-descs) → clean context. Always.", importance: "critical", labels: ["workflow", "sequence"] },
  { id: "M27", tldr: "FORBIDDEN: #[allow(...)] is BANNED. Always use #[expect(...)] instead — exceptional cases only.", importance: "critical", labels: ["code-style", "lints", "forbidden"] },
  { id: "M59", tldr: "NEVER activate autocontinuation — it bugs badly. Always sail manually, batch by batch.", importance: "critical", labels: ["user-preference", "spine"] },
  { id: "M60", tldr: "CP frontend maquette in ui/ (branch 'maquette'). Views: fleet | threads | cockpit | finder. Dark = TUI D&D palette.", importance: "medium", labels: ["maquette", "frontend", "ui"] },
  { id: "M2", tldr: "Meilisearch design complete. Embedded global server, tree-sitter chunking, unified search tool, log redesign.", importance: "critical", labels: ["architecture", "meilisearch"] },
  { id: "M21", tldr: "Context Pilot is a Rust TUI AI coding assistant (~15K LOC) built with Ratatui + Crossterm. 18 module crates + 1 binary.", importance: "high", labels: ["architecture"] },
  { id: "M29", tldr: "Documentation is ABSOLUTELY CRITICAL. Don't half-ass it. Document to HELP future readers — take the extra step.", importance: "critical", labels: ["documentation", "quality"] },
]

/** Context Radar panel — recency-weighted recall. */
export const radarAnchors: RadarAnchor[] = [
  { time: "17:21", signal: "Building 12 cockpit panel maquettes for T16 — shared PanelFrame, router, mock data, cockpit layout rewire." },
  { time: "17:16", signal: "Creating NewThreadDialog using the Base UI dialog primitive." },
  { time: "17:11", signal: "Reworking the thread-centered view: collapse parity, archive, ACTIVE status, working search." },
  { time: "14:37", signal: "T13: rework the fleet Usage page — agent filter, date-range, token vs dollar lenses, forecasts." },
]

export const radarResults: RadarResult[] = [
  { content: "T9 fleet sidebar collapse via clickable border RAIL (shadcn pattern, no buttons). FleetShell renders thin hit-area, brightens on hover.", datetime: "14:09", importance: "medium", score: 0.894 },
  { content: "T9 4-page fleet dashboard + collapsible sidebars all done. FleetShell owns collapsed state, FleetSidebar no identity header.", datetime: "14:02", importance: "medium", score: 0.843 },
  { content: "T5 finder fixes committed + replied. ThreadList rewritten (no agent header, collapsible). Unified sidebar widths via --sidebar-w.", datetime: "13:58", importance: "medium", score: 0.843 },
  { content: "T5 finder file-tabs done: double-click file opens dedicated tab; navigating on a file tab converts it back to folder tab.", datetime: "13:21", importance: "medium", score: 0.790 },
  { content: "FIX: created ui/components/ui/dialog.tsx — Base UI Dialog primitive (portals to document.body, focus-trap, Esc, backdrop).", datetime: "11:57", importance: "medium", score: 0.751 },
]

/** Todo List panel — the threads-feature build plan (slice). */
export const todoItems: TodoItem[] = [
  { id: "X631", name: "Phase 9: TUI — ViewMode + Threads view", status: "in_progress", depth: 0 },
  { id: "X742", name: "render() dispatch: ViewMode::Threads → different layout (no panels)", status: "done", depth: 1 },
  { id: "X738", name: "Thread creation UI: 'n' keybinding with inline name prompt", status: "done", depth: 1 },
  { id: "X740", name: "Inline question form rendering in Threads view message area", status: "done", depth: 1 },
  { id: "X744", name: "Threads view UX redesign: virtual New Thread tab + sidebar nav", status: "done", depth: 0 },
  { id: "X759", name: "Split 5 files over 500 lines — get all callbacks green", status: "done", depth: 0 },
  { id: "X760", name: "Split threads_view.rs (680 → two files)", status: "done", depth: 1 },
  { id: "X765", name: "Finder dramatic enhancement (Apple-grade) — 20+ details", status: "done", depth: 0 },
  { id: "X766", name: "T15: thread-centered view rework", status: "done", depth: 0 },
  { id: "X767", name: "T16: cockpit panel maquettes (one per sidebar panel)", status: "in_progress", depth: 0 },
]

/** Entities panel — SQLite domain database. */
export const entityTables: EntityTable[] = [
  {
    name: "lints",
    rows: 1046,
    columns: "name TEXT PK, type TEXT, description TEXT, default_level, target_level, current_level, justification",
    samples: [
      "(absolute-paths-not-starting-with-crate, rustc, allow, forbid, forbid)",
      "(ambiguous-associated-items, rustc, deny, forbid, forbid)",
    ],
  },
  {
    name: "runs",
    rows: 8,
    columns: "id INTEGER PK, task_name TEXT, binary TEXT, model TEXT, behaviour TEXT, n_messages INTEGER, duration_secs REAL, score REAL",
    samples: [
      "(1, sqlite-db-truncate, v0.2.10, claude-opus-4-8, …, 41, 210, 1.0)",
      "(2, tune-mjcf, v0.2.10, claude-opus-4-8, …, 77, 658, 1.0)",
    ],
  },
  {
    name: "terminalbenchjobs",
    rows: 89,
    columns: "id INTEGER PK, name TEXT, category TEXT, difficulty TEXT, todo_id TEXT",
    samples: [
      "(1, adaptive-rejection-sampler, scientific-computing, medium, X592)",
      "(3, break-filter-js-from-html, security, medium, X574)",
    ],
  },
]

/** Tools panel — grouped tool registry (curated slice). */
export const toolGroups: ToolGroup[] = [
  {
    category: "File",
    tools: [
      { name: "Open", status: "on", desc: "Read file into context" },
      { name: "Edit", status: "on", desc: "Modify file content" },
      { name: "Write", status: "on", desc: "Create or overwrite file" },
    ],
  },
  {
    category: "Context",
    tools: [
      { name: "Think", status: "on", desc: "Record a structured reasoning step" },
      { name: "Close_panel", status: "on", desc: "Remove items from context" },
      { name: "panel_goto_page", status: "off", desc: "Navigate paginated panel" },
    ],
  },
  {
    category: "Threads",
    tools: [
      { name: "Send", status: "on", desc: "Post message to thread" },
      { name: "Read", status: "on", desc: "Read thread messages" },
    ],
  },
  {
    category: "Git",
    tools: [
      { name: "git_execute", status: "on", desc: "Run git commands" },
      { name: "gh_execute", status: "on", desc: "Run gh commands" },
    ],
  },
  {
    category: "Web Search",
    tools: [
      { name: "brave_search", status: "on", desc: "Search the web via Brave" },
      { name: "brave_llm_context", status: "on", desc: "LLM-optimized web content" },
    ],
  },
]

/** Callbacks panel — auto-firing edit hooks. */
export const callbackRows: CallbackRow[] = [
  { id: "CB1", name: "folder-sizes", pattern: "*.rs", blocking: true, timeout: "10s", scope: "global", cwd: "project root" },
  { id: "CB2", name: "lint-integrity", pattern: "*.rs", blocking: true, timeout: "10s", scope: "global", cwd: "project root" },
  { id: "CB3", name: "rust-check", pattern: "*.rs", blocking: true, timeout: "60s", scope: "global", cwd: "project root" },
]

/** Queue panel — pending tool calls awaiting an atomic flush. */
export const queueActions: QueueAction[] = [
  { index: 1, tool: "Write", intent: "Create PanelFrame shell", preview: "ui/src/components/panels/cockpit/PanelFrame.tsx" },
  { index: 2, tool: "Write", intent: "Build Memory panel maquette", preview: "ui/src/components/panels/cockpit/MemoryPanel.tsx" },
  { index: 3, tool: "Edit", intent: "Rewire cockpit layout", preview: "ui/src/App.tsx — CockpitView 3-column" },
]

/** Scratchpad panel — ephemeral working cells. */
export const scratchCells: ScratchCell[] = [
  {
    id: "C1",
    title: "Cockpit maquette — panel checklist",
    preview: "tree · memory · radar · todo · threads · stats · tools · entities · spine · callbacks · queue · scratchpad — 12 total, each wraps PanelFrame.",
  },
  {
    id: "C2",
    title: "Layout decision",
    preview: "Cockpit = LeftRail (240) | PanelPane (flex, the star) | Conversation (420, secondary). Panel-centered: selection in rail drives the center.",
  },
]

