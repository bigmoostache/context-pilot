import { ArrowUp, Paperclip } from "lucide-react"

/** Clean macOS-style composer. Non-functional (design only). */
export function InputBar() {
  return (
    <div className="shrink-0 px-5 pt-2 pb-4">
      <div className="card-shadow flex items-end gap-2 rounded-2xl border border-border bg-card px-3 py-2.5 focus-within:border-(--signal)/60">
        <button className="mb-0.5 text-muted-foreground/60 transition-colors hover:text-(--interactive)">
          <Paperclip className="size-4" />
        </button>
        <div className="flex-1 text-[13.5px] leading-relaxed text-muted-foreground/60">
          Message Context Pilot…
          <span className="cursor-blink ml-0.5 inline-block h-3.5 w-[7px] translate-y-0.5 bg-(--signal)" />
        </div>
        <button className="flex size-7 items-center justify-center rounded-full bg-(--signal) text-(--primary-foreground) transition-[filter] hover:brightness-105">
          <ArrowUp className="size-4" strokeWidth={2.5} />
        </button>
      </div>
    </div>
  )
}
