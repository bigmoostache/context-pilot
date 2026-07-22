import { useEffect, useRef } from "react"
import { animate, createSpring } from "animejs"
import { Archive, Dices, ImagePlus, RefreshCw } from "lucide-react"
import type { Agent } from "@/lib/types"
import { avatarUrl } from "@/lib/api"
import { type ProviderDef } from "@/lib/support/models"
import { ModelPicker } from "../ModelPicker"
import { SessionVitals } from "../../shell/SessionVitals"
import { cn, prefersReducedMotion } from "@/lib/utils"

// ── Agent Settings page sections (mobile) ────────────────────────────
//
// Render subcomponents for the mobile Agent Settings PAGE (the divergent twin's
// index reworked from a dialog into a full-screen page, T636). Desktop keeps its
// floating manage dialog (components/agents/AgentModal); on a phone that dialog
// is replaced by this page, reached from the agents list (swipe → Manage) and
// from a thread page's top-right Settings button. Every field auto-saves, so
// there is no Save button — only the back chevron and a confirmation toast.

// The page's back button + title header are no longer a bespoke sticky bar:
// AgentModal now renders the app-wide glass `CornerButton` (top-left) for the
// return action and a big left-aligned title header matching the agents page,
// so the settings surface reads identically to the rest of the mobile chrome.

/**
 * Avatar hero — a large round avatar with two affordances: a **tap-to-upload**
 * label overlay (image glyph) and a **Shuffle** badge that fetches a random
 * DiceBear avatar. `avatarBust` is a cache-buster that changes after each
 * successful upload so the new image shows immediately.
 */
export function AvatarHero({
  agent,
  avatarBust,
  onAvatarChange,
  onRandomizeAvatar,
}: {
  agent: Agent
  avatarBust: number
  onAvatarChange: (file: File) => void
  onRandomizeAvatar: () => void
}) {
  return (
    <div className="flex flex-col items-center gap-3 px-4 pt-6 pb-4">
      <div className="relative size-24">
        <label
          htmlFor="agent-settings-avatar"
          title="Tap to change avatar"
          className="flex size-24 cursor-pointer items-center justify-center overflow-hidden rounded-full bg-(--signal)/14 text-(--signal) ring-1 ring-(--signal)/25 transition-opacity ring-inset active:opacity-80"
        >
          {agent.hasAvatar ? (
            <img
              src={avatarUrl(agent.id, avatarBust || undefined)}
              alt={agent.name}
              className="size-24 rounded-full object-cover"
            />
          ) : (
            <ImagePlus className="size-8" />
          )}
        </label>
        <input
          id="agent-settings-avatar"
          type="file"
          accept="image/png,image/jpeg,image/gif,image/webp,image/svg+xml"
          className="sr-only"
          onChange={(e) => {
            const file = e.target.files?.[0]
            if (file) onAvatarChange(file)
            e.target.value = ""
          }}
        />
        {/* Shuffle badge — a sibling of the label (not a descendant), so tapping
            it randomizes the avatar without triggering the label's file picker. */}
        <button
          type="button"
          onClick={onRandomizeAvatar}
          title="Shuffle avatar"
          aria-label="Shuffle avatar"
          className="absolute right-0 bottom-0 flex size-8 items-center justify-center rounded-full border border-border bg-card text-muted-foreground shadow-sm transition-colors active:border-(--signal)/40 active:text-(--signal)"
        >
          <Dices className="size-4" />
        </button>
      </div>
      <span className="text-[12px] text-muted-foreground/60">
        Tap the photo to upload · shuffle for a random one
      </span>
    </div>
  )
}

/**
 * A titled settings section — the iOS "Settings app" grouped-list idiom: a small
 * muted label above a rounded inset container. When `card` is set the children
 * are wrapped in that grouped container (rounded card, hairline row dividers via
 * `divide-y`), so a stack of rows reads as one native group; otherwise the
 * children render raw (for bodies that draw their own container, e.g. the model
 * picker or the vitals board).
 */
export function Section({
  label,
  card,
  children,
}: {
  label?: string
  card?: boolean
  children: React.ReactNode
}) {
  return (
    <section className="px-4 py-2.5">
      {label && (
        <span className="mb-1.5 block px-1 text-[12.5px] font-medium text-muted-foreground/60">
          {label}
        </span>
      )}
      {card ? (
        <div className="divide-y divide-border/50 overflow-hidden rounded-2xl border border-border/60 bg-card">
          {children}
        </div>
      ) : (
        children
      )}
    </section>
  )
}

