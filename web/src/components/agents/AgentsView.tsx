import { FolderGit2, AlertTriangle, Plus } from "lucide-react"
import { AgentList } from "./AgentList"
import { AgentConversation } from "./AgentConversation"
import { NewAgentDialog } from "./dialogs/NewAgentDialog"
import { useFleet, useAgents } from "@/lib/live"
import { useAgentSelection, useAgentActions, type Actions } from "@/lib/live/agentView"
import type { AgentDetail } from "@/lib/types"

/**
 * Agent-centered view — the conversation-first layout: agent list (left) |
 * conversation (center). Scoped to the **active agent's realm**: an agent lives
 * in its folder and owns the agents inside it, so we only ever show that
 * agent's agents — never a cross-agent global list.
 *
 * This component is keyed by `activeFleetId` in {@link App}, so its local
 * agent state is reseeded (fresh mount) whenever the realm changes. Its logic
 * lives in two same-file hooks — {@link useAgentSelection} (selection + view
 * state) and {@link useAgentActions} (mutation handlers + notice) — so the
 * render body itself stays within the P8 budgets.
 *
 * The agent list rail collapses from the header rail's Agents tab, and its
 * two actions — New agent, Search — live up there beside it, so neither
 * depends on the rail being visible.
 */
export function AgentsView({
  activeFleetId,
  selectedAgentId,
  onAgentChange,
  onShowInFinder,
  railOpen,
  newOpen,
  onNewOpenChange,
  searchOpen,
  onSearchOpenChange,
  disconnected,
  onReconnect,
}: {
  activeFleetId: string
  /** Selected agent — CONTROLLED by the shell (Root.tsx) so browser Back/Next
   *  can step through the agents visited (T636). The shell owns the value and
   *  its per-agent persistence; this view feeds it into useAgentSelection. */
  selectedAgentId: string
  onAgentChange: (id: string) => void
  /** navigate the Finder to a file's parent directory and select it (T334) */
  onShowInFinder?: (path: string) => void
  /** Whether the agent-list rail is shown. Owned by the shell (Root.tsx), not
   *  here: the only control that toggles it is the header rail's Agents tab,
   *  which is a SIBLING of this view rather than a descendant. */
  railOpen: boolean
  /** New Agent dialog open flag — shell-owned for the same reason as
   *  `railOpen`: its button sits in the header rail. The DIALOG still renders
   *  here, where the agent it creates against is in scope. */
  newOpen: boolean
  onNewOpenChange: (v: boolean) => void
  /** Search palette open flag — shell-owned, palette rendered down in the list. */
  searchOpen: boolean
  onSearchOpenChange: (v: boolean) => void
  disconnected?: boolean
  onReconnect?: () => void
}) {
  const { data: fleets = [] } = useFleet()
  const { data: agents = [] } = useAgents(activeFleetId)
  const fleet = fleets.find((a) => a.id === activeFleetId)

  // The dialog flag is INJECTED into the selection hook rather than used
  // alongside it, so `handleCreate`'s own `setNewOpen(false)` closes the very
  // flag the rail's button opened.
  const sel = useAgentSelection(
    activeFleetId,
    agents,
    {
      open: newOpen,
      setOpen: onNewOpenChange,
    },
    { selectedId: selectedAgentId, setSelectedId: onAgentChange },
  )
  const actions = useAgentActions(activeFleetId, agents, sel)

  // Only bail to a bare empty state when there is genuinely no agent. A fresh
  // agent with zero agents MUST still render the sidebar — that is where the
  // "New Agent" button lives — otherwise the realm is a dead end.
  if (!fleet) {
    return <EmptyRealm fleetName={undefined} />
  }

  const agent = agents.find((t) => t.id === sel.effectiveSelectedId)

  return (
    <div
      className="relative flex min-h-0 flex-1 overflow-hidden"
      style={
        disconnected
          ? { filter: "blur(3px) grayscale(0.5)", transition: "filter 300ms" }
          : { transition: "filter 300ms" }
      }
    >
      {disconnected && (
        <button
          onClick={onReconnect}
          className="absolute inset-0 z-40 cursor-pointer bg-background/30"
          aria-label="Reconnect to fleet"
        />
      )}
      {/* The rail stays mounted and slides in/out via an animated negative
          left-margin (the root's overflow-hidden clips it off-screen when
          closed); the sibling conversation grows to fill the reclaimed space.
          A margin transition — not a transform — is what actually reclaims the
          layout width, so the pane truly widens as the rail leaves.

          The offset is exactly the rail's width, with nothing added: AgentList's
          aside carries no horizontal margin (`my-2` only). It used to be width
          PLUS those margins — a sum kept by hand here while the margins lived in
          another file, and it drifted twice. Should the aside ever regain a
          horizontal margin, this has to grow by the same amount or collapsing
          will leave a dead gutter where the rail used to be. */}
      <div
        // `flex` so the wrapped <aside> (which sizes itself from flex-stretch,
        // not an explicit height) still fills the row's full height — a plain
        // block wrapper collapses it to content height (T670 regression).
        className="flex shrink-0 transition-[margin-left] duration-300 ease-[cubic-bezier(.16,1,.3,1)] motion-reduce:transition-none"
        style={{ marginLeft: railOpen ? 0 : "calc(-1 * var(--sidebar-w))" }}
      >
        <AgentList
          agents={agents}
          fleetId={activeFleetId}
          selectedId={sel.effectiveSelectedId}
          onSelect={sel.setSelectedId}
          showArchived={sel.showArchived}
          onToggleArchived={sel.setShowArchived}
          onArchive={actions.handleArchive}
          onDelete={actions.handleDelete}
          onPause={actions.handlePause}
          searchOpen={searchOpen}
          onSearchOpenChange={onSearchOpenChange}
        />
      </div>

      {/* The floating collapsed-rail cluster that used to sit here is gone: it
          existed only to keep New agent + Search reachable while the rail was
          hidden, and both now live permanently in the header rail. */}

      {/* The conversation pane shows the selected agent, or — for a realm with
          no agent selected/created yet — a hint pointing at the sidebar's New
          Agent button so an empty realm is never a dead end. */}
      {agent ? (
        <AgentConversation
          agent={agent}
          fleetId={activeFleetId}
          onSend={actions.handleSend}
          onAttach={actions.handleAttach}
          pendingFiles={sel.pendingFiles}
          onRemoveFile={(i) => sel.setPendingFiles((prev) => prev.filter((_, idx) => idx !== i))}
          onShowInFinder={onShowInFinder}
          leftRailHidden={!railOpen}
          onBranch={actions.openBranch}
          parentName={parentNameOf(agents, agent)}
          onOpenAgent={sel.setSelectedId}
          onUnarchive={() => {
            actions.handleArchive(agent.id)
            sel.setShowArchived(false)
          }}
        />
      ) : (
        <EmptyRealm fleetName={fleet.name} onNewAgent={() => sel.setNewOpen(true)} />
      )}

      <NewAgentDialog
        open={sel.newOpen}
        onClose={() => sel.setNewOpen(false)}
        onCreate={actions.handleCreate}
        fleetId={activeFleetId}
      />

      <BranchDialog actions={actions} agents={agents} fleetId={activeFleetId} />

      {actions.notice && (
        <div
          role="alert"
          className={
            "card-shadow fixed bottom-6 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-xl border bg-card px-4 py-2.5 text-[12.5px] text-foreground/90 " +
            (actions.notice.tone === "error" ? "border-(--danger)/40" : "border-border")
          }
        >
          {actions.notice.tone === "error" && (
            <AlertTriangle className="size-4 shrink-0 text-(--danger)" />
          )}
          <span>{actions.notice.message}</span>
          {actions.notice.undo && (
            <button
              onClick={actions.notice.undo}
              className="ml-1 shrink-0 rounded-md px-2 py-0.5 text-[12px] font-medium text-(--signal) transition-colors hover:bg-(--signal)/10"
            >
              Undo
            </button>
          )}
        </div>
      )}
    </div>
  )
}

