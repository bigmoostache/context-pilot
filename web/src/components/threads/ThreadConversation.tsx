import { Fragment, useEffect, useMemo, useRef, useState } from "react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "@/components/conversation/Message"
import { QuestionForm } from "./QuestionForm"
import { ThreadComposer, type CommandSuggestion } from "./ThreadComposer"
import { CreateCommandDialog } from "./CreateCommandDialog"
import { QuickLookSheet } from "@/components/finder/QuickLookSheet"
import { useLibrary } from "@/lib/live"
import { sendCommand } from "@/lib/api"
import { zipFiles } from "@/lib/utils"
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
  pendingFiles = [],
  onRemoveFile,
  onShowInFinder,
}: {
  thread: ThreadDetail
  /** owning agent — needed to open the shared Quick Look drawer for an attachment */
  agentId: string
  onSend?: (text: string) => void
  /** upload picked files into this thread (composer paperclip) */
  onAttach?: (files: File[]) => void
  /** files uploaded but not yet sent — shown as chips in the composer (T331) */
  pendingFiles?: UploadedFile[]
  /** remove a pending file by index */
  onRemoveFile?: (index: number) => void
  /** navigate the Finder to a file's parent directory and select it (T334) */
  onShowInFinder?: (path: string) => void
}) {
  // The attachment whose Quick Look drawer is open (null = closed). A
  // `file-upload` chip in any message sets it; the shared QuickLookSheet renders
  // it with the exact same FinderPreview the Finder uses.
  const [sheetFile, setSheetFile] = useState<UploadedFile | null>(null)

  // ── OS-file drag-and-drop onto the whole conversation (T367) ──────────
  //
  // Dragging files from the OS anywhere over the <main> uploads them exactly as
  // the composer's paperclip does (the SAME `onAttach` path → staged pending
  // chips), and the entire surface gets a discrete blur while a drag is in
  // flight (300ms ease in AND out) as the only affordance — no overlay, no
  // dashed border. The whole feature is gated on `onAttach`: with no upload sink
  // the surface neither blurs nor accepts a drop.
  const [dragging, setDragging] = useState(false)
  // dragenter/dragleave fire for every child crossed, so a plain boolean would
  // flicker; a depth counter tracks "is the cursor still somewhere inside".
  const dragDepth = useRef(0)
  // True only for an actual OS *file* drag — a text/selection drag must not blur.
  const isFileDrag = (e: React.DragEvent) => e.dataTransfer?.types?.includes("Files")

  const handleDragEnter = (e: React.DragEvent) => {
    if (!isFileDrag(e)) return
    e.preventDefault()
    dragDepth.current += 1
    setDragging(true)
  }
  const handleDragOver = (e: React.DragEvent) => {
    if (!isFileDrag(e)) return
    // Must preventDefault on every dragover to keep the element a valid drop
    // target; the copy cursor signals an upload.
    e.preventDefault()
    e.dataTransfer.dropEffect = "copy"
  }
  const handleDragLeave = (e: React.DragEvent) => {
    if (!isFileDrag(e)) return
    dragDepth.current = Math.max(0, dragDepth.current - 1)
    if (dragDepth.current === 0) setDragging(false)
  }
  const handleDrop = async (e: React.DragEvent) => {
    if (!isFileDrag(e)) return
    e.preventDefault()
    dragDepth.current = 0
    setDragging(false)
    // Capture the File list synchronously — dataTransfer is cleared once the
    // handler returns, so the array must be built before any await.
    const files = Array.from(e.dataTransfer.files)
    if (files.length === 0) return
    // Zip the dropped file(s) client-side into one archive, then upload that
    // single zip via the same paperclip path (T367). The paperclip picker is
    // unchanged — only drag-and-drop compresses.
    try {
      const archive = await zipFiles(files)
      onAttach?.([archive])
    } catch {
      // Zipping failed (unreadable file / fflate error) — fall back to
      // uploading the raw files so a drop is never silently lost.
      onAttach?.(files)
    }
  }

  // Whether the "create command" dialog (T350) is open — toggled by the pill
  // the composer renders beside the /command suggestion bubbles.
  const [createCmdOpen, setCreateCmdOpen] = useState(false)

  // First-message `/command` suggestions (T348). Surfaced ONLY for an empty
  // thread — the agent's command library is a jumping-off point for the very
  // first message, never a persistent palette. Built from the live prompt
  // library (kind === "command"); each command's slash invocation is `/${id}`
  // (the id is the command's file-stem slug, e.g. "clean" → `/clean`).
  const { data: library = [] } = useLibrary(agentId)
  const isEmpty = thread.log.length === 0
  // Command suggestions are built for EVERY thread (not just empty ones): the
  // composer surfaces them both as first-message bubbles on an empty thread AND
  // mid-draft on any thread when the caret's line is a lone `/` (T350). The
  // `firstMessage` flag below scopes only the empty-composer auto-show.
  const suggestions = useMemo<CommandSuggestion[]>(() => {
    return library
      .filter((item) => item.kind === "command")
      .map((item) => ({ command: `/${item.id}`, name: item.name, description: item.description, body: item.body }))
  }, [library])
  // Pin the conversation to the latest message: scroll to the bottom whenever
  // a thread is opened (id change) or a new NON-AUTO message lands (user or
  // assistant text — not tool-activity traces). Auto messages update the tool
  // counter inside a collapsed <details> and must NOT yank the scroll position
  // away from the message the user is reading (T414).
  const bottomRef = useRef<HTMLDivElement>(null)
  const nonAutoCount = useMemo(
    () => thread.log.filter((m) => !m.auto).length,
    [thread.log],
  )
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" })
  }, [thread.id, nonAutoCount])

  /** Delete a message from this thread via the agent command bridge. */
  const handleDelete = (msg: ThreadMsg) => {
    const ts = typeof msg.ts === "number" ? msg.ts : new Date(msg.ts as string).getTime()
    sendCommand(agentId, { kind: "delete_message", thread_id: thread.id, message_ts: ts })
  }

  return (
    <main
      className="flex min-w-0 flex-1 flex-col bg-background"
      // Discrete drag affordance (T367): a subtle 2px blur over the whole
      // surface while an OS file drag is in flight, eased 300ms in and out. The
      // baseline blur(0px) is kept so the OUT direction interpolates too.
      style={{
        filter: dragging ? "blur(2px)" : "blur(0px)",
        transition: "filter 300ms ease",
      }}
      onDragEnter={onAttach ? handleDragEnter : undefined}
      onDragOver={onAttach ? handleDragOver : undefined}
      onDragLeave={onAttach ? handleDragLeave : undefined}
      onDrop={onAttach ? handleDrop : undefined}
    >
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
                  onShowInFinder={onShowInFinder}
                  onDelete={() => handleDelete(seg.msg)}
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
          pendingFiles={pendingFiles}
          onRemoveFile={onRemoveFile}
          suggestions={suggestions}
          firstMessage={isEmpty}
          onCreateCommand={() => setCreateCmdOpen(true)}
          draftKey={`cp-draft-${agentId}-${thread.id}`}
        />
      </div>

      <QuickLookSheet
        node={sheetFile ? (uploadToNode(sheetFile) as FinderNode) : null}
        agentId={agentId}
        open={sheetFile !== null}
        onClose={() => setSheetFile(null)}
      />

      <CreateCommandDialog
        open={createCmdOpen}
        onClose={() => setCreateCmdOpen(false)}
        agentId={agentId}
      />
    </main>
  )
}
