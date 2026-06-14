import { Paperclip, CornerDownLeft, Loader2 } from "lucide-react"
import type { ThreadStatus } from "@/lib/types"

/**
 * Thread composer — the input surface at the bottom of a thread conversation.
 * When the thread is THEIR_TURN the agent is working, so the composer shows a
 * subdued "working" state instead of an active prompt. Design-only.
 */
export function ThreadComposer({ status }: { status: ThreadStatus }) {
  if (status === "THEIR_TURN") {
    return (
      <div className="shrink-0 border-t border-border bg-[oklch(0.175_0.007_75)] px-4 py-3">
        <div className="flex items-center justify-center gap-2 rounded-[5px] border border-dashed border-border bg-[oklch(0.18_0.007_75)] px-3 py-2.5 text-[12px] text-muted-foreground">
          <Loader2 className="size-3.5 animate-spin text-[var(--signal)]" />
          <span>the agent is working this thread — it'll hand back when it needs you</span>
        </div>
      </div>
    )
  }

  return (
    <div className="shrink-0 border-t border-border bg-[oklch(0.175_0.007_75)] px-4 py-3">
      <div className="flex items-end gap-2 rounded-[5px] border border-border bg-[oklch(0.2_0.008_75)] px-3 py-2.5 focus-within:border-[var(--signal)]/60 focus-within:ring-1 focus-within:ring-[var(--signal)]/40 etch">
        <button className="mb-px text-muted-foreground/60 transition-colors hover:text-[var(--interactive)]">
          <Paperclip className="size-4" />
        </button>
        <div className="flex-1 text-[13px] leading-relaxed text-foreground/85">
          <span className="text-muted-foreground/50">Reply to this thread…</span>
          <span className="cursor-blink ml-0.5 inline-block h-3.5 w-[7px] translate-y-0.5 bg-[var(--signal)]" />
        </div>
        <button className="mb-px flex items-center gap-1.5 rounded-[3px] bg-[var(--signal)] px-2.5 py-1 text-[11px] font-semibold text-[oklch(0.18_0.02_75)] transition-[filter] hover:brightness-110">
          Send
          <CornerDownLeft className="size-3" />
        </button>
      </div>
      <div className="mt-1.5 flex items-center gap-3 px-0.5 text-[10px] text-muted-foreground/55">
        <span className="flex items-center gap-1">
          <CornerDownLeft className="size-2.5" /> send
        </span>
        <span>
          <kbd className="text-[var(--interactive)]/80">⇧⏎</kbd> newline
        </span>
        <span className="ml-auto text-[var(--signal)]/70">your turn</span>
      </div>
    </div>
  )
}
