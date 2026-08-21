import { useState } from "react"
import { Loader2, Power, RefreshCw } from "lucide-react"
import { usePickerProviders } from "@/lib/support/models"
import { useAuth } from "@/lib/providers/auth"
import type { Agent } from "@/lib/types"
import { cn } from "@/lib/utils"
import { useSelectionState } from "./controller"
import { useAgentModalActions } from "./actions"
import type { Controller } from "./parts"
import { IdentityTab, LlmTab, VitalsTab } from "./manageBody"
import { TABS, type TabId } from "./tabs"
import { useLoopNav } from "@/lib/support/a11y"
import { HintBadge } from "@/components/shell/chrome/HintBadge"

/**
 * Agent configuration as a VIEW — the third surface in the header rail, beside
 * Threads and Finder.
 *
 * This replaces the manage DIALOG that used to open from the workspace
 * switcher's "Manage agent" row. The three categories it offers (Identity ·
 * Model · Vitals) are the same three panes the dialog showed, imported from
 * {@link TABS} rather than restated, so the two surfaces can never drift into
 * offering different categories.
 *
 * WHY THE DIALOG STILL EXISTS. `AgentModal` is not dead: the fleet dashboard
 * uses it for agent CREATION (there is no agent yet, so there is no settings
 * view to route to) and for the per-card gear at fleet altitude (where no agent
 * is focused). This view owns the focused-agent path only.
 *
 * DELIBERATELY A COPY OF ThreadsView'S SHAPE, not an abstraction over it. The
 * rail geometry, the collapse animation and the disconnect overlay are repeated
 * here rather than lifted into a shared shell: the two views agree today by
 * intent, and a premature `<RailView>` wrapper would make every future tweak to
 * one of them a negotiation with the other. The duplication is three short
 * blocks and is called out at each site.
 */
export function SettingsView({
  agent,
  railOpen,
  disconnected,
  onReconnect,
}: {
  agent: Agent
  /** Whether the category rail is shown. Owned by the shell (Root.tsx), not
   *  here: the only control that toggles it is the header rail's Settings tab,
   *  which is a SIBLING of this view rather than a descendant. */
  railOpen: boolean
  disconnected?: boolean
  onReconnect?: () => void
}) {
  const [tab, setTab] = useState<TabId>("llm")
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

      {/* Same collapse mechanism as the threads rail: the panel stays mounted
          and slides out on an animated negative left-margin, which is what
          actually RECLAIMS the layout width (a transform would leave the gap
          behind). The wrapper must be `flex` — a plain block collapses the
          aside to content height instead of letting it stretch. */}
      <div
        className="flex shrink-0 transition-[margin-left] duration-300 ease-[cubic-bezier(.16,1,.3,1)] motion-reduce:transition-none"
        style={{ marginLeft: railOpen ? 0 : "calc(-1 * var(--sidebar-w))" }}
      >
        <CategoryRail tab={tab} onSelect={setTab} />
      </div>

      <SettingsPane c={c} tab={tab} agentId={agent.id} />
    </div>
  )
}

/**
 * Assemble the shared {@link Controller} for a focused agent.
 *
 * The same three pieces `AgentModal` wires together — the provider/model
 * selection, the mutation surface, and the live name draft — minus everything
 * that only means something inside a dialog (create mode, `onClose`, the toast
 * sink). Built here rather than exported from `index.tsx` because `actions.ts`
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
    // the save bar, next to the button that caused it.
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
 * The category rail — Identity · Model · Vitals.
 *
 * Styled as a deliberate twin of the thread list's rail (same `--sidebar-w`,
 * same `card-shadow my-2` panel with no horizontal margin, same row treatment:
 * `rounded-lg px-2.5 py-1.5`, selected on `card-shadow bg-card`, otherwise a
 * hover lift). Uniformity is the point of the ask — a second settings-specific
 * rail vocabulary would make the app feel like two apps.
 */
