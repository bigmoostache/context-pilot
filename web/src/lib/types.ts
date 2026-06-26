// ── Context Pilot — domain types ─────────────────────────────────────
//
// Types that have a backend equivalent are RE-EXPORTED from the generated
// OpenAPI client (web/src/lib/api/generated/types.gen.ts). UI-only types
// that exist only on the frontend are defined here. This keeps the backend
// contract as the single source of truth while letting the UI carry its
// own display-layer extensions.

// ── Re-exports from generated OpenAPI client ─────────────────────────
//
// These types mirror Rust backend structs. The generated file is the
// authoritative source — edits go through the Rust types → openapi.json →
// codegen pipeline, never by hand.

export type {
  AccentToken,
  CallbackRow,
  EntityTable,
  FinderKind,
  LibraryItem,
  MemoryCard,
  PanelKind,
  QueueAction,
  RadarAnchor,
  RadarResult,
  ScratchCell,
  SpineNotif,
  ThreadQuestion,
  TodoItem,
  ToolCall,
  ToolGroup,
  ToolRow,
  TreeRow,
} from "./api/generated/types.gen"

// ── Extended re-exports (generated base + UI-only fields) ────────────

import type {
  AccentToken,
  Agent as GenAgent,
  ContextPanel as GenContextPanel,
  FinderNode as GenFinderNode,
  ThreadDetail as GenThreadDetail,
  ThreadMsg as GenThreadMsg,
} from "./api/generated/types.gen"

/** Agent with UI-only `accent` field (computed client-side in reducers). */
export type Agent = GenAgent & {
  accent: AccentToken
}

/** ContextPanel re-exported as-is (kind is already typed as PanelKind in generated). */
export type ContextPanel = GenContextPanel

/** ThreadMsg with UI-only `streaming` flag (set during active LLM output). */
export type ThreadMsg = GenThreadMsg & {
  streaming?: boolean
}

/** ThreadDetail re-exported as-is (field optionality matches backend). */
export type ThreadDetail = Omit<GenThreadDetail, "log"> & {
  log: ThreadMsg[]
}

// ── Derived type aliases (extracted from generated union literals) ────

export type AgentStatus = GenAgent["status"]
export type ThreadStatus = GenThreadDetail["status"]
export type Importance = import("./api/generated/types.gen").MemoryCard["importance"]
export type NotifKind = import("./api/generated/types.gen").SpineNotif["kind"]
export type LibraryKind = import("./api/generated/types.gen").LibraryItem["kind"]

// ── UI-only types (no backend equivalent) ────────────────────────────

/** Backend execution phase — linked to generated Agent.phase. */
type Phase = NonNullable<GenAgent["phase"]>

/** UI phase extends backend Phase: renames idle→ready, adds blocked. */
export type StreamPhase = Exclude<Phase, "idle"> | "ready" | "blocked"

export type MsgRole = "user" | "assistant" | "tool"

export interface ChatMessage {
  id: string
  role: MsgRole
  /** rich text (markdown-ish) for user/assistant */
  text?: string
  /** present when role === "tool" */
  tool?: import("./api/generated/types.gen").ToolCall
  ts: string
  streaming?: boolean
}

export interface Thread {
  id: string
  name: string
  status: ThreadStatus
  messages: number
  unread: number
  last: string
}

export interface StatRow {
  label: string
  value: string
  accent?: AccentToken
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
 * modal. `managedByCompany` decides whether the profile is editable or
 * provisioned (SSO) by an organization.
 */
export interface User {
  name: string
  email: string
  /** 1–2 letter fallback shown in the avatar when there's no picture */
  initials: string
  /** accent token for the avatar fallback gradient */
  accent: AccentToken
  /** true → account is provisioned & managed by an organization (SSO/org) */
  managedByCompany: boolean
  /** the managing organization (present when managedByCompany) */
  company?: string
  /** the user's role label */
  role?: string
}

// ── Filesystem browser ───────────────────────────────────────────

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

/** A node in the Finder's realm filesystem — base fields from the generated
 *  OpenAPI type, extended with UI-enriched fields (kind refinement, preview
 *  payloads, tags) that exist only on the client side. */
export type FinderNode = Omit<GenFinderNode, "modified"> & {
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

// ── View modes ────────────────────────────────────────────────────

/**
 * Top-level surfaces. `fleet` = the mission-control dashboard (the ONLY place
 * agents are managed). The other three are the per-agent views.
 */
export type ViewMode = "fleet" | "cockpit" | "threads" | "finder"

// ── Usage / cost analytics (Usage page) ──────────────────────────

/** Which lens the Usage page is viewed through. */
export type UsageUnit = "usd" | "tokens"

/**
 * One month of usage for one agent. Tokens are split into the three canonical
 * sections the Usage page ALWAYS surfaces:
 *  - **hit**    — cache-read tokens (cheap)
 *  - **miss**   — input / uncached tokens
 *  - **output** — generated tokens (expensive)
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
