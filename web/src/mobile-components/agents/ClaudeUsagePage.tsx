import { useEffect, useRef } from "react"
import { animate, stagger } from "animejs"
import { ChevronLeft, Loader2, RefreshCw } from "lucide-react"
import { useClaudeUsage } from "@/lib/live/useClaudeUsage"
import { cn, prefersReducedMotion } from "@/lib/utils"
import { CornerButton } from "@/mobile-components/shell/CornerButton"
import { UsageLimits } from "@/mobile-components/shell/widgets/UsageLimits"
import { StoredAccounts } from "@/mobile-components/shell/widgets/StoredAccounts"
import { LoginFlow } from "@/mobile-components/shell/widgets/UsageButton"

// ── Claude Code usage — standalone mobile PAGE ───────────────────────────
//
// Divergent twin of `components/agents/ClaudeUsagePage`. The user asked for the
// Anthropic usage surface as its OWN full-screen page styled like the rest of
// the mobile app — NOT buried in the multi-tab Config dialog. So this is the iOS
// grouped-list idiom every other mobile page uses (the Agent Settings page,
// T636/T638): a `fixed inset-0` screen, a floating glass `CornerButton` back
// control, a big left-aligned safe-area header, and each concern in its own
// muted-labelled grouped card.
//
// Placed in agents/ (not shell/widgets/) because widgets/ sits at the 8-entry
// dir cap; the Anthropic account-usage surface is fleet-adjacent (opened from
// the fleet home gear), so agents/ is its honest home.
//
// The usage bars, stored accounts and PKCE login are the SHARED sub-widgets
// (mobile-mirror tokens re-exporting desktop) and all the query/refresh
// orchestration is the shared `useClaudeUsage` hook (M141), so only the page
// chrome + the small token-status row fork here.

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

/** A titled grouped section — muted label above a rounded inset card (the iOS
 *  "Settings app" idiom). `card` wraps the children in the grouped container;
 *  omit it for a body that draws its own (the usage bars). */
function Section({
  label,
  card,
  children,
}: {
  label?: string
  card?: boolean
  children: React.ReactNode
}) {
  return (
    <section className="px-4 py-2.5">
      {label && (
        <span className="mb-1.5 block px-1 text-[12.5px] font-medium text-muted-foreground/60">
          {label}
        </span>
      )}
      {card ? (
        <div className="overflow-hidden rounded-2xl border border-border/60 bg-card p-3.5">
          {children}
        </div>
      ) : (
        children
      )}
    </section>
  )
}

/** Token status row — account email, valid/expired dot + relative expiry, and a
 *  manual refresh button. */
function TokenStatus({
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
  return (
    <div className="flex flex-col gap-2 rounded-2xl border border-border/60 bg-card px-3.5 py-3">
      {email && <span className="truncate text-[12px] text-muted-foreground">{email}</span>}
      {isLoading && !hasData ? (
        <span className="flex items-center gap-2 text-[12px] text-muted-foreground">
          <Loader2 className="size-4 animate-spin" /> Checking token…
        </span>
      ) : hasData ? (
        <div className="flex items-center gap-2.5">
          <span className={cn("size-2 shrink-0 rounded-full", isValid ? "bg-ok" : "bg-danger")} />
          <span className="text-[13px] font-medium text-foreground/90">
            {isValid ? "Token valid" : "Token expired"}
          </span>
          {isValid && expiresAt != null && (
            <span className="text-[11px] text-muted-foreground tabular-nums">
              {formatExpiry(expiresAt)}
            </span>
          )}
          <button
            onClick={onRefresh}
            disabled={refreshPending}
            aria-label="Refresh token"
            className="ml-auto flex size-9 shrink-0 items-center justify-center rounded-lg text-muted-foreground/70 transition-colors active:bg-muted active:text-foreground disabled:opacity-50"
          >
            {refreshPending ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <RefreshCw className="size-4" />
            )}
          </button>
        </div>
      ) : null}
      {refreshError != null && (
        <span className="text-[11px] text-danger">
          Refresh failed: {refreshError instanceof Error ? refreshError.message : "unknown error"}
        </span>
      )}
    </div>
  )
}

/**
 * Standalone mobile Claude Code usage page. Reached from the Agents-screen gear.
 * Shows token status/expiry/refresh, rate-limit bars, stored accounts and the
 * PKCE login — the same surface the desktop header Anthropic button opens, here
 * as a native full-screen page. `onClose` returns to wherever it was opened from.
 */
export function ClaudeUsagePage({ onClose }: { onClose: () => void }) {
  const { tokenStatus, usage, isValid, limits, refreshPending, refreshError, refresh, invalidate } =
    useClaudeUsage(true)
  const data = tokenStatus.data

  // Section cascade (anime.js) — stagger the cards in on mount for the iOS
  // settings-page reveal; runs once, reduced-motion shows them at rest.
  const bodyRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = bodyRef.current
    if (!el || prefersReducedMotion()) return
    animate(el.children, {
      opacity: [0, 1],
      translateY: [12, 0],
      delay: stagger(45),
      duration: 300,
      ease: "out(2)",
    })
  }, [])

  return (
    <div className="fixed inset-0 z-40 flex flex-col bg-background">
      {/* App-wide glass back control, floating top-left — same primitive every
          other mobile page uses. */}
      <CornerButton side="left" label="Back" onClick={onClose}>
        <ChevronLeft />
      </CornerButton>

      <div ref={bodyRef} className="min-h-0 flex-1 overflow-y-auto">
        {/* Header matches the agents / settings pages: big left title, top pad
            clears the floating CornerButton. */}
        <header className="flex flex-col gap-0.5 px-4 pt-[calc(env(safe-area-inset-top)+4.5rem)] pb-1">
          <h1 className="text-[28px] leading-none font-bold tracking-tight text-foreground">
            Claude usage
          </h1>
          <span className="truncate text-[13px] text-muted-foreground/70">
            {data?.account_email ?? "Anthropic session & weekly limits"}
          </span>
        </header>

        <Section label="Token">
          <TokenStatus
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

        <Section label="Usage limits" card>
          <UsageLimits
            isValid={isValid}
            isLoading={usage.isLoading}
            isError={usage.isError}
            limits={limits}
          />
        </Section>

        <Section label="Accounts" card>
          <StoredAccounts isValid={isValid} onSwitch={invalidate} />
        </Section>

        <Section label="Sign in">
          <LoginFlow onDone={invalidate} />
        </Section>

        {/* bottom safe-area breathing room */}
        <div aria-hidden className="h-[max(1.5rem,env(safe-area-inset-bottom))]" />
      </div>
    </div>
  )
}
