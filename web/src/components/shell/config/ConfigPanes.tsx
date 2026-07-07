import { useState } from "react"
import type { CatId } from "./categories"
import { useQuery, useQueryClient } from "@tanstack/react-query"
import { Check, Lock } from "lucide-react"
import { UsagePage } from "@/components/agents/UsagePage"
import { ReleasesPane } from "./ReleasesPane"
import { useProviders } from "@/lib/support/models"
import { fetchSettings, updateSettings, fetchEnvKeys } from "@/lib/api"
import { useDevMode } from "@/lib/providers/devMode"
import { cn } from "@/lib/utils"

// ── per-category bodies ───────────────────────────────────────────
//
// Provider / integration API keys are provisioned out-of-band by the operator
// (vendor, over SSH/Ansible) and never edited from the cockpit. The Services
// pane is read-only: it just shows which integrations are available (key
// present) vs. greyed-out (key absent).
export function CategoryBody({ cat }: { cat: CatId }) {
  switch (cat) {
    case "general": {
      return <GeneralPane />
    }
    case "usage": {
      return <UsagePage />
    }
    case "services": {
      return <ServicesPane />
    }
    case "releases": {
      return <ReleasesPane />
    }
  }
}

/** Grouping of the well-known env keys into display categories (by env name). */
const SERVICE_GROUPS: { label: string; envs: string[] }[] = [
  {
    label: "Model providers",
    envs: [
      "ANTHROPIC_API_KEY",
      "GROQ_API_KEY",
      "XAI_API_KEY",
      "DEEPSEEK_API_KEY",
      "MINIMAX_API_KEY",
    ],
  },
  { label: "Search & embeddings", envs: ["VOYAGE_API_KEY"] },
  { label: "Document AI", envs: ["DATALAB_API_KEY"] },
  { label: "Web & scraping", envs: ["BRAVE_API_KEY", "FIRECRAWL_API_KEY"] },
  { label: "Integrations", envs: ["GITHUB_TOKEN"] },
]

/**
 * Read-only catalogue of integrations the operator has provisioned, grouped by
 * category. Each known service is **Available** when its key is present, or
 * greyed-out "Not configured" otherwise. No keys are shown or editable —
 * provisioning happens out-of-band.
 */
function ServicesPane() {
  const { data: services = [] } = useQuery({ queryKey: ["env-keys"], queryFn: fetchEnvKeys })
  const byEnv = new Map(services.map((s) => [s.env, s]))

  const groups = SERVICE_GROUPS.map((g) => ({
    label: g.label,
    items: g.envs.flatMap((e) => {
      const s = byEnv.get(e)
      return s ? [s] : []
    }),
  })).filter((g) => g.items.length > 0)

  // Catch-all so a newly-added backend key never silently disappears.
  const known = new Set(SERVICE_GROUPS.flatMap((g) => g.envs))
  const other = services.filter((s) => !known.has(s.env))
  if (other.length > 0) groups.push({ label: "Other", items: other })

  return (
    <div className="flex flex-col gap-4">
      {groups.map((g) => (
        <FieldGroup key={g.label} label={g.label}>
          {g.items.map((s) => (
            <ServiceRow key={s.env} label={s.label} available={s.exists} />
          ))}
        </FieldGroup>
      ))}
    </div>
  )
}

function ServiceRow({ label, available }: { label: string; available: boolean }) {
  return (
    <div
      className={cn(
        "flex items-center gap-3 rounded-xl border px-3.5 py-3 transition-colors",
        available ? "border-border bg-card" : "border-border/50 bg-muted/20 opacity-55",
      )}
    >
      <span
        className={cn(
          "flex size-7 shrink-0 items-center justify-center rounded-lg",
          available ? "bg-[var(--ok)]/15 text-[var(--ok)]" : "bg-muted/60 text-muted-foreground/60",
        )}
      >
        {available ? <Check className="size-4" strokeWidth={3} /> : <Lock className="size-3.5" />}
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="text-[13px] font-medium text-foreground/90">{label}</span>
        <span className="text-[11px] text-muted-foreground">
          {available ? "Available" : "Not configured"}
        </span>
      </span>
    </div>
  )
}

