import { loadColor } from "@/lib/panelMeta"
import { cn } from "@/lib/utils"

interface TokenBarProps {
  value: number
  max: number
  /** Override the auto load-based color with a fixed CSS color. */
  color?: string
  /** Animate the fill on mount (default true). */
  animate?: boolean
  className?: string
}

/** A slim, rounded progress bar. Color reflects load unless overridden. */
export function TokenBar({ value, max, color, animate = true, className }: TokenBarProps) {
  const ratio = max > 0 ? Math.min(1, value / max) : 0
  const fill = color ?? loadColor(ratio)
  return (
    <div
      className={cn(
        "w-full overflow-hidden rounded-full bg-muted",
        className,
      )}
    >
      <div
        className={cn("h-full rounded-full", animate && "fill-sweep")}
        style={{ width: `${ratio * 100}%`, background: fill }}
      />
    </div>
  )
}
