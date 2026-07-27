import { useEffect, useRef } from "react"
import { animate, stagger } from "animejs"
import { ChevronLeft, Loader2, RefreshCw } from "lucide-react"
import { useClaudeUsage } from "@/lib/live/useClaudeUsage"
import type { ClaudeUsageLimit } from "@/lib/api/generated/types.gen"
import { cn, prefersReducedMotion } from "@/lib/utils"
import { CornerButton } from "@/mobile-components/shell/CornerButton"
import { StoredAccounts } from "@/mobile-components/shell/widgets/StoredAccounts"
import { LoginFlow } from "@/mobile-components/shell/widgets/UsageButton"

// ── Claude Code usage — standalone mobile PAGE (flat / touch-first) ──────
//
// Divergent twin of `components/agents/ClaudeUsagePage`. Reworked from the
// grouped-card layout to a FLAT, boxless iOS-native screen (T646): no card
// chrome — sections are separated by air + hairlines, the primary metric is a
// hero number, and the controls are big touch targets. The desktop-sized shared
// `UsageLimits` bars are replaced by INLINE full-width animated bars (a big fill
// sweep, animejs) so the surface feels native rather than a shrunk popover.
//
// Only the two genuinely-complex, mutation/PKCE-bearing widgets stay shared —
// `StoredAccounts` (account store/switch/delete) and `LoginFlow` (the OAuth
// state machine) — rendered boxless. Everything else (token line, usage bars,
// hero) is presentational over the shared `useClaudeUsage` hook (M141), so all
// the query/refresh/auto-refresh orchestration lives in one place and only the
// chrome forks here.

