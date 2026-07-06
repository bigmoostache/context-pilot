import type {
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
  UsagePoint,
  UsageRates,
  User,
} from "../types"

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
  {
    depth: 2,
    name: "cp-base",
    kind: "dir",
    size: "—",
    desc: "Foundation crate: State, Module trait, config, tools, panels",
  },
  {
    depth: 2,
    name: "cp-mod-threads",
    kind: "dir",
    size: "—",
    desc: "Threads module: Send/Read, MY_TURN/THEIR_TURN, focus enforcement",
  },
  {
    depth: 2,
    name: "cp-render",
    kind: "dir",
    size: "—",
    desc: "IR rendering crate — semantic styling, Block enum, serializable",
  },
  {
    depth: 1,
    name: "src",
    kind: "dir",
    open: true,
    desc: "Main binary — app loop, llms, modules, state, ui",
  },
  {
    depth: 2,
    name: "app",
    kind: "dir",
    desc: "Event loop, actions, context, reverie, run pipeline",
  },
  {
    depth: 2,
    name: "llms",
    kind: "dir",
    desc: "7 LLM providers + cache breakpoint engine",
    changed: true,
  },
  {
    depth: 2,
    name: "main.rs",
    kind: "file",
    size: "1.2K",
    desc: "Entry point — phased boot, FD limit, panic hook",
  },
  { depth: 1, name: "ui", kind: "dir", open: true, desc: "Frontend design maquette (this app)" },
  { depth: 2, name: "src", kind: "dir", size: "—" },
  { depth: 2, name: "package.json", kind: "file", size: "0.4K" },
  {
    depth: 1,
    name: "Cargo.toml",
    kind: "file",
    size: "2.1K",
    desc: "Workspace root — 21 members, 980 forbid-level lints",
  },
  {
    depth: 1,
    name: "README.md",
    kind: "file",
    size: "5.3K",
    desc: "Manifesto + flame graph telemetry docs",
  },
]

/** Memories panel — long-term recall cards. */
export const memoryCards: MemoryCard[] = [
  {
    id: "M8",
    tldr: "Context Pilot is a SIDE PROJECT. In <3 months: #1 tool for all of work + personal life, 2-4 simultaneous projects.",
    importance: "critical",
    labels: ["talk", "wow", "side-project"],
  },
  {
    id: "M45",
    tldr: "FEATURE COMPLETION SEQUENCE: build → reload → commit+push (with memories/tree-descs) → clean context. Always.",
    importance: "critical",
    labels: ["workflow", "sequence"],
  },
  {
    id: "M27",
    tldr: "FORBIDDEN: #[allow(...)] is BANNED. Always use #[expect(...)] instead — exceptional cases only.",
    importance: "critical",
    labels: ["code-style", "lints", "forbidden"],
  },
  {
    id: "M59",
    tldr: "NEVER activate autocontinuation — it bugs badly. Always sail manually, batch by batch.",
    importance: "critical",
    labels: ["user-preference", "spine"],
  },
  {
    id: "M60",
    tldr: "CP frontend maquette in ui/ (branch 'maquette'). Views: fleet | threads | cockpit | finder. Dark = TUI D&D palette.",
    importance: "medium",
    labels: ["maquette", "frontend", "ui"],
  },
  {
    id: "M2",
    tldr: "Meilisearch design complete. Embedded global server, tree-sitter chunking, unified search tool, log redesign.",
    importance: "critical",
    labels: ["architecture", "meilisearch"],
  },
  {
    id: "M21",
    tldr: "Context Pilot is a Rust TUI AI coding assistant (~15K LOC) built with Ratatui + Crossterm. 18 module crates + 1 binary.",
    importance: "high",
    labels: ["architecture"],
  },
  {
    id: "M29",
    tldr: "Documentation is ABSOLUTELY CRITICAL. Don't half-ass it. Document to HELP future readers — take the extra step.",
    importance: "critical",
    labels: ["documentation", "quality"],
  },
]

/** Context Radar panel — recency-weighted recall. */
export const radarAnchors: RadarAnchor[] = [
  {
    time: "17:21",
    signal:
      "Building 12 cockpit panel maquettes for T16 — shared PanelFrame, router, mock data, cockpit layout rewire.",
  },
  { time: "17:16", signal: "Creating NewThreadDialog using the Base UI dialog primitive." },
  {
    time: "17:11",
    signal:
      "Reworking the thread-centered view: collapse parity, archive, ACTIVE status, working search.",
  },
  {
    time: "14:37",
    signal:
      "T13: rework the fleet Usage page — agent filter, date-range, token vs dollar lenses, forecasts.",
  },
]

