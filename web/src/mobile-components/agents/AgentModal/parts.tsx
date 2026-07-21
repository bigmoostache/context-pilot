import { useEffect, useRef } from "react"
import {
  Archive,
  CornerDownLeft,
  Dices,
  FolderGit2,
  Loader2,
  RefreshCw,
  Settings2,
  Sparkles,
  Wand2,
  X,
} from "lucide-react"
import type { Agent } from "@/lib/types"
import { avatarUrl } from "@/lib/api"
import { type ProviderDef } from "@/lib/support/models"
import { ModelPicker } from "../ModelPicker"
import { AgentAclSection } from "../../auth/AgentAclSection"
import { SessionVitals } from "../../shell/SessionVitals"
import { cn } from "@/lib/utils"

/** Everything the render subcomponents need — assembled in {@link AgentModal}. */
export interface Controller {
  isManage: boolean
  agent: Agent | undefined
  name: string
  setName: (v: string) => void
  providers: ProviderDef[]
  provId: string
  modelId: string
  setSel: (p: string, m: string) => void
  realm: string
  error: string | null
  saving: boolean
  pending: boolean
  canSubmit: boolean
  submit: () => void
  retire: () => void
  restart: () => void
  retireBusy: boolean
  restartBusy: boolean
  avatarBust: number
  onAvatarChange: (file: File) => void
  onRandomizeAvatar: () => void
  authEnabled: boolean
}

/** The submit button's label across the create/manage × idle/pending matrix. */
function submitLabel(pending: boolean, saving: boolean, isManage: boolean): string {
  if (pending) return saving ? "Saving…" : "Creating…"
  return isManage ? "Save changes" : "Create agent"
}

/**
 * Hero header — mobile twin of the desktop AgentModal header. Same avatar
 * file-picker <label> (manage mode) + dice randomize badge + title/close, sized
 * up for touch: the close button is a 36px tap target and press feedback swaps
 * `hover:` for `active:`.
 */
