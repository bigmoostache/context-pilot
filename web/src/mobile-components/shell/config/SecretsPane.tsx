import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Eye, Loader2 } from "lucide-react"
import {
  fetchClaudeTokenStatus,
  fetchEnvKeys,
  revealEnvKey,
  updateEnvKey,
  type EnvKeyStatus,
} from "@/lib/api"
import { LoginFlow } from "@/mobile-components/shell/widgets/UsageButton"
import { cn } from "@/lib/utils"

/**
 * Secrets settings pane (design §13.5) — mobile twin of `components/shell/config/
 * SecretsPane`. Gated on `can_manage_secrets` (superadmin; the caller only
 * renders it for that role, and the backend enforces 403 otherwise, NFR-05).
 * Distinct from the read-only `ServicesPane`: this one **reveals and edits**
 * provider API keys and drives the **Claude OAuth** login flow. Provider keys
 * still route through `GET/PUT /api/env-keys/{name}`; the OAuth flow reuses the
 * `UsageButton` login widget (resolved through the mobile tree).
 *
 * Divergence from desktop is touch-only: the reveal/edit key input carries a
 * **16px font** (iOS Safari auto-zooms below 16px on focus) and the action
 * buttons swap `hover:` for `active:`. Every mutation — reveal, save, OAuth —
 * lives in the shared `@/lib/api` layer, byte-identical to the desktop twin.
 */
export function SecretsPane() {
  const { data: keys = [] } = useQuery({ queryKey: ["env-keys"], queryFn: fetchEnvKeys })
  return (
    <div className="flex flex-col gap-6">
      <section className="flex flex-col gap-2">
        <SectionLabel label="Provider API keys" hint="Reveal or replace a stored key" />
        <div className="flex flex-col gap-2">
          {keys.map((k) => (
            <ProviderKeyRow key={k.env} item={k} />
          ))}
        </div>
      </section>

      <section className="flex flex-col gap-2">
        <SectionLabel label="Claude subscription" hint="OAuth login for Claude Code" />
        <ClaudeOAuthSection />
      </section>
    </div>
  )
}

function SectionLabel({ label, hint }: { label: string; hint: string }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
        {label}
      </span>
      <span className="text-[11px] text-muted-foreground/60">{hint}</span>
    </div>
  )
}

/** One provider key: status, on-demand reveal (fetches the value into an edit
 *  field), and persist via `PUT /api/env-keys/{name}`. */
function ProviderKeyRow({ item }: { item: EnvKeyStatus }) {
  const qc = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [value, setValue] = useState("")

  const reveal = useMutation({
    mutationFn: () => revealEnvKey(item.env),
    onSuccess: (r) => {
      setValue(r.value ?? "")
      setEditing(true)
    },
  })
  const save = useMutation({
    mutationFn: () => updateEnvKey(item.env, value),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["env-keys"] })
      setEditing(false)
      setValue("")
    },
  })

  const cancel = () => {
    setEditing(false)
    setValue("")
    reveal.reset()
    save.reset()
  }

  return (
    <div className="rounded-xl border border-border bg-card px-3.5 py-3">
      <div className="flex items-center gap-3">
        <span
          className={cn(
            "size-2 shrink-0 rounded-full",
            item.exists ? "bg-(--ok)" : "bg-muted-foreground/40",
          )}
        />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[13px] font-medium text-foreground/90">{item.label}</span>
          <span className="truncate text-[11px] text-muted-foreground">
            {item.exists ? "Configured" : "Not configured"}
          </span>
        </span>
        {!editing && (
          <button
            onClick={() => (item.exists ? reveal.mutate() : setEditing(true))}
            disabled={reveal.isPending}
            className="flex shrink-0 items-center gap-1.5 rounded-md border border-border px-3 py-2 text-[13px] font-medium text-foreground/80 transition-colors active:bg-muted/60 disabled:opacity-50"
          >
            {reveal.isPending ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <span className="flex items-center gap-1.5">
                {item.exists && <Eye className="size-3.5" />}
                {item.exists ? "Reveal & edit" : "Set key"}
              </span>
            )}
          </button>
        )}
      </div>

      {editing && (
        <div className="mt-2.5 flex flex-col gap-2">
          <input
            type="text"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder="Paste the key value…"
            autoFocus
            className="w-full rounded-md border border-border bg-muted/50 px-2.5 py-2 font-mono text-[16px] text-foreground placeholder:text-muted-foreground/50 focus:ring-1 focus:ring-(--interactive) focus:outline-none"
          />
          <div className="flex items-center gap-2">
            <button
              onClick={() => save.mutate()}
              disabled={!value.trim() || save.isPending}
              className="flex items-center gap-1.5 rounded-md bg-(--interactive) px-3.5 py-2 text-[13px] font-medium text-(--primary-foreground) transition-[filter] active:brightness-105 disabled:opacity-50"
            >
              {save.isPending && <Loader2 className="size-3.5 animate-spin" />}
              Save
            </button>
            <button
              onClick={cancel}
              className="rounded-md px-3.5 py-2 text-[13px] font-medium text-muted-foreground transition-colors active:bg-muted/60"
            >
              Cancel
            </button>
            {save.isError && (
              <span className="text-[11px] text-red-500">
                {save.error instanceof Error ? save.error.message : "Save failed"}
              </span>
            )}
          </div>
        </div>
      )}
    </div>
  )
}

/** Claude OAuth status + login flow (reuses the shared {@link LoginFlow}). */
function ClaudeOAuthSection() {
  const qc = useQueryClient()
  const { data: token } = useQuery({
    queryKey: ["claude-token-status"],
    queryFn: fetchClaudeTokenStatus,
    retry: 1,
  })
  const valid = token?.valid === true

  return (
    <div className="rounded-xl border border-border bg-card p-3.5">
      <div className="flex items-center gap-2 pb-3">
        <span
          className={cn("size-2 rounded-full", valid ? "bg-(--ok)" : "bg-muted-foreground/40")}
        />
        <span className="text-[12px] font-medium text-foreground/90">
          {valid ? "Signed in" : "Not signed in"}
        </span>
        {token?.account_email && (
          <span className="truncate text-[11px] text-muted-foreground">{token.account_email}</span>
        )}
      </div>
      <LoginFlow onDone={() => void qc.invalidateQueries({ queryKey: ["claude-token-status"] })} />
    </div>
  )
}
