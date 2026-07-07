import { useState } from "react"
import { useMutation } from "@tanstack/react-query"
import { Loader2, Power, Rocket } from "lucide-react"
import { postApiReleasesDeploy, postApiReleasesRestartOrchestrator } from "@/lib/api/generated"
import type { DeployResponse } from "@/lib/api/generated"
import { sdk } from "@/lib/api/client"
import { cn } from "@/lib/utils"

/**
 * Deploy actions for the Releases settings pane.
 *
 * Two buttons: "Deploy to Fleet" (select release + restart all agents) and
 * "Restart Orchestrator" (delayed SIGTERM → procd respawn). The orchestrator
 * button has a click-to-confirm guard since the connection drops on restart.
 */
export function DeploySection({
  activeTag,
  onChanged,
}: {
  activeTag: string | null
  onChanged: () => void
}) {
  const [orchConfirm, setOrchConfirm] = useState(false)

  const deployFleet = useMutation({
    mutationFn: async () => {
      return sdk<DeployResponse>(
        postApiReleasesDeploy({
          body: activeTag ? { tag: activeTag } : {},
        }),
      )
    },
    onSuccess: () => onChanged(),
  })

  const restartOrch = useMutation({
    mutationFn: async () => {
      await postApiReleasesRestartOrchestrator()
    },
    onSuccess: () => {
      setOrchConfirm(false)
      // Connection will drop — reload after a delay to pick up the new orchestrator.
      setTimeout(() => window.location.reload(), 3000)
    },
  })

  const hasDeploy = deployFleet.isSuccess && deployFleet.data

  return (
    <div className="flex flex-col gap-2">
      <h3 className="text-[11px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
        Deploy
      </h3>

      <div className="flex flex-wrap items-center gap-2">
        <button
          onClick={() => deployFleet.mutate()}
          disabled={!activeTag || deployFleet.isPending}
          className={cn(
            "flex items-center gap-1.5 rounded-lg border px-3 py-2 text-[12px] font-medium transition-all disabled:opacity-50",
            "border-[var(--interactive)]/30 bg-[var(--interactive)]/[0.06] text-[var(--interactive)] hover:bg-[var(--interactive)]/[0.12]",
          )}
        >
          {deployFleet.isPending ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Rocket className="size-3.5" />
          )}
          Deploy to Fleet
        </button>

        {orchConfirm ? (
          <button
            onClick={() => {
              restartOrch.mutate()
              setOrchConfirm(false)
            }}
            disabled={restartOrch.isPending}
            className={cn(
              "flex items-center gap-1.5 rounded-lg border px-3 py-2 text-[12px] font-medium transition-all animate-pulse disabled:opacity-50",
              "border-[var(--danger)]/50 bg-[var(--danger)]/[0.12] text-[var(--danger)] hover:bg-[var(--danger)]/[0.18]",
            )}
          >
            {restartOrch.isPending ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <Power className="size-3.5" />
            )}
            Confirm — connection will drop
          </button>
        ) : (
          <button
            onClick={() => setOrchConfirm(true)}
            disabled={restartOrch.isPending}
            className={cn(
              "flex items-center gap-1.5 rounded-lg border px-3 py-2 text-[12px] font-medium transition-all disabled:opacity-50",
              "border-[var(--danger)]/30 bg-[var(--danger)]/[0.06] text-[var(--danger)] hover:bg-[var(--danger)]/[0.12]",
            )}
          >
            <Power className="size-3.5" />
            Restart Orchestrator
          </button>
        )}
      </div>

      {hasDeploy && (
        <div className="rounded-lg border border-[var(--ok)]/30 bg-[var(--ok)]/[0.06] px-3 py-2 text-[11px] text-[var(--ok)]">
          ✓ Deployed <strong>{deployFleet.data.tag}</strong> — {deployFleet.data.restarted.length}{" "}
          agent(s) restarted
          {deployFleet.data.errors.length > 0 && (
            <span className="text-[var(--danger)]">
              {" "}
              · {deployFleet.data.errors.length} error(s)
            </span>
          )}
        </div>
      )}

      {restartOrch.isPending && (
        <div className="flex items-center gap-2 rounded-lg border border-[var(--danger)]/30 bg-[var(--danger)]/[0.06] px-3 py-2 text-[11px] text-[var(--danger)]">
          <Loader2 className="size-3 animate-spin" />
          Restarting orchestrator — page will reload in a few seconds…
        </div>
      )}

      {!activeTag && (
        <p className="text-[10px] text-muted-foreground/50">
          Select a release to enable fleet deployment.
        </p>
      )}
    </div>
  )
}