/**
 * Name field — a borderless full-width input **row** meant to sit inside a
 * grouped {@link Section} card (the card supplies the container, so the row
 * carries no border of its own — the native iOS list-row look). 16px text
 * defeats iOS focus-zoom; auto-saves on blur and on Return via the caller's
 * `onSave` (a no-op when unchanged/empty).
 */
export function NameField({
  name,
  setName,
  onSave,
}: {
  name: string
  setName: (v: string) => void
  onSave: () => void
}) {
  return (
    <input
      value={name}
      onChange={(e) => setName(e.target.value)}
      onBlur={onSave}
      onKeyDown={(e) => {
        if (e.key !== "Enter") return
        e.preventDefault()
        e.currentTarget.blur()
      }}
      placeholder="agent name"
      className="w-full bg-transparent px-4 py-3.5 text-[16px] font-medium text-foreground outline-none placeholder:font-normal placeholder:text-muted-foreground/45"
    />
  )
}

/**
 * Provider & model picker body — the shared two-level picker. Selecting a model
 * fires `onChange` immediately, which the page turns into an auto-save
 * `configure` command (no Save button).
 */
export function ProviderBody({
  providers,
  provId,
  modelId,
  onChange,
}: {
  providers: ProviderDef[]
  provId: string
  modelId: string
  onChange: (provider: string, model: string) => void
}) {
  return <ModelPicker providers={providers} provider={provId} model={modelId} onChange={onChange} />
}

/** Service-vitals section body — the on-demand connectivity board. */
export function VitalsBody({ agentId }: { agentId: string }) {
  return <SessionVitals agentId={agentId} />
}

/**
 * Lifecycle actions — the iOS Settings destructive-action idiom: a grouped card
 * of full-width **rows** with centered text (a hairline divider between them),
 * Restart in the normal tint and Retire in danger red. Restart kills + respawns
 * the process from the current binary; Retire stops it, keeps the folder, and
 * returns to origin. Busy states swap in a spinner.
 */
export function DangerActions({
  restart,
  restartBusy,
  retire,
  retireBusy,
  busy,
}: {
  restart: () => void
  restartBusy: boolean
  retire: () => void
  retireBusy: boolean
  busy: boolean
}) {
  return (
    <div className="px-4 py-2.5 pb-[max(1.25rem,env(safe-area-inset-bottom))]">
      <div className="divide-y divide-border/50 overflow-hidden rounded-2xl border border-border/60 bg-card">
        <button
          onClick={restart}
          disabled={busy}
          className="flex w-full items-center justify-center gap-2 px-4 py-3.5 text-[15px] font-medium text-foreground/90 transition-colors active:bg-muted/50 disabled:cursor-not-allowed disabled:opacity-45"
        >
          <RefreshCw className={cn("size-4", restartBusy && "animate-spin")} />
          {restartBusy ? "Restarting…" : "Restart agent"}
        </button>
        <button
          onClick={retire}
          disabled={busy}
          className="flex w-full items-center justify-center gap-2 px-4 py-3.5 text-[15px] font-medium text-(--danger) transition-colors active:bg-(--danger)/10 disabled:cursor-not-allowed disabled:opacity-45"
        >
          <Archive className={cn("size-4", retireBusy && "animate-pulse")} />
          Retire agent
        </button>
      </div>
    </div>
  )
}

/**
 * Auto-save confirmation toast — springs up from the bottom on each new message
 * (anime.js). Conditionally rendered, so a new message remounts it and the
 * spring re-fires; reduced-motion shows it at rest.
 */
export function SettingsToast({ message }: { message: string | null }) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = ref.current
    if (!el || prefersReducedMotion()) return
    animate(el, {
      opacity: [0, 1],
      translateY: [16, 0],
      ease: createSpring({ stiffness: 420, damping: 30 }),
    })
  }, [])
  if (message === null) return null
  return (
    <div
      ref={ref}
      className="card-shadow fixed bottom-[max(1.5rem,env(safe-area-inset-bottom))] left-1/2 z-50 -translate-x-1/2 rounded-lg border border-border bg-card px-4 py-2 text-[12px] text-foreground/90"
    >
      {message}
    </div>
  )
}
