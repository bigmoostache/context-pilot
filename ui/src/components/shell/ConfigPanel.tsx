import { useState } from "react"
import {
  Bot,
  Check,
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
  Search,
  Sliders,
  Sparkles,
  X,
  Zap,
} from "lucide-react"
import { DialogClose } from "@/components/ui/dialog"
import { UsagePage } from "@/components/agents/UsagePage"
import { cn } from "@/lib/utils"

/**
 * Context Pilot settings surface — a macOS System-Settings-style layout with a
 * category rail on the left and a detail pane on the right. Design-only: every
 * key/value is illustrative, nothing is persisted.
 *
 * This is the **shared body**, decoupled from any container. Two consumers:
 *  - {@link ConfigModal} wraps it in a portaled Dialog (the TopBar gear, used
 *    inside an agent) — pass `variant="dialog"` so the header/footer render
 *    `DialogClose` controls.
 *  - The fleet dashboard renders it **inline** as the "Settings" page — pass
 *    `variant="inline"` so it fills the page and drops the dialog-only chrome.
 */
export function ConfigPanel({
  variant = "dialog",
}: {
  variant?: "dialog" | "inline"
}) {
  const [cat, setCat] = useState<CatId>("general")
  const inline = variant === "inline"

  const active = CATEGORIES.find((c) => c.id === cat) ?? CATEGORIES[0]

  return (
    <div className="flex min-h-0 flex-1">
      {/* category rail */}
      <aside
        className={cn(
          "flex w-[230px] shrink-0 flex-col border-r border-border/70",
          inline ? "bg-surface" : "bg-muted/30",
        )}
      >
        <nav className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-2.5 py-3.5">
          {CATEGORIES.map((c) => {
            const on = c.id === cat
            return (
              <button
                key={c.id}
                onClick={() => setCat(c.id)}
                className={cn(
                  "group flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[12.5px] transition-colors",
                  on ? "bg-card font-medium text-foreground card-shadow" : "text-foreground/75 hover:bg-muted/60",
                )}
              >
                <span
                  className={cn(
                    "flex size-6 shrink-0 items-center justify-center rounded-md transition-colors",
                    on ? "bg-[var(--interactive)]/15 text-[var(--interactive)]" : "text-muted-foreground/70",
                  )}
                >
                  <c.icon className="size-[15px]" />
                </span>
                <span className="min-w-0 flex-1 truncate">{c.label}</span>
                {c.count != null && (
                  <span className="shrink-0 rounded-full bg-muted/70 px-1.5 py-px text-[9.5px] font-semibold tabular-nums text-muted-foreground">
                    {c.count}
                  </span>
                )}
              </button>
            )
          })}
        </nav>
        <div className="px-4 py-3 text-[10.5px] leading-relaxed text-muted-foreground/55">
          Design-only — keys are illustrative and nothing is saved.
        </div>
      </aside>

      {/* detail pane */}
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-[52px] shrink-0 items-center gap-2.5 border-b border-border/70 px-6">
          <active.icon className="size-[17px] text-muted-foreground" />
          <h2 className="text-[14px] font-semibold tracking-tight text-foreground">{active.label}</h2>
          <p className="ml-2 hidden truncate text-[12px] text-muted-foreground md:block">{active.blurb}</p>
          {!inline && (
            <DialogClose
              className="ml-auto flex size-7 items-center justify-center rounded-md text-muted-foreground/55 transition-colors hover:bg-muted/70 hover:text-foreground"
              aria-label="Close"
            >
              <X className="size-4" />
            </DialogClose>
          )}
        </header>

        {cat === "usage" ? (
          <div className="min-h-0 flex-1 overflow-hidden">
            <CategoryBody cat={cat} />
          </div>
        ) : (
          <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
            <CategoryBody cat={cat} />
          </div>
        )}

        <footer className="flex h-[58px] shrink-0 items-center border-t border-border/70 bg-muted/25 px-6">
          <span className="text-[11.5px] text-muted-foreground/70">
            Changes apply on save in the live app.
          </span>
          {inline ? (
            <button className="ml-auto flex items-center gap-2 rounded-lg bg-[var(--interactive)] px-4 py-2 text-[13px] font-medium text-[var(--primary-foreground)] transition-all hover:brightness-105 active:scale-[0.98]">
              <Check className="size-4" strokeWidth={2.5} />
              Save
            </button>
          ) : (
            <DialogClose className="ml-auto flex items-center gap-2 rounded-lg bg-[var(--interactive)] px-4 py-2 text-[13px] font-medium text-[var(--primary-foreground)] transition-all hover:brightness-105 active:scale-[0.98]">
              <Check className="size-4" strokeWidth={2.5} />
              Done
            </DialogClose>
          )}
        </footer>
      </main>
    </div>
  )
}

