import { MessageSquare } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { TokenBar } from "@/components/panels/TokenBar"
import { Tip } from "@/components/ui/tip"
import { usePanels } from "@/lib/live"
import { panelIcon, fmtTokens, loadColor } from "@/lib/panelMeta"
import { cn } from "@/lib/utils"

/**
 * Context navigator — a clean, calm list of the agent's panels with a single
 * context-budget meter. The dense per-panel telemetry (token bars, freeze /
 * miss counters) is intentionally omitted for an uncluttered, enterprise feel.
 *
 * Selection is **controlled** by the parent (the cockpit lifts it so the chosen
 * panel drives the center PanelPane).
 */
export function LeftRail({
  agentId,
  selected,
  onSelect,
}: {
  agentId: string
  selected: string
  onSelect: (id: string) => void
}) {
  const { data: panels = [] } = usePanels(agentId)
  // Threads panel is redundant in the cockpit — there's a dedicated Threads view
  const filteredPanels = panels.filter((p) => p.kind !== "threads")
  // Live context-budget meter: the exact summed weight of the inspection panels
  // the cockpit can see (same figure StatsPanel's "Panel context" reports), set
  // against a reference model budget. This reflects how heavy the *visible
  // panels* are — not the agent's private live context-window occupancy, which
  // the read-only inspection plane cannot serve (surfaced honestly elsewhere).
  const REF_BUDGET = 200_000
  const usedTokens = panels.reduce((sum, p) => sum + p.tokens, 0)
  const usedRatio = Math.min(1, usedTokens / REF_BUDGET)

  return (
    <aside className="rise flex w-[var(--sidebar-w)] shrink-0 flex-col border-r border-border bg-surface">
      {/* budget meter */}
      <div className="px-4 pb-3 pt-4">
        <div className="mb-2 flex items-baseline justify-between">
          <span className="text-[12px] font-medium text-foreground/80">Context</span>
          <span
            className="text-[12px] font-semibold tabular-nums"
            style={{ color: loadColor(usedRatio) }}
          >
            {(usedRatio * 100).toFixed(0)}%
          </span>
        </div>
        <TokenBar value={usedTokens} max={REF_BUDGET} className="h-1.5" />
        <div className="mt-1.5 flex justify-between text-[11px] tabular-nums text-muted-foreground">
          <span>{fmtTokens(usedTokens)}</span>
          <span>of {fmtTokens(REF_BUDGET)}</span>
        </div>
      </div>

      {/* Conversation — the agent dialogue, surfaced as a first-class nav entry
          above the panel list (T24). Selecting it renders the full conversation
          in the center pane, just like any panel. */}
      <div className="px-2 pb-1.5">
        <Tip
          title="Conversation"
          body="The agent's main dialogue — the running chat stream, shown here like any other panel."
          side="right"
          triggerClassName="block"
        >
          <button
            type="button"
            onClick={() => onSelect("conversation")}
            className={cn(
              "group flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors",
              selected === "conversation"
                ? "bg-card text-foreground card-shadow"
                : "text-foreground/70 hover:bg-muted/60",
            )}
          >
            <MessageSquare
              className="size-4 shrink-0"
              style={{ color: selected === "conversation" ? "var(--signal)" : "var(--muted-foreground)" }}
            />
            <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium">Conversation</span>
          </button>
        </Tip>
      </div>

      <div className="px-4 pb-1.5">
        <span className="label">Panels</span>
      </div>

      {/* panel list */}
      <ScrollArea className="min-h-0 flex-1">
        <ul className="px-2 pb-3">
          {filteredPanels.map((p) => {
            const Icon = panelIcon[p.kind]
            const sel = selected === p.id
            return (
              <li key={p.id}>
                <button
                  type="button"
                  onClick={() => onSelect(p.id)}
                  className={cn(
                    "group flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left transition-colors",
                    sel
                      ? "bg-card text-foreground card-shadow"
                      : "text-foreground/70 hover:bg-muted/60",
                  )}
                >
                  <Icon
                    className="size-4 shrink-0"
                    style={{ color: sel ? "var(--signal)" : "var(--muted-foreground)" }}
                  />
                  <span className="min-w-0 flex-1 truncate text-[12.5px]">{p.name}</span>
                  <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground/70">
                    {fmtTokens(p.tokens)}
                  </span>
                </button>
              </li>
            )
          })}
        </ul>
      </ScrollArea>
    </aside>
  )
}
