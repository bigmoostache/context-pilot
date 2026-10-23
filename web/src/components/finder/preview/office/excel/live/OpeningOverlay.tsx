import { useEffect, useState } from "react"

/**
 * Covers the xlsx grid while the worker parses every sheet: a large workbook
 * (tens of MB unpacked) shows nothing for seconds otherwise.
 */
export function OpeningOverlay({ size }: { size: number }) {
  const pct = useEstimatedProgress(size)
  return (
    <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-background">
      <span className="size-5 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-(--signal)" />
      <span className="text-[12.5px] text-muted-foreground">Opening workbook…</span>
      <div className="h-1 w-48 overflow-hidden rounded-full bg-muted">
        <div className="h-full bg-(--signal)" style={{ width: `${pct.toFixed(1)}%` }} />
      </div>
      <span className="text-[11px] text-muted-foreground/70">
        Large workbooks can take up to ~20 s.
      </span>
    </div>
  )
}

/** Measured worker open rate: roughly 8 s per compressed MB (1.2 MB 6.7 s, 1.8 MB 20 s). */
const OPEN_MS_PER_MB = 8000

/**
 * Estimated open progress (0–95). The engine reports none, so the bar follows
 * an exponential curve reaching ~85% at the size-based estimate, capped at 95%.
 */
function useEstimatedProgress(size: number): number {
  const [pct, setPct] = useState(0)
  useEffect(() => {
    const estimate = Math.max(1000, (size / (1024 * 1024)) * OPEN_MS_PER_MB)
    const start = performance.now()
    const id = window.setInterval(() => {
      const t = performance.now() - start
      setPct(95 * (1 - Math.exp((-2.3 * t) / estimate)))
    }, 200)
    return () => {
      window.clearInterval(id)
    }
  }, [size])
  return pct
}
