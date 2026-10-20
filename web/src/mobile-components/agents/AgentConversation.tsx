import { Fragment, memo, useCallback, useMemo, useRef, useState } from "react"
import { ScrollArea } from "@/mobile-components/ui/scroll-area"
import { Message } from "@/mobile-components/conversation/Message"
import { AgentComposer, type CommandSuggestion } from "@/mobile-components/agents/AgentComposer"
import { CreateCommandDialog } from "@/mobile-components/agents/CreateCommandDialog"
import { SystemEditorDialog } from "@/mobile-components/shell/behaviour/SystemEditor"
import { useLibrary } from "@/lib/live"
import { sendCommand } from "@/lib/api"
import type { UploadedFile } from "@/mobile-components/agents/fileUpload/helpers"
import { FormMessageRow } from "@/mobile-components/agents/forms/FormMessageRow"
import { isFormMessage } from "@/mobile-components/agents/forms/helpers"
import { useScrollPin, useAgentForms } from "@/mobile-components/agents/forms/useAgentForms"
import { parseAutoLine, segmentLog, toChatMessage } from "@/lib/support/agentMessages"
import type { AgentDetail, AgentMsg } from "@/lib/types"

/**
 * A collapsed run of auto tool-activity traces, rendered as an aligned
 * three-column grid (verb · tool · intent). Identical to the desktop twin — the
 * memo boundary keys on `msgs` reference so an unrelated SSE delta skips this
 * whole subtree (T510 render-storm fix).
 */
