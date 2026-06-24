import { useState } from "react"
import { useQuery } from "@tanstack/react-query"
import {
  Bot,
  Building2,
  Check,
  CheckCircle2,
  Coins,
  Cpu,
  Database,
  Eye,
  EyeOff,
  FileText,
  Gauge,
  Globe,
  KeyRound,
  Boxes,
  Loader2,
  Lock,
  Pencil,
  Search,
  Send,
  ShieldCheck,
  Sliders,
  Sparkles,
  Zap,
} from "lucide-react"
import { UsagePage } from "@/components/agents/UsagePage"
import { ModelPicker } from "@/components/agents/ModelPicker"
import { PROVIDERS, defaultModel as getDefaultModel, findModel } from "@/lib/support/models"
import { useFleet, sendCommand } from "@/lib/live"
import { fetchEnvKeys, revealEnvKey, updateEnvKey } from "@/lib/api"
import { useAccount } from "@/lib/support/account"
import { useAuth } from "@/lib/support/auth"
import { useDevMode } from "@/lib/support/devMode"
import { cn } from "@/lib/utils"

// ── localStorage keys for global defaults ─────────────────────────────
const LS_DEFAULT_PROVIDER = "cp-default-provider"
const LS_DEFAULT_MODEL = "cp-default-model"

/** Read the persisted default provider+model, with registry fallbacks. */
function readDefaults(): { provider: string; model: string } {
  const p = localStorage.getItem(LS_DEFAULT_PROVIDER) ?? PROVIDERS[0].id
  const m = localStorage.getItem(LS_DEFAULT_MODEL) ?? (getDefaultModel(p)?.id ?? PROVIDERS[0].models[0].id)
  return { provider: p, model: m }
}

/** Persist the default provider+model to localStorage. */
function writeDefaults(provider: string, model: string) {
  localStorage.setItem(LS_DEFAULT_PROVIDER, provider)
  localStorage.setItem(LS_DEFAULT_MODEL, model)
}

// ── categories ────────────────────────────────────────────────────
export type CatId = "general" | "usage" | "providers" | "search" | "docai" | "web" | "integrations"

export const CATEGORIES: {
  id: CatId
  label: string
  blurb: string
  icon: typeof Sliders
  count?: number
}[] = [
  { id: "general", label: "General", blurb: "Defaults & autonomy", icon: Sliders },
  { id: "usage", label: "Usage & Cost", blurb: "Spend & token analytics", icon: Coins },
  { id: "providers", label: "Model Providers", blurb: "LLM backends & keys", icon: Bot, count: 6 },
  { id: "search", label: "Search & Embeddings", blurb: "Indexing & vectors", icon: Search },
  { id: "docai", label: "Document AI", blurb: "OCR & extraction", icon: FileText },
  { id: "web", label: "Web & Scraping", blurb: "Search & crawl", icon: Globe, count: 2 },
  { id: "integrations", label: "Integrations", blurb: "Git & external services", icon: Boxes },
]

