import { useEffect, useRef } from "react"
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

/** Strip the leading auto-trace marker (`/⁠* auto *⁠/ `) from text for display. */
function autoLine(m: ThreadMsg): string {
  const t = m.text ?? ""
  return t.startsWith("/* auto */ ") ? t.slice("/* auto */ ".length) : t
}

/**
 * A rendered segment of the conversation: either a single normal message, or a
 * *run* of consecutive auto tool-activity traces collapsed into one block.
 */
type Segment =
  | { type: "msg"; msg: ThreadMsg }
  | { type: "auto"; msgs: ThreadMsg[] }

/**
 * Fold the flat message log into render segments, collapsing every maximal run
 * of consecutive `auto` traces into a single {@link Segment} so the live
 * tool-activity stream renders as one quiet, expandable group instead of a wall
 * of bubbles.
 */
function segmentLog(log: ThreadMsg[]): Segment[] {
  const out: Segment[] = []
  for (const m of log) {
    if (m.auto) {
      const tail = out[out.length - 1]
      if (tail?.type === "auto") tail.msgs.push(m)
      else out.push({ type: "auto", msgs: [m] })
    } else {
      out.push({ type: "msg", msg: m })
    }
  }
  return out
}

/**
 * A collapsed run of auto tool-activity traces. Renders a quiet, dim summary
 * ("⚙ N tool actions") that expands to the individual `{verb · tool — intent}`
 * lines — so a human watching the thread can audit the agent's live work
 * without it drowning the real conversation.
 */
function AutoRun({ msgs }: { msgs: ThreadMsg[] }) {
  const n = msgs.length
  return (
    <details className="group/auto mb-1.5 ml-7">
      <summary className="flex cursor-pointer list-none items-center gap-1.5 text-[11px] text-muted-foreground/55 transition-colors hover:text-muted-foreground/80">
        <span className="transition-transform group-open/auto:rotate-90">▸</span>
        <span>⚙ {n} tool action{n === 1 ? "" : "s"}</span>
      </summary>
      <div className="mt-1 flex flex-col gap-0.5 border-l border-border/50 pl-3">
        {msgs.map((m) => (
          <span key={m.id} className="font-mono text-[10.5px] text-muted-foreground/65">
            {autoLine(m)}
          </span>
        ))}
      </div>
    </details>
  )
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
  onSend,
}: {
  thread: ThreadDetail
  onSend?: (text: string) => void
}) {
  // Pin the conversation to the latest message: scroll to the bottom whenever
  // a thread is opened (id change) or a new message lands (log grows), so the
  // freshest exchange is always in view — matching the TUI, which keeps the
  // conversation pinned to the bottom.
  const bottomRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" })
  }, [thread.id, thread.log.length])

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

          {segmentLog(thread.log).map((seg) =>
            seg.type === "auto" ? (
              <AutoRun key={`auto-${seg.msgs[0].id}`} msgs={seg.msgs} />
            ) : (
              <div key={seg.msg.id}>
                <Message msg={toChatMessage(seg.msg)} />
                {seg.msg.questions?.map((q, i) => (
                  <div key={i} className="pb-1.5 pl-7">
                    <QuestionForm q={q} onSubmit={(answer) => onSend?.(answer)} />
                  </div>
                ))}
                {seg.msg.fileRef && (
                  <div className="pb-1.5 pl-7">
                    <span className="inline-flex items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 text-[11.5px] text-[var(--interactive)] card-shadow">
                      📎 {seg.msg.fileRef}
                    </span>
                  </div>
                )}
              </div>
            ),
          )}
          {/* scroll anchor — keeps the latest message in view */}
          <div ref={bottomRef} />
        </div>
      </ScrollArea>

      <div className="mx-auto w-full max-w-[720px]">
        <ThreadComposer key={thread.id} status={thread.status} focused={thread.focused} onSend={onSend} />
      </div>
    </main>
  )
}
