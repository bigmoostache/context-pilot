import { useState, useEffect, useCallback } from "react"
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { Loader2, ExternalLink, CheckCircle2, XCircle, LogIn, RefreshCw } from "lucide-react"
import { Popover, PopoverTrigger, PopoverContent } from "@/components/ui/popover"
import { Tip } from "@/components/ui/tip"
import {
  fetchClaudeUsage,
  fetchClaudeTokenStatus,
  startClaudeLogin,
  completeClaudeLogin,
  refreshClaudeLogin,
} from "@/lib/api"
import type { ClaudeUsageLimit } from "@/lib/api/generated/types.gen"
import { cn } from "@/lib/utils"

/** Anthropic "A" logomark (Simple Icons, 24×24 viewBox). */
function AnthropicMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" className={className} aria-hidden>
      <path d="M13.827 3.52h3.603L24 20.48h-3.603l-6.57-16.96zm-7.257 0h3.603l6.57 16.96h-3.603L11.627 16.47H5.166L3.653 20.48H0L6.57 3.52zm1.04 4.96l-2.49 6.7h5.47l-2.49-6.7z" />
    </svg>
  )
}

/** Human label for a usage-limit `kind`. */
function limitLabel(kind: string): string {
  switch (kind) {
    case "session": return "Session"
    case "weekly_all": return "Weekly (all)"
    case "weekly_sonnet": return "Sonnet"
    case "weekly_opus": return "Opus"
    case "weekly_cowork": return "Cowork"
    default: return kind.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())
  }
}

/** Format a reset timestamp as a short relative string. */
function formatReset(iso: string | null | undefined): string {
  if (!iso) return ""
  const d = new Date(iso)
  const now = new Date()
  const diffMs = d.getTime() - now.getTime()
  if (diffMs < 0) return "resetting…"
  const diffH = diffMs / 3_600_000
  if (diffH < 24) {
    return `Resets ${d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`
  }
  return `Resets ${d.toLocaleDateString([], { month: "short", day: "numeric" })}`
}

/** Severity → bar colour. */
function barColor(severity: string, pct: number): string {
  if (severity === "critical" || pct >= 90) return "bg-red-500"
  if (severity === "warning" || pct >= 70) return "bg-amber-500"
  return "bg-emerald-500"
}