// ── per-category bodies ───────────────────────────────────────────
export function CategoryBody({ cat }: { cat: CatId }) {
  // Two independent reasons a key pane can be read-only:
  //   • company-managed account — the org provisions keys centrally;
  //   • auth active + caller is NOT a system administrator — only admins
  //     manage provider/API keys (T346). Regular users still SEE the panes
  //     (greyed) so they know the integration exists.
  // The admin reason takes precedence in the copy when both apply.
  const { user } = useAccount()
  const { user: authUser, authEnabled } = useAuth()
  const companyManaged = user.managedByCompany
  const adminLocked = authEnabled === true && authUser !== null && authUser.role !== "admin"
  const locked = companyManaged || adminLocked
  const reason: LockReason = adminLocked ? "admin" : "company"
  const company = user.company ?? "your organization"

  // Live env-key status from the orchestrator (T399).
  const { data: envKeys } = useQuery({ queryKey: ["env-keys"], queryFn: fetchEnvKeys })
  const ks = (env: string): "connected" | "missing" =>
    envKeys?.find((k) => k.env === env)?.exists ? "connected" : "missing"

  switch (cat) {
    case "general":
      return <GeneralPane />
    case "usage":
      return <UsagePage />
    case "providers":
      return (
        <Stack>
          {locked && <ManagedKeysNotice reason={reason} company={company} />}
          <KeyRow i={0} name="Anthropic" env="ANTHROPIC_API_KEY" icon={Sparkles} status={ks("ANTHROPIC_API_KEY")} hint="Claude 4 family" managed={locked} reason={reason} company={company} />
          <KeyRow i={1} name="Claude Code (OAuth)" env="Keychain · ~/.claude" icon={Cpu} status="connected" hint="opus-4-8 · sonnet-4-6 · fable-5" managed={locked} reason={reason} company={company} noReveal />
          <KeyRow i={2} name="Grok (xAI)" env="XAI_API_KEY" icon={Zap} status={ks("XAI_API_KEY")} hint="grok-4" managed={locked} reason={reason} company={company} />
          <KeyRow i={3} name="Groq" env="GROQ_API_KEY" icon={Gauge} status={ks("GROQ_API_KEY")} hint="Llama 3.x · fast" managed={locked} reason={reason} company={company} />
          <KeyRow i={4} name="DeepSeek" env="DEEPSEEK_API_KEY" icon={Bot} status={ks("DEEPSEEK_API_KEY")} hint="deepseek-chat / reasoner" managed={locked} reason={reason} company={company} />
          <KeyRow i={5} name="MiniMax" env="MINIMAX_API_KEY" icon={Bot} status={ks("MINIMAX_API_KEY")} hint="Token Plan" managed={locked} reason={reason} company={company} />
        </Stack>
      )
    case "search":
      return (
        <Stack>
          {locked && <ManagedKeysNotice reason={reason} company={company} />}
          <KeyRow i={0} name="Voyage AI" env="VOYAGE_API_KEY" icon={Database} status={ks("VOYAGE_API_KEY")} hint="voyage-code-3 · 1024-dim embeddings" managed={locked} reason={reason} company={company} />
          <StatusRow i={1} name="Meilisearch" icon={Search} state="Running" detail="Embedded server · 6 417 chunks · port 49286" />
          <ToggleRow i={2} name="Hybrid semantic search" detail="Blend keyword + vector results" on />
        </Stack>
      )
    case "docai":
      return (
        <Stack>
          {locked && <ManagedKeysNotice reason={reason} company={company} />}
          <KeyRow i={0} name="Datalab" env="DATALAB_API_KEY" icon={FileText} status={ks("DATALAB_API_KEY")} hint="Surya OCR · PDF / image → markdown" managed={locked} reason={reason} company={company} />
          <ToggleRow i={1} name="Cache OCR results" detail="~/.context-pilot/ocr-cache" on />
        </Stack>
      )
    case "web":
      return (
        <Stack>
          {locked && <ManagedKeysNotice reason={reason} company={company} />}
          <KeyRow i={0} name="Brave Search" env="BRAVE_API_KEY" icon={Globe} status={ks("BRAVE_API_KEY")} hint="Independent 40-B index" managed={locked} reason={reason} company={company} />
          <KeyRow i={1} name="Firecrawl" env="FIRECRAWL_API_KEY" icon={Globe} status={ks("FIRECRAWL_API_KEY")} hint="Scrape · search · crawl" managed={locked} reason={reason} company={company} />
        </Stack>
      )
    case "integrations":
      return (
        <Stack>
          {locked && <ManagedKeysNotice reason={reason} company={company} />}
          <KeyRow i={0} name="GitHub" env="GITHUB_TOKEN" icon={Boxes} status={ks("GITHUB_TOKEN")} hint="PRs · issues · gh CLI" managed={locked} reason={reason} company={company} />
        </Stack>
      )
  }
}

/** Why a key pane is read-only — drives the lock copy. */
type LockReason = "company" | "admin"

