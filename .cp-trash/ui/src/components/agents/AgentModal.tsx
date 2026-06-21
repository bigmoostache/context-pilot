import { useEffect, useRef, useState } from "react"
import {
  Archive,
  Bot,
  Check,
  CornerDownLeft,
  FolderGit2,
  Gauge,
  Settings2,
  Sparkles,
  Wand2,
  X,
  Zap,
} from "lucide-react"
import type { Agent } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Agent create / manage dialog — the single source of truth for both flows.
 *
 * Shared by the fleet dashboard (the canonical agent-management surface) and,
 * in *manage* mode, by the TopBar's per-agent shortcut button (T26) so the user
 * can edit the focused agent in one click instead of the four-step
 * switcher → "Manage agents" → find → "Manage" journey.
 *
 * Rendering note: the backdrop is `absolute inset-0`, so the host must provide a
 * viewport-sized positioning context. The fleet dashboard renders it inside a
 * `relative` full-height container; the TopBar renders it as a *sibling* of the
 * `.vibrancy` header (never a descendant) so it anchors to the viewport and
 * escapes the header's backdrop-filter containing block.
 */

export const MODELS = ["claude-opus-4-8", "claude-sonnet-4-6", "claude-fable-5"]

/** Per-model descriptor for the rich picker — surfaces existing info, no new behaviour. */
export const MODEL_META: Record<
  string,
  { tier: string; blurb: string; price: string; icon: typeof Zap }
> = {
  "claude-opus-4-8": { tier: "Most capable", blurb: "Deep reasoning & long tasks", price: "$5 · 200K", icon: Sparkles },
  "claude-sonnet-4-6": { tier: "Balanced", blurb: "Fast, 1M-token context", price: "$3 · 1M", icon: Gauge },
  "claude-fable-5": { tier: "Creative", blurb: "Expressive, vivid prose", price: "$10 · 400K", icon: Zap },
}

/** Derive the realm folder name from the agent name (replaces the folder picker). */
export function slugify(name: string): string {
  const s = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
  return s || "untitled"
}

/** The two ways the dialog can open. */
export type AgentModalMode = { mode: "create" } | { mode: "manage"; agent: Agent }