/** Format epoch ms as a short relative expiry string. */
function formatExpiry(epochMs: number): string {
  const now = Date.now()
  const diff = epochMs - now
  if (diff < 0) return "Expired"
  const mins = Math.floor(diff / 60_000)
  if (mins < 60) return `${mins}m left`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ${mins % 60}m left`
  return `${Math.floor(hrs / 24)}d left`
}

function LimitRow({ limit }: { limit: ClaudeUsageLimit }) {
  const pct = Math.min(limit.percent ?? 0, 100)
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-[12px]">
        <span className="font-medium text-foreground">{limitLabel(limit.kind ?? "")}</span>
        <span className="tabular-nums text-muted-foreground">{pct}%</span>
      </div>
      <div className="h-1.5 w-full rounded-full bg-muted">
        <div
          className={cn("h-full rounded-full transition-all duration-500", barColor(limit.severity ?? "normal", pct))}
          style={{ width: `${pct}%` }}
        />
      </div>
      {limit.resets_at && (
        <p className="text-[11px] text-muted-foreground/70">{formatReset(limit.resets_at)}</p>
      )}
    </div>
  )
}

type LoginStep = "idle" | "starting" | "waiting_for_code" | "completing" | "done" | "error"

/** Login flow sub-component shown inside the popover. */
function LoginFlow({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState<LoginStep>("idle")
  const [authorizeUrl, setAuthorizeUrl] = useState("")
  const [alreadyValid, setAlreadyValid] = useState(false)
  const [code, setCode] = useState("")
  const [error, setError] = useState("")

  const startMutation = useMutation({
    mutationFn: startClaudeLogin,
    onSuccess: (data) => {
      setAuthorizeUrl(data.url)
      setAlreadyValid(data.already_valid === true)
      setStep("waiting_for_code")
      window.open(data.url, "_blank")
    },
    onError: (e) => {
      const msg = e instanceof Error ? e.message
        : (typeof e === "object" && e && "error" in e) ? String((e as { error: string }).error)
        : "Failed to start login"
      setError(msg)
      setStep("error")
    },
  })

  const completeMutation = useMutation({
    mutationFn: (authCode: string) => completeClaudeLogin(authCode),
    onSuccess: () => {
      setStep("done")
      setTimeout(onDone, 1500)
    },
    onError: (e) => {
      const msg = e instanceof Error ? e.message
        : (typeof e === "object" && e && "error" in e) ? String((e as { error: string }).error)
        : "Failed to complete login"
      setError(msg)
      setStep("error")
    },
  })

  // Auto-detect login completion via the callback listener.
  // Polls token status every 2s while waiting for the browser redirect.
  const stableOnDone = useCallback(onDone, [onDone])
  useEffect(() => {
    if (step !== "waiting_for_code") return
    const id = setInterval(async () => {
      try {
        const status = await fetchClaudeTokenStatus()
        if (status.valid && !alreadyValid) {
          clearInterval(id)
          setStep("done")
          setTimeout(stableOnDone, 1500)
        }
      } catch { /* ignore polling errors */ }
    }, 2000)
    return () => clearInterval(id)
  }, [step, stableOnDone])

  if (step === "idle" || step === "starting") {
    return (
      <button
        onClick={() => { setStep("starting"); startMutation.mutate() }}
        disabled={startMutation.isPending}
        className="flex w-full items-center justify-center gap-2 rounded-md bg-foreground px-3 py-1.5 text-[12px] font-medium text-background transition-colors hover:bg-foreground/90 disabled:opacity-50"
      >
        {startMutation.isPending
          ? <><Loader2 className="size-3.5 animate-spin" /> Starting…</>
          : <><LogIn className="size-3.5" /> Login with Claude</>}
      </button>
    )
  }

  if (step === "waiting_for_code") {
    return (
      <div className="space-y-3">
        <p className="text-[12px] text-muted-foreground">
          After authorizing, Anthropic will show you a code. Copy the
          full <code className="text-[11px] bg-muted px-1 rounded">code#state</code> string
          and paste it below:
        </p>
        <a
          href={authorizeUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="flex items-center gap-1 text-[11px] text-[var(--signal)] hover:underline"
        >
          <ExternalLink className="size-3" /> Re-open authorization page
        </a>
        <input
          type="text"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          placeholder="Paste code or full callback URL…"
          autoFocus
          className="w-full rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[12px] text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-1 focus:ring-[var(--signal)]"
        />
        <button
          onClick={() => { setStep("completing"); completeMutation.mutate(code.trim()) }}
          disabled={!code.trim() || completeMutation.isPending}
          className="flex w-full items-center justify-center gap-2 rounded-md bg-foreground px-3 py-1.5 text-[12px] font-medium text-background transition-colors hover:bg-foreground/90 disabled:opacity-50"
        >
          {completeMutation.isPending
            ? <><Loader2 className="size-3.5 animate-spin" /> Verifying…</>
            : "Submit code"}
        </button>
      </div>
    )
  }

  if (step === "completing") {
    return (
      <div className="flex items-center justify-center gap-2 py-3 text-[12px] text-muted-foreground">
        <Loader2 className="size-4 animate-spin" /> Completing login…
      </div>
    )
  }

  if (step === "done") {
    return (
      <div className="flex items-center justify-center gap-2 py-3 text-[12px] text-emerald-500">
        <CheckCircle2 className="size-4" /> Logged in!
      </div>
    )
  }

  // error
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2 text-[12px] text-red-500">
        <XCircle className="size-4 shrink-0" />
        <span>{error}</span>
      </div>
      <button
        onClick={() => { setStep("idle"); setError(""); setCode("") }}
        className="w-full rounded-md bg-muted px-3 py-1.5 text-[12px] font-medium text-foreground transition-colors hover:bg-muted/80"
      >
        Try again
      </button>
    </div>
  )
}

/** Anthropic logo button that opens a popover with live usage bars + login. */
export function UsageButton() {
  const [open, setOpen] = useState(false)
  const queryClient = useQueryClient()

  const refreshMutation = useMutation({
    mutationFn: refreshClaudeLogin,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["claude-token-status"] })
      void queryClient.invalidateQueries({ queryKey: ["claude-usage"] })
    },
  })

  const tokenStatus = useQuery({
    queryKey: ["claude-token-status"],
    queryFn: fetchClaudeTokenStatus,
    enabled: open,
    refetchInterval: open ? 30_000 : false,
    staleTime: 10_000,
    retry: 1,
  })

  const { data, isLoading, isError } = useQuery({
    queryKey: ["claude-usage"],
    queryFn: fetchClaudeUsage,
    enabled: open && tokenStatus.data?.valid === true,
    refetchInterval: open ? 30_000 : false,
    staleTime: 10_000,
    retry: 1,
  })

  const limits = (data?.limits ?? []).filter((l) => l.percent != null && l.percent > 0)
  const isValid = tokenStatus.data?.valid === true

  const handleLoginDone = () => {
    void queryClient.invalidateQueries({ queryKey: ["claude-token-status"] })
    void queryClient.invalidateQueries({ queryKey: ["claude-usage"] })
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <Tip title="Usage limits" body="Claude Code session & weekly rate limits." side="bottom">
        <PopoverTrigger
          className="flex size-7 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted/60 hover:text-foreground"
          aria-label="Claude Code usage"
        >
          <AnthropicMark className="size-[17px]" />
        </PopoverTrigger>
      </Tip>

      <PopoverContent side="bottom" align="end" sideOffset={8} className="w-72 space-y-3 p-4">
        <h4 className="text-[13px] font-semibold">Claude Code Usage</h4>

        {/* ── Account email ────────────────────────────────── */}
        {tokenStatus.data?.account_email && (
          <p className="truncate text-[11px] text-muted-foreground">{tokenStatus.data.account_email}</p>
        )}

        {/* ── Token status ─────────────────────────────────── */}
        {tokenStatus.isLoading && (
          <div className="flex items-center justify-center py-2">
            <Loader2 className="size-4 animate-spin text-muted-foreground" />
          </div>
        )}

        {tokenStatus.data && (
          <div className="flex items-center justify-between rounded-md bg-muted/40 px-2.5 py-1.5">
            <div className="flex items-center gap-1.5 text-[12px]">
              <div className={cn("size-2 rounded-full", isValid ? "bg-emerald-500" : "bg-red-500")} />
              <span className="font-medium text-foreground">
                {isValid ? "Token valid" : "Token expired"}
              </span>
            </div>
            <div className="flex items-center gap-1.5">
              {isValid && tokenStatus.data.expires_at && (
                <span className="text-[11px] tabular-nums text-muted-foreground">
                  {formatExpiry(tokenStatus.data.expires_at)}
                </span>
              )}
              <button
                onClick={() => refreshMutation.mutate()}
                disabled={refreshMutation.isPending}
                title="Refresh token"
                className="flex size-5 items-center justify-center rounded text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
              >
                {refreshMutation.isPending
                  ? <Loader2 className="size-3 animate-spin" />
                  : <RefreshCw className="size-3" />}
              </button>
            </div>
          </div>
        )}

        {/* ── Refresh error ──────────────────────────────── */}
        {refreshMutation.isError && (
          <p className="text-[11px] text-red-500">
            Refresh failed: {refreshMutation.error instanceof Error
              ? refreshMutation.error.message
              : typeof refreshMutation.error === "object" && refreshMutation.error && "error" in refreshMutation.error
                ? String((refreshMutation.error as { error: string }).error)
                : "unknown error"}
          </p>
        )}

        {/* ── Usage limits (only when token valid) ─────────── */}
        {isValid && isLoading && (
          <div className="flex items-center justify-center py-4">
            <Loader2 className="size-4 animate-spin text-muted-foreground" />
          </div>
        )}

        {isValid && isError && (
          <p className="text-[12px] text-muted-foreground">
            Could not fetch usage data.
          </p>
        )}

        {isValid && !isLoading && !isError && limits.length === 0 && (
          <p className="text-[12px] text-muted-foreground">No active usage limits.</p>
        )}

        {isValid && limits.map((l) => (
          <LimitRow key={l.kind} limit={l} />
        ))}

        {/* ── Login flow (always available) ────────────────── */}
        <div className="border-t border-border pt-3">
          <LoginFlow onDone={handleLoginDone} />
        </div>
      </PopoverContent>
    </Popover>
  )
}
