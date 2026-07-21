import { useEffect, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  AlertTriangle,
  ArrowUpCircle,
  CheckCircle2,
  ExternalLink,
  Loader2,
  RefreshCw,
  RotateCcw,
} from "lucide-react"
import { applyUpdate, checkForUpdate, fetchUpdateStatus, setUpdateMode } from "@/lib/api"
import type { UpdateStatus } from "@/lib/api"
import { cn } from "@/lib/utils"

/** The three update postures (update-policy §5.9, O5.2). */
const MODES = [
  { id: "auto", label: "Automatic", detail: "Apply at night, inside the maintenance window" },
  { id: "manual", label: "Manual", detail: "Show what's available — you apply it" },
  { id: "paused", label: "Paused", detail: "Keep checking, never apply" },
] as const

/**
 * Admin-only *Update* pane (O5.2) — mobile twin of `components/shell/config/
 * UpdatePane`.
 *
 * Shows the running version + channel, whether the channel offers a newer
 * release, the `auto`/`manual`/`paused` toggle and the maintenance window —
 * all server-persisted via `/api/update/*`. During an apply the console
 * restarts: the pane polls the status endpoint until it answers again and
 * reports success (running the new version) or an automatic rollback.
 *
 * Divergence from desktop is touch-only: the action buttons grow and swap
 * `hover:` for `active:`, and the maintenance-window `time` inputs carry a 16px
 * font so iOS Safari doesn't auto-zoom on focus. Every mutation — check, apply,
 * mode/channel/window set, the apply-progress poll — is byte-identical to the
 * desktop twin (it lives in the shared `@/lib/api` layer, not forked).
 */
