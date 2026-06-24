// ── Context Pilot maquette — domain types (design-only, no backend) ──

export type PanelKind =
  | "tree"
  | "memory"
  | "threads"
  | "spine"
  | "stats"
  | "entities"
  | "search"
  | "file"
  | "git"
  | "console"
  | "queue"
  | "todo"
  | "callback"
  | "scratchpad"
  | "tools"
  | "radar"

export interface ContextPanel {
  id: string
  kind: PanelKind
  name: string
  tokens: number
  costUsd: number
  cached: boolean
  frozen: number | null
  misses: number
  fixed: boolean
}

export type StreamPhase = "ready" | "streaming" | "tooling" | "blocked"

export type MsgRole = "user" | "assistant" | "tool"

export interface ToolCall {
  name: string
  intent: string
  verb: string
  params: Record<string, string>
  result?: string
  isError?: boolean
}

export interface ChatMessage {
  id: string
  role: MsgRole
  /** rich text (markdown-ish) for user/assistant */
  text?: string
  /** present when role === "tool" */
  tool?: ToolCall
  ts: string
  streaming?: boolean
}

/**
 * Thread turn-status.
 * - `MY_TURN`    — the thread is waiting on the human (needs you).
 * - `ACTIVE`     — the agent is *currently streaming* this thread. Exactly ONE
 *                  thread per realm is ACTIVE at any time; shown in green.
 * - `THEIR_TURN` — the agent owns the thread but isn't actively streaming it
 *                  right now (queued / working in parallel).
 */
export type ThreadStatus = "MY_TURN" | "ACTIVE" | "THEIR_TURN"

export interface Thread {
  id: string
  name: string
  status: ThreadStatus
  messages: number
  unread: number
  last: string
}

export type NotifKind = "user" | "reload" | "custom"

export interface SpineNotif {
  id: string
  kind: NotifKind
  time: string
  text: string
  processed: boolean
}

export interface StatRow {
  label: string
  value: string
  accent?: "signal" | "interactive" | "ok" | "warn" | "danger"
}

export interface StatusModel {
  phase: StreamPhase
  agent: string
  skills: string[]
  branch: string
  queue: number
  think: number
  reverie: boolean
  autoContinue: boolean
  costUsd: number
}

// ── Account / current user (avatar menu + profile) ───────────────

/**
 * The signed-in human's account. Drives the TopBar avatar menu and the Profile
 * modal (T30). `managedByCompany` decides whether the profile is editable or
 * provisioned (SSO) by an organization — the modal renders a different account
 * section for each case.
 */
export interface User {
  name: string
  email: string
  /** 1–2 letter fallback shown in the avatar when there's no picture */
  initials: string
  /** accent token for the avatar fallback gradient */
  accent: "signal" | "interactive" | "ok" | "warn" | "danger"
  /** true → account is provisioned & managed by an organization (SSO/org) */
  managedByCompany: boolean
  /** the managing organization (present when managedByCompany) */
  company?: string
  /** the user's role label */
  role?: string
}

// ── Agents / workspaces (1 agent = 1 folder) ──────────────────────

export type AgentStatus = "working" | "needs-you" | "idle"

/** An agent IS a workspace folder. Switching agents = switching folders. */
export interface Agent {
  id: string
  /** display label */
  name: string
  /** absolute folder path the agent lives in */
  folder: string
  branch: string
  model: string
  /** wire serde provider id (e.g. "claudecodev2") — authoritative for the
   *  model picker, since several providers share the same model API names. */
  provider?: string
  status: AgentStatus
  /** live execution phase (idle/streaming/tooling) — folded from the
   *  PhaseTransition SSE delta so the HUD shows the DISTINCT phase, not just
   *  the working/idle binary `status` collapses it into (T297). Absent before
   *  the first phase transition. */
  phase?: "idle" | "streaming" | "tooling"
  costUsd: number
  /** cumulative-since-boot input tokens — folded from the CostAggregate delta
   *  (same figure the live `cost_aggregate` carries), so the HUD token counter
   *  rides the push plane instead of the 15s poll (T297). */
  inputTokens?: number
  /** cumulative-since-boot output tokens (T297). */
  outputTokens?: number
  /** live context-window occupancy (tokens currently in the window) — folded
   *  from the ContextUsage SSE delta, the agent's OWN authoritative figure, so
   *  the web meter is byte-identical to the ratatui sidebar (T297). */
  contextUsed?: number
  /** the cleaning threshold (reverie trigger point) for the context meter. */
  contextThreshold?: number
  /** the hard context budget (the meter's denominator). */
  contextBudget?: number
  /** cache-HIT half of contextUsed (always-cached prefix + cached panels) —
   *  folded from ContextUsage; the HUD shows `Used (hit)` from it, matching
   *  ratatui's green token-bar segment. hit + miss === used. */
  contextHit?: number
  /** cache-MISS half of contextUsed (uncached panels this turn) — the HUD's
   *  `Used (miss)`, matching ratatui's amber segment. */
  contextMiss?: number
  /** one-line summary of what the agent is currently working on */
  task: string
  /** number of open threads on this agent */
  threads: number
  lastActivity: string
  /** accent color token for the agent's dot/avatar */
  accent: "signal" | "interactive" | "ok" | "warn" | "danger"
  /** whether the agent has a profile picture stored at the orchestrator */
  hasAvatar?: boolean
}

