import {
  Activity,
  Brain,
  Database,
  FileText,
  FolderTree,
  Gauge,
  GitBranch,
  Layers,
  ListTodo,
  MessagesSquare,
  NotebookPen,
  Radar,
  Search,
  SquareTerminal,
  Webhook,
  Wrench,
  type LucideIcon,
} from "lucide-react"
import type { PanelKind } from "../types"

export const panelIcon: Record<PanelKind, LucideIcon> = {
  tree: FolderTree,
  memory: Brain,
  threads: MessagesSquare,
  spine: Activity,
  stats: Gauge,
  entities: Database,
  search: Search,
  file: FileText,
  git: GitBranch,
  console: SquareTerminal,
  queue: Layers,
  todo: ListTodo,
  callback: Webhook,
  scratchpad: NotebookPen,
  tools: Wrench,
  radar: Radar,
}

export type Accent = "signal" | "interactive" | "ok" | "warn" | "danger" | "muted"

export const accentVar: Record<Accent, string> = {
  signal: "var(--signal)",
  interactive: "var(--interactive)",
  ok: "var(--ok)",
  warn: "var(--warn)",
  danger: "var(--danger)",
  muted: "var(--muted-foreground)",
}

/** Load → color: calm when light, warm caution mid, red when heavy. Theme-aware. */
export function loadColor(ratio: number): string {
  if (ratio >= 0.85) return "var(--danger)"
  if (ratio >= 0.6) return "var(--warn)"
  return "var(--ok)"
}

export function fmtTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`
  return `${n}`
}

export function fmtCost(n: number): string {
  return `$${n.toFixed(2)}`
}