export function UpdatePane() {
  const qc = useQueryClient()
  const [applying, setApplying] = useState<{ from: string; to: string } | null>(null)
  const { data, isLoading, isError, error, refetch } = useQuery({
    queryKey: ["update-status"],
    queryFn: fetchUpdateStatus,
    enabled: applying === null,
  })

  if (applying) {
    return (
      <ApplyProgress
        target={applying}
        onSettled={() => {
          setApplying(null)
          void qc.invalidateQueries({ queryKey: ["update-status"] })
        }}
      />
    )
  }

  if (isError) {
    return (
      <div className="flex flex-col items-center justify-center gap-2 py-16 text-muted-foreground">
        <span className="text-[12px] text-(--danger)">
          Failed to load update status{error.message ? `: ${error.message}` : ""}
        </span>
        <button
          onClick={() => void refetch()}
          className="flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-muted-foreground transition-colors active:bg-muted/60 active:text-foreground"
        >
          <RefreshCw className="size-3" />
          Retry
        </button>
      </div>
    )
  }

  if (isLoading || !data) {
    return (
      <div className="flex flex-col items-center justify-center gap-2 py-16 text-muted-foreground">
        <Loader2 className="size-5 animate-spin" />
        <span className="text-[12px]">Loading update status…</span>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-5">
      <VersionCard status={data} onApplying={setApplying} />
      <ModeSection status={data} />
      <ChannelSection status={data} />
      {/* Keyed on the server value: when the box reports a new window the
          editor remounts and re-seeds its inputs — no state-sync effect. */}
      <WindowSection key={`${data.window.start}-${data.window.end}`} status={data} />
    </div>
  )
}

/** Current version + availability + Check now / Update now actions. */
function VersionCard({
  status,
  onApplying,
}: {
  status: UpdateStatus
  onApplying: (t: { from: string; to: string }) => void
}) {
  const qc = useQueryClient()
  const upToDate = !status.available

  const check = useMutation({
    mutationFn: checkForUpdate,
    onSuccess: (fresh) => qc.setQueryData(["update-status"], fresh),
  })
  const apply = useMutation({
    mutationFn: applyUpdate,
    onSuccess: (receipt) => {
      if (receipt.status === "applying" && receipt.from && receipt.to) {
        onApplying({ from: receipt.from, to: receipt.to })
      } else {
        void qc.invalidateQueries({ queryKey: ["update-status"] })
      }
    },
  })

  return (
    <section className="card-shadow flex flex-col gap-3 rounded-xl border border-border bg-card px-4 py-3.5">
      <div className="flex items-center gap-3">
        <span
          className={cn(
            "flex size-9 shrink-0 items-center justify-center rounded-lg",
            upToDate ? "bg-(--ok)/15 text-(--ok)" : "bg-(--interactive)/15 text-(--interactive)",
          )}
        >
          {upToDate ? <CheckCircle2 className="size-5" /> : <ArrowUpCircle className="size-5" />}
        </span>
        <div className="flex min-w-0 flex-1 flex-col">
          <span
            className="text-[13.5px] font-medium text-foreground/90"
            data-testid="update-current"
          >
            {status.current}
            <span className="ml-2 text-[11px] font-normal text-muted-foreground">
              {status.channel} · {status.arch}
            </span>
          </span>
          <span className="text-[11.5px] text-muted-foreground" data-testid="update-availability">
            {upToDate ? "Up to date" : `Update available: ${status.available}`}
            {!upToDate && status.notes_url && (
              <a
                href={status.notes_url}
                target="_blank"
                rel="noreferrer"
                className="ml-1.5 inline-flex items-center gap-0.5 text-(--interactive) active:underline"
              >
                notes <ExternalLink className="size-3" />
              </a>
            )}
          </span>
        </div>
      </div>
      {/* Actions on their own row — full-width, touch-sized — so they don't
          crowd the version label at phone width (desktop packs them inline). */}
      <div className="flex items-center gap-2">
        <button
          data-testid="update-check-now"
          disabled={check.isPending}
          onClick={() => check.mutate()}
          className="flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-border px-3 py-2.5 text-[13px] font-medium text-foreground/85 transition-colors active:bg-muted/60 disabled:opacity-50"
        >
          {check.isPending ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <RefreshCw className="size-3.5" />
          )}
          Check now
        </button>
        {!upToDate && (
          <button
            data-testid="update-apply-now"
            disabled={apply.isPending || status.apply_in_flight}
            onClick={() => apply.mutate()}
            className="flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-(--interactive) px-3 py-2.5 text-[13px] font-medium text-(--primary-foreground) transition-[filter] active:brightness-105 disabled:opacity-50"
          >
            {apply.isPending ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <ArrowUpCircle className="size-3.5" />
            )}
            Update now
          </button>
        )}
      </div>
      <LastOutcome status={status} />
      {check.isError && (
        <span className="text-[11px] text-(--danger)">Check failed: {check.error.message}</span>
      )}
      {apply.isError && (
        <span className="text-[11px] text-(--danger)">Apply failed: {apply.error.message}</span>
      )}
    </section>
  )
}

/** Last check instant + last apply outcome, when known. */
function LastOutcome({ status }: { status: UpdateStatus }) {
  const result = status.last_result
  const checked = status.last_check_ms ? new Date(status.last_check_ms).toLocaleString() : "never"
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 border-t border-border/60 pt-2.5 text-[11px] text-muted-foreground">
      <span>Last check: {checked}</span>
      {result?.kind === "success" && (
        <span className="text-(--ok)">Last update: {result.to} installed</span>
      )}
      {result?.kind === "rolled_back" && (
        <span className="flex items-center gap-1 text-(--danger)">
          <RotateCcw className="size-3" />
          {result.attempted} failed — rolled back{result.to ? ` to ${result.to}` : ""}
        </span>
      )}
      {result?.kind === "failed" && (
        <span className="text-(--danger)">Last attempt failed: {result.message}</span>
      )}
    </div>
  )
}

