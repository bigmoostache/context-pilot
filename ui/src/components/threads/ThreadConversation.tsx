import { useState } from "react"
import { Info } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "@/components/conversation/Message"
import { QuestionForm } from "./QuestionForm"
import { ThreadComposer } from "./ThreadComposer"
import { ThreadDetailsPopup } from "./ThreadDetailsPopup"
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
export function ThreadConversation({
  thread,
  onOpenCockpit,
}: {
  thread: ThreadDetail
  onOpenCockpit: () => void
}) {
  const mine = thread.status === "MY_TURN"
  const [showDetails, setShowDetails] = useState(false)

  return (
    <main className="flex min-w-0 flex-1 flex-col bg-background">
      {/* header */}
      <div className="flex h-11 shrink-0 items-center gap-2.5 border-b border-border px-5">
        <span className="text-[13.5px] font-semibold text-foreground/90">{thread.name}</span>
        <span
          className={cn(
            "rounded-full px-2 py-0.5 text-[10.5px] font-medium",
            mine
              ? "bg-[var(--signal)]/15 text-[var(--signal)]"
              : "bg-muted text-muted-foreground",
          )}
        >
          {mine ? "Your turn" : "Agent working"}
        </span>
        <div className="ml-auto flex items-center gap-3">
          <div className="flex items-center gap-1.5 text-[11.5px] text-muted-foreground">
            <span
              className={cn("size-1.5 rounded-full", mine && "animate-pulse")}
              style={{ background: mine ? "var(--signal)" : "var(--muted-foreground)" }}
            />
            {thread.agent}
          </div>
          <button
            onClick={() => setShowDetails(true)}
            title="Thread details"
            aria-label="Thread details"
            className="flex size-7 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted hover:text-foreground"
          >
            <Info className="size-4" />
          </button>
        </div>
      </div>

      {/* messages */}
      <ScrollArea className="min-h-0 flex-1">
        <div className="mx-auto flex max-w-[720px] flex-col px-5 py-4">
          <div className="mb-3 flex items-center gap-2">
            <span className="h-px flex-1 bg-border/60" />
            <span className="text-[10.5px] text-muted-foreground/50">
              {thread.createdAt} · thread opened
            </span>
            <span className="h-px flex-1 bg-border/60" />
          </div>

          {thread.log.map((m) => (
            <div key={m.id}>
              <Message msg={toChatMessage(m)} />
              {m.questions?.map((q, i) => (
                <div key={i} className="pb-1.5 pl-7">
                  <QuestionForm q={q} />
                </div>
              ))}
              {m.fileRef && (
                <div className="pb-1.5 pl-7">
                  <span className="inline-flex items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 text-[11.5px] text-[var(--interactive)] card-shadow">
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

      {showDetails && (
        <ThreadDetailsPopup
          thread={thread}
          onOpenCockpit={onOpenCockpit}
          onClose={() => setShowDetails(false)}
        />
      )}
    </main>
  )
}
