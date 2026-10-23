import { memo, useCallback, useMemo, useRef, useState } from "react"
import { Loader2, ArchiveRestore } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "@/components/conversation/Message"
import { ToolRun } from "@/components/conversation/ToolRun"
import { AgentComposer, type CommandSuggestion } from "./AgentComposer"
import { CreateCommandDialog } from "./CreateCommandDialog"
import { SystemEditorDialog } from "@/components/shell/behaviour/SystemEditor"
import { useLibrary } from "@/lib/live"
import { sendCommand } from "@/lib/api"
import { collectAgentFiles, useConversationDrop, type UploadedFile } from "./fileUpload/helpers"
import { FormMessageRow } from "./forms/FormMessageRow"
import { isFormMessage } from "./forms/helpers"
import { useScrollPin, useAgentForms } from "./forms/useAgentForms"
import { segmentLog, toChatMessage } from "@/lib/support/agentMessages"
import { AgentAsideRail } from "./fileUpload/AgentAsideRail"
import { useAgentAside } from "./fileUpload/useAgentAside"
import { useAsideDefault } from "@/lib/providers/toggles/asideDefault"
import { FleetIdContext } from "@/lib/support/blocks/openuiKit"
import type { AgentDetail, AgentMsg } from "@/lib/types"

/**
 * One rendered NON-auto message row — the memoized boundary that kills the
 * T510 render storm.
 *
 * `AgentConversation` re-renders on every SSE delta / backstop poll (the
 * agents cache hands it a new `agent` object each time). Without a memo
 * boundary React would re-render — and re-parse the markdown/KaTeX of — every
 * one of a huge agent's (T508 = 1690) message bodies on each of those renders,
 * the 100–238 ms `agents·update` commits the telemetry named. TanStack Query's
 * structural sharing already keeps each unchanged message OBJECT reference
 * stable across renders (a delta append reuses the prior 1689 refs; a poll's
 * fresh-but-deep-equal objects are collapsed back to the old refs), so a
 * `memo` keyed on `msg` identity skips every row but the one that actually
 * changed — turning a 1690-row re-render into a 1-row one.
 *
 * The comparator intentionally ignores the callback props: messages are
 * immutable by `id`, so `msg`-reference equality is the sole correctness
 * signal, and the handlers (`onDelete`/`onSend`/…) are behaviourally stable
 * (they close over the same `fleetId`/agent), so a skipped row safely keeps
 * its prior closures rather than re-rendering on callback churn.
 */