/** The auto / manual / paused selector — server state, no localStorage. */
function ModeSection({ status }: { status: UpdateStatus }) {
  const qc = useQueryClient()
  const setMode = useMutation({
    mutationFn: (mode: "auto" | "manual" | "paused") => setUpdateMode({ mode }),
    onSuccess: (fresh) => qc.setQueryData(["update-status"], fresh),
  })

  return (
    <section className="flex flex-col gap-2">
      <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
        Update mode
      </span>
      <div className="flex flex-col gap-1.5">
        {MODES.map((m) => {
          const on = status.mode === m.id
          return (
            <button
              key={m.id}
              data-testid={`update-mode-${m.id}`}
              aria-pressed={on}
              disabled={setMode.isPending}
              onClick={() => setMode.mutate(m.id)}
              className={cn(
                "card-shadow flex items-center gap-2.5 rounded-xl border px-3.5 py-3 text-left transition-colors",
                on
                  ? "border-(--interactive)/60 bg-(--interactive)/8"
                  : "border-border bg-card active:bg-muted/40",
              )}
            >
              <span
                className={cn(
                  "size-3 shrink-0 rounded-full border-2",
                  on ? "border-(--interactive) bg-(--interactive)" : "border-muted-foreground/40",
                )}
              />
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="text-[13px] font-medium text-foreground/90">{m.label}</span>
                <span className="text-[11px] text-muted-foreground">{m.detail}</span>
              </span>
            </button>
          )
        })}
      </div>
      {setMode.isError && (
        <span className="text-[11px] text-(--danger)">Could not save: {setMode.error.message}</span>
      )}
    </section>
  )
}

/**
 * Nightly opt-in — a single checkbox that flips the box channel
 * `stable ↔ nightly`. Nightly tracks the tip of `master` (every push), so it
 * is untested and may be unstable; the warning banner makes that explicit. The
 * box keeps its existing mode/window, so an `auto` box applies nightly builds
 * in its maintenance window.
 */
function ChannelSection({ status }: { status: UpdateStatus }) {
  const qc = useQueryClient()
  const nightly = status.channel === "nightly"
  const setChannel = useMutation({
    mutationFn: (channel: "stable" | "nightly") => setUpdateMode({ channel }),
    onSuccess: (fresh) => qc.setQueryData(["update-status"], fresh),
  })

  return (
    <section className="flex flex-col gap-2">
      <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
        Release channel
      </span>
      <label
        htmlFor="update-nightly"
        className="card-shadow flex cursor-pointer items-center gap-2.5 rounded-xl border border-border bg-card px-3.5 py-3"
      >
        <input
          id="update-nightly"
          data-testid="update-channel-nightly"
          type="checkbox"
          aria-label="Receive nightly builds"
          checked={nightly}
          disabled={setChannel.isPending}
          onChange={(e) => setChannel.mutate(e.target.checked ? "nightly" : "stable")}
          className="size-4 shrink-0 accent-(--interactive)"
        />
        <span className="flex min-w-0 flex-col">
          <span className="text-[13px] font-medium text-foreground/90">
            Receive nightly builds (unstable)
          </span>
          <span className="text-[11px] text-muted-foreground">
            Follow the tip of the development branch instead of stable releases
          </span>
        </span>
      </label>
      {nightly && (
        <div
          role="alert"
          className="flex items-start gap-2 rounded-lg border border-(--danger)/40 bg-card px-3 py-2.5 text-[11.5px] text-foreground/80"
        >
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-(--danger)" />
          <span>
            Nightly builds ship straight from the latest commit on <code>master</code> with no
            release testing — they can be unstable or break features. Uncheck to return to stable;
            your box will move back to the latest stable release on its next check.
          </span>
        </div>
      )}
      {setChannel.isError && (
        <span className="text-[11px] text-(--danger)">
          Could not save: {setChannel.error.message}
        </span>
      )}
    </section>
  )
}

