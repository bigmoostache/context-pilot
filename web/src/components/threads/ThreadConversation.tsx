import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState } from "react"
import { Loader2 } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "@/components/conversation/Message"
import { QuestionForm } from "./QuestionForm"
import { ThreadComposer, type CommandSuggestion } from "./ThreadComposer"
import { CreateCommandDialog } from "./CreateCommandDialog"
import { QuickLookSheet } from "@/components/finder/QuickLookSheet"
import { useLibrary } from "@/lib/live"
import { sendCommand } from "@/lib/api"
import { extractDroppedFiles, zipDropped } from "@/lib/utils"
import { measure } from "@/lib/support/telemetry"
import { uploadToNode, splitMessageSegments, type UploadedFile } from "./fileUpload/helpers"
import { parseAutoLine, segmentLog, toChatMessage } from "@/lib/support/threadMessages"
import { FileSidebar, type ThreadFile } from "./fileUpload/FileSidebar"
import type { ThreadDetail, ThreadMsg } from "@/lib/types"

/** True only for an actual OS *file* drag — a text/selection drag must not blur. */
function isFileDrag(e: React.DragEvent): boolean {
  return e.dataTransfer.types.includes("Files")
}

/** Keep the surface a valid drop target on every dragover (a file drag only)
 *  and show the copy cursor. Stateless — hoisted to module scope. */
function handleDragOver(e: React.DragEvent) {
  if (!isFileDrag(e)) return
  e.preventDefault()
  e.dataTransfer.dropEffect = "copy"
}

/** The drag-event handler set spread onto the conversation surface. Each is
 *  `undefined` when uploads are disabled so the surface neither blurs nor drops. */
interface DropHandlers {
  onDragEnter: ((e: React.DragEvent) => void) | undefined
  onDragOver: ((e: React.DragEvent) => void) | undefined
  onDragLeave: ((e: React.DragEvent) => void) | undefined
  onDrop: ((e: React.DragEvent) => void) | undefined
}

/**
 * OS-file drag-and-drop onto the conversation surface (T367/T471). Returns the
 * `dragging` blur flag, the `uploading` overlay flag, and the drag handler set
 * — all inert (`undefined`) when `onAttach` is omitted. Extracted from
 * {@link ThreadConversation} so its body stays within the P8 line budget.
 *
 * dragenter/dragleave fire for every child crossed, so a depth counter tracks
 * "is the cursor still somewhere inside" rather than a flicker-prone boolean.
 */
function useConversationDrop(onAttach: ((files: File[]) => void | Promise<void>) | undefined): {
  dragging: boolean
  uploading: boolean
  dropHandlers: DropHandlers
} {
  const [dragging, setDragging] = useState(false)
  const [uploading, setUploading] = useState(false)
  const dragDepthRef = useRef(0)

  const onDragEnter = (e: React.DragEvent) => {
    if (!isFileDrag(e)) return
    e.preventDefault()
    dragDepthRef.current += 1
    setDragging(true)
  }
  const onDragLeave = (e: React.DragEvent) => {
    if (!isFileDrag(e)) return
    dragDepthRef.current = Math.max(0, dragDepthRef.current - 1)
    if (dragDepthRef.current === 0) setDragging(false)
  }
  const runDrop = async (e: React.DragEvent) => {
    if (!isFileDrag(e)) return
    e.preventDefault()
    dragDepthRef.current = 0
    setDragging(false)
    // Recurse into any dropped FOLDERS (plain `dataTransfer.files` can't — a
    // folder drop otherwise yields one unreadable pseudo-file that uploaded as a
    // failed "CORS … status null" request). extractDroppedFiles captures the
    // Entry objects synchronously before its first await, so the neutered
    // DataTransfer doesn't matter (T471).
    const dropped = await extractDroppedFiles(e.dataTransfer)
    if (dropped.length === 0) return
    setUploading(true)
    try {
      // Zip the whole drop (folder structure preserved) into ONE archive and
      // upload it in a single request — no per-file burst; awaiting keeps the
      // loader up until it lands.
      const archive = await zipDropped(dropped)
      await onAttach?.([archive])
    } catch {
      // Zipping failed (unreadable file / fflate error) — fall back to the raw
      // files so a drop is never silently lost.
      await onAttach?.(dropped.map((d) => d.file))
    } finally {
      setUploading(false)
    }
  }

  const dropHandlers: DropHandlers = onAttach
    ? {
        onDragEnter,
        onDragOver: handleDragOver,
        onDragLeave,
        onDrop: (e) => void runDrop(e),
      }
    : { onDragEnter: undefined, onDragOver: undefined, onDragLeave: undefined, onDrop: undefined }

  return { dragging, uploading, dropHandlers }
}

