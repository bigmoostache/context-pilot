import type { AccentToken } from "../api/generated/types.gen"

export type Accent = AccentToken | "muted"

export const accentVar: Record<Accent, string> = {
  signal: "var(--signal)",
  interactive: "var(--interactive)",
  ok: "var(--ok)",
  warn: "var(--warn)",
  danger: "var(--danger)",
  muted: "var(--muted-foreground)",
}

export function fmtTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`
  return String(n)
}

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
