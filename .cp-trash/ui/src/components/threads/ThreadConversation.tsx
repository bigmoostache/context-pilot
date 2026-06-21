import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "@/components/conversation/Message"
import { QuestionForm } from "./QuestionForm"
import { ThreadComposer } from "./ThreadComposer"
import type { ChatMessage, ThreadDetail, ThreadMsg } from "@/lib/types"

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

/**
 * Center pane — the selected thread's full conversation + composer.
 *
 * Intentionally header-less: the thread's identity (name + turn status) already
 * lives in the highlighted row of the {@link ThreadList} on the left, so a
 * repeated title bar here added no information. The conversation now starts
 * straight at the "thread opened" divider for a calmer, wider surface.
 */
export function ThreadConversation({
  thread,
}: {
  thread: ThreadDetail
}) {
  return (
    <main className="flex min-w-0 flex-1 flex-col bg-background">
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
    </main>
  )
}