/** Relative expiry string. */
function formatExpiry(epochMs: number): string {
  const diff = epochMs - Date.now()
  if (diff < 0) return "Expired"
  const mins = Math.floor(diff / 60_000)
  if (mins < 60) return `${mins}m left`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ${mins % 60}m left`
  return `${Math.floor(hrs / 24)}d left`
}

/** Human label for a usage-limit `kind` (mirrors the desktop helper). */
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

/** Relative reset string for a limit's `resets_at`. */
function formatReset(iso: string | null | undefined): string {
  if (!iso) return ""
  const d = new Date(iso)
  const diffMs = d.getTime() - Date.now()
  if (diffMs < 0) return "resetting…"
  if (diffMs / 3_600_000 < 24) {
    return `Resets ${d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`
  }
  return `Resets ${d.toLocaleDateString([], { month: "short", day: "numeric" })}`
}

/** Severity/percent → bar fill colour token. */
function barColor(severity: string, pct: number): string {
  if (severity === "critical" || pct >= 90) return "var(--danger)"
  if (severity === "warning" || pct >= 70) return "var(--warn)"
  return "var(--ok)"
}

/** A flat titled section — a muted label over full-bleed content, no card box.
 *  Sections are separated by a top hairline + generous padding (the iOS
 *  "Settings app" grouped feel without the boxes). */
function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <section className="border-t border-border/40 px-5 py-6 first:border-t-0">
      <span className="mb-4 block text-[12px] font-semibold tracking-wide text-muted-foreground/60 uppercase">
        {label}
      </span>
      {children}
    </section>
  )
}

/** Hero — the session-usage percentage as a big count-up number over a tall
 *  track, plus the reset caption. animejs counts the number up and sweeps the
 *  fill on mount. */
function Hero({ pct, resets }: { pct: number; resets: string }) {
  const numRef = useRef<HTMLSpanElement>(null)
  const fillRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const num = numRef.current
    const fill = fillRef.current
    if (!num || !fill) return
    if (prefersReducedMotion()) {
      num.textContent = String(pct)
      fill.style.transform = `scaleX(${pct / 100})`
      return
    }
    const o = { v: 0 }
    animate(o, {
      v: pct,
      duration: 900,
      ease: "out(3)",
      onUpdate: () => {
        num.textContent = String(Math.round(o.v))
      },
    })
    animate(fill, {
      scaleX: [0, pct / 100],
      duration: 900,
      ease: "out(3)",
    })
  }, [pct])
  return (
    <div className="px-5 pt-2 pb-6">
      <div className="flex items-baseline gap-1.5">
        <span
          ref={numRef}
          className="text-[64px] leading-none font-bold tracking-tight text-foreground tabular-nums"
        >
          0
        </span>
        <span className="text-[28px] font-semibold text-muted-foreground/50">%</span>
        <span className="ml-auto text-[13px] font-medium text-muted-foreground/60">
          session used
        </span>
      </div>
      <div className="mt-4 h-2.5 w-full overflow-hidden rounded-full bg-muted">
        <div
          ref={fillRef}
          className="h-full origin-left rounded-full bg-(--signal)"
          style={{ transform: "scaleX(0)" }}
        />
      </div>
      {resets && <p className="mt-2 text-[12.5px] text-muted-foreground/60">{resets}</p>}
    </div>
  )
}

/** Token line — email + valid/expired dot + relative expiry as a flat headline,
 *  with a big full-width Refresh pill (spring pop on tap). No box. */
function TokenLine({
  email,
  isLoading,
  hasData,
  isValid,
  expiresAt,
  refreshPending,
  refreshError,
  onRefresh,
}: {
  email: string | null | undefined
  isLoading: boolean
  hasData: boolean
  isValid: boolean
  expiresAt: number | null | undefined
  refreshPending: boolean
  refreshError: unknown
  onRefresh: () => void
}) {
  const btnRef = useRef<HTMLButtonElement>(null)
  const press = () => {
    const el = btnRef.current
    if (!el || prefersReducedMotion()) return
    animate(el, { scale: [0.94, 1], duration: 300, ease: "out(3)" })
  }
  return (
    <div className="flex flex-col gap-4">
      {email && <span className="truncate text-[14px] text-muted-foreground">{email}</span>}
      {isLoading && !hasData ? (
        <span className="flex items-center gap-2.5 text-[15px] text-muted-foreground">
          <Loader2 className="size-5 animate-spin" /> Checking token…
        </span>
      ) : hasData ? (
        <div className="flex items-center gap-3">
          <span className={cn("size-2.5 shrink-0 rounded-full", isValid ? "bg-ok" : "bg-danger")} />
          <span className="text-[18px] font-semibold text-foreground">
            {isValid ? "Token valid" : "Token expired"}
          </span>
          {isValid && expiresAt != null && (
            <span className="ml-auto text-[13px] text-muted-foreground tabular-nums">
              {formatExpiry(expiresAt)}
            </span>
          )}
        </div>
      ) : null}
      <button
        ref={btnRef}
        onClick={() => {
          press()
          onRefresh()
        }}
        disabled={refreshPending}
        className="flex h-13 w-full items-center justify-center gap-2.5 rounded-2xl bg-muted text-[16px] font-semibold text-foreground/90 transition-colors active:bg-muted/70 disabled:opacity-50"
      >
        {refreshPending ? (
          <Loader2 className="size-5 animate-spin" />
        ) : (
          <RefreshCw className="size-5" />
        )}
        {refreshPending ? "Refreshing…" : "Refresh token"}
      </button>
      {refreshError != null && (
        <span className="text-[13px] text-danger">
          Refresh failed: {refreshError instanceof Error ? refreshError.message : "unknown error"}
        </span>
      )}
    </div>
  )
}

/** Big inline usage bars — one full-width row per non-zero limit, each a tall
 *  rounded bar whose fill sweeps in (animejs, staggered) on mount. Replaces the
 *  desktop-sized shared `UsageLimits` on this page. */
function UsageBars({
  limits,
  isLoading,
  isError,
}: {
  limits: ClaudeUsageLimit[]
  isLoading: boolean
  isError: boolean
}) {
  const wrapRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = wrapRef.current
    if (!el || prefersReducedMotion() || limits.length === 0) return
    const fills = el.querySelectorAll<HTMLElement>("[data-bar-fill]")
    fills.forEach((f, i) => {
      f.style.transform = "scaleX(0)"
      animate(f, {
        scaleX: [0, Number(f.dataset["pct"] ?? 0) / 100],
        delay: i * 70,
        duration: 800,
        ease: "out(3)",
      })
    })
  }, [limits])

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-6">
        <Loader2 className="size-6 animate-spin text-muted-foreground" />
      </div>
    )
  }
  if (isError) return <p className="text-[14px] text-muted-foreground">Could not fetch usage.</p>
  if (limits.length === 0) {
    return <p className="text-[14px] text-muted-foreground">No active usage limits.</p>
  }
  return (
    <div ref={wrapRef} className="flex flex-col gap-5">
      {limits.map((l) => {
        const pct = Math.min(l.percent, 100)
        return (
          <div key={l.kind} className="flex flex-col gap-2">
            <div className="flex items-baseline justify-between">
              <span className="text-[15px] font-semibold text-foreground">
                {limitLabel(l.kind)}
              </span>
              <span className="text-[15px] font-semibold text-foreground/80 tabular-nums">
                {pct}%
              </span>
            </div>
            <div className="h-3 w-full overflow-hidden rounded-full bg-muted">
              <div
                data-bar-fill
                data-pct={pct}
                className="h-full origin-left rounded-full"
                style={{ background: barColor(l.severity, pct), transform: `scaleX(${pct / 100})` }}
              />
            </div>
            {l.resets_at && (
              <span className="text-[12.5px] text-muted-foreground/60">
                {formatReset(l.resets_at)}
              </span>
            )}
          </div>
        )
      })}
    </div>
  )
}

/**
 * Standalone mobile Claude Code usage page — flat, touch-first (T646). Reached
 * from the Agents-screen top-right CornerButton. A hero session number, a big
 * token line + refresh pill, big animated usage bars, and the shared stored
 * accounts + PKCE login, all boxless. `onClose` returns to the opener.
 */
export function ClaudeUsagePage({ onClose }: { onClose: () => void }) {
  const {
    tokenStatus,
    usage,
    isValid,
    limits,
    sessionPct,
    refreshPending,
    refreshError,
    refresh,
    invalidate,
  } = useClaudeUsage(true)
  const data = tokenStatus.data
  const sessionLimit = usage.data?.limits?.find((l) => l.kind === "session")

  // Section cascade — stagger the flat sections in on mount (reduced-motion at
  // rest). The hero + bars run their own fill animations on top.
  const bodyRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = bodyRef.current
    if (!el || prefersReducedMotion()) return
    animate(el.children, {
      opacity: [0, 1],
      translateY: [14, 0],
      delay: stagger(55),
      duration: 320,
      ease: "out(2)",
    })
  }, [])

  return (
    <div className="fixed inset-0 z-40 flex flex-col bg-background">
      <CornerButton side="left" label="Back" onClick={onClose}>
        <ChevronLeft />
      </CornerButton>

      <div ref={bodyRef} className="min-h-0 flex-1 overflow-y-auto">
        <header className="flex flex-col gap-0.5 px-5 pt-[calc(env(safe-area-inset-top)+4.5rem)] pb-2">
          <h1 className="text-[30px] leading-none font-bold tracking-tight text-foreground">
            Claude usage
          </h1>
          <span className="truncate text-[13.5px] text-muted-foreground/60">
            {data?.account_email ?? "Anthropic session & weekly limits"}
          </span>
        </header>

        {isValid && <Hero pct={sessionPct} resets={formatReset(sessionLimit?.resets_at)} />}

        <Section label="Token">
          <TokenLine
            email={data?.account_email}
            isLoading={tokenStatus.isLoading}
            hasData={data !== undefined}
            isValid={isValid}
            expiresAt={data?.expires_at}
            refreshPending={refreshPending}
            refreshError={refreshError}
            onRefresh={refresh}
          />
        </Section>

        <Section label="Usage limits">
          <UsageBars limits={limits} isLoading={usage.isLoading} isError={usage.isError} />
        </Section>

        <Section label="Accounts">
          <StoredAccounts isValid={isValid} onSwitch={invalidate} />
        </Section>

        <Section label="Sign in">
          <LoginFlow onDone={invalidate} />
        </Section>

        <div aria-hidden className="h-[max(2rem,env(safe-area-inset-bottom))]" />
      </div>
    </div>
  )
}
