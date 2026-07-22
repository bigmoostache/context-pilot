import { Loader2, RefreshCw } from "lucide-react"
import { useClaudeUsage } from "@/lib/live/useClaudeUsage"
import { cn } from "@/lib/utils"
import { UsageLimits } from "../shell/widgets/UsageLimits"
import { StoredAccounts } from "../shell/widgets/StoredAccounts"
import { LoginFlow } from "../shell/widgets/UsageButton"

// ── Claude Code usage — standalone page body (shared source-of-truth) ─────
//
// The Claude Code OAuth/usage surface as a self-contained PAGE body (token
// status + expiry + refresh, rate-limit bars, stored accounts, PKCE login),
// factored out of the header `UsageButton` popover so it can be presented as a
// full screen rather than a floating dialog. Desktop renders it in a plain
// scroll container; the mobile twin (mobile-components/agents/ClaudeUsagePage)
// wraps the SAME content in a full-screen iOS page (fixed inset, back chevron,
// grouped inset cards). All the query/refresh/auto-refresh orchestration lives
// in the shared `useClaudeUsage` hook (M141 — logic out of the component), and
// the three sub-widgets (UsageLimits / StoredAccounts / LoginFlow) are shared,
// so the two presentations never drift.
//
// Lives in agents/ (not shell/widgets/) because widgets/ is at the 8-entry dir
// cap; the Anthropic account-usage surface is fleet-adjacent (opened from the
// fleet home) so agents/ is an honest home for it.

/** Relative expiry string (mirrors the UsageButton helper). */
function formatExpiry(epochMs: number): string {
  const diff = epochMs - Date.now()
  if (diff < 0) return "Expired"
  const mins = Math.floor(diff / 60_000)
  if (mins < 60) return `${mins}m left`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ${mins % 60}m left`
  return `${Math.floor(hrs / 24)}d left`
}

/**
 * The token status row: account email, valid/expired dot + relative expiry, and
 * a manual refresh button. Presentational — every value comes from the shared
 * {@link useClaudeUsage} state passed down by the page.
 */
export function ClaudeTokenStatus({
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
    <div className="flex flex-col gap-2 rounded-lg border border-border bg-card px-3.5 py-3">
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
            className="ml-auto flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
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
 * Desktop Claude usage page body — a plain vertical stack of the token status,
 * usage bars, stored accounts and login flow. Rendered in a normal scroll
 * container (no dialog chrome). The mobile twin diverges into a full-screen iOS
 * page; both share this content via the sub-widgets + the useClaudeUsage hook.
 */
export function ClaudeUsagePage() {
  const { tokenStatus, usage, isValid, limits, refreshPending, refreshError, refresh, invalidate } =
    useClaudeUsage(true)
  const data = tokenStatus.data

  return (
    <div className="mx-auto flex w-full max-w-lg flex-col gap-4 p-6">
      <h1 className="text-[18px] font-semibold text-foreground">Claude Code usage</h1>
      <ClaudeTokenStatus
        email={data?.account_email}
        isLoading={tokenStatus.isLoading}
        hasData={data !== undefined}
        isValid={isValid}
        expiresAt={data?.expires_at}
        refreshPending={refreshPending}
        refreshError={refreshError}
        onRefresh={refresh}
      />
      <UsageLimits
        isValid={isValid}
        isLoading={usage.isLoading}
        isError={usage.isError}
        limits={limits}
      />
      <StoredAccounts isValid={isValid} onSwitch={invalidate} />
      <div className="border-t border-border pt-3">
        <LoginFlow onDone={invalidate} />
      </div>
    </div>
  )
}