/**
 * A collapsed run of auto tool-activity traces, rendered as an aligned
 * three-column grid (verb · tool · intent) so the agent's live work is easy to
 * scan at a glance. Verbs and tool names carry distinct accent colours from the
 * app palette; intents are dimmed context.
 *
 * `memo`-wrapped: an auto-run re-renders only when its `msgs` array reference
 * changes. `segmentLog` is memoized on `thread.log` in the parent, so the
 * segment objects (hence this `msgs` array) stay reference-stable across the
 * renders an unrelated SSE delta triggers — the shallow prop compare then skips
 * this whole subtree. Part of the T510 render-storm fix.
 */
const AutoRun = memo(function AutoRun({ msgs }: { msgs: ThreadMsg[] }) {
  const n = msgs.length
  return (
    <details className="group/auto mb-2 ml-7 [contain-intrinsic-size:auto_2rem] [content-visibility:auto]">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[12.5px] font-medium text-muted-foreground/75 transition-colors hover:bg-muted/40 hover:text-muted-foreground">
        <span className="text-muted-foreground/60 transition-transform group-open/auto:rotate-90">
          ▸
        </span>
        <span>
          ⚙ {n} tool action{n === 1 ? "" : "s"}
        </span>
      </summary>
      <div className="mt-1 grid grid-cols-[auto_auto_1fr] gap-x-3 gap-y-0.5 border-l border-border/60 pl-3 font-mono text-[11px]">
        {msgs.map((m) => {
          const { verb, tool, intent } = parseAutoLine(m)
          return (
            <Fragment key={m.id}>
              <span className="text-(--interactive)">{verb}</span>
              <span className="text-foreground/70">{tool}</span>
              <span className="truncate text-muted-foreground/55">{intent}</span>
            </Fragment>
          )
        })}
      </div>
    </details>
  )
})

/**
 * One rendered NON-auto message row — the memoized boundary that kills the
 * T510 render storm.
 *
 * `ThreadConversation` re-renders on every SSE delta / backstop poll (the
 * threads cache hands it a new `thread` object each time). Without a memo
 * boundary React would re-render — and re-parse the markdown/KaTeX of — every
 * one of a huge thread's (T508 = 1690) message bodies on each of those renders,
 * the 100–238 ms `threads·update` commits the telemetry named. TanStack Query's
 * structural sharing already keeps each unchanged message OBJECT reference
 * stable across renders (a delta append reuses the prior 1689 refs; a poll's
 * fresh-but-deep-equal objects are collapsed back to the old refs), so a
 * `memo` keyed on `msg` identity skips every row but the one that actually
 * changed — turning a 1690-row re-render into a 1-row one.
 *
 * The comparator intentionally ignores the callback props: messages are
 * immutable by `id`, so `msg`-reference equality is the sole correctness
 * signal, and the handlers (`onDelete`/`onSend`/…) are behaviourally stable
 * (they close over the same `agentId`/thread), so a skipped row safely keeps
 * its prior closures rather than re-rendering on callback churn.
 */
