import { Fragment, memo, useCallback, useMemo, useRef, useState } from "react"
import { Loader2, ArchiveRestore } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "@/components/conversation/Message"
import { ThreadComposer, type CommandSuggestion } from "./ThreadComposer"
import { CreateCommandDialog } from "./CreateCommandDialog"
import { AgentEditorDialog } from "@/components/shell/behaviour/AgentEditorDialog"
import { useLibrary } from "@/lib/live"
import { sendCommand } from "@/lib/api"
import { collectThreadFiles, useConversationDrop, type UploadedFile } from "./fileUpload/helpers"
import { FormMessageRow } from "./forms/FormMessageRow"
import { isFormMessage } from "./forms/helpers"
import { useScrollPin, useThreadForms } from "./forms/useThreadForms"
import { parseAutoLine, segmentLog, toChatMessage } from "@/lib/support/threadMessages"
import { ThreadAsideRail } from "./fileUpload/ThreadAsideRail"
import { useThreadAside } from "./fileUpload/useThreadAside"
import { useAsideDefault } from "@/lib/providers/toggles/asideDefault"
import type { ThreadDetail, ThreadMsg } from "@/lib/types"

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
    <details className="group/auto mb-2 ml-7">
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
  }: {
    msg: ThreadMsg
    agentId: string
    onOpenFile: (file: UploadedFile) => void
    onShowInFinder: ((path: string) => void) | undefined
    onDelete: (msg: ThreadMsg) => void
  }) {
    return (
      <div
        // The freeze fix that MATTERS is the memo boundary above (skip
        // re-rendering unchanged rows on every SSE delta). The old
        // `content-visibility:auto` layout-skip was removed (T643): a row's
        // intrinsic-size estimate diverges wildly from a tall message's real
        // height, so any nearby reflow — e.g. a form field changing height on
        // click — made the browser recompute visibility and JUMP the scroll,
        // which read as "the whole app breaks" on interaction. Plain rows +
        // memo match the pre-refactor behaviour and stay smooth.
        className=""
      >
        <Message
          msg={toChatMessage(msg)}
          agentId={agentId}
          onOpenFile={onOpenFile}
          onShowInFinder={onShowInFinder}
          onDelete={() => onDelete(msg)}
        />
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
 * Large restore-from-archive bar shown above the composer while viewing an
 * archived thread (T709). Signal-accented, full width within the composer
 * column; clicking fires the parent's restore command.
 */
function UnarchiveBar({ onUnarchive }: { onUnarchive: () => void }) {
  return (
    <div className="px-5 py-2">
      <button
        onClick={onUnarchive}
        className="flex w-full items-center justify-center gap-2 rounded-xl border border-(--signal)/40 bg-(--signal)/10 px-4 py-1.5 text-[13px] font-medium text-(--signal) shadow-sm transition-colors hover:bg-(--signal)/20 hover:shadow-sm"
      >
        <ArchiveRestore className="size-4" />
        Unarchive thread
      </button>
    </div>
  )
}

/**
 * The per-command edit dialog (T654) — a thin wrapper over the shared
 * {@link AgentEditorDialog} in command-edit mode, prefilled from the picked
 * suggestion (which carries name/description/body, so no extra fetch). Extracted
 * to module scope so the {@link ThreadConversation} render stays within the P8
 * max-lines-per-function budget.
 */
function CommandEditDialog({
  sugg,
  agentId,
  onClose,
}: {
  sugg: CommandSuggestion
  agentId: string
  onClose: () => void
}) {
  return (
    <AgentEditorDialog
      open
      onClose={onClose}
      agentId={agentId}
      variant="command"
      mode={{ kind: "edit", itemId: sugg.command.slice(1), builtin: false }}
      initial={{ name: sugg.name, description: sugg.description, body: sugg.body ?? "" }}
    />
  )
}

/**
 * Project the live prompt library into the composer's `/command` suggestions.
 * A command's slash invocation is `/${id}` (the file-stem slug). Hoisted to
 * module scope so the {@link ThreadConversation} render stays within budget.
 */
function buildSuggestions(
  library: { kind: string; id: string; name: string; description: string; body?: string }[],
): CommandSuggestion[] {
  return library
    .filter((item) => item.kind === "command")
    .map((item) => ({
      command: `/${item.id}`,
      name: item.name,
      description: item.description,
      body: item.body,
    }))
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
  onUnarchive,
  leftRailHidden = false,
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
  /** restore this thread from the archive — only rendered when the thread is archived (T709) */
  onUnarchive?: (() => void) | undefined
  /** Whether the left thread-list rail is hidden (T680). Forwarded to the right
   *  aside so a file preview widens to half the viewport when the left rail is
   *  collapsed. Defaults to false (the 40vw behaviour) so nothing breaks if a
   *  caller omits it. */
  leftRailHidden?: boolean | undefined
}) {
  // Unified right-rail aside state (T662) — see useThreadAside. Per-thread
  // show/hide (T677) is seeded from the global default (Settings › General).
  const { defaultHidden } = useAsideDefault()
  const aside = useThreadAside(agentId, thread.id, defaultHidden)

  // ── OS-file drag-and-drop onto the whole conversation (T367) ──────────
  // Dragging files from the OS anywhere over the <main> uploads them exactly as
  // the composer's paperclip does (the SAME `onAttach` path → staged pending
  // chips), and the entire surface gets a discrete blur while a drag is in
  // flight (300ms ease in AND out). The whole feature is gated on `onAttach`.
  const { dragging, uploading, dropHandlers } = useConversationDrop(onAttach)

  // Whether the "create command" dialog (T350) is open — toggled by the pill
  // the composer renders beside the /command suggestion bubbles.
  const [createCmdOpen, setCreateCmdOpen] = useState(false)
  // The command whose editor is open (null = closed) — the bubble row's
  // per-command Edit button sets it, prefilling the shared AgentEditorDialog in
  // command-edit mode (T654). CommandSuggestion carries name/description/body, so
  // the editor prefills with no extra fetch.
  const [editCmd, setEditCmd] = useState<CommandSuggestion | null>(null)

  // First-message `/command` suggestions (T348). Surfaced ONLY for an empty
  // thread — the agent's command library is a jumping-off point for the very
  // first message, never a persistent palette. Built from the live prompt
  // library (kind === "command"); each command's slash invocation is `/${id}`
  // (the id is the command's file-stem slug, e.g. "clean" → `/clean`).
  const { data: library = [] } = useLibrary(agentId)
  // Command suggestions are built for EVERY thread (not just empty ones): the
  // composer surfaces them both as first-message bubbles on an empty thread AND
  // mid-draft on any thread when the caret's line is a lone `/` (T350). The
  // `firstMessage` flag below scopes only the empty-composer auto-show.
  const suggestions = useMemo<CommandSuggestion[]>(() => buildSuggestions(library), [library])
  // Pin the conversation to the latest message: scroll to the bottom whenever
  // a thread is opened (id change) or a new NON-AUTO message lands (user or
  // assistant text — not tool-activity traces). Auto messages update the tool
  // counter inside a collapsed <details> and must NOT yank the scroll position
  // away from the message the user is reading (T414).
  const bottomRef = useRef<HTMLDivElement>(null)
  const nonAutoCount = useMemo(() => thread.log.filter((m) => !m.auto).length, [thread.log])
  // Pin to the latest message on thread-open / new non-auto message (T414/T512).
  useScrollPin(bottomRef, thread.id, nonAutoCount)

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

  const threadFiles = useMemo(() => collectThreadFiles(thread.log), [thread.log])
  // Form derivations: answered-state lookup + submit handler (docs/forms.md §5).
  const { answersByForm, onFormSubmit } = useThreadForms(thread.log, agentId, thread.id)

  return (
    <main
      className="relative flex min-w-0 flex-1 flex-row overflow-hidden bg-background"
      // Filter is applied ONLY while dragging. A permanent `blur(0px)` (the old
      // idle value) is still a non-`none` filter, so it promotes the ENTIRE
      // conversation to a single GPU compositor layer. On a very tall thread
      // (scrollHeight can hit ~100k px) that layer exceeds Firefox's max GPU
      // texture size; a repaint triggered by any interaction (e.g. ticking a
      // form checkbox) then fails to allocate the texture and Firefox paints
      // the whole <main> BLANK while the DOM stays intact — recovered only by a
      // scroll-resetting refresh. Chromium/WebKit tile differently and never
      // hit it. Using `undefined` when idle drops the layer entirely (T644).
      style={
        dragging
          ? { filter: "blur(2px)", transition: "filter 300ms ease" }
          : { transition: "filter 300ms ease" }
      }
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
      <div className="mx-2 flex min-w-0 flex-1 flex-col pb-2">
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
              ) : isFormMessage(seg.msg.text ?? "") ? (
                <FormMessageRow
                  key={seg.msg.id}
                  msg={seg.msg}
                  agentId={agentId}
                  threadId={thread.id}
                  answersByForm={answersByForm}
                  onFormSubmit={onFormSubmit}
                  onOpenFile={aside.openFile}
                  onShowInFinder={onShowInFinder}
                  onDelete={handleDelete}
                />
              ) : (
                <MessageRow
                  key={seg.msg.id}
                  msg={seg.msg}
                  agentId={agentId}
                  onOpenFile={aside.openFile}
                  onShowInFinder={onShowInFinder}
                  onDelete={handleDelete}
                />
              ),
            )}
            {/* scroll anchor — keeps the latest message in view */}
            <div ref={bottomRef} />
          </div>
        </ScrollArea>

        <div className="mx-auto w-full max-w-[720px]">
          {thread.archived && onUnarchive && <UnarchiveBar onUnarchive={onUnarchive} />}
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
            onEditCommand={(s) => setEditCmd(s)}
            draftKey={`cp-draft-${agentId}-${thread.id}`}
            commandKey={`cp-cmd-${agentId}-${thread.id}`}
          />
        </div>
      </div>

      {/* ── Unified right rail: Files + Tasks tabs, inline preview + show/hide
          chrome (T662/T677). Extracted to ThreadAsideRail to keep this render
          body under the 500-line file budget. */}
      <ThreadAsideRail
        agentId={agentId}
        files={threadFiles}
        tasks={thread.tasks ?? []}
        aside={aside}
        leftRailHidden={leftRailHidden}
      />

      <CreateCommandDialog
        open={createCmdOpen}
        onClose={() => setCreateCmdOpen(false)}
        agentId={agentId}
      />

      {editCmd && (
        <CommandEditDialog sugg={editCmd} agentId={agentId} onClose={() => setEditCmd(null)} />
      )}
    </main>
  )
}
