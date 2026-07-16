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
  FinderKind,
  LibraryItem,
  ToolCall,
} from "./api/generated/types.gen"

// ── Extended re-exports (generated base + UI-only fields) ────────────

import type {
  AccentToken,
  Agent as GenAgent,
  FinderNode as GenFinderNode,
  ThreadDetail as GenThreadDetail,
  ThreadMsg as GenThreadMsg,
} from "./api/generated/types.gen"

/**
 * Agent with UI-only `accent` field (computed client-side in reducers) and a
 * `status` widened to include the client-only `"waiting"` restart status (set
 * by {@link useRestartFlow}; never emitted by the backend). The base
 * `GenAgent["status"]` is the narrow backend union, so we override it here.
 */
export type Agent = Omit<GenAgent, "status"> & {
  accent: AccentToken
  status: AgentStatus
}

/**
 * ThreadMsg with UI-only `streaming` flag (set during active LLM output).
 *
 * `ts` is widened from the generated `number` (epoch ms from REST) to also
 * accept an ISO string: SSE-appended messages carry their timestamp as an ISO
 * string (see reducers `message_created`), and the mock fixtures use display
 * strings. Consumers normalise both (`typeof ts === "number" ? … : new Date(ts)`).
 */
export type ThreadMsg = Omit<GenThreadMsg, "ts"> & {
  ts?: number | string | undefined
  streaming?: boolean | undefined
}

/** ThreadDetail re-exported as-is (field optionality matches backend). */
export type ThreadDetail = Omit<GenThreadDetail, "log"> & {
  log: ThreadMsg[]
}

// ── Derived type aliases (extracted from generated union literals) ────

/**
 * Agent status — backend statuses plus the client-only `"waiting"` status set
 * by {@link useRestartFlow} during a controlled restart. The new agent's
 * `Lifecycle::Running` delta naturally clears it back to `"idle"`.
 */
export type AgentStatus = GenAgent["status"] | "waiting"
export type ThreadStatus = GenThreadDetail["status"]

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
  text?: string | undefined
  /** present when role === "tool" */
  tool?: import("./api/generated/types.gen").ToolCall | undefined
  ts: string
  streaming?: boolean | undefined
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
  company?: string | undefined
  /** the user's role label */
  role?: string | undefined
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
export type ViewMode = "fleet" | "threads" | "finder" | "costs"

// ── Usage / cost analytics (Usage page) ──────────────────────────

/** Which lens the Usage page is viewed through. */
export type UsageUnit = "usd" | "tokens"
