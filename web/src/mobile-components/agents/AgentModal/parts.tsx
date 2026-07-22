import { useEffect, useRef } from "react"
import { animate, createSpring } from "animejs"
import {
  Archive,
  ChevronLeft,
  Dices,
  ImagePlus,
  RefreshCw,
  Settings2,
} from "lucide-react"
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

/**
 * Sticky iOS-style nav header — a back chevron (returns to wherever the page was
 * opened from, via the caller), a centred title, and the agent's name as a
 * subtitle. Spring-rises on mount (anime.js); reduced-motion shows it at rest.
 */
export function SettingsHeader({ agentName, onBack }: { agentName: string; onBack: () => void }) {
  const ref = useRef<HTMLElement>(null)
  useEffect(() => {
    const el = ref.current
    if (!el || prefersReducedMotion()) return
    animate(el, {
      opacity: [0, 1],
      translateY: [-8, 0],
      ease: createSpring({ stiffness: 420, damping: 32 }),
    })
  }, [])
  return (
    <header
      ref={ref}
      className="sticky top-0 z-10 flex items-center gap-2 border-b border-border/70 bg-background/85 px-2 py-2.5 pt-[max(0.625rem,env(safe-area-inset-top))] backdrop-blur-md"
    >
      <button
        onClick={onBack}
        aria-label="Back"
        className="flex size-9 items-center justify-center rounded-lg text-(--interactive) transition-colors active:bg-muted/70"
      >
        <ChevronLeft className="size-6" />
      </button>
      <div className="flex min-w-0 flex-1 flex-col leading-tight">
        <span className="text-[15px] font-semibold tracking-tight text-foreground">
          Agent Settings
        </span>
        <span className="truncate text-[11.5px] text-muted-foreground/70">{agentName}</span>
      </div>
      <Settings2 className="mr-1 size-4 shrink-0 text-muted-foreground/40" />
    </header>
  )
}

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

/** A titled settings section wrapper — a small uppercase label above its body,
 *  with consistent horizontal gutters. */
export function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2 px-4 py-3">
      <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
        {label}
      </span>
      {children}
    </section>
  )
}

/**
 * Name field — 16px (defeats iOS focus-zoom), **auto-saves on blur** and on
 * Return. The caller's `onSave` no-ops when the value is unchanged/empty, so a
 * focus-in-focus-out without an edit costs nothing.
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
    <div className="group flex items-center gap-2.5 rounded-xl border border-border bg-card px-3.5 py-3 transition-colors focus-within:border-(--interactive)/70 focus-within:ring-2 focus-within:ring-(--interactive)/15">
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
        className="w-full bg-transparent text-[16px] font-medium text-foreground outline-none placeholder:font-normal placeholder:text-muted-foreground/45"
      />
    </div>
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
 * Danger / lifecycle actions — Restart (kill + respawn the process from the
 * current binary) and Retire (stop the process, keep the folder, return to
 * origin). Full-width touch buttons, safe-area padded, with busy spinners.
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
    <div className="mt-2 flex flex-col gap-2 px-4 pt-3 pb-[max(1.25rem,env(safe-area-inset-bottom))]">
      <button
        onClick={restart}
        disabled={busy}
        className="flex items-center justify-center gap-2 rounded-xl border border-border bg-card px-4 py-3 text-[14px] font-medium text-foreground/85 transition-colors active:bg-muted/60 disabled:cursor-not-allowed disabled:opacity-50"
      >
        <RefreshCw className={cn("size-4", restartBusy && "animate-spin")} />
        {restartBusy ? "Restarting…" : "Restart agent"}
      </button>
      <button
        onClick={retire}
        disabled={busy}
        className="flex items-center justify-center gap-2 rounded-xl border border-(--danger)/30 bg-(--danger)/10 px-4 py-3 text-[14px] font-medium text-(--danger) transition-colors active:bg-(--danger)/20 disabled:cursor-not-allowed disabled:opacity-50"
      >
        <Archive className={cn("size-4", retireBusy && "animate-pulse")} />
        Retire agent
      </button>
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
