import { useEffect, useMemo, useRef, useState } from "react"
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import {
  fetchClaudeUsage,
  fetchClaudeTokenStatus,
  refreshClaudeLogin,
  fetchClaudeAccounts,
  storeClaudeAccount,
  switchClaudeAccount,
  deleteClaudeAccount,
  startClaudeLogin,
  completeClaudeLogin,
} from "@/lib/api"
import type { ClaudeAccountSummary } from "@/lib/api/generated/types.gen"
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

// ── Claude stored-accounts orchestration ─────────────────────────────
//
// The multi-account store/switch/delete state behind the "Stored accounts"
// surface. Co-located with the usage hook (same Claude-Code domain, and the
// lib/live directory is at its 8-entry structure cap) so BOTH the desktop
// `StoredAccounts` widget and the mobile usage page's big-touch account list
// consume ONE implementation and only their chrome forks (M141).

/** Everything an accounts surface needs: the live list plus the three mutators
 *  (store the current account, switch to a stored one, delete a stored one) with
 *  their in-flight / error flags. A successful switch invalidates the
 *  token-dependent caches and fires `onSwitched`. */
export interface ClaudeAccounts {
  accounts: ClaudeAccountSummary[]
  /** store the currently-active credential as a named account. */
  storeCurrent: () => void
  storing: boolean
  storeError: unknown
  /** switch the active credential to a stored account (by email). */
  switchTo: (email: string) => void
  switching: boolean
  switchError: unknown
  /** delete a stored account (by email). */
  remove: (email: string) => void
}

/**
 * Drive the Claude stored-accounts list. `onSwitched` runs after a successful
 * switch (the owning surface passes its token-cache invalidator, so the rest of
 * the app re-reads the now-current credential).
 */
export function useClaudeAccounts(onSwitched: () => void): ClaudeAccounts {
  const queryClient = useQueryClient()

  const accountsQuery = useQuery({
    queryKey: ["claude-accounts"],
    queryFn: fetchClaudeAccounts,
    staleTime: 10_000,
    retry: 1,
  })

  const storeMutation = useMutation({
    mutationFn: storeClaudeAccount,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["claude-accounts"] })
    },
  })

  const switchMutation = useMutation({
    mutationFn: switchClaudeAccount,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["claude-token-status"] })
      void queryClient.invalidateQueries({ queryKey: ["claude-usage"] })
      void queryClient.invalidateQueries({ queryKey: ["claude-accounts"] })
      onSwitched()
    },
  })

  const deleteMutation = useMutation({
    mutationFn: deleteClaudeAccount,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["claude-accounts"] })
    },
  })

  return {
    accounts: accountsQuery.data?.accounts ?? [],
    storeCurrent: () => storeMutation.mutate(),
    storing: storeMutation.isPending,
    storeError: storeMutation.isError ? storeMutation.error : null,
    switchTo: (email: string) => switchMutation.mutate(email),
    switching: switchMutation.isPending,
    switchError: switchMutation.isError ? switchMutation.error : null,
    remove: (email: string) => deleteMutation.mutate(email),
  }
}

// ── Claude OAuth (PKCE) login flow ───────────────────────────────────
//
// The Claude Code OAuth login STATE MACHINE. Co-located here (same domain +
// lib/live dir cap) so the desktop `LoginFlow` widget and the mobile usage
// page's big-touch sign-in both drive the SAME flow and only their chrome forks
// (M141). Owns the step state, the start/complete mutations, and the
// auto-detect poller that finishes the flow when a genuinely-new token lands.

export type LoginStep = "idle" | "starting" | "waiting_for_code" | "completing" | "done" | "error"

/** The login flow surface: current step, the authorize URL + pasted code, an
 *  error string, and the three transitions (start, submit the code, reset). */
export interface ClaudeLogin {
  step: LoginStep
  authorizeUrl: string
  code: string
  setCode: (v: string) => void
  error: string
  /** begin the OAuth flow — opens the authorize page and advances to paste. */
  start: () => void
  starting: boolean
  /** submit the pasted code#state to complete the exchange. */
  submit: () => void
  submitting: boolean
  /** reset back to idle (clears code + error) — the "Try again" action. */
  reset: () => void
}

/**
 * Drive the Claude OAuth PKCE login. `onDone` fires ~1.5s after a successful
 * login (whether via the completion mutation or the auto-detect poller).
 *
 * Auto-detect (T472): while waiting for the pasted code, a 2s poller watches the
 * token status; a token whose expiry has STRICTLY advanced past the baseline
 * snapshot (taken when the flow started) is accepted as a fresh login. A
 * pre-existing (possibly stale) token can never be mistaken for a fresh login.
 */
export function useClaudeLogin(onDone: () => void): ClaudeLogin {
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

  // Keep the latest onDone in a ref so the poll effect binds ONCE (deps [step])
  // without re-subscribing when the parent recreates onDone.
  const onDoneRef = useRef(onDone)
  useEffect(() => {
    onDoneRef.current = onDone
  })

  const baselineExpiryRef = useRef<number | null>(null)
  useEffect(() => {
    if (step !== "waiting_for_code") return
    let cancelled = false
    // Snapshot the starting expiry once, before polling begins.
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
    const handlePollResult = (status: Awaited<ReturnType<typeof fetchClaudeTokenStatus>>) => {
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

  return {
    step,
    authorizeUrl,
    code,
    setCode,
    error,
    start: () => {
      setStep("starting")
      startMutation.mutate()
    },
    starting: startMutation.isPending,
    submit: () => {
      setStep("completing")
      completeMutation.mutate(code.trim())
    },
    submitting: completeMutation.isPending,
    reset: () => {
      setStep("idle")
      setError("")
      setCode("")
    },
  }
}