const MessageRow = memo(
  function MessageRow({
    msg,
    agentId,
    onOpenFile,
    onShowInFinder,
    onDelete,
    onSend,
  }: {
    msg: ThreadMsg
    agentId: string
    onOpenFile: (file: UploadedFile) => void
    onShowInFinder: ((path: string) => void) | undefined
    onDelete: (msg: ThreadMsg) => void
    onSend: ((text: string) => void) | undefined
  }) {
    return (
      <div
        // THE freeze fix (layout half): `content-visibility:auto` lets the
        // browser SKIP layout + paint for any message row scrolled out of view;
        // the memo boundary above is the COMMIT half (skip re-rendering
        // unchanged rows). Together they collapse both costs on a huge thread.
        className="[contain-intrinsic-size:auto_5rem] [content-visibility:auto]"
      >
        <Message
          msg={toChatMessage(msg)}
          agentId={agentId}
          onOpenFile={onOpenFile}
          onShowInFinder={onShowInFinder}
          onDelete={() => onDelete(msg)}
        />
        {msg.questions?.map((q, i) => (
          <div key={i} className="pb-1.5 pl-7">
            <QuestionForm q={q} onSubmit={(answer) => onSend?.(answer)} />
          </div>
        ))}
        {msg.fileRef && (
          <div className="pb-1.5 pl-7">
            <span className="card-shadow inline-flex items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 text-[11.5px] text-(--interactive)">
              📎 {msg.fileRef}
            </span>
          </div>
        )}
      </div>
    )
  },
  (a, b) => a.msg === b.msg && a.agentId === b.agentId,
)

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
  onSend?: ((text: string) => void) | undefined
  /** upload picked files into this thread (composer paperclip). May be async so
   *  callers can `await` it to keep an in-flight loader up (T471). */
  onAttach?: ((files: File[]) => void | Promise<void>) | undefined
  /** files uploaded but not yet sent — shown as chips in the composer (T331) */
  pendingFiles?: UploadedFile[] | undefined
  /** remove a pending file by index */
  onRemoveFile?: ((index: number) => void) | undefined
  /** navigate the Finder to a file's parent directory and select it (T334) */
  onShowInFinder?: ((path: string) => void) | undefined
}) {
  // The attachment whose Quick Look drawer is open (null = closed). A
  // `file-upload` chip in any message sets it; the shared QuickLookSheet renders
  // it with the exact same FinderPreview the Finder uses.
  const [sheetFile, setSheetFile] = useState<UploadedFile | null>(null)

  // ── OS-file drag-and-drop onto the whole conversation (T367) ──────────
  // Dragging files from the OS anywhere over the <main> uploads them exactly as
  // the composer's paperclip does (the SAME `onAttach` path → staged pending
  // chips), and the entire surface gets a discrete blur while a drag is in
  // flight (300ms ease in AND out). The whole feature is gated on `onAttach`.
  const { dragging, uploading, dropHandlers } = useConversationDrop(onAttach)

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
      .map((item) => ({
        command: `/${item.id}`,
        name: item.name,
        description: item.description,
        body: item.body,
      }))
  }, [library])
  // Pin the conversation to the latest message: scroll to the bottom whenever
  // a thread is opened (id change) or a new NON-AUTO message lands (user or
  // assistant text — not tool-activity traces). Auto messages update the tool
  // counter inside a collapsed <details> and must NOT yank the scroll position
  // away from the message the user is reading (T414).
  const bottomRef = useRef<HTMLDivElement>(null)
  const nonAutoCount = useMemo(() => thread.log.filter((m) => !m.auto).length, [thread.log])
  useEffect(() => {
    const el = bottomRef.current
    if (!el) return
    // Pin the conversation to the bottom on thread-open / new message. This must
    // CONVERGE, not fire once: the `content-visibility:auto` on each row (the
    // T510 layout fix) gives every OFF-SCREEN row only its `contain-intrinsic-
    // size` placeholder height (5rem) until it's scrolled into view. On open all
    // rows are off-screen, so the container's scrollHeight is an ESTIMATE — a
    // single `scrollIntoView` lands short of the true bottom (real rows are
    // usually taller than 5rem), which was the T512 "opens not scrolled down"
    // regression. Re-scrolling across a few animation frames fixes it: each
    // scroll reveals the next chunk's REAL heights, the estimate corrects, and
    // the position converges on the actual bottom within a handful of frames.
    // `scrollIntoView` forces a synchronous layout, so it's wrapped in measure()
    // for freeze attribution; a bounded 6-frame loop on thread-open is
    // imperceptible (~100ms) and not on any hot path.
    let raf = 0
    let tries = 0
    const settle = () => {
      measure("threads:scrollIntoView", () => el.scrollIntoView({ block: "end" }))
      tries += 1
      if (tries < 6) raf = requestAnimationFrame(settle)
    }
    raf = requestAnimationFrame(settle)
    return () => cancelAnimationFrame(raf)
  }, [thread.id, nonAutoCount])

  /** Delete a message from this thread via the agent command bridge. Stable
   *  across renders (deps: agentId + thread.id) so it doesn't defeat the
   *  {@link MessageRow} memo boundary. */
  const handleDelete = useCallback(
    (msg: ThreadMsg) => {
      const ts = typeof msg.ts === "number" ? msg.ts : new Date(msg.ts ?? "").getTime()
      void sendCommand(agentId, { kind: "delete_message", thread_id: thread.id, message_ts: ts })
    },
    [agentId, thread.id],
  )

  // Fold the flat log into render segments ONCE per log change (not per
  // render). Memoizing keeps each segment object reference-stable across the
  // renders an SSE delta triggers, so the memoized AutoRun rows hold too.
  const segments = useMemo(() => segmentLog(thread.log), [thread.log])

  // Collect every file-upload block across all messages for the sidebar rail.
  const threadFiles = useMemo<ThreadFile[]>(() => {
    const result: ThreadFile[] = []
    for (const msg of thread.log) {
      const cm = toChatMessage(msg)
      for (const seg of splitMessageSegments(cm.text ?? "")) {
        if (seg.type === "file") result.push({ file: seg.file, role: cm.role })
      }
    }
    return result
  }, [thread.log])

  return (
    <main
      className="relative flex min-w-0 flex-1 flex-row bg-background"
      style={{
        filter: dragging ? "blur(2px)" : "blur(0px)",
        transition: "filter 300ms ease",
      }}
      onDragEnter={dropHandlers.onDragEnter}
      onDragOver={dropHandlers.onDragOver}
      onDragLeave={dropHandlers.onDragLeave}
      onDrop={dropHandlers.onDrop}
    >
      {/* Upload progress (T471) */}
      {uploading && (
        <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-background/40 backdrop-blur-[1px]">
          <div className="card-shadow flex items-center gap-2 rounded-xl border border-border bg-card px-4 py-2.5 text-[12.5px] text-foreground/90">
            <Loader2 className="size-4 animate-spin text-(--signal)" />
            Uploading…
          </div>
        </div>
      )}

      {/* ── Conversation column ── */}
      <div className="flex min-w-0 flex-1 flex-col">
        <ScrollArea className="min-h-0 flex-1">
        <div className="mx-auto flex max-w-[720px] flex-col px-5 py-4">
          <div className="mb-3 flex items-center gap-2">
            <span className="h-px flex-1 bg-border/60" />
            <span className="text-[10.5px] text-muted-foreground/50">
              {thread.createdAt} · thread opened
            </span>
            <span className="h-px flex-1 bg-border/60" />
          </div>

          {segments.map((seg) =>
            seg.type === "auto" ? (
              <AutoRun key={`auto-${seg.msgs[0]?.id ?? seg.type}`} msgs={seg.msgs} />
            ) : (
              <MessageRow
                key={seg.msg.id}
                msg={seg.msg}
                agentId={agentId}
                onOpenFile={setSheetFile}
                onShowInFinder={onShowInFinder}
                onDelete={handleDelete}
                onSend={onSend}
              />
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
          paused={thread.paused}
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

      </div>

      {/* ── File attachments rail ── */}
      {threadFiles.length > 0 && (
        <FileSidebar files={threadFiles} onOpen={setSheetFile} />
      )}

      <QuickLookSheet
        node={sheetFile ? uploadToNode(sheetFile) : null}
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
