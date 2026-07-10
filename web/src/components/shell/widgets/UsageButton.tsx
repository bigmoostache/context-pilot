import { useState, useEffect, useRef } from "react"
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
import { StoredAccounts } from "./StoredAccounts"

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
    case "session": {
      return "Session"
    }
    case "weekly_all": {
      return "Weekly (all)"
    }
    case "weekly_sonnet": {
      return "Sonnet"
    }
    case "weekly_opus": {
      return "Opus"
    }
    case "weekly_cowork": {
      return "Cowork"
    }
    default: {
      return kind.replaceAll("_", " ").replaceAll(/\b\w/g, (c) => c.toUpperCase())
    }
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
  const pct = Math.min(limit.percent, 100)
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-[12px]">
        <span className="font-medium text-foreground">{limitLabel(limit.kind)}</span>
        <span className="text-muted-foreground tabular-nums">{pct}%</span>
      </div>
      <div className="h-1.5 w-full rounded-full bg-muted">
        <div
          className={cn(
            "h-full rounded-full transition-all duration-500",
            barColor(limit.severity, pct),
          )}
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

/** Paste-your-code step — the largest LoginFlow branch, extracted for P8 budget. */
function WaitingForCode({
  authorizeUrl,
  code,
  setCode,
  onSubmit,
  submitting,
}: {
  authorizeUrl: string
  code: string
  setCode: (v: string) => void
  onSubmit: () => void
  submitting: boolean
}) {
  return (
    <div className="space-y-3">
      <p className="text-[12px] text-muted-foreground">
        After authorizing, Anthropic will show you a code. Copy the full{" "}
        <code className="rounded-sm bg-muted px-1 text-[11px]">code#state</code> string and paste it
        below:
      </p>
      <a
        href={authorizeUrl}
        target="_blank"
        rel="noopener noreferrer"
        className="flex items-center gap-1 text-[11px] text-(--signal) hover:underline"
      >
        <ExternalLink className="size-3" /> Re-open authorization page
      </a>
      <input
        type="text"
        value={code}
        onChange={(e) => setCode(e.target.value)}
        placeholder="Paste code or full callback URL…"
        autoFocus
        className="w-full rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[12px] text-foreground placeholder:text-muted-foreground/50 focus:ring-1 focus:ring-(--signal) focus:outline-none"
      />
      <button
        onClick={onSubmit}
        disabled={!code.trim() || submitting}
        className="flex w-full items-center justify-center gap-2 rounded-md bg-foreground px-3 py-1.5 text-[12px] font-medium text-background transition-colors hover:bg-foreground/90 disabled:opacity-50"
      >
        {submitting ? (
          <>
            <Loader2 className="size-3.5 animate-spin" /> Verifying…
          </>
        ) : (
          "Submit code"
        )}
      </button>
    </div>
  )
}