const AutoRun = memo(function AutoRun({ msgs }: { msgs: AgentMsg[] }) {
  const n = msgs.length
  return (
    <details className="group/auto mb-2 ml-5">
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
 * render storm. Identical comparator to desktop (`msg` identity + `fleetId`): a
 * delta re-render re-renders only the one changed row, not the whole agent.
 */
const MessageRow = memo(
  function MessageRow({
    msg,
    fleetId,
    onOpenFile,
    onShowInFinder,
    onDelete,
    onBranch,
    fresh,
  }: {
    msg: AgentMsg
    fleetId: string
    onOpenFile: (file: UploadedFile) => void
    onShowInFinder: ((path: string) => void) | undefined
    onDelete: (msg: AgentMsg) => void
    onBranch: (msg: AgentMsg) => void
    fresh: boolean
  }) {
    return (
      <div className="">
        <Message
          msg={toChatMessage(msg)}
          fleetId={fleetId}
          onOpenFile={onOpenFile}
          onShowInFinder={onShowInFinder}
          onDelete={() => onDelete(msg)}
          onBranch={() => onBranch(msg)}
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
  (a, b) => a.msg === b.msg && a.fleetId === b.fleetId && a.fresh === b.fresh,
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
 * Mobile agent conversation — the divergent twin of `components/agents/
 * AgentConversation`.
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
export function AgentConversation({
  agent,
  fleetId,
  onSend,
  onAttach,
  pendingFiles = [],
  onRemoveFile,
  onShowInFinder,
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
  /** open the Branch dialog on a message of this agent (branch out) */
  onBranch?: ((agentId: string, msg: AgentMsg) => void) | undefined
  /** name of the parent agent when this one was branched out of it (undefined
   *  when not a branch, or the parent no longer exists) */
  parentName?: string | undefined
  /** select another agent (the origin label's link back to the parent) */
  onOpenAgent?: ((id: string) => void) | undefined
}) {
  // T662 (mobile): the file-preview drawer was retired to match desktop. A
  // single full-width phone column can't host desktop's inline side-rail
  // preview, so tapping a file chip now opens it in the Finder (its natural
  // home) instead of a agent-local Quick Look sheet.
  const openInFinder = useCallback(
    (file: UploadedFile) => onShowInFinder?.(file.path),
    [onShowInFinder],
  )

  // Whether the "create command" dialog (T350) is open — toggled by the pill
  // the composer renders beside the /command suggestion bubbles.
  const [createCmdOpen, setCreateCmdOpen] = useState(false)
  // The command whose editor is open (null = closed) — mirrors the desktop
  // twin: the bubble row's per-command Edit button prefills the shared
  // SystemEditorDialog in command-edit mode (T654).
  const [editCmd, setEditCmd] = useState<CommandSuggestion | null>(null)

  // First-message `/command` suggestions (T348), built from the live prompt
  // library (kind === "command"); each command's slash invocation is `/${id}`.
  const { data: library = [] } = useLibrary(fleetId)
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

  // Pin the conversation to the latest message on agent-open / new non-auto
  // message (T414/T512) — auto tool-traces update a collapsed counter and must
  // not yank the scroll away from what the user is reading.
  const bottomRef = useRef<HTMLDivElement>(null)
  const nonAuto = useMemo(() => agent.log.filter((m) => !m.auto), [agent.log])
  useScrollPin(bottomRef, agent.id, nonAuto.length)

  // #5 Sent-bubble pop (anime.js): mark the SINGLE newest non-auto message as
  // `fresh` when the log grows, so its bubble springs in (send/receive
  // confirmation) — but NOT the whole initial batch, and NOT on a agent switch.
  // This is React's blessed "adjust state while rendering" pattern (NOT an
  // effect — a guarded setState during render, which re-renders in place with no
  // intermediate paint and is StrictMode-safe): comparing the prior non-auto
  // count/agent to the current, the message that just landed (last non-auto)
  // becomes the fresh one. Message's useBubblePop pops exactly that row.
  const [prevAgent, setPrevAgent] = useState(agent.id)
  const [prevCount, setPrevCount] = useState(nonAuto.length)
  const [freshId, setFreshId] = useState<string | null>(null)
  if (prevAgent !== agent.id) {
    setPrevAgent(agent.id)
    setPrevCount(nonAuto.length)
    setFreshId(null)
  } else if (nonAuto.length !== prevCount) {
    // Grew on an already-seen agent (prevCount>0 skips the first paint) → the
    // last non-auto message is the one that just landed.
    setFreshId(prevCount > 0 && nonAuto.length > prevCount ? (nonAuto.at(-1)?.id ?? null) : null)
    setPrevCount(nonAuto.length)
  }

  const { handleDelete, handleBranch } = useMessageActions(fleetId, agent.id, onBranch)

  // Fold the flat log into render segments ONCE per log change (not per render)
  // so each segment object stays reference-stable across delta re-renders.
  const segments = useMemo(() => segmentLog(agent.log), [agent.log])

  // The composer floats as a glass overlay over the bottom of the conversation
  // (so its backdrop-blur has content to frost), so the last message must be
  // able to scroll clear of it. A MEASURED spacer (composer height × N) proved
  // fragile: the scroll-pin fires before the ResizeObserver reports a height, so
  // the pin lands on a still-zero spacer and the newest message stays tucked
  // under the composer (T639). Instead reserve a fixed, generous "ghost message"
  // of dead space at the end of the scroll content — taller than the composer in
  // any resting state (+ the home-indicator safe area) — so the last real
  // message always clears it with no dependency on measurement timing.

  // Form derivations: answered-state lookup + submit handler (docs/forms.md §5).
  const { answersByForm, onFormSubmit } = useAgentForms(agent.log, fleetId, agent.id)

  return (
    <main className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      <ScrollArea className="min-h-0 flex-1">
        {/* Tight horizontal gutter (WhatsApp/Messenger convention) — px-2 not
            p-4 so bubbles claim nearly the full phone width; vertical padding
            stays roomier for scroll breathing room. The TOP pad carries
            env(safe-area-inset-top) (T639) so the first message sits below the
            iOS status bar at rest but scrolls edge-to-edge UNDER it — the shell
            no longer pads the viewport down, each scroller owns its inset. */}
        <div className="flex flex-col px-2 pt-[calc(env(safe-area-inset-top)+0.75rem)] pb-3">
          <OpenedDivider agent={agent} parentName={parentName} onOpenAgent={onOpenAgent} />

          {segments.map((seg) =>
            seg.type === "auto" ? (
              <AutoRun key={`auto-${seg.msgs[0]?.id ?? seg.type}`} msgs={seg.msgs} />
            ) : isFormMessage(seg.msg.text ?? "") ? (
              <FormMessageRow
                key={seg.msg.id}
                msg={seg.msg}
                fleetId={fleetId}
                agentId={agent.id}
                answersByForm={answersByForm}
                onFormSubmit={onFormSubmit}
                onOpenFile={openInFinder}
                onShowInFinder={onShowInFinder}
                onDelete={handleDelete}
                onBranch={handleBranch}
              />
            ) : (
              <MessageRow
                key={seg.msg.id}
                msg={seg.msg}
                fleetId={fleetId}
                onOpenFile={openInFinder}
                onShowInFinder={onShowInFinder}
                onDelete={handleDelete}
                onBranch={handleBranch}
                fresh={seg.msg.id === freshId}
              />
            ),
          )}
          {/* Fixed "ghost message" spacer — a tall block of dead space so the
              last real message can always scroll clear of the floating glass
              composer that overlays the bottom (T639). Fixed (not measured) so
              it never depends on ResizeObserver timing vs the scroll-pin; sized
              taller than the resting composer + the home-indicator safe area. */}
          <div aria-hidden style={{ height: "calc(16rem + env(safe-area-inset-bottom))" }} />
          {/* scroll anchor — keeps the latest message in view */}
          <div ref={bottomRef} />
        </div>
      </ScrollArea>

      {/* Floating glass composer — absolutely positioned over the bottom of the
          conversation so its backdrop-blur frosts the messages scrolling beneath
          it (T637). The reserved spacer above keeps the last message reachable. */}
      <div className="absolute inset-x-0 bottom-0 z-10">
        <AgentComposer
          key={agent.id}
          status={agent.status}
          focused={agent.focused}
          paused={agent.paused}
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

      <CreateCommandDialog
        open={createCmdOpen}
        onClose={() => setCreateCmdOpen(false)}
        fleetId={fleetId}
      />

      {editCmd && (
        <SystemEditorDialog
          open
          onClose={() => setEditCmd(null)}
          fleetId={fleetId}
          variant="command"
          mode={{ kind: "edit", itemId: editCmd.command.slice(1), builtin: false }}
          initial={{
            name: editCmd.name,
            description: editCmd.description,
            body: editCmd.body ?? "",
          }}
        />
      )}
    </main>
  )
}
