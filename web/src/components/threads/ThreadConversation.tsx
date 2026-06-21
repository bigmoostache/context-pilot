import { Fragment, useEffect, useRef, useState } from "react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "@/components/conversation/Message"
import { QuestionForm } from "./QuestionForm"
import { ThreadComposer } from "./ThreadComposer"
import { QuickLookSheet } from "@/components/finder/QuickLookSheet"
import { uploadToNode, type UploadedFile } from "./fileUpload"
import type { ChatMessage, ThreadDetail, ThreadMsg } from "@/lib/types"
import type { FinderNode } from "@/lib/types"

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

/** Parse an auto-trace message into its three columns: verb, tool, intent. */
function parseAutoLine(m: ThreadMsg): { verb: string; tool: string; intent: string } {
  const raw = m.text ?? ""
  const t = raw.startsWith("/* auto */ ") ? raw.slice("/* auto */ ".length) : raw
  const dotIdx = t.indexOf(" · ")
  if (dotIdx < 0) return { verb: t, tool: "", intent: "" }
  const verb = t.slice(0, dotIdx)
  const rest = t.slice(dotIdx + 3)
  const dashIdx = rest.indexOf(" — ")
  if (dashIdx < 0) return { verb, tool: rest, intent: "" }
  return { verb, tool: rest.slice(0, dashIdx), intent: rest.slice(dashIdx + 3) }
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
 * A collapsed run of auto tool-activity traces, rendered as an aligned
 * three-column grid (verb · tool · intent) so the agent's live work is easy to
 * scan at a glance. Verbs and tool names carry distinct accent colours from the
 * app palette; intents are dimmed context.
 */
function AutoRun({ msgs }: { msgs: ThreadMsg[] }) {
  const n = msgs.length
  return (
    <details className="group/auto mb-2 ml-7">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[12.5px] font-medium text-muted-foreground/75 transition-colors hover:bg-muted/40 hover:text-muted-foreground">
        <span className="text-muted-foreground/60 transition-transform group-open/auto:rotate-90">▸</span>
        <span>⚙ {n} tool action{n === 1 ? "" : "s"}</span>
      </summary>
      <div className="mt-1 grid grid-cols-[auto_auto_1fr] gap-x-3 gap-y-0.5 border-l border-border/60 pl-3 font-mono text-[11px]">
        {msgs.map((m) => {
          const { verb, tool, intent } = parseAutoLine(m)
          return (
            <Fragment key={m.id}>
              <span className="text-[var(--interactive)]">{verb}</span>
              <span className="text-foreground/70">{tool}</span>
              <span className="truncate text-muted-foreground/55">{intent}</span>
            </Fragment>
          )
        })}
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
  agentId,
  onSend,
  onAttach,
}: {
  thread: ThreadDetail
  /** owning agent — needed to open the shared Quick Look drawer for an attachment */
  agentId: string
  onSend?: (text: string) => void
  /** upload picked files into this thread (composer paperclip) */
  onAttach?: (files: File[]) => void
}) {
  // The attachment whose Quick Look drawer is open (null = closed). A
  // `file-upload` chip in any message sets it; the shared QuickLookSheet renders
  // it with the exact same FinderPreview the Finder uses.
  const [sheetFile, setSheetFile] = useState<UploadedFile | null>(null)
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
                <Message
                  msg={toChatMessage(seg.msg)}
                  agentId={agentId}
                  onOpenFile={setSheetFile}
                />
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
        <ThreadComposer
          key={thread.id}
          status={thread.status}
          focused={thread.focused}
          onSend={onSend}
          onAttach={onAttach}
        />
      </div>

      <QuickLookSheet
        node={sheetFile ? (uploadToNode(sheetFile) as FinderNode) : null}
        agentId={agentId}
        open={sheetFile !== null}
        onClose={() => setSheetFile(null)}
      />
    </main>
  )
}
