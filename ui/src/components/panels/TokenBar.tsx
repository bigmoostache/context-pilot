import { loadColor } from "@/lib/panelMeta"
import { cn } from "@/lib/utils"

interface TokenBarProps {
  value: number
  max: number
  /** force a color instead of the load gradient */
  color?: string
  className?: string
  /** show the animated fill sweep on mount */
  animate?: boolean
}

/** A hairline phosphor fill bar — the signature CP token meter. */
export function TokenBar({ value, max, color, className, animate = true }: TokenBarProps) {
  const ratio = max > 0 ? Math.min(value / max, 1) : 0
  const fill = color ?? loadColor(ratio)
  return (
    <div
      className={cn(
        "relative h-1 w-full overflow-hidden rounded-[1px] bg-[oklch(0.26_0.006_75)]",
        className,
      )}
    >
      <div
        className={cn("h-full rounded-[1px]", animate && "fill-sweep")}
        style={{
          width: `${Math.max(ratio * 100, 1.5)}%`,
          background: fill,
          boxShadow: `0 0 6px ${fill}`,
        }}
      />
    </div>
  )
}
