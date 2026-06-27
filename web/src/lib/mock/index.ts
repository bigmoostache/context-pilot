import type {
  ContextPanel,
  ChatMessage,
  Thread,
  SpineNotif,
  StatRow,
  StatusModel,
  Agent,
  FsNode,
  LibraryItem,
} from "../types"

export * from "./extra"

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
