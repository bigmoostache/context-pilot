import { useEffect } from "react"
import { Bot, Clock, Hash, MessageSquare, PanelsTopLeft, X } from "lucide-react"
import type { ThreadDetail } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Thread "dossier" popup — the metadata that used to live in a permanent right
 * rail, now summoned on demand from the conversation header. Keeps the messaging
 * surface wide while still exposing status, counts, timestamps, and the bridge
 * into the panel-centered cockpit. Centered modal with backdrop; Esc / click-out
 * to dismiss (matches the ConfigModal / AgentModal motion vocabulary).
 */
export function ThreadDetailsPopup({
  thread,
  onOpenCockpit,
  onClose,
}: {
  thread: ThreadDetail
  onOpenCockpit: () => void
  onClose: () => void
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [onClose])

  const mine = thread.status === "MY_TURN"

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 backdrop-blur-[2px] [animation:backdrop-fade_.18s_ease-out]"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        className="w-[360px] overflow-hidden rounded-2xl border border-border bg-popover shadow-[var(--shadow-pop)] [animation:modal-pop_.2s_cubic-bezier(.16,1,.3,1)]"
      >
        {/* header */}
        <div className="flex items-start gap-3 border-b border-border/70 bg-surface/60 px-5 py-4">
          <div className="flex min-w-0 flex-1 flex-col gap-1.5">
            <span className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-muted-foreground/70">
              Thread details
            </span>
            <span className="truncate text-[15px] font-semibold tracking-tight text-foreground">
              {thread.name}
            </span>
            <span
              className={cn(
                "inline-flex w-fit items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium",
                mine
                  ? "bg-[var(--signal)]/15 text-[var(--signal)]"
                  : "bg-muted text-muted-foreground",
              )}
            >
              <span
                className={cn("size-1.5 rounded-full", mine && "animate-pulse")}
                style={{ background: mine ? "var(--signal)" : "var(--muted-foreground)" }}
              />
              {mine ? "Your turn" : "Agent working"}
            </span>
          </div>
          <button
            onClick={onClose}
            className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted hover:text-foreground"
            aria-label="Close"
          >
            <X className="size-4" />
          </button>
        </div>

        {/* metadata */}
        <div className="flex flex-col gap-2.5 px-5 py-4">
          <Meta icon={Bot} label="Agent" value={thread.agent} />
          <Meta icon={MessageSquare} label="Messages" value={`${thread.log.length}`} />
          <Meta icon={Clock} label="Created" value={thread.createdAt} />
          <Meta icon={Clock} label="Last activity" value={thread.lastActivity} />
          <Meta icon={Hash} label="ID" value={thread.id} />
        </div>

        {/* cockpit bridge */}
        <div className="flex flex-col gap-2 border-t border-border/70 bg-surface/40 px-5 py-4">
          <button
            onClick={() => {
              onOpenCockpit()
              onClose()
            }}
            className="flex items-center justify-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-[12.5px] font-medium text-foreground/85 transition-colors hover:border-[var(--interactive)]/50 hover:text-[var(--interactive)] card-shadow"
          >
            <PanelsTopLeft className="size-4" />
            Open agent cockpit
          </button>
          <p className="px-0.5 text-[11px] leading-relaxed text-muted-foreground/60">
            Inspect this agent's full context — panels, token budget, and cache.
          </p>
        </div>
      </div>
    </div>
  )
}

function Meta({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof Bot
  label: string
  value: string
}) {
  return (
    <div className="flex items-center gap-2.5">
      <Icon className="size-4 text-muted-foreground/50" />
      <span className="text-[12px] text-muted-foreground">{label}</span>
      <span className="ml-auto truncate text-[12px] font-medium text-foreground/85">{value}</span>
    </div>
  )
}