// ── categories ────────────────────────────────────────────────────
type CatId = "general" | "usage" | "providers" | "search" | "docai" | "web" | "integrations"

const CATEGORIES: {
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
function CategoryBody({ cat }: { cat: CatId }) {
  switch (cat) {
    case "general":
      return <GeneralPane />
    case "usage":
      return <UsagePage />
    case "providers":
      return (
        <Stack>
          <KeyRow i={0} name="Anthropic" env="ANTHROPIC_API_KEY" icon={Sparkles} status="connected" hint="Claude 4 family" sample="sk-ant-••••••••••3f7a" />
          <KeyRow i={1} name="Claude Code (OAuth)" env="Keychain · ~/.claude" icon={Cpu} status="connected" hint="opus-4-8 · sonnet-4-6 · fable-5" sample="oauth-••••••••••2c19" />
          <KeyRow i={2} name="Grok (xAI)" env="XAI_API_KEY" icon={Zap} status="missing" hint="grok-4" />
          <KeyRow i={3} name="Groq" env="GROQ_API_KEY" icon={Gauge} status="connected" hint="Llama 3.x · fast" sample="gsk_••••••••••8b02" />
          <KeyRow i={4} name="DeepSeek" env="DEEPSEEK_API_KEY" icon={Bot} status="missing" hint="deepseek-chat / reasoner" />
          <KeyRow i={5} name="MiniMax" env="MINIMAX_API_KEY" icon={Bot} status="connected" hint="Token Plan" sample="sk-cp-••••••••••5Wk8" />
        </Stack>
      )
    case "search":
      return (
        <Stack>
          <KeyRow i={0} name="Voyage AI" env="VOYAGE_API_KEY" icon={Database} status="connected" hint="voyage-code-3 · 1024-dim embeddings" sample="pa-••••••••••d41e" />
          <StatusRow i={1} name="Meilisearch" icon={Search} state="Running" detail="Embedded server · 6 417 chunks · port 49286" />
          <ToggleRow i={2} name="Hybrid semantic search" detail="Blend keyword + vector results" on />
        </Stack>
      )
    case "docai":
      return (
        <Stack>
          <KeyRow i={0} name="Datalab" env="DATALAB_API_KEY" icon={FileText} status="connected" hint="Surya OCR · PDF / image → markdown" sample="dl-••••••••••9a23" />
          <ToggleRow i={1} name="Cache OCR results" detail="~/.context-pilot/ocr-cache" on />
        </Stack>
      )
    case "web":
      return (
        <Stack>
          <KeyRow i={0} name="Brave Search" env="BRAVE_API_KEY" icon={Globe} status="connected" hint="Independent 40-B index" sample="BSA-••••••••••71fd" />
          <KeyRow i={1} name="Firecrawl" env="FIRECRAWL_API_KEY" icon={Globe} status="connected" hint="Scrape · search · crawl" sample="fc-••••••••••e0c8" />
        </Stack>
      )
    case "integrations":
      return (
        <Stack>
          <KeyRow i={0} name="GitHub" env="GITHUB_TOKEN" icon={Boxes} status="connected" hint="PRs · issues · gh CLI" sample="ghp_••••••••••a7d5" />
        </Stack>
      )
  }
}

function GeneralPane() {
  const MODELS = [
    { id: "claude-opus-4-8", tier: "Most capable", price: "$5 · 200K", icon: Sparkles },
    { id: "claude-sonnet-4-6", tier: "Balanced", price: "$3 · 1M", icon: Gauge },
    { id: "claude-fable-5", tier: "Creative", price: "$10 · 400K", icon: Zap },
  ]
  const [model, setModel] = useState(MODELS[0].id)
  return (
    <Stack>
      <FieldGroup label="Default model" hint="Used for new agents unless overridden">
        <div className="flex flex-col gap-2">
          {MODELS.map((m, i) => {
            const on = m.id === model
            return (
              <button
                key={m.id}
                onClick={() => setModel(m.id)}
                style={{ animationDelay: `${i * 45}ms` }}
                className={cn(
                  "opt-rise flex items-center gap-3 rounded-xl border px-3 py-2.5 text-left transition-all",
                  on
                    ? "border-[var(--interactive)] bg-[var(--interactive)]/[0.07] ring-2 ring-[var(--interactive)]/15"
                    : "border-border bg-card hover:border-[var(--interactive)]/40 hover:bg-muted/30",
                )}
              >
                <span
                  className={cn(
                    "flex size-8 shrink-0 items-center justify-center rounded-lg transition-colors",
                    on ? "bg-[var(--interactive)]/15 text-[var(--interactive)]" : "bg-muted/60 text-muted-foreground/70",
                  )}
                >
                  <m.icon className="size-4" />
                </span>
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="font-mono text-[12.5px] font-medium text-foreground/90">{m.id}</span>
                  <span className="text-[11px] text-muted-foreground">{m.tier}</span>
                </div>
                <span className="shrink-0 font-mono text-[10.5px] tabular-nums text-muted-foreground/65">{m.price}</span>
                <span
                  className={cn(
                    "flex size-5 shrink-0 items-center justify-center rounded-full border transition-all",
                    on ? "border-[var(--interactive)] bg-[var(--interactive)] text-[var(--primary-foreground)]" : "border-border text-transparent",
                  )}
                >
                  <Check className="size-3" strokeWidth={3} />
                </span>
              </button>
            )
          })}
        </div>
      </FieldGroup>

      <ToggleRow i={0} name="Auto-continuation" detail="Let the agent keep working without a nudge" />
      <ToggleRow i={1} name="Reverie (context optimizer)" detail="Background cleaner reshapes context when it grows" on />
      <ToggleRow i={2} name="Think reminders" detail="Periodic nudge to reason before acting" on />
    </Stack>
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
  sample,
}: {
  i: number
  name: string
  env: string
  icon: typeof Bot
  status: "connected" | "missing"
  hint: string
  sample?: string
}) {
  const connected = status === "connected"
  const [reveal, setReveal] = useState(false)
  const value = sample ?? ""
  const shown = reveal && connected ? value.replace(/•+/, "sk-live-7Q2a8FnZ") : value

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
        <StatusPill connected={connected} />
      </div>
      <div className="flex items-center gap-2 rounded-lg border border-border bg-background/60 px-2.5 py-1.5">
        <KeyRound className="size-3.5 shrink-0 text-muted-foreground/55" />
        <code className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-foreground/75">
          {connected ? shown : <span className="text-muted-foreground/45">not configured</span>}
        </code>
        <span className="shrink-0 rounded bg-muted/70 px-1.5 py-px font-mono text-[9.5px] text-muted-foreground/70">{env}</span>
        {connected && (
          <button
            onClick={() => setReveal((r) => !r)}
            className="shrink-0 text-muted-foreground/55 transition-colors hover:text-foreground"
            aria-label={reveal ? "Hide" : "Reveal"}
          >
            {reveal ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
          </button>
        )}
      </div>
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
}: {
  i: number
  name: string
  detail: string
  on?: boolean
}) {
  const [on, setOn] = useState(initial)
  return (
    <button
      onClick={() => setOn((v) => !v)}
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

function StatusPill({ connected }: { connected: boolean }) {
  return connected ? (
    <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full bg-[var(--interactive)]/12 px-2 py-0.5 text-[10.5px] font-medium text-[var(--interactive)]">
      <Check className="size-3" strokeWidth={3} />
      Connected
    </span>
  ) : (
    <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full bg-muted/70 px-2 py-0.5 text-[10.5px] font-medium text-muted-foreground/70">
      <span className="size-1.5 rounded-full bg-muted-foreground/40" />
      Not set
    </span>
  )
}