/** Banner shown atop key-bearing panes when the keys are read-only. */
function ManagedKeysNotice({ reason, company }: { reason: LockReason; company: string }) {
  const admin = reason === "admin"
  return (
    <div className="flex items-start gap-3 rounded-xl border border-[var(--interactive)]/30 bg-[var(--interactive)]/[0.06] px-3.5 py-3">
      <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-[var(--interactive)]/14 text-[var(--interactive)]">
        {admin ? <ShieldCheck className="size-4" /> : <Building2 className="size-4" />}
      </span>
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[12.5px] font-semibold text-foreground/90">{admin ? "Reserved to Administrators" : `Keys managed by ${company}`}</span>
        <span className="text-[11.5px] leading-relaxed text-muted-foreground">{admin ? "Only system administrators can manage provider and API keys. Contact an administrator to change a key." : "API keys are provisioned centrally by your organization and can't be edited here. Contact your administrator to change a provider key."}</span>
      </div>
    </div>
  )
}

function GeneralPane() {
  const defaults = readDefaults()
  const [provId, setProvId] = useState(defaults.provider)
  const [modelId, setModelId] = useState(defaults.model)

  const fleet = useFleet()
  const [applying, setApplying] = useState(false)
  const [applyResult, setApplyResult] = useState<string | null>(null)

  const handleChange = (p: string, m: string) => {
    setProvId(p)
    setModelId(m)
    writeDefaults(p, m)
    setApplyResult(null) // clear stale result when selection changes
  }

  const applyToAll = async () => {
    const agents = fleet.data ?? []
    if (agents.length === 0) return
    setApplying(true)
    setApplyResult(null)
    let ok = 0
    let fail = 0
    for (const a of agents) {
      try {
        await sendCommand(a.id, { kind: "configure", provider: provId, model: modelId })
        ok++
      } catch {
        fail++
      }
    }
    setApplying(false)
    const label = findModel(provId, modelId)?.displayName ?? modelId
    setApplyResult(
      fail === 0
        ? `Applied ${label} to ${ok} agent${ok === 1 ? "" : "s"}`
        : `Applied to ${ok}, failed ${fail}`,
    )
  }

  return (
    <Stack>
      <FieldGroup label="Default model" hint="Used for new agents unless overridden">
        <ModelPicker provider={provId} model={modelId} onChange={handleChange} />
        {/* Apply to all existing agents */}
        <div className="mt-1 flex items-center gap-2">
          <button
            onClick={() => void applyToAll()}
            disabled={applying || !fleet.data?.length}
            className={cn(
              "flex items-center gap-1.5 rounded-lg border px-3 py-2 text-[12px] font-medium transition-all",
              applying
                ? "cursor-wait border-border bg-muted text-muted-foreground"
                : "border-[var(--interactive)]/30 bg-[var(--interactive)]/[0.06] text-[var(--interactive)] hover:bg-[var(--interactive)]/[0.12]",
            )}
          >
            {applying ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <Send className="size-3.5" />
            )}
            {applying ? "Applying…" : "Apply to all existing agents"}
          </button>
          {applyResult && (
            <span className="flex items-center gap-1 text-[11px] text-[var(--ok)]">
              <CheckCircle2 className="size-3.5" />
              {applyResult}
            </span>
          )}
        </div>
      </FieldGroup>

      <ToggleRow i={0} name="Auto-continuation" detail="Let the agent keep working without a nudge" />
      <ToggleRow i={1} name="Reverie (context optimizer)" detail="Background cleaner reshapes context when it grows" on />
      <ToggleRow i={2} name="Think reminders" detail="Periodic nudge to reason before acting" on />
      <DevModeToggle i={3} />
    </Stack>
  )
}

/**
 * The one **functional** General-pane toggle (T301): the global dev-mode flag,
 * persisted via {@link useDevMode}. Unlike the decorative sibling rows it is a
 * real controlled switch — flipping it reveals/hides the developer-only Cockpit
 * tab in the TopBar in real time (App gates the view on the same flag). Off by
 * default.
 */
function DevModeToggle({ i }: { i: number }) {
  const { devMode, setDevMode } = useDevMode()
  return (
    <ToggleRow
      i={i}
      name="Developer mode"
      detail="Reveal the Cockpit — the agent's live context-panel inspector"
      value={devMode}
      onChange={setDevMode}
    />
  )
}

