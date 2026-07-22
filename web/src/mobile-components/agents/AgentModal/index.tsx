import { useCallback, useEffect, useRef, useState } from "react"
import { animate, stagger } from "animejs"
import { ChevronLeft, X } from "lucide-react"
import { usePickerProviders } from "@/lib/support/models"
import { useRenameAgent, sendCommand } from "@/lib/live"
import type { Agent } from "@/lib/types"
import { prefersReducedMotion } from "@/lib/utils"
import { CornerButton } from "@/mobile-components/shell/CornerButton"
import { useSelectionState } from "./controller"
import { useAgentModalActions } from "./actions"
import {
  AvatarHero,
  Section,
  NameField,
  ProviderBody,
  VitalsBody,
  DangerActions,
  SettingsToast,
} from "./parts"

export type { AgentModalMode } from "./controller"

/**
 * Agent Settings PAGE — the mobile twin of `components/agents/AgentModal`,
 * reworked from a dialog into a **full-screen page** (T636). Desktop keeps its
 * floating manage dialog; on a phone the dialog is replaced by this page,
 * reached from two places (the agents list swipe → Manage, and a thread page's
 * top-right Settings button). The caller owns where "Back" returns to (it wrote
 * `cameToAgentSettingsFrom` before opening us and reads it in `onClose`).
 *
 * There is **no Save button** — every field auto-saves the moment it changes:
 *   • name → `renameAgent` on blur / Return (no-op when unchanged);
 *   • provider/model → a `configure` command the instant a model is picked;
 *   • avatar → uploaded immediately on file-pick / shuffle.
 * Each save flashes a confirmation toast. Restart and Retire sit at the bottom;
 * Retire returns to the origin via `onClose`.
 *
 * Config mutations (avatar upload + DiceBear shuffle, restart flow, retire) are
 * the SAME shared hooks the desktop dialog uses (`useAgentModalActions`), so the
 * two surfaces never drift; only name + model auto-save are wired here directly
 * (the dialog batches them behind its Save button, the page saves per-field).
 */
export function AgentModal({
  agent,
  onClose,
  onFlash,
}: {
  agent: Agent
  onClose: () => void
  onFlash?: (m: string) => void
}) {
  // Server-computed picker list: only providers with a configured key, org
  // allowlist already applied (empty ⇒ all).
  const { data: providers = [] } = usePickerProviders()
  const sel = useSelectionState(true, agent, providers)
  const [name, setName] = useState(agent.name)
  const renameAgent = useRenameAgent()

  // One transient confirmation toast at a time; cleared on unmount so a late
  // save-ack can't setState a dead page.
  const [toast, setToast] = useState<string | null>(null)
  const toastTimerRef = useRef<number | null>(null)
  const flash = useCallback(
    (m: string) => {
      onFlash?.(m)
      if (toastTimerRef.current !== null) window.clearTimeout(toastTimerRef.current)
      setToast(m)
      toastTimerRef.current = window.setTimeout(() => setToast(null), 2200)
    },
    [onFlash],
  )
  useEffect(
    () => () => {
      if (toastTimerRef.current !== null) window.clearTimeout(toastTimerRef.current)
    },
    [],
  )

  // Shared avatar / restart / retire logic (the dialog's own hook). `submit` is
  // unused here — the page auto-saves name + model per-field instead. `onClose`
  // is the back-to-origin handler, so retire success returns there.
  const actions = useAgentModalActions({
    isManage: true,
    agent,
    name,
    sel,
    providers,
    onClose,
    onFlash: flash,
  })

  /** Auto-save the name on blur / Return — no-op when unchanged or empty. */
  const saveName = () => {
    const n = name.trim()
    if (n === "" || n === agent.name || renameAgent.isPending) return
    renameAgent.mutate(
      { agentId: agent.id, name: n },
      {
        onSuccess: () => flash(`Renamed to ${n}`),
        onError: (e) => flash(e instanceof Error ? e.message : "Could not rename the agent"),
      },
    )
  }

  /** Auto-save the provider/model the instant it's picked (fires `configure`). */
  const setSelAndSave = (p: string, m: string) => {
    sel.setSel(p, m)
    sendCommand(agent.id, { kind: "configure", provider: p, model: m })
      .then(() => flash("Model updated"))
      .catch((e: unknown) => flash(e instanceof Error ? e.message : "Could not update the model"))
  }

  // #1 Section cascade (anime.js): stagger the settings sections in on mount for
  // an iOS settings-page reveal. Runs once; reduced-motion shows them at rest.
  const bodyRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = bodyRef.current
    if (!el || prefersReducedMotion()) return
    animate(el.children, {
      opacity: [0, 1],
      translateY: [12, 0],
      delay: stagger(45),
      duration: 300,
      ease: "out(2)",
    })
  }, [])

  return (
    <div className="fixed inset-0 z-40 flex flex-col bg-background">
      {/* App-wide glass return button, floating top-left — same primitive every
          other mobile surface uses. Back returns to origin via onClose. z-30
          (CornerButton default) keeps it above the scrolling body. */}
      <CornerButton side="left" label="Back" onClick={onClose}>
        <ChevronLeft />
      </CornerButton>

      <div ref={bodyRef} className="min-h-0 flex-1 overflow-y-auto">
        {/* Header standardised to the agents-page layout: a big left-aligned
            title + subtitle (agent name). Top padding clears the floating
            CornerButton so the title sits below it, not under it. */}
        <header className="flex flex-col gap-0.5 px-4 pt-[calc(env(safe-area-inset-top)+4.5rem)] pb-1">
          <h1 className="text-[28px] leading-none font-bold tracking-tight text-foreground">
            Agent Settings
          </h1>
          <span className="truncate text-[13px] text-muted-foreground/70">{agent.name}</span>
        </header>

        <AvatarHero
          agent={agent}
          avatarBust={actions.avatarBust}
          onAvatarChange={actions.onAvatarChange}
          onRandomizeAvatar={actions.onRandomizeAvatar}
        />

        <Section label="Name">
          <NameField name={name} setName={setName} onSave={saveName} />
        </Section>

        <Section label="Provider & model">
          <ProviderBody
            providers={providers}
            provId={sel.provId}
            modelId={sel.modelId}
            onChange={setSelAndSave}
          />
        </Section>

        <Section label="Service vitals">
          <VitalsBody agentId={agent.id} />
        </Section>

        {actions.error && (
          <div
            role="alert"
            className="mx-4 mb-1 flex items-start gap-2 rounded-lg border border-(--danger)/30 bg-(--danger)/10 px-3 py-2 text-[12px] leading-snug text-(--danger)"
          >
            <X className="mt-px size-3.5 shrink-0" />
            <span>{actions.error}</span>
          </div>
        )}

        <DangerActions
          restart={actions.restart}
          restartBusy={actions.restartBusy}
          retire={actions.retire}
          retireBusy={actions.retireBusy}
          busy={actions.pending}
        />
      </div>

      <SettingsToast message={toast} />
    </div>
  )
}
