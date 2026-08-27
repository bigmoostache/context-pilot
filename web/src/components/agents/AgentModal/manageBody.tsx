import { useState } from "react"
import { Loader2, FolderGit2, ScrollText, Dices, ImagePlus } from "lucide-react"
import { useIdentity, sendCommand } from "@/lib/live"
import { avatarUrl, type AgentIdentity } from "@/lib/api"
import type { Agent } from "@/lib/types"
import { ModelPicker } from "../ModelPicker"
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
        {tab === "identity" && c.agent && <IdentityTab agentId={c.agent.id} />}
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
  const { name, setName, realm, providers, provId, modelId, setSel } = c
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
      <div className="flex flex-col gap-2">
        <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
          Provider &amp; Model
        </span>
        <ModelPicker providers={providers} provider={provId} model={modelId} onChange={setSel} />
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

// ── Identity tab ──────────────────────────────────────────────────────

/** The ten identity fields in canonical order (mirrors cp-agora types.rs and
 *  the `Agora_set_identity` tool params 1:1). */
const IDENTITY_FIELDS: { key: keyof AgentIdentity; label: string }[] = [
  { key: "identity", label: "Identity" },
  { key: "values", label: "Values" },
  { key: "principles", label: "Principles" },
  { key: "character", label: "Character" },
  { key: "expertise", label: "Expertise" },
  { key: "role", label: "Role" },
  { key: "operational_responsibilities", label: "Operational responsibilities" },
  { key: "knowledge_responsibilities", label: "Knowledge responsibilities" },
  { key: "organic_responsibilities", label: "Organic responsibilities" },
  { key: "direct_management", label: "Direct management" },
]

/**
 * Editable self-identity form — the frontend twin of the agent's own
 * `Agora_set_identity` tool. Seeds from {@link useIdentity} (the tier-②
 * config.json read invalidated by the `identity_changed` SSE delta), then
 * saves through the shared `set_identity` bridge command (fire-and-forget: the
 * agent re-validates the per-value word cap and the SSE delta refetches). The
 * borderless field styling mirrors {@link AgentEditorDialog}.
 */
export function IdentityTab({ agentId }: { agentId: string }) {
  const { data } = useIdentity(agentId)
  const [form, setForm] = useState<AgentIdentity | null>(null)
  // Fingerprint of the last server snapshot the form adopted. Lets the form
  // track live server changes while pristine, without an effect.
  const [serverSnapshot, setServerSnapshot] = useState<string | null>(null)
  const [dirty, setDirty] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Track the server identity while the form is PRISTINE — a render-phase
  // adjust-state (NOT an effect, which would trip set-state-in-effect). The
  // SSE identity_changed delta invalidates qk.identity and refetches `data`;
  // as long as the user hasn't started editing (`!dirty`) the form mirrors it,
  // so an external change (an AI tool edit) shows live. Once the user types,
  // `dirty` freezes the form so in-progress edits are never clobbered; a save
  // clears `dirty` and the next server snapshot re-syncs.
  if (data) {
    const fp = JSON.stringify(data)
    if (!dirty && fp !== serverSnapshot) {
      setServerSnapshot(fp)
      setForm(data)
    }
  }

  if (!form) {
    return (
      <div className="flex flex-1 items-center justify-center gap-2 py-16 text-muted-foreground">
        <Loader2 className="size-4 animate-spin" /> Loading…
      </div>
    )
  }

  const set = (k: keyof AgentIdentity, v: string) => {
    setDirty(true)
    setForm((f) => (f ? { ...f, [k]: v } : f))
  }

  const save = () => {
    if (saving) return
    setSaving(true)
    setError(null)
    sendCommand(agentId, { kind: "set_identity", ...form })
      .then(() => setDirty(false))
      .catch((e: unknown) =>
        setError(e instanceof Error ? e.message : "Could not save the identity."),
      )
      .finally(() => setSaving(false))
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-1.5 px-5 pt-4 pb-1 text-[12px] text-muted-foreground">
        <span className="flex size-[15px] items-center justify-center rounded-sm bg-(--signal)/15 text-(--signal)">
          <ScrollText className="size-2.5" />
        </span>
        <span className="text-foreground/70">Self-identity</span>
        <span className="text-muted-foreground/50">· how the agent sees itself</span>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-3.5 px-5 pt-2 pb-4">
        {IDENTITY_FIELDS.map((f) => (
          <label key={f.key} className="flex flex-col gap-1">
            <span className="text-[10.5px] font-semibold tracking-[0.06em] text-muted-foreground/75 uppercase">
              {f.label}
            </span>
            <input
              value={form[f.key]}
              onChange={(e) => set(f.key, e.target.value)}
              placeholder="—"
              className="w-full border-b border-transparent bg-transparent pb-1 text-[13.5px] text-foreground/90 transition-colors outline-none placeholder:text-muted-foreground/40 focus:border-(--signal)/40"
            />
          </label>
        ))}
        {error && <span className="text-[11px] text-(--danger)">{error}</span>}
      </div>
      <div className="sticky bottom-0 flex items-center gap-3 border-t border-border/70 bg-popover/95 px-5 py-3 backdrop-blur-sm">
        <span className="text-[11px] text-muted-foreground/70">
          Keep each value short — under ten words.
        </span>
        <button
          type="button"
          onClick={save}
          disabled={saving}
          className="ml-auto flex items-center gap-1.5 rounded-md bg-(--signal) px-3.5 py-1.5 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {saving && <Loader2 className="size-3.5 animate-spin" />}
          Save identity
        </button>
      </div>
    </div>
  )
}
