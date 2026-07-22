import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState } from "react"
import { ScrollArea } from "@/mobile-components/ui/scroll-area"
import { Message } from "@/mobile-components/conversation/Message"
import { ThreadComposer, type CommandSuggestion } from "@/mobile-components/threads/ThreadComposer"
import { CreateCommandDialog } from "@/mobile-components/threads/CreateCommandDialog"
import { QuickLookSheet } from "@/mobile-components/finder/QuickLookSheet"
import { useLibrary } from "@/lib/live"
import { sendCommand } from "@/lib/api"
import { uploadToNode, type UploadedFile } from "@/mobile-components/threads/fileUpload/helpers"
import { FormMessageRow } from "@/mobile-components/threads/forms/FormMessageRow"
import { isFormMessage } from "@/mobile-components/threads/forms/helpers"
import { useScrollPin, useThreadForms } from "@/mobile-components/threads/forms/useThreadForms"
import { parseAutoLine, segmentLog, toChatMessage } from "@/lib/support/threadMessages"
import type { ThreadDetail, ThreadMsg } from "@/lib/types"

/**
 * A collapsed run of auto tool-activity traces, rendered as an aligned
 * three-column grid (verb · tool · intent). Identical to the desktop twin — the
 * memo boundary keys on `msgs` reference so an unrelated SSE delta skips this
 * whole subtree (T510 render-storm fix).
 */