/** Claude OAuth login flow — reused by the Settings → Secrets pane (design §13.5). */
export function LoginFlow({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState<LoginStep>("idle")
  const [authorizeUrl, setAuthorizeUrl] = useState("")
  const [code, setCode] = useState("")
  const [error, setError] = useState("")

  const startMutation = useMutation({
    mutationFn: startClaudeLogin,
    onSuccess: (data) => {
      setAuthorizeUrl(data.url)
      setStep("waiting_for_code")
      window.open(data.url, "_blank")
    },
    onError: (e) => {
      // The SDK client always throws an `Error` whose message already carries
      // the backend `error` field (client.ts extracts it), so no object-shape
      // fallback is needed — the else-branch would be unreachable (`never`).
      setError(e instanceof Error ? e.message : "Failed to start login")
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
      setError(e instanceof Error ? e.message : "Failed to complete login")
      setStep("error")
    },
  })

  // Auto-detect login completion via the callback listener.
  // Polls token status every 2s while waiting for the browser redirect.
  // A pre-existing (possibly stale) token must NOT be mistaken for "just logged
  // in" — otherwise the flow auto-completes before the user pastes a fresh code
  // (T472). We snapshot the expiry at the moment login starts and only accept a
  // token whose expiry has strictly advanced (a genuinely new credential).
  // Keep latest onDone in a ref so polling effect binds ONCE (deps [step])
  // without re-subscribing when parent recreates onDone.
  const onDoneRef = useRef(onDone)
  useEffect(() => {
    onDoneRef.current = onDone
  })
  const baselineExpiryRef = useRef<number | null>(null)
  useEffect(() => {
    if (step !== "waiting_for_code") return
    let cancelled = false
    // Record the starting expiry once, before polling begins.
    if (baselineExpiryRef.current === null) {
      void fetchClaudeTokenStatus()
        .then((s) => {
          if (!cancelled) baselineExpiryRef.current = s.valid ? (s.expires_at ?? 0) : 0
        })
        .catch(() => {
          if (!cancelled) baselineExpiryRef.current = 0
        })
    }
    let doneTimer: number | undefined
    const id = setInterval(async () => {
      try {
        const status = await fetchClaudeTokenStatus()
        const baseline = baselineExpiryRef.current ?? 0
        // Only a valid token with a NEWER expiry than the baseline is a fresh login.
        if (status.valid && (status.expires_at ?? 0) > baseline) {
          clearInterval(id)
          setStep("done")
          doneTimer = window.setTimeout(() => onDoneRef.current(), 1500)
        }
      } catch {
        /* ignore polling errors */
      }
    }, 2000)
    return () => {
      cancelled = true
      clearInterval(id)
      if (doneTimer !== undefined) window.clearTimeout(doneTimer)
    }
  }, [step])

  if (step === "idle" || step === "starting") {
    return (
      <button
        onClick={() => {
          setStep("starting")
          startMutation.mutate()
        }}
        disabled={startMutation.isPending}
        className="flex w-full items-center justify-center gap-2 rounded-md bg-foreground px-3 py-1.5 text-[12px] font-medium text-background transition-colors hover:bg-foreground/90 disabled:opacity-50"
      >
        {startMutation.isPending ? (
          <>
            <Loader2 className="size-3.5 animate-spin" /> Starting…
          </>
        ) : (
          <>
            <LogIn className="size-3.5" /> Login with Claude
          </>
        )}
      </button>
    )
  }

  if (step === "waiting_for_code") {
    return (
      <WaitingForCode
        authorizeUrl={authorizeUrl}
        code={code}
        setCode={setCode}
        submitting={completeMutation.isPending}
        onSubmit={() => {
          setStep("completing")
          completeMutation.mutate(code.trim())
        }}
      />
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
        onClick={() => {
          setStep("idle")
          setError("")
          setCode("")
        }}
        className="w-full rounded-md bg-muted px-3 py-1.5 text-[12px] font-medium text-foreground transition-colors hover:bg-muted/80"
      >
        Try again
      </button>
    </div>
  )
}

type TokenStatus = Awaited<ReturnType<typeof fetchClaudeTokenStatus>>

/** Account email + token status row + refresh control — extracted for P8 budget. */
function TokenStatusRow({
  data,
  isLoading,
  isValid,
  refreshPending,
  refreshError,
  onRefresh,
}: {
  data: TokenStatus | undefined
  isLoading: boolean
  isValid: boolean
  refreshPending: boolean
  refreshError: unknown
  onRefresh: () => void
}) {
  return (
    <>
      {data?.account_email && (
        <p className="truncate text-[11px] text-muted-foreground">{data.account_email}</p>
      )}
      {isLoading && (
        <div className="flex items-center justify-center py-2">
          <Loader2 className="size-4 animate-spin text-muted-foreground" />
        </div>
      )}
      {data && (
        <div className="flex items-center justify-between rounded-md bg-muted/40 px-2.5 py-1.5">
          <div className="flex items-center gap-1.5 text-[12px]">
            <div className={cn("size-2 rounded-full", isValid ? "bg-emerald-500" : "bg-red-500")} />
            <span className="font-medium text-foreground">
              {isValid ? "Token valid" : "Token expired"}
            </span>
          </div>
          <div className="flex items-center gap-1.5">
            {isValid && data.expires_at != null && (
              <span className="text-[11px] text-muted-foreground tabular-nums">
                {formatExpiry(data.expires_at)}
              </span>
            )}
            <button
              onClick={onRefresh}
              disabled={refreshPending}
              title="Refresh token"
              className="flex size-5 items-center justify-center rounded-sm text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
            >
              {refreshPending ? (
                <Loader2 className="size-3 animate-spin" />
              ) : (
                <RefreshCw className="size-3" />
              )}
            </button>
          </div>
        </div>
      )}
      {refreshError != null && (
        <p className="text-[11px] text-red-500">
          Refresh failed: {refreshError instanceof Error ? refreshError.message : "unknown error"}
        </p>
      )}
    </>
  )
}

/** Usage-limit bars (loading/error/empty placeholder) — extracted for P8 budget. */
function UsageLimits({
  isValid,
  isLoading,
  isError,
  limits,
}: {
  isValid: boolean
  isLoading: boolean
  isError: boolean
  limits: ClaudeUsageLimit[]
}) {
  if (!isValid) return null
  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-4">
        <Loader2 className="size-4 animate-spin text-muted-foreground" />
      </div>
    )
  }
  if (isError)
    return <p className="text-[12px] text-muted-foreground">Could not fetch usage data.</p>
  if (limits.length === 0) {
    return <p className="text-[12px] text-muted-foreground">No active usage limits.</p>
  }
  return (
    <>
      {limits.map((l) => (
        <LimitRow key={l.kind} limit={l} />
      ))}
    </>
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

  const limits = (data?.limits ?? []).filter((l) => l.percent > 0)
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

        {/* ── Account email · token status · refresh ───────── */}
        <TokenStatusRow
          data={tokenStatus.data}
          isLoading={tokenStatus.isLoading}
          isValid={isValid}
          refreshPending={refreshMutation.isPending}
          refreshError={refreshMutation.isError ? refreshMutation.error : null}
          onRefresh={() => refreshMutation.mutate()}
        />

        {/* ── Usage limits (only when token valid) ─────────── */}
        <UsageLimits isValid={isValid} isLoading={isLoading} isError={isError} limits={limits} />

        {/* ── Stored accounts (multi-account vault) ────────── */}
        <div className="border-t border-border pt-3">
          <StoredAccounts isValid={isValid} onSwitch={handleLoginDone} />
        </div>

        {/* ── Login flow (always available) ────────────────── */}
        <div className="border-t border-border pt-3">
          <LoginFlow onDone={handleLoginDone} />
        </div>
      </PopoverContent>
    </Popover>
  )
}
