import type {
  ContextPanel,
  ChatMessage,
  Thread,
  SpineNotif,
  StatRow,
  StatusModel,
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
