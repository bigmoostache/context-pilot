import { Loader2, RefreshCw } from "lucide-react"
import type { useClaudeUsage } from "@/lib/live/useClaudeUsage"
import { cn } from "@/lib/utils"
import { UsageLimits } from "../shell/widgets/UsageLimits"
import { StoredAccounts } from "../shell/widgets/StoredAccounts"

// ── Claude Code usage — shared presentational body ───────────────────────
//
// The Claude Code OAuth/usage surface as a presentational body (token status +
// expiry + refresh, rate-limit bars, stored accounts) driven entirely by props
// from the shared `useClaudeUsage` hook — NO hook or query of its own, so it can
// be dropped into any container without owning a second subscription. The header
// `UsageButton` popover renders it (below its own title, then appends the login
// flow); the divergent mobile twin (mobile-components/agents/ClaudeUsagePage)
// wraps the SAME concerns in a full-screen iOS page.
//
// It deliberately does NOT render `LoginFlow`: LoginFlow lives inside
// `UsageButton`, and importing it here while `UsageButton` imports this body
// would form an import cycle (import-x/no-cycle). The owning surface renders the
// login flow itself, after this body.
//
// Lives in agents/ (not shell/widgets/) because widgets/ is at the 8-entry dir
// cap; the Anthropic account-usage surface is fleet-adjacent (opened from the
// fleet home) so agents/ is an honest home for it.

type ClaudeUsage = ReturnType<typeof useClaudeUsage>

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

/** Token status row — account email, valid/expired dot + relative expiry, and a
 *  manual refresh button. Presentational; every value comes from the shared
 *  usage state passed in. */
function ClaudeTokenStatus({
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
    <div className="flex flex-col gap-2 rounded-md bg-muted/40 px-2.5 py-2">
      {email && <span className="truncate text-[11px] text-muted-foreground">{email}</span>}
      {isLoading && !hasData ? (
        <span className="flex items-center gap-2 text-[12px] text-muted-foreground">
          <Loader2 className="size-4 animate-spin" /> Checking token…
        </span>
      ) : hasData ? (
        <div className="flex items-center gap-2">
          <span className={cn("size-2 shrink-0 rounded-full", isValid ? "bg-ok" : "bg-danger")} />
          <span className="text-[12px] font-medium text-foreground">
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
            className="ml-auto flex size-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
          >
            {refreshPending ? (
              <Loader2 className="size-3 animate-spin" />
            ) : (
              <RefreshCw className="size-3" />
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
 * Shared Claude usage body — token status, usage-limit bars, and stored
 * accounts, container-agnostic (a plain vertical stack, no title/padding of its
 * own). Driven by the {@link useClaudeUsage} result passed by the owning
 * surface, which also renders the login flow after it (kept out of here to avoid
 * an import cycle with `UsageButton`, where `LoginFlow` lives).
 */
export function ClaudeUsageBody({ usage }: { usage: ClaudeUsage }) {
  const { tokenStatus, usage: usageQuery, isValid, limits, refreshPending, refreshError, refresh } =
    usage
  const data = tokenStatus.data

  return (
    <div className="space-y-3">
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
        isLoading={usageQuery.isLoading}
        isError={usageQuery.isError}
        limits={limits}
      />
      <div className="border-t border-border pt-3">
        <StoredAccounts isValid={isValid} onSwitch={usage.invalidate} />
      </div>
    </div>
  )
}
