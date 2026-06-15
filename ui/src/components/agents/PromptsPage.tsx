import { useState } from "react"
import { Bot, Plus, Sparkles, TerminalSquare, Zap } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { library } from "@/lib/mock"
import type { LibraryItem, LibraryKind } from "@/lib/types"
import { cn } from "@/lib/utils"

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
  agent: { label: "Agent", plural: "Agents", icon: Bot, accent: "var(--signal)", blurb: "System prompts — a personality & operating contract." },
  skill: { label: "Skill", plural: "Skills", icon: Zap, accent: "var(--interactive)", blurb: "Reference material loaded into context on demand." },
  command: { label: "Command", plural: "Commands", icon: TerminalSquare, accent: "var(--ok)", blurb: "Slash-commands that expand into a prompt." },
}

const TABS: (LibraryKind | "all")[] = ["all", "agent", "skill", "command"]

export function PromptsPage() {
  const [tab, setTab] = useState<LibraryKind | "all">("all")

  const counts = {
    agent: library.filter((i) => i.kind === "agent").length,
    skill: library.filter((i) => i.kind === "skill").length,
    command: library.filter((i) => i.kind === "command").length,
  }
  const shown = tab === "all" ? library : library.filter((i) => i.kind === tab)

  return (
    <ScrollArea className="min-h-0 flex-1 bg-background">
      <div className="mx-auto flex w-full max-w-[920px] flex-col gap-6 px-8 py-9">
        {/* header */}
        <header className="flex items-end justify-between gap-4">
          <div className="flex flex-col gap-1.5">
            <span className="label">Prompt library</span>
            <h1 className="text-[24px] font-semibold tracking-tight text-foreground">Prompts</h1>
            <p className="max-w-[560px] text-[13px] text-muted-foreground">
              A global library of agents, skills and commands — shared across every agent in the fleet.
            </p>
          </div>
          <button className="flex shrink-0 items-center gap-2 rounded-lg bg-[var(--interactive)] px-3.5 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105">
            <Plus className="size-4" />
            New
          </button>
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
              <KindSection key={k} kind={k} items={library.filter((i) => i.kind === k)} />
            ))}
          </div>
        ) : (
          <Grid items={shown} />
        )}

        <p className="text-center text-[11px] text-muted-foreground/55">
          Design-only — editing a prompt opens its <code className="font-mono">.md</code> file in the live app.
        </p>
      </div>
    </ScrollArea>
  )
}

function KindSection({ kind, items }: { kind: LibraryKind; items: LibraryItem[] }) {
  const M = KIND_META[kind]
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <M.icon className="size-4" style={{ color: M.accent }} />
        <h2 className="text-[13px] font-semibold text-foreground/90">{M.plural}</h2>
        <span className="text-[11px] text-muted-foreground/55">{M.blurb}</span>
      </div>
      <Grid items={items} />
    </section>
  )
}

function Grid({ items }: { items: LibraryItem[] }) {
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {items.map((item, i) => (
        <LibraryCard key={item.id} item={item} i={i} />
      ))}
    </div>
  )
}

function LibraryCard({ item, i }: { item: LibraryItem; i: number }) {
  const M = KIND_META[item.kind]
  const mono = item.kind === "command"
  return (
    <div
      style={{ animationDelay: `${Math.min(i, 10) * 35}ms` }}
      className="opt-rise group flex flex-col gap-2.5 rounded-xl border border-border bg-card p-4 card-shadow transition-colors hover:border-[color-mix(in_oklab,var(--interactive)_45%,transparent)]"
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
        {item.active && (
          <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-[var(--ok)]/14 px-1.5 py-0.5 text-[9.5px] font-medium text-[var(--ok)]">
            <Sparkles className="size-2.5" />
            Active
          </span>
        )}
        {item.builtin && !item.active && (
          <span className="shrink-0 rounded-full bg-muted/70 px-1.5 py-0.5 text-[9.5px] font-medium text-muted-foreground/70">
            Built-in
          </span>
        )}
      </div>
      <p className="line-clamp-2 min-h-[2.4em] text-[12px] leading-snug text-foreground/70">{item.description}</p>
    </div>
  )
}