export function AgentModal({
  modal,
  onClose,
  onFlash,
}: {
  modal: AgentModalMode
  onClose: () => void
  /** Optional toast sink — the fleet dashboard supplies one; the TopBar omits it. */
  onFlash?: (m: string) => void
}) {
  const isManage = modal.mode === "manage"
  const agent = isManage ? modal.agent : undefined
  const [name, setName] = useState(agent?.name ?? "")
  const [model, setModel] = useState(agent?.model ?? MODELS[0])
  const nameRef = useRef<HTMLInputElement>(null)

  // Realm folder: in create mode it's derived live from the name (no picker);
  // in manage mode it's the agent's fixed, read-only realm.
  const realm = isManage ? (agent?.folder ?? "") : `~/code/${slugify(name)}`
  const canSubmit = isManage || name.trim().length > 0

  const submit = () => {
    if (!canSubmit) return
    onFlash?.(
      isManage
        ? `Saved changes to ${name || agent?.name}`
        : `Created “${slugify(name)}” in ${realm}`,
    )
    onClose()
  }

  // Ergonomy: autofocus the name, Esc closes, ⌘/Ctrl+Enter submits.
  useEffect(() => {
    const t = window.setTimeout(() => nameRef.current?.focus(), 60)
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) submit()
    }
    window.addEventListener("keydown", onKey)
    return () => {
      window.clearTimeout(t)
      window.removeEventListener("keydown", onKey)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name, model, isManage])

  return (
    <div
      className="backdrop-fade absolute inset-0 z-40 flex items-center justify-center bg-black/40 backdrop-blur-[3px]"
      onClick={onClose}
    >
      <div
        className="modal-pop relative flex w-[460px] flex-col overflow-hidden rounded-2xl border border-border bg-popover pop-shadow"
        onClick={(e) => e.stopPropagation()}
      >
        {/* hero header — a soft accent wash + grain */}
        <div className="relative flex items-start gap-3.5 border-b border-border/70 px-6 pb-5 pt-6">
          <div
            className="pointer-events-none absolute inset-0 opacity-[0.5]"
            style={{
              background: isManage
                ? "radial-gradient(120% 100% at 0% 0%, color-mix(in oklab, var(--signal) 16%, transparent), transparent 60%)"
                : "radial-gradient(120% 100% at 0% 0%, color-mix(in oklab, var(--interactive) 18%, transparent), transparent 60%)",
            }}
          />
          <span
            className={cn(
              "relative flex size-11 shrink-0 items-center justify-center rounded-xl ring-1 ring-inset",
              isManage
                ? "bg-[var(--signal)]/14 text-[var(--signal)] ring-[var(--signal)]/25"
                : "bg-[var(--interactive)]/14 text-[var(--interactive)] ring-[var(--interactive)]/25",
            )}
          >
            {isManage ? <Settings2 className="size-[22px]" /> : <Wand2 className="size-[22px]" />}
          </span>
          <div className="relative flex flex-1 flex-col gap-0.5 pt-0.5">
            <h3 className="text-[17px] font-semibold tracking-tight text-foreground">
              {isManage ? `Manage ${agent?.name}` : "Create an agent"}
            </h3>
            <p className="text-[12px] leading-relaxed text-muted-foreground">
              {isManage
                ? "Rename, switch model, or archive. The realm folder is fixed."
                : "Name it, pick a model — its realm folder is created for you."}
            </p>
          </div>
          <button
            onClick={onClose}
            className="relative -mr-1 -mt-1 flex size-7 items-center justify-center rounded-md text-muted-foreground/55 transition-colors hover:bg-muted/70 hover:text-foreground"
            aria-label="Close"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="flex flex-col gap-5 px-6 py-5">
          {/* name — the star field, with a leading glyph + live realm preview */}
          <div className="flex flex-col gap-2">
            <span className="text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
              Agent name
            </span>
            <div className="group flex items-center gap-2.5 rounded-xl border border-border bg-card px-3.5 py-2.5 transition-colors focus-within:border-[var(--interactive)]/70 focus-within:ring-2 focus-within:ring-[var(--interactive)]/15">
              <FolderGit2 className="size-[18px] shrink-0 text-muted-foreground/55 transition-colors group-focus-within:text-[var(--interactive)]" />
              <input
                ref={nameRef}
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="my-project"
                className="w-full bg-transparent text-[15px] font-medium text-foreground outline-none placeholder:font-normal placeholder:text-muted-foreground/45"
              />
            </div>
            {/* live-derived realm — the ergonomic replacement for the folder picker */}
            <div className="flex items-center gap-1.5 pl-0.5 text-[11.5px]">
              <span className="text-muted-foreground/60">Realm</span>
              <span className="text-muted-foreground/40">→</span>
              <code className="rounded-md bg-muted/60 px-1.5 py-0.5 font-mono text-[11px] text-foreground/75">
                {realm}
              </code>
              {!isManage && (
                <span className="text-muted-foreground/45">· created automatically</span>
              )}
            </div>
          </div>

          {/* model — rich selectable cards instead of plain chips */}
          <div className="flex flex-col gap-2">
            <span className="text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
              Model
            </span>
            <div className="flex flex-col gap-2">
              {MODELS.map((m, i) => {
                const meta = MODEL_META[m]
                const Icon = meta?.icon ?? Bot
                const active = m === model
                return (
                  <button
                    key={m}
                    onClick={() => setModel(m)}
                    style={{ animationDelay: `${i * 45}ms` }}
                    className={cn(
                      "opt-rise group flex items-center gap-3 rounded-xl border px-3 py-2.5 text-left transition-all",
                      active
                        ? "border-[var(--interactive)] bg-[var(--interactive)]/[0.07] ring-2 ring-[var(--interactive)]/15"
                        : "border-border bg-card hover:border-[var(--interactive)]/40 hover:bg-muted/30",
                    )}
                  >
                    <span
                      className={cn(
                        "flex size-8 shrink-0 items-center justify-center rounded-lg transition-colors",
                        active
                          ? "bg-[var(--interactive)]/15 text-[var(--interactive)]"
                          : "bg-muted/60 text-muted-foreground/70",
                      )}
                    >
                      <Icon className="size-4" />
                    </span>
                    <div className="flex min-w-0 flex-1 flex-col">
                      <span className="flex items-center gap-2">
                        <span className="font-mono text-[12.5px] font-medium text-foreground/90">{m}</span>
                        <span className="rounded bg-muted/70 px-1.5 py-px text-[9.5px] font-semibold uppercase tracking-wide text-muted-foreground">
                          {meta?.tier}
                        </span>
                      </span>
                      <span className="text-[11px] text-muted-foreground">{meta?.blurb}</span>
                    </div>
                    <span className="shrink-0 font-mono text-[10.5px] tabular-nums text-muted-foreground/65">
                      {meta?.price}
                    </span>
                    <span
                      className={cn(
                        "flex size-5 shrink-0 items-center justify-center rounded-full border transition-all",
                        active
                          ? "border-[var(--interactive)] bg-[var(--interactive)] text-[var(--primary-foreground)]"
                          : "border-border text-transparent",
                      )}
                    >
                      <Check className="size-3" strokeWidth={3} />
                    </span>
                  </button>
                )
              })}
            </div>
          </div>
        </div>

        {/* footer actions */}
        <div className="flex items-center gap-2 border-t border-border/70 bg-muted/25 px-6 py-4">
          {isManage && (
            <button
              onClick={() => {
                onFlash?.(`Archived ${agent?.name}`)
                onClose()
              }}
              className="flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] font-medium text-[var(--danger)] transition-colors hover:bg-[var(--danger)]/10"
            >
              <Archive className="size-3.5" />
              Archive
            </button>
          )}
          <button
            onClick={submit}
            disabled={!canSubmit}
            className={cn(
              "ml-auto flex items-center gap-2 rounded-lg px-4 py-2 text-[13px] font-medium text-[var(--primary-foreground)] transition-all",
              canSubmit
                ? "bg-[var(--interactive)] hover:brightness-105 active:scale-[0.98]"
                : "cursor-not-allowed bg-muted text-muted-foreground/60",
            )}
          >
            {isManage ? <Settings2 className="size-4" /> : <Sparkles className="size-4" />}
            {isManage ? "Save changes" : "Create agent"}
            <kbd className="ml-1 hidden items-center gap-0.5 rounded bg-black/15 px-1 py-px font-mono text-[9.5px] opacity-80 sm:flex">
              <CornerDownLeft className="size-2.5" />⌘
            </kbd>
          </button>
        </div>
      </div>
    </div>
  )
}