function CategoryRail({ tab, onSelect }: { tab: TabId; onSelect: (t: TabId) => void }) {
  // ⌘/Ctrl+Up/Down loop through the categories (T634), same as the thread list.
  // TABS is the on-screen order; the hook wraps and reports the two rows to
  // badge while the modifier is held.
  const orderedIds = TABS.map((t) => t.id)
  const { modHeld, prevId, nextId } = useLoopNav(orderedIds, tab, (id) => onSelect(id as TabId))
  const navHintOf = (id: string): "up" | "down" | undefined =>
    id === prevId ? "up" : id === nextId ? "down" : undefined
  return (
    <aside className="card-shadow my-2 flex w-(--sidebar-w) shrink-0 flex-col overflow-hidden rounded-none border border-border bg-surface-2">
      <div
        className="flex h-full flex-col"
        style={{ width: "var(--sidebar-w)", minWidth: "var(--sidebar-w)" }}
      >
        <div className="p-2">
          {TABS.map((t) => {
            const on = t.id === tab
            const navHint = navHintOf(t.id)
            return (
              <button
                key={t.id}
                type="button"
                onClick={() => onSelect(t.id)}
                className={cn(
                  "group relative mb-0.5 flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left transition-colors",
                  on ? "card-shadow bg-card" : "hover:card-shadow hover:bg-card",
                )}
              >
                {navHint && (
                  <HintBadge label={navHint === "up" ? "↑" : "↓"} shown={modHeld} side="left" />
                )}
                <span
                  className={cn(
                    "flex size-6 shrink-0 items-center justify-center rounded-md transition-colors",
                    on ? "bg-(--interactive)/15 text-(--interactive)" : "text-muted-foreground/70",
                  )}
                >
                  <t.icon className="size-[15px]" />
                </span>
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span
                    className={cn(
                      "truncate text-[13px]",
                      on ? "font-medium text-foreground" : "text-foreground/85",
                    )}
                  >
                    {t.label}
                  </span>
                  {/* The blurb is the rail's answer to the thread row's preview
                      line: it says what the row leads to before it is clicked. */}
                  <span className="truncate text-[11.5px] text-muted-foreground/70">{t.blurb}</span>
                </span>
              </button>
            )
          })}
        </div>
      </div>
    </aside>
  )
}

/** The detail pane — the selected category, plus the save/lifecycle bar the
 *  dialog used to carry in its footer. */
function SettingsPane({ c, tab, agentId }: { c: Controller; tab: TabId; agentId: string }) {
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-background">
      <div className="mx-auto flex min-h-0 w-full max-w-[820px] flex-1 flex-col overflow-y-auto">
        {/* Identity brings its own sticky save bar (it writes through a
            different command than the rest), so it is rendered bare. */}
        {tab === "identity" && <IdentityTab agentId={agentId} />}
        {tab === "llm" && (
          <>
            <LlmTab c={c} />
            <SaveBar c={c} />
          </>
        )}
        {tab === "vitals" && (
          <>
            <VitalsTab c={c} agentId={agentId} />
            <LifecycleBar c={c} />
          </>
        )}
      </div>
    </div>
  )
}

/**
 * Save bar for the Model pane.
 *
 * In the dialog these fields were persisted by the modal FOOTER's button. A
 * view has no footer, and an unsaved rename that vanishes when the user clicks
 * another category is a data-loss bug, so the affordance moves next to the
 * fields it commits. Mirrors the Identity pane's own sticky bar exactly.
 */
function SaveBar({ c }: { c: Controller }) {
  return (
    <div className="flex items-center gap-3 border-t border-border/70 px-6 py-3">
      {c.error && <span className="text-[11px] text-(--danger)">{c.error}</span>}
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

/** Restart / Retire, beside the vitals they act on — the other half of the
 *  dialog footer. Retire is destructive and keeps its danger colouring. */
function LifecycleBar({ c }: { c: Controller }) {
  return (
    <div className="flex items-center gap-2 border-t border-border/70 px-6 py-3">
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
        className="ml-auto flex items-center gap-1.5 rounded-md border border-(--border-strong) px-3 py-1.5 text-[12.5px] font-medium text-foreground/85 transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
      >
        {c.restartBusy ? (
          <Loader2 className="size-3.5 animate-spin" />
        ) : (
          <RefreshCw className="size-3.5" />
        )}
        {c.restartBusy ? "Restarting…" : "Restart"}
      </button>
    </div>
  )
}
