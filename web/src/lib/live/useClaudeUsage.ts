import { useEffect, useMemo, useRef } from "react"
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { fetchClaudeUsage, fetchClaudeTokenStatus, refreshClaudeLogin } from "@/lib/api"
import { resetProviderCache } from "@/lib/support/models"

// ── Claude Code usage/token orchestration (shared logic) ─────────────
//
// The desktop header "Anthropic" button and the mobile Config usage section
// render the SAME Claude Code OAuth/usage state, so the query + mutation +
// invalidation + auto-refresh orchestration lives here ONCE (repo rule: logic
// out of the component, in @/lib) and both surfaces consume it — only their
// chrome forks. This is pure data/effect composition: it imports nothing from
// the components layer, so it stays boundary-clean (@/lib may import @/lib +
// @/lib/api only).

type TokenStatus = Awaited<ReturnType<typeof fetchClaudeTokenStatus>>
type Usage = Awaited<ReturnType<typeof fetchClaudeUsage>>

/** Everything a usage surface needs: the two live queries, a refresh control,
 *  the token-dependent cache invalidator (reused as the login/switch `onDone`),
 *  and the derived flags/values (valid token, non-zero limits, session %). */
export interface ClaudeUsage {
  tokenStatus: ReturnType<typeof useQuery<TokenStatus>>
  usage: ReturnType<typeof useQuery<Usage>>
  /** true when the current token is valid (gates the usage bars + stored-store). */
  isValid: boolean
  /** usage limits with a non-zero percent (the ones worth drawing a bar for). */
  limits: NonNullable<Usage["limits"]>
  /** session-limit percent clamped to 0–100 (feeds the desktop camembert). */
  sessionPct: number
  /** true while a manual/auto token refresh is in flight. */
  refreshPending: boolean
  /** last refresh error (or null), for surfacing a failure line. */
  refreshError: unknown
  /** trigger a manual token refresh. */
  refresh: () => void
  /** drop every token-dependent cache (token/usage/providers) — the login +
   *  account-switch success handler. */
  invalidate: () => void
}

/**
 * Drive the Claude Code usage/token state. `polling` (default true) turns on the
 * faster 30s cadence a foreground/open surface wants; a background indicator can
 * pass false for the 5min idle cadence. Auto-refreshes the token when it is
 * within 30 minutes of expiry (once per expiry window, ref-guarded so recreating
 * the mutation object doesn't re-subscribe the effect).
 */
export function useClaudeUsage(polling = true): ClaudeUsage {
  const queryClient = useQueryClient()

  // A token change can flip the OAuth providers between usable and not, so every
  // token-affecting success must also refresh the provider registry — drop the
  // memoised singleton and invalidate the ["providers"] query (prefix-matches
  // the picker's ["providers","picker"]) so mounted pickers refetch instead of
  // showing "No models available" until a full reload.
  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ["claude-token-status"] })
    void queryClient.invalidateQueries({ queryKey: ["claude-usage"] })
    resetProviderCache()
    void queryClient.invalidateQueries({ queryKey: ["providers"] })
  }

  const refreshMutation = useMutation({
    mutationFn: refreshClaudeLogin,
    onSuccess: invalidate,
  })

  const interval = polling ? 30_000 : 300_000
  const tokenStatus = useQuery({
    queryKey: ["claude-token-status"],
    queryFn: fetchClaudeTokenStatus,
    refetchInterval: interval,
    staleTime: 10_000,
    retry: 1,
  })

  const usage = useQuery({
    queryKey: ["claude-usage"],
    queryFn: fetchClaudeUsage,
    // Only poll usage while the token is valid — feeds the bars + indicator.
    enabled: tokenStatus.data?.valid === true,
    refetchInterval: interval,
    staleTime: 10_000,
    retry: 1,
  })

  const isValid = tokenStatus.data?.valid === true
  const limits = (usage.data?.limits ?? []).filter((l) => l.percent > 0)

  const sessionPct = useMemo(() => {
    const session = usage.data?.limits?.find((l) => l.kind === "session")
    return session ? Math.min(session.percent, 100) : 0
  }, [usage.data?.limits])

  // Auto-refresh when the token expires within 30 minutes. The ref pattern keeps
  // the effect bound to [tokenStatus.data] without re-subscribing when the
  // mutation object is recreated; the attempt guard fires once per expiry window.
  const refreshMutateRef = useRef(refreshMutation.mutate)
  useEffect(() => {
    refreshMutateRef.current = refreshMutation.mutate
  })
  const autoRefreshAttemptedRef = useRef(false)
  useEffect(() => {
    const status = tokenStatus.data
    if (!status?.valid || status.expires_at == null) return
    const remaining = status.expires_at - Date.now()
    if (remaining > 0 && remaining < 30 * 60_000) {
      if (!autoRefreshAttemptedRef.current) {
        autoRefreshAttemptedRef.current = true
        refreshMutateRef.current()
      }
    } else {
      autoRefreshAttemptedRef.current = false
    }
  }, [tokenStatus.data])

  return {
    tokenStatus,
    usage,
    isValid,
    limits,
    sessionPct,
    refreshPending: refreshMutation.isPending,
    refreshError: refreshMutation.isError ? refreshMutation.error : null,
    refresh: () => refreshMutation.mutate(),
    invalidate,
  }
}
