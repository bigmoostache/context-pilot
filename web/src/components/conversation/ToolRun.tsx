import { memo, useLayoutEffect, useMemo, useRef, useState } from "react"
import { animate, createSpring, stagger, type JSAnimation } from "animejs"
import { ChevronRight } from "lucide-react"
import { parseAutoLine } from "@/lib/support/agentMessages"
import { prefersReducedMotion } from "@/lib/utils"
import type { AgentMsg } from "@/lib/types"

/** `[data-slide]` spans per timeline row (verb, intent): maps a span index to its row. */
const SLIDES_PER_ROW = 2

/**
 * Staggered row reveal/hide inside `body`. Rows only fade, so their dots stay
 * on the timeline line; the text spans carry the slide. `fresh` = opening from
 * fully closed (rows start hidden); a reopen mid-collapse settles from wherever
 * the rows are. Returns [fade, slide] so the caller can pause them.
 */
function animateRows(body: HTMLElement, open: boolean, fresh: boolean): JSAnimation[] {
  const rows = body.querySelectorAll<HTMLElement>("[data-row]")
  const slides = body.querySelectorAll<HTMLElement>("[data-slide]")
  const step = Math.min(24, 220 / Math.max(rows.length, 1))
  const rowOf = (i: number | undefined) => Math.floor((i ?? 0) / SLIDES_PER_ROW)
  if (!open) {
    const last = rows.length - 1
    return [
      animate(rows, {
        opacity: 0,
        duration: 220,
        delay: stagger(step, { from: "last" }),
        ease: "inQuad",
      }),
      animate(slides, {
        translateX: -6,
        duration: 220,
        delay: (_t, i) => (last - rowOf(i)) * step,
        ease: "inQuad",
      }),
    ]
  }
  if (!fresh) {
    return [
      animate(rows, { opacity: 1, duration: 220, ease: "outQuad" }),
      animate(slides, { translateX: 0, duration: 220, ease: "outQuad" }),
    ]
  }
  for (const r of rows) r.style.opacity = "0"
  return [
    animate(rows, {
      opacity: [0, 1],
      duration: 340,
      delay: stagger(step, { start: 60 }),
      ease: "outQuad",
    }),
    animate(slides, {
      translateX: [-6, 0],
      duration: 340,
      delay: (_t, i) => 60 + rowOf(i) * step,
      ease: "outQuad",
    }),
  ]
}

/**
 * A run of consecutive tool-activity traces, collapsed to one quiet summary
 * line ("12 tool calls"). Opening it unfolds a hairline
 * timeline of verb · intent rows (the tool name is not shown).
 *
 * Motion is anime.js: the body's height tweens from its current value (so a
 * toggle mid-flight reverses smoothly instead of jumping), rows fade in with a
 * stagger capped at ~220 ms total whatever the run length, and the chevron
 * turns on a spring. Collapsing mirrors it: rows fade out bottom-up while the
 * height eases to 0, then the rows unmount so a closed run costs nothing. Reduced motion jumps straight to the end state.
 *
 * `memo`: re-renders only when `msgs` changes (T510 render-storm fix — the
 * parent memoizes `segmentLog`, so the array stays reference-stable).
 */
