import { useState } from "react"
import { Bot, Download, Plus, TerminalSquare, Zap } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { library } from "@/lib/mock"
import type { LibraryItem, LibraryKind } from "@/lib/types"
import { cn } from "@/lib/utils"
import { ImportModal, PromptModal } from "./PromptModal"
import { FLEET_MAX_W } from "./FleetShell"

/**
 * Global prompt library — the fleet dashboard's "Prompts" page. Manages the
 * three prompt kinds (agents, skills, commands) as a single global library,
 * the captain's intended end-state (today they're per-agent). Design-only:
 * cards are illustrative, create/edit actions are decorative.
 */

const KIND_META: Record<
  LibraryKind,
  { label: string; plural: string; icon: typeof Bot; accent: string; blurb: string }
> = {
  agent: { label: "System prompt", plural: "System", icon: Bot, accent: "var(--signal)", blurb: "System prompts — a personality & operating contract." },
  skill: { label: "Skill", plural: "Skills", icon: Zap, accent: "var(--interactive)", blurb: "Reference material loaded into context on demand." },
  command: { label: "Command", plural: "Commands", icon: TerminalSquare, accent: "var(--ok)", blurb: "Slash-commands that expand into a prompt." },
}

const TABS: (LibraryKind | "all")[] = ["all", "agent", "skill", "command"]

export function PromptsPage() {
  const [tab, setTab] = useState<LibraryKind | "all">("all")
  const [editing, setEditing] = useState<LibraryItem | "new" | null>(null)
  const [importing, setImporting] = useState(false)

  const counts = {
    agent: library.filter((i) => i.kind === "agent").length,
    skill: library.filter((i) => i.kind === "skill").length,
    command: library.filter((i) => i.kind === "command").length,
  }
  const shown = tab === "all" ? library : library.filter((i) => i.kind === tab)

  return (
    <ScrollArea className="min-h-0 flex-1 bg-background">
      <div className={cn("mx-auto flex w-full flex-col gap-7 px-8 py-9", FLEET_MAX_W)}>
        {/* header */}
        <header className="flex items-end justify-between gap-4">
          <div className="flex flex-col gap-1.5">
            <span className="label">Prompt library</span>
            <h1 className="text-[24px] font-semibold tracking-tight text-foreground">Prompts</h1>
            <p className="max-w-[560px] text-[13px] text-muted-foreground">
              A global library of system prompts, skills and commands — shared across every agent in the fleet.
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <button
              onClick={() => setImporting(true)}
              className="flex items-center gap-2 rounded-lg border border-border bg-card px-3.5 py-2 text-[12.5px] font-medium text-foreground/80 transition-colors hover:border-[var(--interactive)]/50 hover:text-foreground"
            >
              <Download className="size-4" />
              Import
            </button>
            <button
              onClick={() => setEditing("new")}
              className="flex items-center gap-2 rounded-lg bg-[var(--interactive)] px-3.5 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
            >
              <Plus className="size-4" />
              New
            </button>
          </div>
        </header>

        {/* filter tabs */}
        <div className="flex items-center gap-0.5 self-start rounded-lg border border-border bg-muted/60 p-0.5">
          {TABS.map((t) => {
            const label = t === "all" ? "All" : KIND_META[t].plural
            const count = t === "all" ? library.length : counts[t]
            return (
              <button
                key={t}
                onClick={() => setTab(t)}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12px] font-medium transition-all",
                  tab === t ? "bg-card text-foreground card-shadow" : "text-muted-foreground hover:text-foreground",
                )}
              >
                {label}
                <span className="rounded-full bg-muted/80 px-1.5 py-px text-[9.5px] tabular-nums text-muted-foreground">
                  {count}
                </span>
              </button>
            )
          })}
        </div>

        {/* grid */}
        {tab === "all" ? (
          <div className="flex flex-col gap-6">
            {(["agent", "skill", "command"] as LibraryKind[]).map((k) => (
              <KindSection key={k} kind={k} items={library.filter((i) => i.kind === k)} onOpen={setEditing} />
            ))}
          </div>
        ) : (
          <Grid items={shown} onOpen={setEditing} />
        )}

        <p className="text-center text-[11px] text-muted-foreground/55">
          Design-only — editing a prompt opens its <code className="font-mono">.md</code> file in the live app.
        </p>
      </div>

      {editing && <PromptModal item={editing} onClose={() => setEditing(null)} />}
      {importing && <ImportModal onClose={() => setImporting(false)} />}
    </ScrollArea>
  )
}

function KindSection({
  kind,
  items,
  onOpen,
}: {
  kind: LibraryKind
  items: LibraryItem[]
  onOpen: (i: LibraryItem) => void
}) {
  const M = KIND_META[kind]
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <M.icon className="size-4" style={{ color: M.accent }} />
        <h2 className="text-[13px] font-semibold text-foreground/90">{M.plural}</h2>
        <span className="text-[11px] text-muted-foreground/55">{M.blurb}</span>
      </div>
      <Grid items={items} onOpen={onOpen} />
    </section>
  )
}

function Grid({ items, onOpen }: { items: LibraryItem[]; onOpen: (i: LibraryItem) => void }) {
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {items.map((item, i) => (
        <LibraryCard key={item.id} item={item} i={i} onOpen={onOpen} />
      ))}
    </div>
  )
}

function LibraryCard({
  item,
  i,
  onOpen,
}: {
  item: LibraryItem
  i: number
  onOpen: (i: LibraryItem) => void
}) {
  const M = KIND_META[item.kind]
  const mono = item.kind === "command"
  return (
    <button
      onClick={() => onOpen(item)}
      style={{ animationDelay: `${Math.min(i, 10) * 35}ms` }}
      className="opt-rise group flex flex-col gap-2.5 rounded-xl border border-border bg-card p-4 text-left card-shadow transition-colors hover:border-[color-mix(in_oklab,var(--interactive)_45%,transparent)]"
    >
      <div className="flex items-center gap-2.5">
        <span
          className="flex size-8 shrink-0 items-center justify-center rounded-lg"
          style={{ background: `color-mix(in oklab, ${M.accent} 15%, transparent)`, color: M.accent }}
        >
          <M.icon className="size-[18px]" />
        </span>
        <div className="flex min-w-0 flex-1 flex-col leading-tight">
          <span className={cn("truncate text-[13.5px] font-semibold text-foreground/90", mono && "font-mono text-[13px]")}>
            {item.name}
          </span>
          {item.meta && <span className="truncate text-[10.5px] text-muted-foreground/65">{item.meta}</span>}
        </div>
        {item.builtin && (
          <span className="shrink-0 rounded-full bg-muted/70 px-1.5 py-0.5 text-[9.5px] font-medium text-muted-foreground/70">
            Built-in
          </span>
        )}
      </div>
      <p className="line-clamp-2 min-h-[2.4em] text-[12px] leading-snug text-foreground/70">{item.description}</p>
    </button>
  )
}