/** A node in the (mock) filesystem browser. */
export interface FsNode {
  name: string
  path: string
  kind: "dir" | "file"
  /** when this dir hosts an agent, its id (lets us badge it) */
  agentId?: string
  children?: FsNode[]
}

// ── Finder (per-agent file manager) ───────────────────────────────

export type FinderKind =
  | "folder"
  | "code"
  | "doc"
  | "pdf"
  | "sheet"
  | "slides"
  | "image"
  | "markdown"
  | "json"
  | "archive"
  | "audio"
  | "video"
  | "binary"

/** macOS-style finder tags (colored dots). */
export type FinderTag = "red" | "orange" | "yellow" | "green" | "blue" | "purple" | "gray"

/** One spreadsheet preview payload. */
export interface SheetPreview {
  columns: string[]
  rows: string[][]
}

/** One slide in a deck preview. */
export interface SlidePreview {
  title: string
  bullets: string[]
}

/** A node in the Finder's (mock) realm filesystem — confined to one agent. */
export interface FinderNode {
  name: string
  path: string
  kind: FinderKind
  /** size in bytes (files only) */
  size?: number
  /** direct (non-hidden) child count — folders only, supplied by the backend */
  count?: number
  /** human relative modified time, e.g. "2d ago" */
  modified: string
  /** human relative created time */
  created?: string
  /** colored finder tags */
  tags?: FinderTag[]
  /** shown in Favorites sidebar */
  starred?: boolean
  children?: FinderNode[]
  // ── optional preview payloads, by kind ──
  code?: { lang: string; lines: string[] }
  sheet?: SheetPreview
  slides?: SlidePreview[]
  pdf?: { pages: number; title: string; excerpt: string[] }
  image?: { gradient: string; w: number; h: number }
  /** audio / video payload */
  media?: { kind: "audio" | "video"; duration: string; poster?: string; peaks?: number[] }
  /** markdown / json / plain-doc preview body */
  text?: string
}

export type FinderViewMode = "grid" | "list" | "columns" | "gallery"
export type FinderSortKey = "name" | "size" | "modified" | "kind"

// ── Thread-centered view ──────────────────────────────────────────

/**
 * Top-level surfaces. `fleet` = the mission-control dashboard (the ONLY place
 * agents are managed). The other three are the per-agent views.
 */
export type ViewMode = "fleet" | "cockpit" | "threads" | "finder"

/** A single embedded question form inside a thread message (CP signature). */
export interface ThreadQuestion {
  prompt: string
  options: string[]
  /** allow multiple selections */
  multi?: boolean
  /** offer a free-text "other" field */
  allowOther?: boolean
}

/** One message in a thread's conversation. */
export interface ThreadMsg {
  id: string
  author: "user" | "assistant"
  text?: string
  ts: string
  /** when present, render an embedded tool-call card */
  tool?: ToolCall
  /** when present, render an embedded question form (awaiting the user) */
  questions?: ThreadQuestion[]
  /** an attached file reference */
  fileRef?: string
  streaming?: boolean
  /** true → an auto tool-activity trace (collapsed into a run in the UI) */
  auto?: boolean
}

// ── Global prompt library (Prompts page) ─────────────────────────

/** Kind of library entry — mirrors the TUI's prompt library. */
export type LibraryKind = "agent" | "skill" | "command"

