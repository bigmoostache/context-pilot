import { useEffect, useRef, useState } from "react"
import {
  Archive,
  CornerDownLeft,
  FolderGit2,
  Loader2,
  Settings2,
  Sparkles,
  Wand2,
  X,
} from "lucide-react"
import type { Agent } from "@/lib/types"
import { useCreateAgent, sendCommand } from "@/lib/live"
import { PROVIDERS, defaultModel, findModel, resolveFromApiName } from "@/lib/models"
import { ModelPicker } from "./ModelPicker"
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

  // Provider + model — resolve from the agent's current api model name (manage)
  // or fall back to persisted global defaults → registry defaults (create).
  const resolved = isManage && agent?.model ? resolveFromApiName(agent.model) : undefined
  const createDefault = (() => {
    if (isManage) return { p: resolved?.provider.id ?? PROVIDERS[0].id, m: resolved?.model.id ?? (defaultModel(PROVIDERS[0].id)?.id ?? PROVIDERS[0].models[0].id) }
    const lsP = localStorage.getItem("cp-default-provider") ?? PROVIDERS[0].id
    const lsM = localStorage.getItem("cp-default-model") ?? (defaultModel(lsP)?.id ?? PROVIDERS[0].models[0].id)
    return { p: lsP, m: lsM }
  })()
  const [provId, setProvId] = useState(createDefault.p)
  const [modelId, setModelId] = useState(createDefault.m)
  const nameRef = useRef<HTMLInputElement>(null)

  // Realm folder: in create mode it's derived live from the name (no picker);
  // in manage mode it's the agent's fixed, read-only realm.
  const realm = isManage ? (agent?.folder ?? "") : `~/code/${slugify(name)}`

  const createAgent = useCreateAgent()
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const pending = createAgent.isPending || saving
  const canSubmit = (isManage || name.trim().length > 0) && !pending

  const submit = () => {
    if (!canSubmit) return
    if (isManage && agent) {
      setSaving(true)
      setError(null)
      sendCommand(agent.id, { kind: "configure", provider: provId, model: modelId })
        .then(() => {
          onFlash?.(`Model updated to ${findModel(provId, modelId)?.displayName ?? modelId}`)
          onClose()
        })
        .catch((e: unknown) => {
          setError(e instanceof Error ? e.message : "Failed to update model")
        })
        .finally(() => setSaving(false))
      return
    }
    setError(null)
    createAgent.mutate(
      { name: name.trim(), model: findModel(provId, modelId)?.apiName },
      {
        onSuccess: (receipt) => {
          onFlash?.(`Spawning “${slugify(name)}” in ${receipt.folder}`)
          onClose()
        },
        onError: (e) => {
          setError(
            e instanceof Error ? e.message : "Could not create the agent. Please try again.",
          )
        },
      },
    )
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
  }, [name, provId, modelId, isManage])

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

          {/* provider + model — two-level picker like the TUI's Ctrl+H */}
          <div className="flex flex-col gap-2">
            <span className="text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
              Provider &amp; Model
            </span>
            <ModelPicker
              provider={provId}
              model={modelId}
              onChange={(p, m) => { setProvId(p); setModelId(m) }}
            />
          </div>
        </div>

        {/* create error — surfaced inline so a spawn failure isn't silent */}
        {error && (
          <div
            role="alert"
            className="mx-6 mb-1 flex items-start gap-2 rounded-lg border border-[var(--danger)]/30 bg-[var(--danger)]/10 px-3 py-2 text-[11.5px] leading-snug text-[var(--danger)]"
          >
            <X className="mt-px size-3.5 shrink-0" />
            <span>{error}</span>
          </div>
        )}

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
            {pending ? (
              <Loader2 className="size-4 animate-spin" />
            ) : isManage ? (
              <Settings2 className="size-4" />
            ) : (
              <Sparkles className="size-4" />
            )}
            {pending ? (saving ? "Saving…" : "Creating…") : isManage ? "Save changes" : "Create agent"}
            <kbd className="ml-1 hidden items-center gap-0.5 rounded bg-black/15 px-1 py-px font-mono text-[9.5px] opacity-80 sm:flex">
              <CornerDownLeft className="size-2.5" />⌘
            </kbd>
          </button>
        </div>
      </div>
    </div>
  )
}
