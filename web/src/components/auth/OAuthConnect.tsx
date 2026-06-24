import { useState } from "react"
import { Check, Cpu, ExternalLink, Loader2 } from "lucide-react"
import { Input } from "@/components/ui/input"
import { oauthStart, oauthFinish } from "@/lib/api"
import { cn } from "@/lib/utils"

/**
 * "Connect Claude Code" — the manual OAuth paste flow. The registered client
 * only supports a code-display redirect, so the user opens the Anthropic
 * authorize page, signs in, copies the shown `code#state`, and pastes it back.
 * On success the backend writes `~/.claude/.credentials.json` and `onConnected`
 * fires (the caller refetches status/settings).
 *
 * Used both in onboarding (where it satisfies the "≥1 provider" rule without an
 * API key) and in Settings → Model Providers.
 */
export function OAuthConnect({
  onConnected,
  label = "Connect Claude Code (OAuth)",
  compact = false,
}: {
  onConnected: () => void
  label?: string
  compact?: boolean
}) {
  const [phase, setPhase] = useState<"idle" | "await-code" | "done">("idle")
  const [code, setCode] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const begin = async () => {
    setError(null)
    setBusy(true)
    try {
      const { authorize_url } = await oauthStart()
      window.open(authorize_url, "_blank", "noopener,noreferrer")
      setPhase("await-code")
    } catch (e) {
      setError(e instanceof Error ? e.message : "Could not start sign-in")
    } finally {
      setBusy(false)
    }
  }

  const submit = async () => {
    if (busy || code.trim() === "") return
    setError(null)
    setBusy(true)
    try {
      await oauthFinish(code.trim())
      setPhase("done")
      onConnected()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Sign-in failed")
    } finally {
      setBusy(false)
    }
  }

  if (phase === "done") {
    return (
      <div className="flex items-center gap-2 rounded-xl border border-[var(--ok)]/30 bg-[var(--ok)]/[0.07] px-3.5 py-2.5 text-[12.5px] text-[var(--ok)]">
        <Check className="size-4" strokeWidth={3} /> Claude Code connected
      </div>
    )
  }

  return (
    <div className={cn("flex flex-col gap-2.5 rounded-xl border border-border bg-card", compact ? "p-3" : "p-3.5", "card-shadow")}>
      <div className="flex items-center gap-2.5">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-[var(--interactive)]/12 text-[var(--interactive)]">
          <Cpu className="size-4" />
        </span>
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="text-[13px] font-medium text-foreground/90">Claude Code (OAuth)</span>
          <span className="text-[11px] text-muted-foreground">Sign in with your Anthropic account — no API key needed</span>
        </div>
      </div>

      {phase === "idle" ? (
        <button
          type="button"
          onClick={() => void begin()}
          disabled={busy}
          className="flex items-center justify-center gap-2 rounded-lg bg-[var(--interactive)] px-3.5 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-all hover:brightness-105 disabled:opacity-50"
        >
          {busy ? <Loader2 className="size-3.5 animate-spin" /> : <ExternalLink className="size-3.5" />}
          {busy ? "Opening…" : label}
        </button>
      ) : (
        <div className="flex flex-col gap-2">
          <p className="text-[11.5px] leading-relaxed text-muted-foreground">
            A new tab opened on Anthropic. Sign in, then copy the code shown and paste it here.
          </p>
          <div className="flex items-center gap-2">
            <Input
              value={code}
              onChange={(e) => setCode(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault()
                  void submit()
                }
              }}
              placeholder="Paste the code (code#state)"
              autoComplete="off"
              autoFocus
              className="flex-1"
            />
            <button
              type="button"
              onClick={() => void submit()}
              disabled={busy || code.trim() === ""}
              className="flex items-center gap-1.5 rounded-lg bg-[var(--interactive)] px-3 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-all hover:brightness-105 disabled:opacity-50"
            >
              {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Check className="size-3.5" strokeWidth={2.5} />}
              Finish
            </button>
          </div>
        </div>
      )}

      {error && (
        <p role="alert" className="text-[11.5px] text-destructive">
          {error}
        </p>
      )}
    </div>
  )
}