/** One entry in the global prompt library. */
export interface LibraryItem {
  id: string
  /** display name */
  name: string
  kind: LibraryKind
  description: string
  /** e.g. how many agents currently use this skill, or "/cmd" for commands */
  meta?: string
  /** for commands: the prompt body the `/command` expands to — seeded into the
   *  thread composer when a suggestion bubble is clicked (T350). Absent for
   *  agents/skills (their bodies are large and unused by the library list). */
  body?: string
  /** a built-in entry ships with the app and can't be deleted */
  builtin?: boolean
  /** currently active (agents) / loaded (skills) somewhere */
  active?: boolean
}

// ── Usage / cost analytics (Usage page) ──────────────────────────

/** Which lens the Usage page is viewed through. */
export type UsageUnit = "usd" | "tokens"

/**
 * One month of usage for one agent. Tokens are split into the three canonical
 * sections the Usage page ALWAYS surfaces:
 *  - **hit**    — cache-read tokens (cheap)
 *  - **miss**   — input / uncached tokens
 *  - **output** — generated tokens (expensive)
 * Dollar figures are derived from these via {@link UsageRates}, so the token
 * and dollar lenses stay perfectly consistent.
 */
export interface UsagePoint {
  agentId: string
  /** calendar month key, e.g. "2026-06" */
  month: string
  hitTokens: number
  missTokens: number
  outputTokens: number
  /** the in-progress current month (drives forecasting) */
  partial?: boolean
  /** fraction of the month elapsed (0–1), present when partial */
  elapsed?: number
}

/** Per-section price, in USD per token. */
export interface UsageRates {
  hit: number
  miss: number
  output: number
}

/** A full thread with its conversation log — drives the thread-centered view. */
export interface ThreadDetail {
  id: string
  name: string
  status: ThreadStatus
  /** the agent (folder/realm) this thread lives in — threads never cross agents */
  agentId: string
  /** which agent is assigned to / working this thread */
  agent: string
  createdAt: string
  lastActivity: string
  /** epoch-ms of the most recent message — used for sort-by-recency (live data) */
  lastActivityMs?: number
  unread: number
  /** archived threads are hidden from the main list, viewable on demand */
  archived?: boolean
  /** paused threads suppress MY_TURN notifications but remain visible (T371) */
  paused?: boolean
  /** true when this is the thread the AI is currently focused on */
  focused?: boolean
  log: ThreadMsg[]
}

// ── Cockpit panel maquettes (one designed view per sidebar panel) ─────────
// Lightweight, design-only shapes feeding the panel-centered "cockpit" view.
// Each mirrors what its real Context Pilot TUI panel surfaces.

export type Importance = "low" | "medium" | "high" | "critical"

/** One memory card in the Memories panel (id · tl;dr · importance · labels). */
export interface MemoryCard {
  id: string
  tldr: string
  importance: Importance
  labels: string[]
}

/** A recency-weighted anchor signal at the top of the Context Radar. */
export interface RadarAnchor {
  time: string
  signal: string
}

/** A scored recall result in the Context Radar body. */
export interface RadarResult {
  content: string
  datetime: string
  importance: Importance
  score: number
}

/** One (possibly nested) todo row. `depth` drives indentation. */
export interface TodoItem {
  id: string
  name: string
  status: "pending" | "in_progress" | "done"
  depth: number
}

/** A table in the Entities (SQLite) panel. */
export interface EntityTable {
  name: string
  rows: number
  /** compact column signature, e.g. "name TEXT PK, type TEXT, …" */
  columns: string
  /** a couple of sample row tuples, pre-stringified */
  samples: string[]
}

/** One tool row inside a Tools-panel category group. */
export interface ToolRow {
  name: string
  status: "on" | "off"
  desc: string
}

/** A category grouping of tools in the Tools panel. */
export interface ToolGroup {
  category: string
  tools: ToolRow[]
}

/** A callback definition row in the Callbacks panel. */
export interface CallbackRow {
  id: string
  name: string
  pattern: string
  blocking: boolean
  timeout: string
  scope: string
  cwd: string
}

/** A queued tool call awaiting flush in the Queue panel. */
export interface QueueAction {
  index: number
  tool: string
  intent: string
  preview: string
}

/** A scratchpad cell (title + body preview). */
export interface ScratchCell {
  id: string
  title: string
  preview: string
}

/** One row of the Directory Tree panel (flattened for rendering). */
export interface TreeRow {
  depth: number
  name: string
  kind: "dir" | "file"
  /** token-size chip (files/dirs), e.g. "19.5K" */
  size?: string
  /** cartographer description */
  desc?: string
  /** [!] — file changed since its description was written */
  changed?: boolean
  /** open folder (▼) vs closed (▶) */
  open?: boolean
}
