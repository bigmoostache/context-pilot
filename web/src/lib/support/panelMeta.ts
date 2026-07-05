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
import type { AccentToken } from "../api/generated/types.gen"

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

export type Accent = AccentToken | "muted"

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
  return String(n)
}

// Reference context-window budget used to render token-usage ratios/bars.
// Single source of truth shared by the LeftRail and the cockpit StatsPanel
// (was duplicated as a literal in each — L21).
export const REF_BUDGET = 200_000

export function fmtCost(n: number): string {
  return `$${n.toFixed(2)}`
}

/**
 * Shared content width for the whole fleet home (the Agents ⇄ Prompts shell and
 * both its sub-pages centre on this so their left/right edges line up as you
 * flip between tabs). Lives here — a leaf presentation-token module — rather
 * than in FleetShell so the two sub-pages (FleetDashboard, PromptsPage) can
 * import it without forming an import cycle with the shell that renders them.
 */
export const FLEET_MAX_W = "max-w-[960px]"
