import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from "react"
import { prefersReducedMotion } from "@/lib/utils"

/** Soft landing with a long tail, no overshoot (same curve as the selection glide). */
const EASE = "cubic-bezier(0.32, 0.72, 0, 1)"

/** A row that just left the list, kept on screen while it collapses. */
interface Ghost<T> {
  item: T
  /** its position in the list it left, where it collapses in place */
  index: number
}

/**
 * Renders `items` and keeps every row that leaves the list (archived, deleted)
 * on screen while it collapses, so the rows below slide up into the gap instead
 * of jumping. Nothing animates when `layoutKey` changes (another list is shown)
 * or under reduced motion.
 *
 * Change detection runs on the id signature, not the array identity: callers
 * rebuild the array on every render.
 */
export function useDepartures<T extends { id: string }>(
  items: T[],
  layoutKey: string,
  render: (item: T) => ReactNode,
): ReactNode[] {
  const sig = `${layoutKey}|${items.map((i) => i.id).join(",")}`
  const [prev, setPrev] = useState({ sig, key: layoutKey, items })
  const [ghosts, setGhosts] = useState<Ghost<T>[]>([])
  const drop = useCallback((id: string) => {
    setGhosts((gs) => gs.filter((g) => g.item.id !== id))
  }, [])

  if (prev.sig !== sig) {
    const ids = new Set(items.map((i) => i.id))
    const sameList = prev.key === layoutKey
    const gone =
      sameList && !prefersReducedMotion()
        ? prev.items.flatMap((item, index) => (ids.has(item.id) ? [] : [{ item, index }]))
        : []
    setPrev({ sig, key: layoutKey, items })
    setGhosts(sameList ? [...ghosts.filter((g) => !ids.has(g.item.id)), ...gone] : [])
  }

  const live: ReactNode[] = items.map((item) => render(item))
  // Re-insert each ghost at its old index, lowest first, so later indices still hold.
  return ghosts
    .toSorted((a, b) => a.index - b.index)
    .reduce(
      (acc, g) =>
        acc.toSpliced(
          Math.min(g.index, acc.length),
          0,
          <CollapseOut key={`gone-${g.item.id}`} id={g.item.id} onDone={drop}>
            {render(g.item)}
          </CollapseOut>,
        ),
      live,
    )
}

/**
 * Collapses its child on the vertical axis: height shrinks to zero (the rows
 * below follow it up), the content squashes slightly toward its centre line,
 * and it fades out over the first half so the gap closes on nothing.
 */
function CollapseOut({
  id,
  onDone,
  children,
}: {
  id: string
  onDone: (id: string) => void
  children: ReactNode
}) {
  const ref = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const anim = el.animate(
      [
        { height: `${el.offsetHeight}px`, opacity: 1, transform: "scaleY(1)" },
        { opacity: 0, offset: 0.5 },
        { height: "0px", opacity: 0, transform: "scaleY(0.82)" },
      ],
      { duration: 360, easing: EASE, fill: "forwards" },
    )
    anim.onfinish = () => onDone(id)
    return () => anim.cancel()
  }, [id, onDone])
  return (
    <div ref={ref} aria-hidden inert className="pointer-events-none origin-center overflow-hidden">
      {children}
    </div>
  )
}

/** Row-hover title marquee (WAA): dwell, scroll left ~10 chars/s, dwell, reset. No-op if it fits / reduced-motion. */
export function useTitleMarquee() {
  const trackRef = useRef<HTMLSpanElement>(null)
  const animRef = useRef<Animation | null>(null)
  const onMouseEnter = () => {
    const el = trackRef.current
    if (!el || prefersReducedMotion()) return
    const dist = Math.max(0, el.scrollWidth - el.clientWidth)
    if (dist === 0) return
    const scrollS = dist / (el.scrollWidth / Math.max(1, el.textContent.length)) / 10
    const total = 0.6 + scrollS
    animRef.current = el.animate(
      [
        { transform: "translateX(0)", offset: 0 },
        { transform: "translateX(0)", offset: 0.3 / total },
        { transform: `translateX(-${dist}px)`, offset: (0.3 + scrollS) / total },
        { transform: `translateX(-${dist}px)`, offset: 1 },
      ],
      { duration: total * 1000, iterations: Infinity, easing: "linear" },
    )
  }
  const onMouseLeave = () => animRef.current?.cancel()
  return { trackRef, onMouseEnter, onMouseLeave }
}
