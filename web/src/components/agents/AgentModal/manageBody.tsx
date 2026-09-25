import { useState } from "react"
import { Loader2, FolderGit2, Dices, ImagePlus } from "lucide-react"
import { avatarUrl } from "@/lib/api"
import type { Agent } from "@/lib/types"
import { AgentAclSection } from "../../auth/AgentAclSection"
import { SessionVitals } from "../../shell/SessionVitals"
import { cn } from "@/lib/utils"
import type { Controller } from "./parts"
import { TABS, type TabId } from "./tabs"

/**
 * Manage-mode body — a ConfigPanel-style left rail (Identity / Model / Vitals) +
 * detail pane, replacing the old two-column grid. The rail mirrors the global
 * settings dialog's structure exactly; each pane is a small subcomponent so the
 * bodies stay within the P8 complexity budget.
 */
export function TabbedManageBody({ c }: { c: Controller }) {
  const [tab, setTab] = useState<TabId>("llm")
  return (
    <div className="grid min-h-0 flex-1 grid-cols-[190px_minmax(0,1fr)] overflow-hidden">
      {/* category rail */}
      <aside className="flex flex-col gap-0.5 border-r border-border/70 bg-muted/25 px-2.5 py-4">
        {TABS.map((t) => {
          const on = t.id === tab
          return (
            <button
              key={t.id}
              type="button"
              onClick={() => setTab(t.id)}
              className={cn(
                "group flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[12.5px] transition-colors",
                on
                  ? "card-shadow bg-card font-medium text-foreground"
                  : "text-foreground/75 hover:bg-muted/60",
              )}
            >
              <span
                className={cn(
                  "flex size-6 shrink-0 items-center justify-center rounded-md transition-colors",
                  on ? "bg-(--interactive)/15 text-(--interactive)" : "text-muted-foreground/70",
                )}
              >
                <t.icon className="size-[15px]" />
              </span>
              <span className="min-w-0 flex-1 truncate">{t.label}</span>
            </button>
          )
        })}
      </aside>

      {/* detail pane */}
      <main className="flex min-h-0 flex-col overflow-y-auto">
        {tab === "llm" && <LlmTab c={c} />}
        {tab === "vitals" && c.agent && <VitalsTab c={c} agentId={c.agent.id} />}
      </main>
    </div>
  )
}

// ── Model tab ─────────────────────────────────────────────────────────

/**
 * Agent image editor — the same avatar affordance the create/manage dialog
 * carries in its header, surfaced as a labelled settings field. The whole
 * upload path (file pick, DiceBear randomize, cache-bust) lives in
 * {@link useAgentModalActions}; this only renders it, so there is no logic
 * duplication (M141) — a manual pick and a random shuffle land the bytes
 * through the exact same mutation.
 *
 * The avatar wrapper is a native `<label>` over a visually-hidden file input:
 * activating it opens the picker with no ref, no onClick, keyboard-accessible
 * for free. The Dices badge is a SIBLING of the label (not a descendant) so
 * shuffling never also trips the label's file dialog.
 */
function AvatarField({
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
    <div className="flex flex-col gap-2">
      <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
        Agent image
      </span>
      <div className="flex items-center gap-3.5">
        <div className="relative size-16 shrink-0">
          <label
            htmlFor="agent-settings-avatar"
            title="Click to change the agent image"
            className="flex size-16 cursor-pointer items-center justify-center overflow-hidden rounded-2xl bg-(--signal)/14 text-(--signal) ring-1 ring-(--signal)/25 transition-opacity ring-inset hover:opacity-80"
          >
            {agent.hasAvatar ? (
              <img
                src={avatarUrl(agent.id, avatarBust || undefined)}
                alt={agent.name}
                className="size-16 rounded-2xl object-cover"
              />
            ) : (
              <ImagePlus className="size-6" />
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
          {/* Sibling of the label, so a shuffle never also opens the picker. */}
          <button
            type="button"
            onClick={onRandomizeAvatar}
            title="Shuffle a random image"
            aria-label="Shuffle a random image"
            className="absolute -right-1.5 -bottom-1.5 flex size-6 items-center justify-center rounded-full border border-border bg-card text-muted-foreground shadow-sm transition-colors hover:border-(--signal)/40 hover:text-(--signal)"
          >
            <Dices className="size-3" />
          </button>
        </div>
        <span className="text-[12px] leading-relaxed text-muted-foreground/70">
          Click the image to upload a PNG, JPG, GIF, WebP or SVG — or shuffle for a random one. It
          applies immediately, no save needed.
        </span>
      </div>
    </div>
  )
}

/** Name (rename) + realm preview + provider/model picker — the fields the
 *  footer's Save button persists (configure + rename). In the settings VIEW
 *  there is no footer, so that surface renders its own save bar beside this.
 *  The agent image editor leads the form (it commits on its own, immediately). */
export function LlmTab({ c }: { c: Controller }) {
  const { name, setName, realm } = c
  return (
    <div className="flex flex-col gap-5 px-6 py-5">
      {c.agent && (
        <AvatarField
          agent={c.agent}
          avatarBust={c.avatarBust}
          onAvatarChange={c.onAvatarChange}
          onRandomizeAvatar={c.onRandomizeAvatar}
        />
      )}
      <div className="flex flex-col gap-2">
        <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
          Agent name
        </span>
        <div className="group flex items-center gap-2.5 rounded-xl border border-border bg-card px-3.5 py-2.5 transition-colors focus-within:border-(--interactive)/70 focus-within:ring-2 focus-within:ring-(--interactive)/15">
          <FolderGit2 className="size-[18px] shrink-0 text-muted-foreground/55 transition-colors group-focus-within:text-(--interactive)" />
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="my-project"
            className="w-full bg-transparent text-[15px] font-medium text-foreground outline-none placeholder:font-normal placeholder:text-muted-foreground/45"
          />
        </div>
        <div className="flex items-center gap-1.5 pl-0.5 text-[11.5px]">
          <span className="text-muted-foreground/60">Realm</span>
          <span className="text-muted-foreground/40">·</span>
          <code className="rounded-md bg-muted/60 px-1.5 py-0.5 font-mono text-[11px] text-foreground/75">
            {realm}
          </code>
        </div>
      </div>
    </div>
  )
}

// ── Vitals tab ────────────────────────────────────────────────────────

/** Service vitals + (when auth is on) the per-agent ACL section. */
export function VitalsTab({ c, agentId }: { c: Controller; agentId: string }) {
  return (
    <div className="flex flex-col gap-5 px-6 py-5">
      <SessionVitals agentId={agentId} />
      {c.authEnabled && <AgentAclSection agentId={agentId} />}
    </div>
  )
}

// ── Identity tab removed (X525: Agora crate deleted) ──────────────────
