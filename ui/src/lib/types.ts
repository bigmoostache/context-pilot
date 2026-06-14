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