const AutoRun = memo(function AutoRun({ msgs }: { msgs: ThreadMsg[] }) {
  const n = msgs.length
  return (
    <details className="group/auto mb-2 ml-5 [contain-intrinsic-size:auto_2rem] [content-visibility:auto]">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[12.5px] font-medium text-muted-foreground/75 transition-colors active:bg-muted/40">
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
 * One rendered NON-auto message row — the memoized boundary that kills the T510
 * render storm. Identical comparator to desktop (`msg` identity + `agentId`): a
 * delta re-render re-renders only the one changed row, not the whole thread.
 */
const MessageRow = memo(
  function MessageRow({
    msg,
    agentId,
    onOpenFile,
    onShowInFinder,
    onDelete,
    fresh,
  }: {
    msg: ThreadMsg
    agentId: string
    onOpenFile: (file: UploadedFile) => void
    onShowInFinder: ((path: string) => void) | undefined
    onDelete: (msg: ThreadMsg) => void
    fresh: boolean
  }) {
    return (
      <div className="[contain-intrinsic-size:auto_5rem] [content-visibility:auto]">
        <Message
          msg={toChatMessage(msg)}
          agentId={agentId}
          onOpenFile={onOpenFile}
          onShowInFinder={onShowInFinder}
          onDelete={() => onDelete(msg)}
          fresh={fresh}
        />
        {msg.fileRef && (
          <div className="pb-1.5 pl-5">
            <span className="card-shadow inline-flex items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 text-[11.5px] text-(--interactive)">
              📎 {msg.fileRef}
            </span>
          </div>
        )}
      </div>
    )
  },
  (a, b) => a.msg === b.msg && a.agentId === b.agentId && a.fresh === b.fresh,
)

/**
 * Mobile thread conversation — the divergent twin of `components/threads/
 * ThreadConversation`.
 *
 * Two structural forks from desktop, both dictated by the phone form factor:
 *
 *   • **Single full-width column.** Desktop is a `flex-row` of a centered
 *     `max-w-[720px]` message column beside a `FileSidebar` attachments rail.
 *     A phone has room for neither the side rail nor the centering gutters, so
 *     the conversation is one full-bleed column; attachments still render as
 *     inline chips within their messages (tap opens the shared Quick Look
 *     sheet), just without the dedicated rail.
 *   • **No OS drag-and-drop.** The desktop surface accepts files dragged from
 *     the OS (blur + zip + upload). A touch device has no OS file-drag, so the
 *     entire drag apparatus is dropped — attaching is the composer paperclip
 *     only.
 *
 * Everything else — the memoized `AutoRun` / `MessageRow` rows, the scroll-pin,
 * the forms plumbing, delete-message — is the shared desktop behaviour, so the
 * two conversations stay in lock-step on logic and fork only on chrome.
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

  // Whether the "create command" dialog (T350) is open — toggled by the pill
  // the composer renders beside the /command suggestion bubbles.
  const [createCmdOpen, setCreateCmdOpen] = useState(false)

  // First-message `/command` suggestions (T348), built from the live prompt
  // library (kind === "command"); each command's slash invocation is `/${id}`.
  const { data: library = [] } = useLibrary(agentId)
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

  // Pin the conversation to the latest message on thread-open / new non-auto
  // message (T414/T512) — auto tool-traces update a collapsed counter and must
  // not yank the scroll away from what the user is reading.
  const bottomRef = useRef<HTMLDivElement>(null)
  const nonAuto = useMemo(() => thread.log.filter((m) => !m.auto), [thread.log])
  useScrollPin(bottomRef, thread.id, nonAuto.length)

  // #5 Sent-bubble pop (anime.js): mark the SINGLE newest non-auto message as
  // `fresh` when the log grows, so its bubble springs in (send/receive
  // confirmation) — but NOT the whole initial batch, and NOT on a thread switch.
  // This is React's blessed "adjust state while rendering" pattern (NOT an
  // effect — a guarded setState during render, which re-renders in place with no
  // intermediate paint and is StrictMode-safe): comparing the prior non-auto
  // count/thread to the current, the message that just landed (last non-auto)
  // becomes the fresh one. Message's useBubblePop pops exactly that row.
  const [prevThread, setPrevThread] = useState(thread.id)
  const [prevCount, setPrevCount] = useState(nonAuto.length)
  const [freshId, setFreshId] = useState<string | null>(null)
  if (prevThread !== thread.id) {
    setPrevThread(thread.id)
    setPrevCount(nonAuto.length)
    setFreshId(null)
  } else if (nonAuto.length !== prevCount) {
    // Grew on an already-seen thread (prevCount>0 skips the first paint) → the
    // last non-auto message is the one that just landed.
    setFreshId(
      prevCount > 0 && nonAuto.length > prevCount ? (nonAuto.at(-1)?.id ?? null) : null,
    )
    setPrevCount(nonAuto.length)
  }

  /** Delete a message via the agent command bridge. Stable across renders
   *  (deps: agentId + thread.id) so it doesn't defeat the {@link MessageRow}
   *  memo boundary. */
  const handleDelete = useCallback(
    (msg: ThreadMsg) => {
      const ts = typeof msg.ts === "number" ? msg.ts : new Date(msg.ts ?? "").getTime()
      void sendCommand(agentId, { kind: "delete_message", thread_id: thread.id, message_ts: ts })
    },
    [agentId, thread.id],
  )

  // Fold the flat log into render segments ONCE per log change (not per render)
  // so each segment object stays reference-stable across delta re-renders.
  const segments = useMemo(() => segmentLog(thread.log), [thread.log])

  // The composer floats as a glass overlay over the bottom of the conversation
  // (so its backdrop-blur has content to frost). Measure its live height with a
  // ResizeObserver and reserve 1.5× that as a bottom spacer in the scroll
  // content, so the last message can always be scrolled clear of the floating
  // composer (T637). The observer callback is event-driven (not a render-phase
  // setState), and the height grows as the textarea auto-grows.
  const composerRef = useRef<HTMLDivElement>(null)
  const [composerH, setComposerH] = useState(0)
  useEffect(() => {
    const el = composerRef.current
    if (!el) return
    const ro = new ResizeObserver((entries) => {
      const h = entries[0]?.contentRect.height
      if (typeof h === "number") setComposerH(h)
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])


  // Form derivations: answered-state lookup + submit handler (docs/forms.md §5).
  const { answersByForm, onFormSubmit } = useThreadForms(thread.log, agentId, thread.id)

  return (
    <main className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      <ScrollArea className="min-h-0 flex-1">
        {/* Tight horizontal gutter (WhatsApp/Messenger convention) — px-2 not
            p-4 so bubbles claim nearly the full phone width; vertical padding
            stays roomier for scroll breathing room. */}
        <div className="flex flex-col px-2 py-3">
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
            ) : isFormMessage(seg.msg.text ?? "") ? (
              <FormMessageRow
                key={seg.msg.id}
                msg={seg.msg}
                agentId={agentId}
                threadId={thread.id}
                answersByForm={answersByForm}
                onFormSubmit={onFormSubmit}
                onOpenFile={setSheetFile}
                onShowInFinder={onShowInFinder}
                onDelete={handleDelete}
              />
            ) : (
              <MessageRow
                key={seg.msg.id}
                msg={seg.msg}
                agentId={agentId}
                onOpenFile={setSheetFile}
                onShowInFinder={onShowInFinder}
                onDelete={handleDelete}
                fresh={seg.msg.id === freshId}
              />
            ),
          )}
          {/* Bottom spacer = 1.5× the floating composer's height, so the last
              real message can always scroll clear of the glass composer that
              overlays the bottom of this scroll area (T637). */}
          <div aria-hidden style={{ height: composerH * 1.5 }} />
          {/* scroll anchor — keeps the latest message in view */}
          <div ref={bottomRef} />
        </div>
      </ScrollArea>

      {/* Floating glass composer — absolutely positioned over the bottom of the
          conversation so its backdrop-blur frosts the messages scrolling beneath
          it (T637). The reserved spacer above keeps the last message reachable. */}
      <div ref={composerRef} className="absolute inset-x-0 bottom-0 z-10">
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
          firstMessage={thread.log.length === 0}
          onCreateCommand={() => setCreateCmdOpen(true)}
          draftKey={`cp-draft-${agentId}-${thread.id}`}
        />
      </div>

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