export function AgentModalHeader({
  isManage,
  agent,
  avatarBust,
  onAvatarChange,
  onRandomizeAvatar,
  onClose,
}: {
  isManage: boolean
  agent: Agent | undefined
  avatarBust: number
  onAvatarChange: (file: File) => void
  onRandomizeAvatar: () => void
  onClose: () => void
}) {
  return (
    <div className="relative flex items-start gap-3.5 border-b border-border/70 px-4 pt-5 pb-4">
      <div
        className="pointer-events-none absolute inset-0 opacity-[0.5]"
        style={{
          background: isManage
            ? "radial-gradient(120% 100% at 0% 0%, color-mix(in oklab, var(--signal) 16%, transparent), transparent 60%)"
            : "radial-gradient(120% 100% at 0% 0%, color-mix(in oklab, var(--interactive) 18%, transparent), transparent 60%)",
        }}
      />
      {isManage ? (
        <div className="relative flex size-11 shrink-0">
          <label
            htmlFor="agent-avatar-input"
            title="Tap to change avatar"
            className={cn(
              "flex size-11 cursor-pointer items-center justify-center overflow-hidden rounded-xl ring-1 transition-opacity ring-inset active:opacity-80",
              "bg-(--signal)/14 text-(--signal) ring-(--signal)/25",
            )}
          >
            {agent?.hasAvatar ? (
              <img
                src={avatarUrl(agent.id, avatarBust || undefined)}
                alt={agent.name}
                className="size-11 rounded-xl object-cover"
              />
            ) : (
              <Settings2 className="size-[22px]" />
            )}
          </label>
          {/* Dice badge — sibling of the label (NOT a descendant), so tapping it
              randomizes the avatar without triggering the label's file picker. */}
          <button
            type="button"
            onClick={onRandomizeAvatar}
            title="Randomize avatar"
            aria-label="Randomize avatar"
            className="absolute -right-1.5 -bottom-1.5 flex size-6 items-center justify-center rounded-full border border-border bg-card text-muted-foreground shadow-sm transition-colors active:border-(--signal)/40 active:text-(--signal)"
          >
            <Dices className="size-3" />
          </button>
        </div>
      ) : (
        <span className="relative flex size-11 shrink-0 items-center justify-center rounded-xl bg-(--interactive)/14 text-(--interactive) ring-1 ring-(--interactive)/25 ring-inset">
          <Wand2 className="size-[22px]" />
        </span>
      )}
      {isManage && (
        <input
          id="agent-avatar-input"
          type="file"
          accept="image/png,image/jpeg,image/gif,image/webp,image/svg+xml"
          className="sr-only"
          onChange={(e) => {
            const file = e.target.files?.[0]
            if (file) onAvatarChange(file)
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
        className="relative -mt-1 -mr-1 flex size-9 items-center justify-center rounded-md text-muted-foreground/55 transition-colors active:bg-muted/70 active:text-foreground"
        aria-label="Close"
      >
        <X className="size-5" />
      </button>
    </div>
  )
}

/**
 * Body — mobile twin. The desktop manage layout is a two-column grid
 * (form | vitals+ACL); on a phone there's no room, so it **stacks to a single
 * column** (form first, then vitals + ACL below). The name field is 16px to
 * defeat iOS focus-zoom.
 */
export function AgentModalBody({ c }: { c: Controller }) {
  const { isManage, agent, name, setName, realm, providers, provId, modelId, setSel } = c
  const nameRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    const t = window.setTimeout(() => nameRef.current?.focus(), 60)
    return () => window.clearTimeout(t)
  }, [])
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-4 py-5">
      <div className="flex flex-col gap-5">
        <div className="flex flex-col gap-2">
          <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
            Agent name
          </span>
          <div className="group flex items-center gap-2.5 rounded-xl border border-border bg-card px-3.5 py-3 transition-colors focus-within:border-(--interactive)/70 focus-within:ring-2 focus-within:ring-(--interactive)/15">
            <FolderGit2 className="size-[18px] shrink-0 text-muted-foreground/55 transition-colors group-focus-within:text-(--interactive)" />
            <input
              ref={nameRef}
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="my-project"
              className="w-full bg-transparent text-[16px] font-medium text-foreground outline-none placeholder:font-normal placeholder:text-muted-foreground/45"
            />
          </div>
          <div className="flex flex-wrap items-center gap-1.5 pl-0.5 text-[11.5px]">
            <span className="text-muted-foreground/60">Realm</span>
            <span className="text-muted-foreground/40">→</span>
            <code className="rounded-md bg-muted/60 px-1.5 py-0.5 font-mono text-[11px] text-foreground/75">
              {realm}
            </code>
            {!isManage && <span className="text-muted-foreground/45">· created automatically</span>}
          </div>
        </div>
        <div className="flex flex-col gap-2">
          <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
            Provider &amp; Model
          </span>
          <ModelPicker providers={providers} provider={provId} model={modelId} onChange={setSel} />
        </div>
      </div>
      {isManage && agent && (
        <div className="flex flex-col gap-5 border-t border-border/50 pt-5">
          <SessionVitals agentId={agent.id} />
          {c.authEnabled && <AgentAclSection agentId={agent.id} />}
        </div>
      )}
    </div>
  )
}

/** Footer — mobile twin. Retire + restart (manage) and the primary submit,
 *  full-height touch buttons; the ⌘↵ hint is hidden (no hardware keyboard). */
export function AgentModalFooter({ c }: { c: Controller }) {
  const { isManage, pending, saving, canSubmit } = c
  return (
    <div className="flex items-center gap-2 border-t border-border/70 bg-muted/25 px-4 py-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
      {isManage && (
        <button
          onClick={c.retire}
          disabled={pending}
          title="Stop the agent's process and move it to Retired. The realm folder is kept."
          className="flex items-center gap-1.5 rounded-lg px-3 py-2.5 text-[13px] font-medium text-(--danger) transition-colors active:bg-(--danger)/10 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Archive className={cn("size-4", c.retireBusy && "animate-pulse")} />
          Retire
        </button>
      )}
      {isManage && (
        <button
          onClick={c.restart}
          disabled={pending}
          title="Kill and respawn the agent's process from the current binary."
          className="flex items-center gap-1.5 rounded-lg px-3 py-2.5 text-[13px] font-medium text-foreground/80 transition-colors active:bg-muted/70 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <RefreshCw className={cn("size-4", c.restartBusy && "animate-spin")} />
          {c.restartBusy ? "…" : "Restart"}
        </button>
      )}
      <button
        onClick={c.submit}
        disabled={!canSubmit}
        className={cn(
          "ml-auto flex items-center gap-2 rounded-lg px-4 py-2.5 text-[14px] font-medium text-(--primary-foreground) transition-all",
          canSubmit
            ? "bg-(--interactive) active:scale-[0.98] active:brightness-105"
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
        {submitLabel(pending, saving, isManage)}
        <kbd className="ml-1 hidden items-center gap-0.5 rounded-sm bg-black/15 px-1 py-px font-mono text-[9.5px] opacity-80">
          <CornerDownLeft className="size-2.5" />⌘
        </kbd>
      </button>
    </div>
  )
}
