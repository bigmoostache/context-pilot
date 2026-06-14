import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "./Message"
import { InputBar } from "./InputBar"
import { messages } from "@/lib/mock"

export function Conversation() {
  const userCount = messages.filter((m) => m.role === "user").length
  const aiCount = messages.filter((m) => m.role === "assistant").length

  return (
    <main className="rise flex min-w-0 flex-1 flex-col bg-[oklch(0.155_0.006_75)]">
      {/* header strip */}
      <div className="flex h-7 shrink-0 items-center gap-2 border-b border-border bg-[oklch(0.175_0.007_75)] px-3">
        <span className="label" style={{ color: "var(--signal)" }}>
          conversation
        </span>
        <span className="text-[10px] tabular-nums text-muted-foreground/60">
          {messages.length} msgs · {userCount}u · {aiCount}a
        </span>
        <div className="ml-auto flex items-center gap-1.5">
          <span className="size-1.5 animate-pulse rounded-full bg-[var(--signal)] shadow-[0_0_5px_var(--signal)]" />
          <span className="text-[10px] uppercase tracking-wider text-[var(--signal)]">streaming</span>
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="mx-auto flex max-w-[760px] flex-col px-4 py-3">
          {messages.map((m) => (
            <Message key={m.id} msg={m} />
          ))}
        </div>
      </ScrollArea>

      <div className="mx-auto w-full max-w-[760px]">
        <InputBar />
      </div>
    </main>
  )
}
