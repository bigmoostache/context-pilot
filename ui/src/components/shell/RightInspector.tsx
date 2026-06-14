import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ScrollArea } from "@/components/ui/scroll-area"
import { stats, threads, spine } from "@/lib/mock"
import { accentVar } from "@/lib/panelMeta"
import { cn } from "@/lib/utils"

export function RightInspector() {
  return (
    <aside className="rise flex w-[320px] shrink-0 flex-col border-l border-border bg-[oklch(0.165_0.006_75)]">
      <Tabs defaultValue="stats" className="flex min-h-0 flex-1 flex-col gap-0">
        <TabsList className="h-8 w-full shrink-0 justify-start rounded-none border-b border-border bg-[oklch(0.18_0.007_75)] p-0">
          {["stats", "threads", "spine"].map((t) => (
            <TabsTrigger
              key={t}
              value={t}
              className={cn(
                "h-8 flex-1 rounded-none border-0 border-b-2 border-transparent text-[10px] uppercase tracking-[0.14em] text-muted-foreground",
                "data-[state=active]:border-[var(--signal)] data-[state=active]:bg-transparent data-[state=active]:text-[var(--signal)] data-[state=active]:shadow-none",
              )}
            >
              {t}
            </TabsTrigger>
          ))}
        </TabsList>

        <TabsContent value="stats" className="min-h-0 flex-1 overflow-hidden">
          <ScrollArea className="h-full">
            <div className="flex flex-col px-3 py-2">
              {stats.map((s) => (
                <div
                  key={s.label}
                  className="flex items-center justify-between border-b border-border/40 py-1.5 last:border-0"
                >
                  <span className="text-[11px] text-muted-foreground">{s.label}</span>
                  <span
                    className="text-[11.5px] font-semibold tabular-nums"
                    style={{ color: s.accent ? accentVar[s.accent] : "var(--foreground)" }}
                  >
                    {s.value}
                  </span>
                </div>
              ))}
            </div>
          </ScrollArea>
        </TabsContent>

        <TabsContent value="threads" className="min-h-0 flex-1 overflow-hidden">
          <ScrollArea className="h-full">
            <ul className="flex flex-col px-2 py-2">
              {threads.map((th) => {
                const mine = th.status === "MY_TURN"
                return (
                  <li
                    key={th.id}
                    className="flex flex-col gap-1 rounded-[3px] px-2 py-1.5 hover:bg-[oklch(0.2_0.008_75)]"
                  >
                    <div className="flex items-center gap-1.5">
                      <span className="text-[12px] text-foreground/90">{th.name}</span>
                      <span className="text-[9px] text-muted-foreground/50">{th.id}</span>
                      {th.unread > 0 && (
                        <span className="size-1.5 rounded-full bg-[var(--signal)] shadow-[0_0_5px_var(--signal)]" />
                      )}
                      <span
                        className={cn(
                          "ml-auto rounded-[2px] px-1.5 py-px text-[9px] uppercase tracking-wider",
                          mine
                            ? "bg-[var(--signal)]/15 text-[var(--signal)]"
                            : "bg-[oklch(0.26_0.006_75)] text-muted-foreground",
                        )}
                      >
                        {th.status}
                      </span>
                    </div>
                    <span className="truncate text-[10.5px] text-muted-foreground/70">
                      {th.last}
                    </span>
                    <span className="text-[9px] tabular-nums text-muted-foreground/40">
                      {th.messages} messages
                    </span>
                  </li>
                )
              })}
            </ul>
          </ScrollArea>
        </TabsContent>

        <TabsContent value="spine" className="min-h-0 flex-1 overflow-hidden">
          <ScrollArea className="h-full">
            <ul className="flex flex-col px-2 py-2">
              {spine.map((n) => (
                <li key={n.id} className="flex gap-2 border-b border-border/30 py-1.5 last:border-0">
                  <span
                    className={cn(
                      "mt-1 size-1.5 shrink-0 rounded-full",
                      n.kind === "user" ? "bg-[var(--interactive)]" : "bg-[var(--grid)]",
                    )}
                  />
                  <div className="flex min-w-0 flex-1 flex-col">
                    <div className="flex items-center gap-1.5">
                      <span className="text-[9px] uppercase tracking-wider text-muted-foreground/50">
                        {n.kind}
                      </span>
                      <span className="text-[9px] tabular-nums text-muted-foreground/40">
                        {n.time}
                      </span>
                      <span className="ml-auto text-[9px] text-[var(--ok)]/60">processed</span>
                    </div>
                    <span className="truncate text-[10.5px] text-foreground/75">{n.text}</span>
                  </div>
                </li>
              ))}
            </ul>
          </ScrollArea>
        </TabsContent>
      </Tabs>
    </aside>
  )
}