/** Shown in the conversation pane when no agent is selected — either the
 *  realm is empty, or nothing is picked yet. When `onNewAgent` is supplied it
 *  offers a primary action so an empty realm can bootstrap its first agent
 *  without hunting for the sidebar button. */
function EmptyRealm({
  fleetName,
  onNewAgent,
}: {
  fleetName?: string | undefined
  onNewAgent?: (() => void) | undefined
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 bg-background text-center">
      <span className="flex size-12 items-center justify-center rounded-2xl bg-muted text-muted-foreground/60">
        <FolderGit2 className="size-6" />
      </span>
      <p className="max-w-[320px] text-[13px] text-muted-foreground">
        {fleetName ? (
          <>
            <span className="font-medium text-foreground/80">{fleetName}</span> has no agents yet —
            start one to put it to work in its folder.
          </>
        ) : (
          "Select a fleet to see its agents."
        )}
      </p>
      {onNewAgent && (
        <button
          onClick={onNewAgent}
          className="flex items-center gap-2 rounded-lg bg-(--signal) px-3.5 py-2 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105"
        >
          <Plus className="size-4" />
          New Agent
        </button>
      )}
    </div>
  )
}

/** The branch-out dialog: name + first message of a agent branched out of the
 *  open one at a given message. Keyed by the branch point so each opening
 *  starts from a blank (non-persisted) draft; absent while no branch is open. */
function BranchDialog({
  actions,
  agents,
  fleetId,
}: {
  actions: Actions
  agents: AgentDetail[]
  fleetId: string
}) {
  const target = actions.branchTarget
  if (!target) return null
  return (
    <NewAgentDialog
      key={`${target.agentId}-${target.messageTs}`}
      open
      onClose={actions.closeBranch}
      onCreate={actions.handleBranch}
      fleetId={fleetId}
      branch={{
        parentName: agents.find((t) => t.id === target.agentId)?.name ?? target.agentId,
        excerpt: target.excerpt,
      }}
    />
  )
}

/** Name of `agent`'s parent when it was branched out of one that still exists. */
function parentNameOf(agents: AgentDetail[], agent: AgentDetail): string | undefined {
  return agent.branchedFrom ? agents.find((t) => t.id === agent.branchedFrom)?.name : undefined
}