const MessageRow = memo(
  function MessageRow({
    msg,
    fleetId,
    onOpenFile,
    onShowInFinder,
    onDelete,
    onBranch,
    hideActions,
  }: {
    msg: AgentMsg
    fleetId: string
    onOpenFile: (file: UploadedFile) => void
    onShowInFinder: ((path: string) => void) | undefined
    onDelete: (msg: AgentMsg) => void
    onBranch: (msg: AgentMsg) => void
    /** drop the timestamp + action row (narrow chat beside a file preview) */
    hideActions: boolean
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
          fleetId={fleetId}
          onOpenFile={onOpenFile}
          onShowInFinder={onShowInFinder}
          onDelete={() => onDelete(msg)}
          onBranch={() => onBranch(msg)}
          hideActions={hideActions}
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
  (a, b) => a.msg === b.msg && a.fleetId === b.fleetId && a.hideActions === b.hideActions,
)

/**
 * The "agent opened" divider at the top of the conversation. For a agent
 * branched out of another it reads "branched from <parent>", a link back to the
 * parent while it still exists (a deleted parent shows its bare id).
 */
function OpenedDivider({
  agent,
  parentName,
  onOpenAgent,
}: {
  agent: AgentDetail
  parentName: string | undefined
  onOpenAgent: ((id: string) => void) | undefined
}) {
  const parent = agent.branchedFrom
  let origin: React.ReactNode = "agent opened"
  if (parent && (parentName === undefined || !onOpenAgent)) origin = `branched from ${parent}`
  else if (parent && onOpenAgent)
    origin = (
      <>
        branched from{" "}
        <button
          type="button"
          onClick={() => onOpenAgent(parent)}
          className="underline decoration-dotted underline-offset-2 transition-colors hover:text-foreground"
        >
          {parentName}
        </button>
      </>
    )
  return (
    <div className="mb-3 flex items-center gap-2">
      <span className="h-px flex-1 bg-border/60" />
      <span className="text-[10.5px] text-muted-foreground/50">
        {agent.createdAt == null ? "" : new Date(agent.createdAt).toLocaleString()} · {origin}
      </span>
      <span className="h-px flex-1 bg-border/60" />
    </div>
  )
}

/**
 * Per-message actions for a agent's rows: delete (via the agent command
 * bridge) and open the Branch dialog. Both are stable across renders (deps:
 * fleetId / the stable `onBranch` + agent id) so they don't defeat the
 * {@link MessageRow} memo boundary, which ignores callback props.
 */
function useMessageActions(
  fleetId: string,
  agentId: string,
  onBranch: ((agentId: string, msg: AgentMsg) => void) | undefined,
) {
  const handleDelete = useCallback(
    (msg: AgentMsg) => {
      const ts = typeof msg.ts === "number" ? msg.ts : new Date(msg.ts ?? "").getTime()
      void sendCommand(fleetId, { kind: "delete_message", agent_id: agentId, message_ts: ts })
    },
    [fleetId, agentId],
  )
  const handleBranch = useCallback((msg: AgentMsg) => onBranch?.(agentId, msg), [onBranch, agentId])
  return { handleDelete, handleBranch }
}

/**
 * Large restore-from-archive bar shown above the composer while viewing an
 * archived agent (T709). Signal-accented, full width within the composer
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
        Unarchive agent
      </button>
    </div>
  )
}

/**
 * The per-command edit dialog (T654) — a thin wrapper over the shared
 * {@link SystemEditorDialog} in command-edit mode, prefilled from the picked
 * suggestion (which carries name/description/body, so no extra fetch). Extracted
 * to module scope so the {@link AgentConversation} render stays within the P8
 * max-lines-per-function budget.
 */
function CommandEditDialog({
  sugg,
  fleetId,
  onClose,
}: {
  sugg: CommandSuggestion
  fleetId: string
  onClose: () => void
}) {
  return (
    <SystemEditorDialog
      open
      onClose={onClose}
      fleetId={fleetId}
      variant="command"
      mode={{ kind: "edit", itemId: sugg.command.slice(1), builtin: false }}
      initial={{ name: sugg.name, description: sugg.description, body: sugg.body ?? "" }}
    />
  )
}

/**
 * Project the live prompt library into the composer's `/command` suggestions.
 * A command's slash invocation is `/${id}` (the file-stem slug). Hoisted to
 * module scope so the {@link AgentConversation} render stays within budget.
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
 * Center pane — the selected agent's full conversation + composer.
 *
 * Intentionally header-less: the agent's identity (name + turn status) already
 * lives in the highlighted row of the {@link AgentList} on the left, so a
 * repeated title bar here added no information. The conversation now starts
 * straight at the "agent opened" divider for a calmer, wider surface.
 */
export function AgentConversation({
  agent,
  fleetId,
  onSend,
  onAttach,
  pendingFiles = [],
  onRemoveFile,
  onShowInFinder,
  onUnarchive,
  onBranch,
  parentName,
  onOpenAgent,
}: {
  agent: AgentDetail
  /** owning agent — needed to open the shared Quick Look drawer for an attachment */
  fleetId: string
  onSend?: ((text: string) => Promise<boolean>) | undefined
  /** upload picked files into this agent (composer paperclip). May be async so
   *  callers can `await` it to keep an in-flight loader up (T471). */
  onAttach?: ((files: File[]) => void | Promise<void>) | undefined
  /** files uploaded but not yet sent — shown as chips in the composer (T331) */
  pendingFiles?: UploadedFile[] | undefined
  /** remove a pending file by index */
  onRemoveFile?: ((index: number) => void) | undefined
  /** navigate the Finder to a file's parent directory and select it (T334) */
  onShowInFinder?: ((path: string) => void) | undefined
  /** restore this agent from the archive — only rendered when the agent is archived (T709) */
  onUnarchive?: (() => void) | undefined
  /** open the Branch dialog on a message of this agent (branch out) */
  onBranch?: ((agentId: string, msg: AgentMsg) => void) | undefined
  /** name of the parent agent when this one was branched out of it (undefined
   *  when not a branch, or the parent no longer exists) */
  parentName?: string | undefined
  /** select another agent (the origin label's link back to the parent) */
  onOpenAgent?: ((id: string) => void) | undefined
}) {
  // Unified right-rail aside state (T662) — see useAgentAside. Per-agent
  // show/hide (T677) is seeded from the global default (Settings › General).
  const { defaultHidden } = useAsideDefault()
  const aside = useAgentAside(fleetId, agent.id, defaultHidden)

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
  // per-command Edit button sets it, prefilling the shared SystemEditorDialog in
  // command-edit mode (T654). CommandSuggestion carries name/description/body, so
  // the editor prefills with no extra fetch.
  const [editCmd, setEditCmd] = useState<CommandSuggestion | null>(null)

  // First-message `/command` suggestions (T348). Surfaced ONLY for an empty
  // agent — the agent's command library is a jumping-off point for the very
  // first message, never a persistent palette. Built from the live prompt
  // library (kind === "command"); each command's slash invocation is `/${id}`
  // (the id is the command's file-stem slug, e.g. "clean" → `/clean`).
  const { data: library = [] } = useLibrary(fleetId)
  // Command suggestions are built for EVERY agent (not just empty ones): the
  // composer surfaces them both as first-message bubbles on an empty agent AND
  // mid-draft on any agent when the caret's line is a lone `/` (T350). The
  // `firstMessage` flag below scopes only the empty-composer auto-show.
  const suggestions = useMemo<CommandSuggestion[]>(() => buildSuggestions(library), [library])
  // Pin the conversation to the latest message: scroll to the bottom whenever
  // a agent is opened (id change) or a new NON-AUTO message lands (user or
  // assistant text — not tool-activity traces). Auto messages update the tool
  // counter inside a collapsed <details> and must NOT yank the scroll position
  // away from the message the user is reading (T414).
  const bottomRef = useRef<HTMLDivElement>(null)
  const nonAutoCount = useMemo(() => agent.log.filter((m) => !m.auto).length, [agent.log])
  // Pin to the latest message on agent-open / new non-auto message (T414/T512).
  useScrollPin(bottomRef, agent.id, nonAutoCount)

  const { handleDelete, handleBranch } = useMessageActions(fleetId, agent.id, onBranch)

  // Fold the flat log into render segments ONCE per log change (not per
  // render). Memoizing keeps each segment object reference-stable across the
  // renders an SSE delta triggers, so the memoized AutoRun rows hold too.
  const segments = useMemo(() => segmentLog(agent.log), [agent.log])

  const agentFiles = useMemo(() => collectAgentFiles(agent.log), [agent.log])
  // Form derivations: answered-state lookup + submit handler (docs/forms.md §5).
  const { answersByForm, onFormSubmit } = useAgentForms(agent.log, fleetId, agent.id)

  // A preview (file or note) takes 70% of the row: the narrow chat drops per-message chrome.
  const previewing = !aside.hidden && aside.previewing

  return (
    <FleetIdContext value={fleetId}>
      <main
        className="@container relative flex min-w-0 flex-1 flex-row overflow-hidden bg-background"
        // Filter is applied ONLY while dragging. A permanent `blur(0px)` (the old
        // idle value) is still a non-`none` filter, so it promotes the ENTIRE
        // conversation to a single GPU compositor layer. On a very tall agent
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
        <div data-ou-pane className="mx-2 flex min-w-0 flex-1 flex-col pb-2">
          <ScrollArea className="min-h-0 flex-1">
            <div className="mx-auto flex max-w-[720px] flex-col px-5 py-4">
              <OpenedDivider agent={agent} parentName={parentName} onOpenAgent={onOpenAgent} />

              {segments.map((seg) =>
                seg.type === "auto" ? (
                  <ToolRun key={`auto-${seg.msgs[0]?.id ?? seg.type}`} msgs={seg.msgs} />
                ) : isFormMessage(seg.msg.text ?? "") ? (
                  <FormMessageRow
                    key={seg.msg.id}
                    msg={seg.msg}
                    fleetId={fleetId}
                    agentId={agent.id}
                    answersByForm={answersByForm}
                    onFormSubmit={onFormSubmit}
                    onOpenFile={aside.openFile}
                    onShowInFinder={onShowInFinder}
                    onDelete={handleDelete}
                    onBranch={handleBranch}
                    hideActions={previewing}
                  />
                ) : (
                  <MessageRow
                    key={seg.msg.id}
                    msg={seg.msg}
                    fleetId={fleetId}
                    onOpenFile={aside.openFile}
                    onShowInFinder={onShowInFinder}
                    onDelete={handleDelete}
                    onBranch={handleBranch}
                    hideActions={previewing}
                  />
                ),
              )}
              {/* scroll anchor — keeps the latest message in view */}
              <div ref={bottomRef} />
            </div>
          </ScrollArea>

          <div className="mx-auto w-full max-w-[720px]">
            {agent.archived && onUnarchive && <UnarchiveBar onUnarchive={onUnarchive} />}
            <AgentComposer
              key={agent.id}
              status={agent.status}
              paused={agent.paused}
              warning={agent.warning}
              onSend={onSend}
              onAttach={onAttach}
              pendingFiles={pendingFiles}
              onRemoveFile={onRemoveFile}
              suggestions={suggestions}
              firstMessage={agent.log.length === 0}
              onCreateCommand={() => setCreateCmdOpen(true)}
              onEditCommand={(s) => setEditCmd(s)}
              draftKey={`cp-draft-${fleetId}-${agent.id}`}
              commandKey={`cp-cmd-${fleetId}-${agent.id}`}
            />
          </div>
        </div>

        {/* ── Unified right rail: Files + Tasks tabs, inline preview + show/hide
          chrome (T662/T677). Extracted to AgentAsideRail to keep this render
          body under the 500-line file budget. */}
        <AgentAsideRail
          fleetId={fleetId}
          agentId={agent.id}
          files={agentFiles}
          tasks={agent.tasks ?? []}
          notes={agent.notes ?? []}
          aside={aside}
        />

        <CreateCommandDialog
          open={createCmdOpen}
          onClose={() => setCreateCmdOpen(false)}
          fleetId={fleetId}
        />

        {editCmd && (
          <CommandEditDialog sugg={editCmd} fleetId={fleetId} onClose={() => setEditCmd(null)} />
        )}
      </main>
    </FleetIdContext>
  )
}