// The default model is intentionally NOT set here: each user picks their own
// model per agent in the create/manage dialog. Admins shape that choice via the
// org-wide allowlist below ({@link AllowedModelsSection}), not a forced default.
function GeneralPane() {
  return (
    <Stack>
      <AllowedModelsSection />

      <ToggleRow
        i={0}
        name="Auto-continuation"
        detail="Let the agent keep working without a nudge"
      />
      <ToggleRow
        i={1}
        name="Reverie (context optimizer)"
        detail="Background cleaner reshapes context when it grows"
        on
      />
      <ToggleRow i={2} name="Think reminders" detail="Periodic nudge to reason before acting" on />
      <DevModeToggle i={3} />
    </Stack>
  )
}

/**
 * Admin-only allowlist of usable models (org-wide). The operator (vendor)
 * provisions provider keys out-of-band; the admin decides which of the working
 * models their users may pick. An empty allowlist means *all allowed* — exposed
 * here as an "Allow all models" master switch. When restricted, each model has
 * a checkbox; the stored list is the set of checked `"provider:model"` ids.
 * Regular users never see this (they only get the filtered picker).
 */
function AllowedModelsSection() {
  const qc = useQueryClient()
  // The full usable catalog (every provider with a key) — the admin toggles the
  // org allowlist over it; each model already carries its server-built `key`.
  const { data: providers = [] } = useProviders()
  const { data: settings } = useQuery({ queryKey: ["settings"], queryFn: fetchSettings })
  const [busy, setBusy] = useState(false)

  if (!settings?.is_admin) return null

  const allowed = settings.allowed_models
  const allowedSet = new Set(allowed)
  const restricted = allowed.length > 0
  const everyKey = providers.flatMap((p) => p.models.map((m) => m.key))

  const save = async (next: string[]) => {
    setBusy(true)
    try {
      await updateSettings({ allowed_models: next })
      // The allowlist drives the server-filtered picker registry — refresh both.
      await qc.invalidateQueries({ queryKey: ["settings"] })
      await qc.invalidateQueries({ queryKey: ["providers", "picker"] })
    } finally {
      setBusy(false)
    }
  }

  return (
    <FieldGroup label="Allowed models" hint="Which models your users may pick">
      <label className="flex items-center gap-2.5 rounded-lg border border-border bg-card px-3.5 py-2.5">
        <input
          type="checkbox"
          checked={!restricted}
          disabled={busy}
          onChange={(e) => void save(e.target.checked ? [] : everyKey)}
          className="size-4 accent-[var(--interactive)]"
        />
        <span className="flex min-w-0 flex-col">
          <span className="text-[13px] font-medium text-foreground/90">Allow all models</span>
          <span className="text-[11px] text-muted-foreground">
            {restricted
              ? "Restricted — only the checked models below are available"
              : "Every provisioned model is available to your users"}
          </span>
        </span>
      </label>

      {restricted && (
        <div className="flex flex-col gap-3 pt-1">
          {providers.map((p) => (
            <div key={p.id} className="flex flex-col gap-1.5">
              <span className="text-[10.5px] font-semibold uppercase tracking-[0.06em] text-muted-foreground/70">
                {p.name}
              </span>
              <div className="flex flex-col gap-1">
                {p.models.map((m) => {
                  const key = m.key
                  const checked = allowedSet.has(key)
                  return (
                    <label
                      key={key}
                      className="flex items-center gap-2.5 rounded-md px-2 py-1.5 hover:bg-muted/40"
                    >
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={busy}
                        onChange={() =>
                          void save(checked ? allowed.filter((k) => k !== key) : [...allowed, key])
                        }
                        className="size-3.5 accent-[var(--interactive)]"
                      />
                      <span className="text-[12.5px] text-foreground/85">{m.displayName}</span>
                    </label>
                  )
                })}
              </div>
            </div>
          ))}
        </div>
      )}
    </FieldGroup>
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
        <span className="text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
          {label}
        </span>
        {hint && <span className="text-[11px] text-muted-foreground/60">{hint}</span>}
      </div>
      {children}
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
