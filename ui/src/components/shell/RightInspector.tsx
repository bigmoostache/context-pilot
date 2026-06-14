import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ScrollArea } from "@/components/ui/scroll-area"
import { stats, threads, spine } from "@/lib/mock"
import { accentVar } from "@/lib/panelMeta"
import { cn } from "@/lib/utils"

export function RightInspector() {
  return (
    <aside className="rise flex w-[320px] shrink-0 flex-col border-l border-border bg-surface">
      <Tabs defaultValue="stats" className="flex min-h-0 flex-1 flex-col gap-0">
        <TabsList className="h-10 w-full shrink-0 justify-start gap-1 rounded-none border-b border-border bg-transparent p-2">
          {["stats", "threads", "spine"].map((t) => (
            <TabsTrigger
              key={t}
              value={t}
              className={cn(
                "h-6 flex-1 rounded-md border-0 text-[12px] font-medium capitalize text-muted-foreground",
                "data-[state=active]:bg-card data-[state=active]:text-foreground data-[state=active]:card-shadow",
              )}
            >
              {t}
            </TabsTrigger>
          ))}
        </TabsList>

        <TabsContent value="stats" className="min-h-0 flex-1 overflow-hidden">
          <ScrollArea className="h-full">
            <div className="flex flex-col px-4 py-2">
              {stats.map((s) => (
                <div
                  key={s.label}
                  className="flex items-center justify-between border-b border-border/40 py-2 last:border-0"
                >
                  <span className="text-[12px] text-muted-foreground">{s.label}</span>
                  <span
                    className="text-[12.5px] font-semibold tabular-nums"
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
            <ul className="flex flex-col gap-1 px-2 py-2">
              {threads.map((th) => {
                const mine = th.status === "MY_TURN"
                return (
                  <li
                    key={th.id}
                    className="flex flex-col gap-1 rounded-md px-2.5 py-2 hover:bg-muted/60"
                  >
                    <div className="flex items-center gap-1.5">
                      <span className="text-[12.5px] font-medium text-foreground/90">
                        {th.name}
                      </span>
                      {th.unread > 0 && (
                        <span
                          className="size-1.5 rounded-full"
                          style={{ background: "var(--signal)" }}
                        />
                      )}
                      <span
                        className={cn(
                          "ml-auto rounded-full px-2 py-0.5 text-[10px] font-medium",
                          mine
                            ? "bg-[var(--signal)]/15 text-[var(--signal)]"
                            : "bg-muted text-muted-foreground",
                        )}
                      >
                        {mine ? "Your turn" : "Working"}
                      </span>
                    </div>
                    <span className="truncate text-[11.5px] text-muted-foreground">
                      {th.last}
                    </span>
                  </li>
                )
              })}
            </ul>
          </ScrollArea>
        </TabsContent>

        <TabsContent value="spine" className="min-h-0 flex-1 overflow-hidden">
          <ScrollArea className="h-full">
            <ul className="flex flex-col px-3 py-2">
              {spine.map((n) => (
                <li key={n.id} className="flex gap-2.5 border-b border-border/30 py-2 last:border-0">
                  <span
                    className="mt-1.5 size-1.5 shrink-0 rounded-full"
                    style={{
                      background: n.kind === "user" ? "var(--interactive)" : "var(--muted-foreground)",
                    }}
                  />
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <div className="flex items-center gap-1.5">
                      <span className="text-[11px] capitalize text-muted-foreground">{n.kind}</span>
                      <span className="text-[10.5px] tabular-nums text-muted-foreground/50">
                        {n.time}
                      </span>
                    </div>
                    <span className="truncate text-[11.5px] text-foreground/75">{n.text}</span>
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
