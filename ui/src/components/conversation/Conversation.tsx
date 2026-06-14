import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "./Message"
import { InputBar } from "./InputBar"
import { messages } from "@/lib/mock"

export function Conversation() {
  return (
    <main className="rise flex min-w-0 flex-1 flex-col bg-background">
      {/* header strip */}
      <div className="flex h-11 shrink-0 items-center gap-2.5 border-b border-border px-5">
        <span className="text-[13px] font-semibold text-foreground/90">Conversation</span>
        <span className="text-[11.5px] text-muted-foreground">{messages.length} messages</span>
        <div className="ml-auto flex items-center gap-1.5">
          <span className="size-1.5 rounded-full bg-[var(--signal)]" />
          <span className="text-[11px] text-muted-foreground">Streaming</span>
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="mx-auto flex max-w-[760px] flex-col px-5 py-4">
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