// ── building blocks ───────────────────────────────────────────────
function Stack({ children }: { children: React.ReactNode }) {
  return <div className="flex flex-col gap-2.5">{children}</div>
}

function FieldGroup({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <div className="flex flex-col gap-2 pb-1">
      <div className="flex items-baseline gap-2">
        <span className="text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">{label}</span>
        {hint && <span className="text-[11px] text-muted-foreground/60">{hint}</span>}
      </div>
      {children}
    </div>
  )
}

function KeyRow({
  i,
  name,
  env,
  icon: Icon,
  status,
  hint,
  managed = false,
  noReveal = false,
  reason = "company",
  company,
}: {
  i: number
  name: string
  env: string
  icon: typeof Bot
  status: "connected" | "missing"
  hint: string
  managed?: boolean
  noReveal?: boolean
  reason?: LockReason
  company?: string
}) {
  const connected = status === "connected"
  const [revealedValue, setRevealedValue] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [editing, setEditing] = useState(false)
  const [editValue, setEditValue] = useState("")
  const [saving, setSaving] = useState(false)

  /** Fetch the full key on demand, or toggle it off. */
  const handleReveal = () => {
    if (revealedValue) { setRevealedValue(null); setEditing(false); return }
    setLoading(true)
    void revealEnvKey(env)
      .then((r) => setRevealedValue(r.value ?? r.masked ?? ""))
      .catch(() => setRevealedValue("reveal failed"))
      .finally(() => setLoading(false))
  }

  const handleStartEdit = () => { setEditValue(revealedValue ?? ""); setEditing(true) }
  const handleSave = () => {
    setSaving(true)
    void updateEnvKey(env, editValue)
      .then((r) => { setRevealedValue(r.value ?? editValue); setEditing(false) })
      .catch(() => setEditing(false))
      .finally(() => setSaving(false))
  }

  return (
    <div
      style={{ animationDelay: `${i * 40}ms` }}
      className="opt-rise flex flex-col gap-2 rounded-xl border border-border bg-card px-3.5 py-3 card-shadow"
    >
      <div className="flex items-center gap-2.5">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-muted/60 text-muted-foreground/75">
          <Icon className="size-4" />
        </span>
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[13px] font-medium text-foreground/90">{name}</span>
          <span className="truncate text-[11px] text-muted-foreground">{hint}</span>
        </div>
        {managed && connected ? <ManagedPill /> : <StatusPill connected={connected} />}
      </div>
      <div
        className={cn(
          "flex items-center gap-2 rounded-lg border border-border px-2.5 py-1.5",
          managed && connected ? "bg-muted/40" : "bg-background/60",
        )}
      >
        <KeyRound className="size-3.5 shrink-0 text-muted-foreground/55" />
        {editing ? (
          <input
            value={editValue}
            onChange={(e) => setEditValue(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") handleSave(); if (e.key === "Escape") setEditing(false) }}
            className="min-w-0 flex-1 bg-transparent font-mono text-[11.5px] text-foreground/90 outline-none placeholder:text-muted-foreground/40"
            placeholder="Enter value…"
            autoFocus
          />
        ) : (
          <code className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-foreground/75">
            {connected ? (revealedValue ?? <span className="text-muted-foreground/45">•••••••••••••••</span>) : <span className="text-muted-foreground/45">not configured</span>}
          </code>
        )}
        <span className="shrink-0 rounded bg-muted/70 px-1.5 py-px font-mono text-[9.5px] text-muted-foreground/70">{env}</span>
        {editing ? (
          <button onClick={handleSave} disabled={saving} className="shrink-0 text-[var(--interactive)] transition-colors hover:text-[var(--interactive)]/80 disabled:opacity-50" aria-label="Save">
            {saving ? <Loader2 className="size-3.5 animate-spin" /> : <Check className="size-3.5" strokeWidth={3} />}
          </button>
        ) : connected && !noReveal &&
          (managed ? (
            <Lock className="size-3.5 shrink-0 text-muted-foreground/50" aria-label="Locked by organization" />
          ) : (<>
            {revealedValue && (
              <button onClick={handleStartEdit} className="shrink-0 text-muted-foreground/55 transition-colors hover:text-foreground" aria-label="Edit">
                <Pencil className="size-3.5" />
              </button>
            )}
            <button onClick={handleReveal} disabled={loading} className="shrink-0 text-muted-foreground/55 transition-colors hover:text-foreground disabled:opacity-50" aria-label={revealedValue ? "Hide" : "Reveal"}>
              {loading ? <Loader2 className="size-3.5 animate-spin" /> : revealedValue ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
            </button>
          </>))}
      </div>
      {managed && connected && (
        <span className="flex items-center gap-1 pl-0.5 text-[10.5px] text-muted-foreground/65">
          <Lock className="size-3" />
          {reason === "admin" ? "Reserved to administrators — contact an administrator to change." : `Managed by ${company ?? "your organization"} — contact your administrator to change.`}
        </span>
      )}
    </div>
  )
}

function StatusRow({
  i,
  name,
  icon: Icon,
  state,
  detail,
}: {
  i: number
  name: string
  icon: typeof Bot
  state: string
  detail: string
}) {
  return (
    <div
      style={{ animationDelay: `${i * 40}ms` }}
      className="opt-rise flex items-center gap-2.5 rounded-xl border border-border bg-card px-3.5 py-3 card-shadow"
    >
      <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-muted/60 text-muted-foreground/75">
        <Icon className="size-4" />
      </span>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[13px] font-medium text-foreground/90">{name}</span>
        <span className="truncate text-[11px] text-muted-foreground">{detail}</span>
      </div>
      <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full bg-[var(--interactive)]/12 px-2 py-0.5 text-[10.5px] font-medium text-[var(--interactive)]">
        <span className="size-1.5 animate-pulse rounded-full bg-[var(--interactive)]" />
        {state}
      </span>
    </div>
  )
}

function ToggleRow({
  i,
  name,
  detail,
  on: initial = false,
  value,
  onChange,
}: {
  i: number
  name: string
  detail: string
  on?: boolean
  /** when provided, the row is CONTROLLED — `value` is the source of truth and
   *  `onChange` is fired on toggle (no internal state). Used by the functional
   *  dev-mode row; the decorative rows omit it and keep local state. */
  value?: boolean
  onChange?: (next: boolean) => void
}) {
  const [localOn, setLocalOn] = useState(initial)
  const controlled = value !== undefined
  const on = controlled ? value : localOn
  const handleToggle = () => {
    if (controlled) onChange?.(!on)
    else setLocalOn((v) => !v)
  }
  return (
    <button
      onClick={handleToggle}
      style={{ animationDelay: `${i * 40}ms` }}
      className="opt-rise flex items-center gap-2.5 rounded-xl border border-border bg-card px-3.5 py-3 text-left card-shadow"
    >
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[13px] font-medium text-foreground/90">{name}</span>
        <span className="truncate text-[11px] text-muted-foreground">{detail}</span>
      </div>
      <span
        className={cn(
          "relative h-[22px] w-[38px] shrink-0 rounded-full transition-colors",
          on ? "bg-[var(--interactive)]" : "bg-muted-foreground/25",
        )}
      >
        <span
          className={cn(
            "absolute top-[2px] size-[18px] rounded-full bg-white shadow-sm transition-all",
            on ? "left-[18px]" : "left-[2px]",
          )}
        />
      </span>
    </button>
  )
}

/** A small "Locked" pill for keys the company manages. */
function ManagedPill() {
  return <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full bg-muted/70 px-2 py-0.5 text-[10.5px] font-medium text-muted-foreground/80"><Lock className="size-3" />Locked</span>
}

function StatusPill({ connected }: { connected: boolean }) {
  return connected
    ? <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full bg-[var(--interactive)]/12 px-2 py-0.5 text-[10.5px] font-medium text-[var(--interactive)]"><Check className="size-3" strokeWidth={3} />Connected</span>
    : <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full bg-muted/70 px-2 py-0.5 text-[10.5px] font-medium text-muted-foreground/70"><span className="size-1.5 rounded-full bg-muted-foreground/40" />Not set</span>
}