export const ToolRun = memo(function ToolRun({ msgs }: { msgs: AgentMsg[] }) {
  const [open, setOpen] = useState(false)
  const [mounted, setMounted] = useState(false)
  const bodyRef = useRef<HTMLDivElement>(null)
  const chevronRef = useRef<SVGSVGElement>(null)
  const heightAnimRef = useRef<Animation | null>(null)
  const rowAnimsRef = useRef<JSAnimation[]>([])
  const firstRef = useRef(true)
  // Rows with neither verb nor intent (e.g. queue-flush echoes) carry nothing to show.
  const rows = useMemo(
    () =>
      msgs
        .map((m) => ({ id: m.id, ...parseAutoLine(m) }))
        .filter((r) => r.verb.trim() !== "" || r.intent.trim() !== ""),
    [msgs],
  )
  const n = rows.length

  useLayoutEffect(() => {
    if (firstRef.current) {
      firstRef.current = false
      return
    }
    const reduced = prefersReducedMotion()
    const chevron = chevronRef.current
    if (chevron) {
      animate(chevron, {
        rotate: open ? 90 : 0,
        ...(reduced ? { duration: 0 } : { ease: createSpring({ stiffness: 420, damping: 26 }) }),
      })
    }
    const el = bodyRef.current
    if (!el) return
    // Read the live height BEFORE cancelling, so a mid-flight toggle reverses
    // from where the body is on screen, not from the cancelled tween's target.
    const from = el.getBoundingClientRect().height
    heightAnimRef.current?.cancel()
    if (reduced) {
      el.style.height = open ? "auto" : "0px"
      // Deferred: unmounting the rows synchronously inside the layout effect
      // would re-render mid-layout. A microtask lands right after it commits.
      if (!open) queueMicrotask(() => setMounted(false))
      return
    }
    for (const a of rowAnimsRef.current) a.pause()
    rowAnimsRef.current = animateRows(el, open, from === 0)
    // Height runs on WAAPI with explicit px keyframes: anime.js left the body
    // pinned at `auto` for the whole close, so the chat only moved when the
    // rows unmounted (measured frame by frame). The inline height is set to
    // the end value up front; the animation overrides it while it runs.
    const to = open ? el.scrollHeight : 0
    el.style.height = `${String(to)}px`
    const h = el.animate([{ height: `${String(from)}px` }, { height: `${String(to)}px` }], {
      duration: open ? 420 : 340,
      delay: open ? 0 : 60,
      easing: open ? "cubic-bezier(0.22, 1, 0.36, 1)" : "cubic-bezier(0.76, 0, 0.24, 1)",
      fill: "backwards",
    })
    h.onfinish = () => {
      if (open) el.style.height = "auto"
      else setMounted(false)
    }
    heightAnimRef.current = h
  }, [open])

  if (n === 0) return null

  const toggle = () => {
    if (!open) setMounted(true)
    setOpen(!open)
  }

  return (
    <div className="mb-2">
      <button
        type="button"
        onClick={toggle}
        aria-expanded={open}
        className="group/run -ml-1 inline-flex max-w-full items-center gap-1.5 rounded-md py-0.5 pr-2 pl-1 text-[11.5px] text-muted-foreground/65 transition-colors outline-none hover:bg-muted/40 hover:text-foreground/80 focus-visible:bg-muted/40"
      >
        <ChevronRight
          ref={chevronRef}
          className="size-3 shrink-0 text-muted-foreground/50 group-hover/run:text-foreground/60"
        />
        <span className="font-medium tabular-nums">
          {n} tool call{n === 1 ? "" : "s"}
        </span>
      </button>
      {mounted && (
        // overflow-anchor:none: otherwise the browser anchors scroll on a row in
        // here and holds it still while the body shrinks, so the chat below
        // doesn't move until the rows unmount, then jumps all at once.
        <div ref={bodyRef} style={{ height: 0 }} className="overflow-hidden [overflow-anchor:none]">
          <div className="relative mt-1 ml-[5px] grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 py-0.5 pl-3">
            <span className="absolute inset-y-1.5 left-0 w-px bg-foreground/8" />
            {rows.map(({ id, verb, intent }) => {
              return (
                <div
                  key={id}
                  data-row
                  className="relative col-span-2 grid grid-cols-subgrid items-baseline rounded-sm py-[3px] text-[11px] leading-tight"
                >
                  <span className="absolute top-[9px] left-[-14.5px] size-[4px] rounded-full bg-muted-foreground/35" />
                  <span
                    data-slide
                    className="text-[10px] font-medium tracking-wide text-signal/85 uppercase"
                  >
                    {verb}
                  </span>
                  <span data-slide className="truncate text-muted-foreground/60">
                    {intent}
                  </span>
                </div>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
})
