import { CornerDownLeft } from "lucide-react"

/** Mock input bar with blinking caret + key hints. Non-functional (design only). */
export function InputBar() {
  return (
    <div className="shrink-0 border-t border-border bg-[oklch(0.175_0.007_75)] px-3 py-2">
      <div className="flex items-start gap-2 rounded-[4px] border border-border bg-[oklch(0.195_0.008_75)] px-2.5 py-2 focus-within:ring-1 focus-within:ring-[var(--signal)]/50 etch">
        <span className="mt-px text-[var(--signal)]">▌</span>
        <div className="flex-1 text-[12.5px] leading-relaxed text-foreground/85">
          ship the maquette and open a thread with the screenshots
          <span className="cursor-blink ml-0.5 inline-block h-3.5 w-[7px] translate-y-0.5 bg-[var(--signal)]" />
        </div>
      </div>
      <div className="mt-1.5 flex items-center gap-3 px-0.5 text-[10px] text-muted-foreground/60">
        <span className="flex items-center gap-1">
          <CornerDownLeft className="size-2.5" /> send
        </span>
        <span>
          <kbd className="text-[var(--interactive)]/80">⌃V</kbd> cycle view
        </span>
        <span>
          <kbd className="text-[var(--interactive)]/80">@</kbd> file
        </span>
        <span className="ml-auto tabular-nums">esc to stop</span>
      </div>
    </div>
  )
}
