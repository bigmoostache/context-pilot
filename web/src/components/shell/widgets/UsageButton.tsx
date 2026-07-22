import { useState, useEffect, useRef, useMemo } from "react"
import { useMutation } from "@tanstack/react-query"
import { Loader2, ExternalLink, CheckCircle2, XCircle, LogIn } from "lucide-react"
import { Popover, PopoverTrigger, PopoverContent } from "@/components/ui/popover"
import { Tip } from "@/components/ui/tip"
import { fetchClaudeTokenStatus, startClaudeLogin, completeClaudeLogin } from "@/lib/api"
import { useClaudeUsage } from "@/lib/live/useClaudeUsage"
import { ClaudeUsageBody } from "@/components/agents/ClaudeUsagePage"

/** Anthropic "A" logomark (Simple Icons, 24×24 viewBox). */
function AnthropicMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" className={className} aria-hidden>
      <path d="M13.827 3.52h3.603L24 20.48h-3.603l-6.57-16.96zm-7.257 0h3.603l6.57 16.96h-3.603L11.627 16.47H5.166L3.653 20.48H0L6.57 3.52zm1.04 4.96l-2.49 6.7h5.47l-2.49-6.7z" />
    </svg>
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
    const handlePollResult = (status: TokenStatus) => {
      const baseline = baselineExpiryRef.current ?? 0
      // Only a valid token with a NEWER expiry than the baseline is a fresh login.
      if (status.valid && (status.expires_at ?? 0) > baseline) {
        clearInterval(id)
        setStep("done")
        doneTimer = window.setTimeout(() => onDoneRef.current(), 1500)
      }
    }
    const id = setInterval(() => {
      fetchClaudeTokenStatus()
        .then(handlePollResult)
        .catch(() => {
          /* ignore polling errors */
        })
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

/** Anthropic logo button that opens a popover with live usage bars + login. */
export function UsageButton() {
  const [open, setOpen] = useState(false)

  // All the query/mutation/auto-refresh orchestration lives in the shared
  // useClaudeUsage hook (also consumed by the mobile Config usage page), so this
  // button is pure chrome over it. `polling` follows the popover: the 30s
  // foreground cadence while open, the 5min idle cadence while closed.
  const usage = useClaudeUsage(open)
  const { sessionPct, invalidate } = usage

  // Camembert (pie-chart) background behind the Anthropic logo — shows
  // session usage at a glance without opening the popover.
  const pieBg = useMemo(() => {
    if (sessionPct <= 0) return
    const color =
      sessionPct >= 90
        ? "rgb(239 68 68 / 0.25)"
        : sessionPct >= 70
          ? "rgb(245 158 11 / 0.25)"
          : "rgb(16 185 129 / 0.25)"
    return `conic-gradient(from 0deg, ${color} ${String(sessionPct)}%, transparent ${String(sessionPct)}%)`
  }, [sessionPct])

  const handleLoginDone = invalidate

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <Tip title="Usage limits" body="Claude Code session & weekly rate limits." side="bottom">
        <PopoverTrigger
          className="flex size-7 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted/60 hover:text-foreground"
          aria-label="Claude Code usage"
          style={{ background: pieBg }}
        >
          <AnthropicMark className="size-[17px]" />
        </PopoverTrigger>
      </Tip>

      <PopoverContent side="bottom" align="end" sideOffset={8} className="w-72 space-y-3 p-4">
        <h4 className="text-[13px] font-semibold">Claude Code Usage</h4>

        {/* ── Shared usage body: token status · usage bars · stored accounts ── */}
        <ClaudeUsageBody usage={usage} />

        {/* ── Login flow (always available) ────────────────── */}
        <div className="border-t border-border pt-3">
          <LoginFlow onDone={handleLoginDone} />
        </div>
      </PopoverContent>
    </Popover>
  )
}
