import { memo, useLayoutEffect, useRef, useState } from "react"
import { animate, createSpring, stagger, type JSAnimation } from "animejs"
import { ChevronRight } from "lucide-react"
import { parseAutoLine } from "@/lib/support/agentMessages"
import { prefersReducedMotion } from "@/lib/utils"
import type { AgentMsg } from "@/lib/types"

/** Up to three distinct tool names, in first-use order, for the collapsed summary. */
function toolDigest(msgs: AgentMsg[]): string {
  const seen: string[] = []
  for (const m of msgs) {
    const { tool } = parseAutoLine(m)
    if (tool && !seen.includes(tool)) seen.push(tool)
  }
  const head = seen.slice(0, 3).join(", ")
  return seen.length > 3 ? `${head} +${String(seen.length - 3)}` : head
}

/**
 * A run of consecutive tool-activity traces, collapsed to one quiet summary
 * line ("12 tool calls · Edit, Git, Send"). Opening it unfolds a hairline
 * timeline of verb · tool · intent rows.
 *
 * Motion is anime.js: the body's height tweens from its current value (so a
 * toggle mid-flight reverses smoothly instead of jumping), rows fade in with a
 * stagger capped at ~220 ms total whatever the run length, and the chevron
 * turns on a spring. Collapsing tweens height to 0, then unmounts the rows so a
 * closed run costs nothing. Reduced motion jumps straight to the end state.
 *
 * `memo`: re-renders only when `msgs` changes (T510 render-storm fix — the
 * parent memoizes `segmentLog`, so the array stays reference-stable).
 */
export const ToolRun = memo(function ToolRun({ msgs }: { msgs: AgentMsg[] }) {
  const [open, setOpen] = useState(false)
  const [mounted, setMounted] = useState(false)
  const bodyRef = useRef<HTMLDivElement>(null)
  const chevronRef = useRef<SVGSVGElement>(null)
  const anim = useRef<JSAnimation | null>(null)
  const first = useRef(true)
  const n = msgs.length

  useLayoutEffect(() => {
    if (first.current) {
      first.current = false
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
    anim.current?.pause()
    if (reduced) {
      el.style.height = open ? "auto" : "0px"
      if (!open) setMounted(false)
      return
    }
    const from = el.offsetHeight
    if (open) {
      if (from === 0) {
        const rows = el.querySelectorAll<HTMLElement>("[data-row]")
        for (const r of rows) r.style.opacity = "0"
        animate(rows, {
          opacity: [0, 1],
          translateX: [-6, 0],
          duration: 340,
          delay: stagger(Math.min(24, 220 / Math.max(rows.length, 1)), { start: 60 }),
          ease: "outQuad",
        })
      }
      anim.current = animate(el, {
        height: [from, el.scrollHeight],
        duration: 420,
        ease: "outQuint",
        onComplete: () => {
          el.style.height = "auto"
        },
      })
    } else {
      anim.current = animate(el, {
        height: [from, 0],
        duration: 260,
        ease: "inOutQuad",
        onComplete: () => {
          setMounted(false)
        },
      })
    }
  }, [open])

  const toggle = () => {
    if (!open) setMounted(true)
    setOpen(!open)
  }

  return (
    <div className="mb-2 ml-7">
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
        <span className="text-muted-foreground/35">·</span>
        <span className="truncate font-mono text-[10.5px] text-muted-foreground/50">
          {toolDigest(msgs)}
        </span>
      </button>
      {mounted && (
        <div ref={bodyRef} style={{ height: 0 }} className="overflow-hidden">
          <div className="relative mt-1 ml-[5px] grid grid-cols-[auto_auto_minmax(0,1fr)] gap-x-3 py-0.5 pl-3">
            <span className="absolute inset-y-1.5 left-0 w-px bg-foreground/8" />
            {msgs.map((m) => {
              const { verb, tool, intent } = parseAutoLine(m)
              return (
                <div
                  key={m.id}
                  data-row
                  className="relative col-span-3 grid grid-cols-subgrid items-baseline rounded-sm py-[3px] text-[11px] leading-tight"
                >
                  <span className="absolute top-[9px] left-[-14.5px] size-[4px] rounded-full bg-muted-foreground/35" />
                  <span className="text-[10px] tracking-wide text-muted-foreground/50 uppercase">
                    {verb}
                  </span>
                  <span className="font-mono text-foreground/75">{tool}</span>
                  <span className="truncate text-muted-foreground/60">{intent}</span>
                </div>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
})
