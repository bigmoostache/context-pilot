import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "@/components/conversation/Message"
import { QuestionForm } from "./QuestionForm"
import { ThreadComposer } from "./ThreadComposer"
import type { ChatMessage, ThreadDetail, ThreadMsg } from "@/lib/types"
import { cn } from "@/lib/utils"

/** Map a thread message onto the shared ChatMessage shape for the renderer. */
function toChatMessage(m: ThreadMsg): ChatMessage {
  return {
    id: m.id,
    role: m.tool ? "tool" : m.author,
    text: m.text,
    tool: m.tool,
    ts: m.ts,
    streaming: m.streaming,
  }
}

/** Center pane — the selected thread's full conversation + composer. */
export function ThreadConversation({ thread }: { thread: ThreadDetail }) {
  const mine = thread.status === "MY_TURN"

  return (
    <main className="flex min-w-0 flex-1 flex-col bg-[oklch(0.155_0.006_75)]">
      {/* header */}
      <div className="flex h-9 shrink-0 items-center gap-2.5 border-b border-border bg-[oklch(0.175_0.007_75)] px-4">
        <span className="text-[13px] font-semibold text-foreground/90">{thread.name}</span>
        <span
          className="inline-flex items-center gap-1.5 rounded-[3px] px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider"
          style={{
            background: mine ? "var(--signal)" : "oklch(0.24 0.008 75)",
            color: mine ? "oklch(0.16 0.02 75)" : "var(--interactive)",
          }}
        >
          {mine ? "my turn" : "their turn"}
        </span>
        <div className="ml-auto flex items-center gap-1.5 text-[10px] text-muted-foreground/60">
          <span
            className={cn("size-1.5 rounded-full", mine && "animate-pulse")}
            style={{
              background: mine ? "var(--signal)" : "var(--interactive)",
              boxShadow: mine ? "0 0 5px var(--signal)" : "none",
            }}
          />
          {thread.agent}
        </div>
      </div>

      {/* messages */}
      <ScrollArea className="min-h-0 flex-1">
        <div className="mx-auto flex max-w-[720px] flex-col px-5 py-4">
          <div className="mb-3 flex items-center gap-2">
            <span className="h-px flex-1 bg-border/50" />
            <span className="text-[9px] uppercase tracking-[0.16em] text-muted-foreground/40">
              {thread.createdAt} · thread opened
            </span>
            <span className="h-px flex-1 bg-border/50" />
          </div>

          {thread.log.map((m) => (
            <div key={m.id}>
              <Message msg={toChatMessage(m)} />
              {m.questions?.map((q, i) => (
                <div key={i} className="pb-1.5 pl-1">
                  <QuestionForm q={q} />
                </div>
              ))}
              {m.fileRef && (
                <div className="pb-1.5 pl-1">
                  <span className="inline-flex items-center gap-1.5 rounded-[3px] border border-border bg-[oklch(0.19_0.007_75)] px-2 py-1 text-[11px] text-[var(--interactive)]">
                    📎 {m.fileRef}
                  </span>
                </div>
              )}
            </div>
          ))}
        </div>
      </ScrollArea>

      <div className="mx-auto w-full max-w-[720px]">
        <ThreadComposer status={thread.status} />
      </div>
    </main>
  )
}
