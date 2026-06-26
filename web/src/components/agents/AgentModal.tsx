import { useEffect, useMemo, useRef, useState } from "react"
import { useQuery } from "@tanstack/react-query"
import {
  Archive,
  CornerDownLeft,
  FolderGit2,
  Loader2,
  RefreshCw,
  Settings2,
  Sparkles,
  Wand2,
  X,
} from "lucide-react"
import type { Agent } from "@/lib/types"
import { useCreateAgent, useRenameAgent, useRestartAgent, useRetireAgent, useUploadAvatar, sendCommand } from "@/lib/live"
import { avatarUrl, fetchSettings } from "@/lib/api"
import { useAuth } from "@/lib/support/auth"
import { useProviders, defaultModel, findModel, resolveSelection, filterAllowed } from "@/lib/support/models"
import { ModelPicker } from "./ModelPicker"
import { AgentAclSection } from "../auth/AgentAclSection"
import { SessionVitals } from "../shell/SessionVitals"
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

  // The org allowlist constrains which models can be picked (empty ⇒ all).
  const { data: allProviders = [] } = useProviders()
  const { data: settings } = useQuery({ queryKey: ["settings"], queryFn: fetchSettings })
  const providers = useMemo(
    () => filterAllowed(allProviders, settings?.allowed_models ?? []),
    [allProviders, settings],
  )

  // Provider + model — resolve from the agent's authoritative provider id +
  // current api model name (manage) or fall back to persisted global defaults →
  // registry defaults (create). The provider id is decisive: several providers
  // share model api names, so name-only resolution would mislabel (e.g. a
  // Claude Code V2 agent showing as Anthropic).
  const resolved =
    isManage && agent ? resolveSelection(providers, agent.provider, agent.model) : undefined
  const createDefault = (() => {
    if (isManage) return { p: resolved?.provider.id ?? providers[0]?.id ?? "", m: resolved?.model.id ?? (defaultModel(providers, providers[0]?.id ?? "")?.id ?? providers[0]?.models[0]?.id ?? "") }
    const lsP = localStorage.getItem("cp-default-provider") ?? providers[0]?.id ?? ""
    const lsM = localStorage.getItem("cp-default-model") ?? (defaultModel(providers, lsP)?.id ?? providers[0]?.models[0]?.id ?? "")
    return { p: lsP, m: lsM }
  })()
  const [provId, setProvId] = useState(createDefault.p)
  const [modelId, setModelId] = useState(createDefault.m)
  const nameRef = useRef<HTMLInputElement>(null)

  // Realm folder: in create mode it's derived live from the name (no picker);
  // in manage mode it's the agent's fixed, read-only realm.
  const realm = isManage ? (agent?.folder ?? "") : `~/code/${slugify(name)}`

  const createAgent = useCreateAgent()
  const restartAgent = useRestartAgent()
  const retireAgent = useRetireAgent()
  const renameAgent = useRenameAgent()
  const uploadAvatar = useUploadAvatar()
  const avatarInputRef = useRef<HTMLInputElement>(null)
  const [avatarBust, setAvatarBust] = useState(0)
  const { authEnabled } = useAuth()
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const pending = createAgent.isPending || saving || restartAgent.isPending || retireAgent.isPending
  const canSubmit = (isManage || name.trim().length > 0) && !pending

  /** Restart a (possibly stale-binary) agent so a fresh process can accept
   *  commands the old binary rejected with `502 agent unreachable`. */
  const restart = () => {
    if (!agent || restartAgent.isPending) return
    setError(null)
    restartAgent.mutate(agent.id, {
      onSuccess: () => {
        onFlash?.(`Restarting ${agent.name} — it will reconnect in a moment`)
        onClose()
      },
      onError: (e) => {
        setError(e instanceof Error ? e.message : "Could not restart the agent.")
      },
    })
  }

  /** Retire (archive) the agent: stop its process + console server, keep its
   *  folder, and move it to the dashboard's Retired section. Reversible. */
  const retire = () => {
    if (!agent || retireAgent.isPending) return
    setError(null)
    retireAgent.mutate(agent.id, {
      onSuccess: () => {
        onFlash?.(`Retired ${agent.name} — moved to the Retired section`)
        onClose()
      },
      onError: (e) => {
        setError(e instanceof Error ? e.message : "Could not retire the agent.")
      },
    })
  }

  const submit = () => {
    if (!canSubmit) return
    if (isManage && agent) {
      setSaving(true)
      setError(null)
      const tasks: Promise<unknown>[] = [
        sendCommand(agent.id, { kind: "configure", provider: provId, model: modelId }),
      ]
      const nameChanged = name.trim() !== (agent.name ?? "")
      if (nameChanged) {
        tasks.push(renameAgent.mutateAsync({ agentId: agent.id, name: name.trim() }))
      }
      Promise.all(tasks)
        .then(() => {
          onFlash?.(nameChanged
            ? `Saved changes to ${name.trim()}`
            : `Model updated to ${findModel(providers, provId, modelId)?.displayName ?? modelId}`)
          onClose()
        })
        .catch((e: unknown) => {
          setError(e instanceof Error ? e.message : "Failed to save changes")
        })
        .finally(() => setSaving(false))
      return
    }
    setError(null)
    createAgent.mutate(
      { name: name.trim(), model: findModel(providers, provId, modelId)?.apiName },
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
        className={cn(
          "modal-pop relative flex max-h-[calc(100vh-3rem)] flex-col overflow-hidden rounded-2xl border border-border bg-popover pop-shadow",
          isManage ? "w-[960px] max-w-[calc(100vw-3rem)]" : "w-[460px]",
        )}
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
              isManage && "cursor-pointer overflow-hidden transition-opacity hover:opacity-80",
            )}
            onClick={isManage ? () => avatarInputRef.current?.click() : undefined}
            title={isManage ? "Click to change avatar" : undefined}
          >
            {isManage && agent?.hasAvatar ? (
              <img
                src={avatarUrl(agent.id, avatarBust || undefined)}
                alt={agent.name}
                className="size-11 rounded-xl object-cover"
              />
            ) : isManage ? (
              <Settings2 className="size-[22px]" />
            ) : (
              <Wand2 className="size-[22px]" />
            )}
          </span>
          {isManage && (
            <input
              ref={avatarInputRef}
              type="file"
              accept="image/png,image/jpeg,image/gif,image/webp,image/svg+xml"
              className="hidden"
              onChange={(e) => {
                const file = e.target.files?.[0]
                if (!file || !agent) return
                uploadAvatar.mutate({ agentId: agent.id, file }, {
                  onSuccess: () => setAvatarBust(Date.now()),
                  onError: (err) => setError(err instanceof Error ? err.message : "Avatar upload failed"),
                })
                e.target.value = ""
              }}
            />
          )}
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

        <div
          className={cn(
            "min-h-0 flex-1 overflow-y-auto px-6 py-5",
            isManage ? "grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-7" : "flex flex-col gap-5",
          )}
        >
          {/* left column — the agent form (name + provider/model) */}
          <div className="flex flex-col gap-5">
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
              providers={providers}
              provider={provId}
              model={modelId}
              onChange={(p, m) => { setProvId(p); setModelId(m) }}
            />
          </div>
          </div>

          {/* right column — vitals (always) + ACL (auth only) */}
          {isManage && agent && (
            <div className="flex flex-col gap-5 border-l border-border/50 pl-7">
              <SessionVitals agentId={agent.id} />
              {authEnabled && <AgentAclSection agentId={agent.id} />}
            </div>
          )}
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
              onClick={retire}
              disabled={pending}
              title="Stop the agent's process and console server and move it to the Retired section. The realm folder is kept — unretire brings it back."
              className="flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] font-medium text-[var(--danger)] transition-colors hover:bg-[var(--danger)]/10 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Archive className={cn("size-3.5", retireAgent.isPending && "animate-pulse")} />
              Retire
            </button>
          )}
          {isManage && (
            <button
              onClick={restart}
              disabled={pending}
              title="Kill and respawn the agent's process from the current binary — fixes a stale agent that rejects commands with 'agent unreachable'."
              className="flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] font-medium text-foreground/80 transition-colors hover:bg-muted/70 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <RefreshCw
                className={cn("size-3.5", restartAgent.isPending && "animate-spin")}
              />
              Restart
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