export const radarResults: RadarResult[] = [
  {
    content:
      "T9 fleet sidebar collapse via clickable border RAIL (shadcn pattern, no buttons). FleetShell renders thin hit-area, brightens on hover.",
    datetime: "14:09",
    importance: "medium",
    score: 0.894,
  },
  {
    content:
      "T9 4-page fleet dashboard + collapsible sidebars all done. FleetShell owns collapsed state, FleetSidebar no identity header.",
    datetime: "14:02",
    importance: "medium",
    score: 0.843,
  },
  {
    content:
      "T5 finder fixes committed + replied. ThreadList rewritten (no agent header, collapsible). Unified sidebar widths via --sidebar-w.",
    datetime: "13:58",
    importance: "medium",
    score: 0.843,
  },
  {
    content:
      "T5 finder file-tabs done: double-click file opens dedicated tab; navigating on a file tab converts it back to folder tab.",
    datetime: "13:21",
    importance: "medium",
    score: 0.79,
  },
  {
    content:
      "FIX: created ui/components/ui/dialog.tsx — Base UI Dialog primitive (portals to document.body, focus-trap, Esc, backdrop).",
    datetime: "11:57",
    importance: "medium",
    score: 0.751,
  },
]

/** Todo List panel — the threads-feature build plan (slice). */
export const todoItems: TodoItem[] = [
  { id: "X631", name: "Phase 9: TUI — ViewMode + Threads view", status: "in_progress", depth: 0 },
  {
    id: "X742",
    name: "render() dispatch: ViewMode::Threads → different layout (no panels)",
    status: "done",
    depth: 1,
  },
  {
    id: "X738",
    name: "Thread creation UI: 'n' keybinding with inline name prompt",
    status: "done",
    depth: 1,
  },
  {
    id: "X740",
    name: "Inline question form rendering in Threads view message area",
    status: "done",
    depth: 1,
  },
  {
    id: "X744",
    name: "Threads view UX redesign: virtual New Thread tab + sidebar nav",
    status: "done",
    depth: 0,
  },
  {
    id: "X759",
    name: "Split 5 files over 500 lines — get all callbacks green",
    status: "done",
    depth: 0,
  },
  { id: "X760", name: "Split threads_view.rs (680 → two files)", status: "done", depth: 1 },
  {
    id: "X765",
    name: "Finder dramatic enhancement (Apple-grade) — 20+ details",
    status: "done",
    depth: 0,
  },
  { id: "X766", name: "T15: thread-centered view rework", status: "done", depth: 0 },
  {
    id: "X767",
    name: "T16: cockpit panel maquettes (one per sidebar panel)",
    status: "in_progress",
    depth: 0,
  },
]

/** Entities panel — SQLite domain database. */
export const entityTables: EntityTable[] = [
  {
    name: "lints",
    rows: 1046,
    columns:
      "name TEXT PK, type TEXT, description TEXT, default_level, target_level, current_level, justification",
    samples: [
      "(absolute-paths-not-starting-with-crate, rustc, allow, forbid, forbid)",
      "(ambiguous-associated-items, rustc, deny, forbid, forbid)",
    ],
  },
  {
    name: "runs",
    rows: 8,
    columns:
      "id INTEGER PK, task_name TEXT, binary TEXT, model TEXT, behaviour TEXT, n_messages INTEGER, duration_secs REAL, score REAL",
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
  {
    id: "CB1",
    name: "folder-sizes",
    pattern: "*.rs",
    blocking: true,
    timeout: "10s",
    scope: "global",
    cwd: "project root",
  },
  {
    id: "CB2",
    name: "lint-integrity",
    pattern: "*.rs",
    blocking: true,
    timeout: "10s",
    scope: "global",
    cwd: "project root",
  },
  {
    id: "CB3",
    name: "rust-check",
    pattern: "*.rs",
    blocking: true,
    timeout: "60s",
    scope: "global",
    cwd: "project root",
  },
]

/** Scratchpad panel — ephemeral working cells. */
export const scratchCells: ScratchCell[] = [
  {
    id: "C1",
    title: "Cockpit maquette — panel checklist",
    preview:
      "tree · memory · radar · todo · threads · stats · tools · entities · spine · callbacks · queue · scratchpad — 12 total, each wraps PanelFrame.",
  },
  {
    id: "C2",
    title: "Layout decision",
    preview:
      "Cockpit = LeftRail (240) | PanelPane (flex, the star) | Conversation (420, secondary). Panel-centered: selection in rail drives the center.",
  },
]

/** Queue panel — pending tool calls awaiting an atomic flush. */
export const queueActions: QueueAction[] = [
  {
    index: 1,
    tool: "Write",
    intent: "Create PanelFrame shell",
    preview: "ui/src/components/panels/cockpit/PanelFrame.tsx",
  },
  {
    index: 2,
    tool: "Write",
    intent: "Build Memory panel maquette",
    preview: "ui/src/components/panels/cockpit/MemoryPanel.tsx",
  },
  {
    index: 3,
    tool: "Edit",
    intent: "Rewire cockpit layout",
    preview: "ui/src/App.tsx — CockpitView 3-column",
  },
]

// ── Account / current user (avatar menu + Profile modal) ──────────
// The signed-in human. `managedByCompany: false` is the default for self-hosted
// instances with no org provisioning. Flip to `true` in the Profile modal's
// design-only preview toggle to see the enterprise/SSO layout.
export const currentUser: User = {
  name: "Guillaume Draznieks",
  email: "g.draznieks@gmail.com",
  initials: "GD",
  accent: "interactive",
  managedByCompany: false,
  company: undefined,
  role: "Admin",
}
