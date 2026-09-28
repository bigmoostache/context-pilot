import { useState } from "react"
import { Loader2, Power, RefreshCw } from "lucide-react"
import { usePickerProviders } from "@/lib/support/models"
import { useAuth } from "@/lib/providers/auth"
import type { Agent } from "@/lib/types"
import { useSelectionState } from "./controller"
import { useAgentModalActions } from "./actions"
import type { Controller } from "./parts"
import { LlmTab, VitalsTab } from "./manageBody"

/**
 * Agent configuration as a VIEW — the third surface in the header rail, beside
 * Threads and Finder.
 *
 * This replaces the manage DIALOG that used to open from the workspace
 * switcher's "Manage agent" row. The two sections it offers (Model · Vitals)
 * are the same {@link LlmTab}/{@link VitalsTab} bodies the dialog shows,
 * imported rather than restated, so the two surfaces can never drift.
 *
 * WHY THE DIALOG STILL EXISTS. `AgentModal` is not dead: the fleet dashboard
 * uses it for agent CREATION (there is no agent yet, so there is no settings
 * view to route to) and for the per-card gear at fleet altitude (where no agent
 * is focused). This view owns the focused-agent path only.
 *
 * ONE SCROLLING PAGE, NO CATEGORY RAIL. The two sections are adjacent, not
 * routed: an agent's model and its service health are one object seen two ways,
 * and a rail made the user pay a click to check whether anything moved. The
 * header rail's Settings tab therefore lost its re-click-to-collapse behaviour
 * (T617's activity-bar idiom) — it navigates, and that is all it does now.
 *
 * The pane bodies are still a deliberate copy of ThreadsView's SHAPE rather
 * than an abstraction over it, as noted above: the two views agree today by
 * intent, and a premature `<RailView>` wrapper would make every future tweak to
 * one of them a negotiation with the other.
 */
export function SettingsView({
  agent,
  disconnected,
  onReconnect,
}: {
  agent: Agent
  disconnected?: boolean
  onReconnect?: () => void
}) {
  const c = useAgentController(agent)

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
          aria-label="Reconnect to agent"
        />
      )}

      {/* Same shell ThreadsView uses: a single measured, self-centering column
          (`max-w-[820px]`) that owns the scroll, so a long vitals board does
          not stretch the form fields above it. */}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-background">
        <div className="mx-auto flex min-h-0 w-full max-w-[820px] flex-1 flex-col overflow-y-auto">
          <LlmTab c={c} />
          {/* Inset to the content gutter: the two sections each carry their own
              `px-6`, so a full-bleed rule would hang past the fields. A plain
              hairline, not a titled section break: {@link VitalsTab} leads with
              SessionVitals' OWN "Service vitals" heading, and a second label
              above it would read as two headings for one board. */}
          <div className="mx-6 border-t border-border/50" />
          <VitalsTab c={c} agentId={agent.id} />
          <FooterBar c={c} />
        </div>
      </div>
    </div>
  )
}

/**
 * Assemble the shared {@link Controller} for a focused agent.
 *
 * The same pieces `AgentModal` wires together — the provider/model selection,
 * the mutation surface, and the live name draft — minus everything that only
 * means something inside a dialog (create mode, `onClose`, the toast sink).
 * Built here rather than exported from `index.tsx` because `actions.ts`
 * already imports `controller.ts`, so a shared assembly in either of those
 * files would close an import cycle.
 */
function useAgentController(agent: Agent): Controller {
  const [name, setName] = useState(agent.name)
  const { data: providers = [] } = usePickerProviders()
  const sel = useSelectionState(true, agent, providers)
  const { authEnabled } = useAuth()
  const actions = useAgentModalActions({
    isManage: true,
    agent,
    name,
    sel,
    providers,
    // The view is never dismissed by a save — the user stays on it, the way
    // they stay on the threads view after sending a message. `onClose` is
    // still required by the contract because it is also what Esc fires, and
    // there is nothing to close here.
    onClose: () => {
      /* no dialog to dismiss */
    },
    // No toast sink: the fleet dashboard has one because a retire happens
    // while looking at the card that vanishes. Here the failure surfaces in
    // the footer, next to the button that caused it.
    onFlash: undefined,
  })

  return {
    isManage: true,
    agent,
    name,
    setName,
    providers,
    provId: sel.provId,
    modelId: sel.modelId,
    setSel: sel.setSel,
    // Fixed and read-only for an existing agent: the realm is the folder it
    // was created in and lives inside.
    realm: agent.folder,
    canSubmit: !actions.pending,
    authEnabled: authEnabled ?? false,
    ...actions,
  }
}

/**
 * The one footer the merged page keeps: the dialog's lifecycle pair plus the
 * save affordance, in a single row.
 *
 * The form fields above were committed by the dialog's footer button; a view
 * has no footer, so that affordance had to move somewhere the user cannot
 * scroll past. It lands here, at the end of the scroll, with the danger and
 * lifecycle actions grouped opposite it.
 *
 * GROUPING. Retire and Restart both act on the AGENT (kill, respawn) and are
 * styled as the quieter pair on the left; Save commits the FORM and keeps the
 * accent, alone on the right. A save button sharing a right edge with two
 * other buttons reads as one button group; pushing it clear of them is what
 * makes "this is the commit" legible.
 */
function FooterBar({ c }: { c: Controller }) {
  return (
    <div className="flex items-center gap-2 px-6 py-3">
      {c.error && <span className="mr-2 text-[11px] text-(--danger)">{c.error}</span>}
      <button
        type="button"
        onClick={c.retire}
        disabled={c.retireBusy}
        className="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-[12.5px] font-medium text-(--danger) transition-colors hover:bg-(--danger)/10 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {c.retireBusy ? (
          <Loader2 className="size-3.5 animate-spin" />
        ) : (
          <Power className="size-3.5" />
        )}
        Retire agent
      </button>
      <button
        type="button"
        onClick={c.restart}
        disabled={c.restartBusy}
        className="flex items-center gap-1.5 rounded-md border border-(--border-strong) px-3 py-1.5 text-[12.5px] font-medium text-foreground/85 transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
      >
        {c.restartBusy ? (
          <Loader2 className="size-3.5 animate-spin" />
        ) : (
          <RefreshCw className="size-3.5" />
        )}
        {c.restartBusy ? "Restarting…" : "Restart"}
      </button>
      <button
        type="button"
        onClick={c.submit}
        disabled={!c.canSubmit}
        className="ml-auto flex items-center gap-1.5 rounded-md bg-(--signal) px-3.5 py-1.5 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {c.pending && <Loader2 className="size-3.5 animate-spin" />}
        {c.saving ? "Saving…" : "Save changes"}
      </button>
    </div>
  )
}
