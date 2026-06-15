import { Bot, Clock, Hash, MessageSquare, PanelsTopLeft, X } from "lucide-react"
import { Dialog, DialogContent, DialogClose } from "@/components/ui/dialog"
import type { ThreadDetail } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Thread "dossier" popup — the metadata that used to live in a permanent right
 * rail, now summoned on demand from the conversation header. Keeps the messaging
 * surface wide while still exposing status, counts, timestamps, and the bridge
 * into the panel-centered cockpit.
 *
 * Built on the portaled {@link Dialog} primitive (renders into `document.body`,
 * focus-trapped, Esc / click-out to dismiss) for the same reason as the settings
 * sheet — a hand-rolled `fixed` overlay can be trapped by a transformed/blurred
 * ancestor's containing block.
 */
export function ThreadDetailsPopup({
  thread,
  open,
  onOpenCockpit,
  onClose,
}: {
  thread: ThreadDetail
  open: boolean
  onOpenCockpit: () => void
  onClose: () => void
}) {
  const mine = thread.status === "MY_TURN"

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="w-[360px] max-w-[calc(100vw-3rem)]">
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
          <DialogClose
            className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted hover:text-foreground"
            aria-label="Close"
          >
            <X className="size-4" />
          </DialogClose>
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
      </DialogContent>
    </Dialog>
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
