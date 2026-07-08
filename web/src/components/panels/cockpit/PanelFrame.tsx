import type { ReactNode } from "react"
import type { LucideIcon } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { fmtTokens, fmtCost } from "@/lib/support/panelMeta"

/**
 * Shared shell for every cockpit panel maquette. Gives each panel a consistent
 * macOS-style chrome: an accent-tiled icon, a name + optional subtitle, and a
 * right-aligned token / cost chip echoing the panel's context economics. The
 * body scrolls inside a padded column so individual panels only worry about
 * their own content, never the frame.
 */
export function PanelFrame({
  icon: Icon,
  name,
  subtitle,
  tokens,
  cost,
  accent = "var(--signal)",
  children,
}: {
  icon: LucideIcon
  name: string
  subtitle?: string | undefined
  tokens?: number | undefined
  cost?: number | undefined
  accent?: string | undefined
  children: ReactNode
}) {
  return (
    <section className="rise flex min-w-0 flex-1 flex-col bg-background">
      <header className="flex h-11 shrink-0 items-center gap-3 border-b border-border px-5">
        <span
          className="flex size-7 shrink-0 items-center justify-center rounded-md"
          style={{ background: `color-mix(in oklab, ${accent} 16%, transparent)`, color: accent }}
        >
          <Icon className="size-4" />
        </span>
        <div className="flex min-w-0 flex-col leading-tight">
          <span className="truncate text-[13px] font-semibold text-foreground/90">{name}</span>
          {subtitle && (
            <span className="truncate text-[11px] text-muted-foreground">{subtitle}</span>
          )}
        </div>
        {(tokens !== undefined || cost !== undefined) && (
          <div className="ml-auto flex items-center gap-2 text-[11px] text-muted-foreground tabular-nums">
            {tokens !== undefined && (
              <span className="rounded-md bg-muted/70 px-1.5 py-0.5">{fmtTokens(tokens)} tok</span>
            )}
            {cost !== undefined && (
              <span className="rounded-md bg-muted/70 px-1.5 py-0.5">{fmtCost(cost)}</span>
            )}
          </div>
        )}
      </header>

      <ScrollArea className="min-h-0 flex-1">
        <div className="px-5 py-4">{children}</div>
      </ScrollArea>
    </section>
  )
}

/** A small uppercase section label reused across panel bodies. */
export function PanelSection({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="mb-5 last:mb-0">
      <div className="mb-2 text-[11px] font-medium tracking-wide text-muted-foreground/70 uppercase">
        {label}
      </div>
      {children}
    </div>
  )
}

const IMPORTANCE_COLOR: Record<string, string> = {
  critical: "var(--danger)",
  high: "var(--warn)",
  medium: "var(--interactive)",
  low: "var(--muted-foreground)",
}

/** Importance pill shared by Memories + Radar. */
export function ImportanceDot({ level }: { level: string }) {
  return (
    <span
      className="inline-block size-1.5 shrink-0 rounded-full"
      style={{ background: IMPORTANCE_COLOR[level] ?? "var(--muted-foreground)" }}
      title={level}
    />
  )
}

/** A subtle rounded chip for labels/tags. */
export function Chip({ children, accent }: { children: ReactNode; accent?: string | undefined }) {
  return (
    <span
      className="rounded-md px-1.5 py-0.5 text-[10px] font-medium"
      style={{
        background: accent ? `color-mix(in oklab, ${accent} 14%, transparent)` : "var(--muted)",
        color: accent ?? "var(--muted-foreground)",
      }}
    >
      {children}
    </span>
  )
}

/**
 * Honest empty-state for a cockpit panel whose data the read-only web
 * inspection plane cannot serve.
 *
 * A few panels (Tools, Context Radar, Entities) read state that lives only in
 * the running agent — the tool catalog is compiled into the agent binary, the
 * radar is a live half-life ranking, and the entity DB is an open SQLite
 * connection. The backend reads agents' on-disk tier-② files; none of those
 * three are reconstructable from disk, so their endpoints return an empty shape
 * by design. Rather than render a blank list (which reads as "nothing exists"),
 * the panel shows this explicit notice so the boundary is legible, not a bug.
 */
export function InspectionUnavailable({ reason }: { reason: string }) {
  return (
    <div
      role="note"
      className="flex flex-col items-center gap-1.5 rounded-lg border border-dashed border-border px-6 py-10 text-center"
    >
      <span className="text-[12.5px] font-medium text-foreground/80">
        Unavailable over the web inspection plane
      </span>
      <span className="max-w-sm text-[11.5px] leading-relaxed text-muted-foreground">{reason}</span>
    </div>
  )
}