/** Maintenance-window editor (box-local `HH:MM` bounds, auto mode only). */
function WindowSection({ status }: { status: UpdateStatus }) {
  const qc = useQueryClient()
  // Seeded from the server value; the parent remounts this section (key) when
  // that value changes, so no synchronisation effect is needed.
  const [start, setStart] = useState(status.window.start)
  const [end, setEnd] = useState(status.window.end)

  const save = useMutation({
    mutationFn: () => setUpdateMode({ window: { start, end } }),
    onSuccess: (fresh) => qc.setQueryData(["update-status"], fresh),
  })
  const dirty = start !== status.window.start || end !== status.window.end

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
          Maintenance window
        </span>
        <span className="text-[11px] text-muted-foreground/60">
          Automatic updates run only in this nightly slot (box-local time)
        </span>
      </div>
      <div className="card-shadow flex flex-wrap items-center gap-3 rounded-xl border border-border bg-card px-3.5 py-3">
        <TimeField label="From" value={start} onChange={setStart} testId="update-window-start" />
        <TimeField label="To" value={end} onChange={setEnd} testId="update-window-end" />
        <button
          data-testid="update-window-save"
          disabled={!dirty || save.isPending}
          onClick={() => save.mutate()}
          className="ml-auto rounded-lg border border-border px-3.5 py-2 text-[13px] font-medium text-foreground/85 transition-colors active:bg-muted/60 disabled:opacity-40"
        >
          {save.isPending ? "Saving…" : "Save window"}
        </button>
      </div>
      {save.isError && (
        <span className="text-[11px] text-(--danger)">Could not save: {save.error.message}</span>
      )}
    </section>
  )
}

function TimeField({
  label,
  value,
  onChange,
  testId,
}: {
  label: string
  value: string
  onChange: (v: string) => void
  testId: string
}) {
  return (
    <label className="flex items-center gap-2 text-[12px] text-muted-foreground">
      {label}
      <input
        data-testid={testId}
        type="time"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="rounded-md border border-border bg-background px-2 py-1.5 text-[16px] text-foreground tabular-nums"
      />
    </label>
  )
}

/**
 * The applying state (T5.2.3): the console is restarting into the new
 * version — poll the status endpoint until it answers, then report which
 * version came back (target = success, previous = rolled back).
 */
function ApplyProgress({
  target,
  onSettled,
}: {
  target: { from: string; to: string }
  onSettled: () => void
}) {
  const [outcome, setOutcome] = useState<"success" | "rolled_back" | "unknown" | null>(null)

  useEffect(() => {
    const startedAt = Date.now()
    const settle = (o: "success" | "rolled_back" | "unknown") => {
      clearInterval(poll)
      setOutcome(o)
    }
    const poll = setInterval(() => {
      fetchUpdateStatus()
        .then((status) => {
          if (status.current === target.to) {
            settle("success")
          } else if (status.last_result?.kind === "rolled_back" || status.current === target.from) {
            settle("rolled_back")
          } else if (Date.now() - startedAt > 5 * 60_000) {
            settle("unknown")
          }
        })
        .catch(() => {
          // The console is restarting — keep polling until it answers.
          if (Date.now() - startedAt > 5 * 60_000) settle("unknown")
        })
    }, 2000)
    return () => clearInterval(poll)
  }, [target.from, target.to])

  if (outcome === null) {
    return (
      <div
        className="flex flex-col items-center justify-center gap-3 py-16 text-muted-foreground"
        data-testid="update-applying"
      >
        <Loader2 className="size-6 animate-spin" />
        <span className="text-[13px] font-medium text-foreground/85">
          Updating {target.from} → {target.to}
        </span>
        <span className="text-[11.5px]">
          The console is restarting — this page will recover on its own.
        </span>
      </div>
    )
  }

  return (
    <div
      className="flex flex-col items-center justify-center gap-3 py-16"
      data-testid="update-apply-result"
    >
      {outcome === "success" ? (
        <>
          <CheckCircle2 className="size-6 text-(--ok)" />
          <span className="text-[13px] font-medium text-foreground/85">Updated to {target.to}</span>
        </>
      ) : outcome === "rolled_back" ? (
        <>
          <RotateCcw className="size-6 text-(--danger)" />
          <span className="text-[13px] font-medium text-foreground/85">
            Update failed — rolled back to {target.from}
          </span>
        </>
      ) : (
        <span className="text-[13px] text-muted-foreground">
          Still waiting for the console… check back in a minute.
        </span>
      )}
      <button
        onClick={onSettled}
        className="rounded-lg border border-border px-3.5 py-2 text-[13px] font-medium text-foreground/85 transition-colors active:bg-muted/60"
      >
        Back to update settings
      </button>
    </div>
  )
}
