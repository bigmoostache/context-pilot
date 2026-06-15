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

export type ThreadStatus = "MY_TURN" | "THEIR_TURN"

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
  status: AgentStatus
  costUsd: number
  /** number of open threads on this agent */
  threads: number
  lastActivity: string
  /** accent color token for the agent's dot/avatar */
  accent: "signal" | "interactive" | "ok" | "warn" | "danger"
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

// ── Thread-centered view ──────────────────────────────────────────

export type ViewMode = "agents" | "cockpit" | "threads"

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
  unread: number
  log: ThreadMsg[]
}
