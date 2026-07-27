import { CheckCircle2, ExternalLink, Loader2, LogIn, XCircle } from "lucide-react"
import { useClaudeLogin } from "@/lib/live/useClaudeUsage"

// ── Claude OAuth login — mobile big-touch twin ───────────────────────
//
// Divergent (marker-less) twin of `components/shell/widgets/UsageButton`. The
// desktop file exports the Anthropic-logo popover button AND the `LoginFlow`
// PKCE surface; on mobile the popover button is never rendered (there is no
// TopBar), but `LoginFlow` IS — on the standalone usage page and in the Config
// Secrets pane — so it must be a real, native-sized surface rather than the
// dense desktop chrome the stub re-exported.
//
// This forks PRESENTATION only: the full step machine (start / paste-code /
// complete / auto-poll) stays shared in `useClaudeLogin` (M141). Every control
// is a big (≥52px) touch target with a 16px paste field (below 16px iOS Safari
// auto-zooms on focus). Boxless — the owning page wraps it in a flat `Section`.
//
// `UsageButton` is re-exported as a no-render stub purely for path/export
// parity with the desktop twin (the mirror requires the same export surface);
// nothing on mobile mounts it (leak guard forbids importing the desktop one).

/** Paste-your-code step — big touch field + submit, native sizing. */
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
    <div className="flex flex-col gap-4">
      <p className="text-[14px] text-muted-foreground">
        After authorizing, Anthropic shows you a code. Copy the full{" "}
        <code className="rounded-sm bg-muted px-1 text-[13px]">code#state</code> string and paste it
        below.
      </p>
      <a
        href={authorizeUrl}
        target="_blank"
        rel="noopener noreferrer"
        className="flex items-center gap-1.5 text-[13.5px] font-medium text-(--signal) active:underline"
      >
        <ExternalLink className="size-4" /> Re-open authorization page
      </a>
      <input
        type="text"
        value={code}
        onChange={(e) => setCode(e.target.value)}
        placeholder="Paste code or full callback URL…"
        autoFocus
        className="h-13 w-full rounded-2xl border border-border bg-muted/50 px-4 text-[16px] text-foreground placeholder:text-muted-foreground/50 focus:border-(--signal) focus:outline-none"
      />
      <button
        onClick={onSubmit}
        disabled={!code.trim() || submitting}
        className="flex h-13 w-full items-center justify-center gap-2.5 rounded-2xl bg-foreground text-[16px] font-semibold text-background transition-[filter] active:brightness-110 disabled:opacity-50"
      >
        {submitting ? <Loader2 className="size-5 animate-spin" /> : null}
        {submitting ? "Verifying…" : "Submit code"}
      </button>
    </div>
  )
}

/**
 * Claude OAuth login flow — mobile big-touch. Same step machine as desktop via
 * the shared {@link useClaudeLogin} hook; only the chrome is native-sized.
 * `onDone` fires ~1.5s after a successful login.
 */
export function LoginFlow({ onDone }: { onDone: () => void }) {
  const { step, authorizeUrl, code, setCode, error, start, starting, submit, submitting, reset } =
    useClaudeLogin(onDone)

  if (step === "idle" || step === "starting") {
    return (
      <button
        onClick={start}
        disabled={starting}
        className="flex h-13 w-full items-center justify-center gap-2.5 rounded-2xl bg-foreground text-[16px] font-semibold text-background transition-[filter] active:brightness-110 disabled:opacity-50"
      >
        {starting ? <Loader2 className="size-5 animate-spin" /> : <LogIn className="size-5" />}
        {starting ? "Starting…" : "Login with Claude"}
      </button>
    )
  }

  if (step === "waiting_for_code") {
    return (
      <WaitingForCode
        authorizeUrl={authorizeUrl}
        code={code}
        setCode={setCode}
        submitting={submitting}
        onSubmit={submit}
      />
    )
  }

  if (step === "completing") {
    return (
      <div className="flex items-center justify-center gap-2.5 py-4 text-[15px] text-muted-foreground">
        <Loader2 className="size-5 animate-spin" /> Completing login…
      </div>
    )
  }

  if (step === "done") {
    return (
      <div className="flex items-center justify-center gap-2.5 py-4 text-[15px] font-medium text-(--ok)">
        <CheckCircle2 className="size-5" /> Logged in!
      </div>
    )
  }

  // error
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2.5 text-[14px] text-(--danger)">
        <XCircle className="size-5 shrink-0" />
        <span>{error}</span>
      </div>
      <button
        onClick={reset}
        className="h-13 w-full rounded-2xl bg-muted text-[16px] font-semibold text-foreground/90 transition-colors active:bg-muted/70"
      >
        Try again
      </button>
    </div>
  )
}

/** No-render stub — export parity with the desktop twin only. The Anthropic
 *  popover button is desktop TopBar chrome; mobile has no TopBar and never
 *  mounts it, so this returns null. */
export function UsageButton() {
  return null
}
