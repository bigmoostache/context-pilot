import { ArrowUp, Paperclip, Loader2 } from "lucide-react"
import type { ThreadStatus } from "@/lib/types"

/**
 * Thread composer. When the thread isn't MY_TURN the agent owns it — either
 * actively streaming (ACTIVE) or working it in parallel (THEIR_TURN) — so the
 * composer shows a subdued "working" state instead of an active prompt.
 */
export function ThreadComposer({ status }: { status: ThreadStatus }) {
  if (status !== "MY_TURN") {
    const active = status === "ACTIVE"
    return (
      <div className="shrink-0 px-5 pb-4 pt-2">
        <div className="flex items-center justify-center gap-2 rounded-2xl border border-dashed border-border bg-muted/40 px-3 py-3 text-[12.5px] text-muted-foreground">
          <Loader2
            className="size-4 animate-spin"
            style={{ color: active ? "var(--ok)" : "var(--signal)" }}
          />
          <span>
            {active
              ? "The agent is streaming this thread right now…"
              : "The agent is working this thread — it'll hand back when it needs you."}
          </span>
        </div>
      </div>
    )
  }

  return (
    <div className="shrink-0 px-5 pb-4 pt-2">
      <div className="flex items-end gap-2 rounded-2xl border border-border bg-card px-3 py-2.5 card-shadow focus-within:border-[var(--signal)]/60">
        <button className="mb-0.5 text-muted-foreground/60 transition-colors hover:text-[var(--interactive)]">
          <Paperclip className="size-4" />
        </button>
        <div className="flex-1 text-[13.5px] leading-relaxed text-muted-foreground/60">
          Reply to this thread…
          <span className="cursor-blink ml-0.5 inline-block h-3.5 w-[7px] translate-y-0.5 bg-[var(--signal)]" />
        </div>
        <button className="flex size-7 items-center justify-center rounded-full bg-[var(--signal)] text-[var(--primary-foreground)] transition-[filter] hover:brightness-105">
          <ArrowUp className="size-4" strokeWidth={2.5} />
        </button>
      </div>
    </div>
  )
}
