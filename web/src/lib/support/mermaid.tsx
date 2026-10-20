import { Dialog as DialogPrimitive } from "@base-ui/react/dialog"
import { Maximize2, XIcon, ZoomIn, ZoomOut } from "lucide-react"
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
  type ReactNode,
} from "react"

import { animateCloseToOrigin, cn } from "@/lib/utils"

/** The mermaid API surface (the dynamic import's default export). */
type MermaidApi = Awaited<typeof import("mermaid")>["default"]

/**
 * Memoised import promise + the theme mermaid was last initialised for.
 *
 * mermaid config is GLOBAL to the library, so `initialize` must run once per
 * theme — not once per component mount (the old code re-initialised on every
 * diagram, pure waste). Held on a const object (fields mutated, binding never
 * reassigned) to keep module state without a reassigned top-level `let`.
 */
const memo: { mod: Promise<MermaidApi> | null; theme: "dark" | "light" | null } = {
  mod: null,
  theme: null,
}

/**
 * Rendered-SVG cache keyed by `"<theme>:<source>"`. Identical diagrams (repeats)
 * and re-mounts (scrolling a message row in/out) reuse the SVG verbatim instead
 * of re-running the expensive dagre/d3 layout + serialize on the main agent.
 */
const svgCache = new Map<string, string>()

/**
 * Import mermaid once (memoised promise), (re)initialise only when the theme
 * changed since the last call, and return the API. The first diagram on the
 * page pays the chunk load; every diagram after reuses this.
 */
async function loadMermaid(dark: boolean): Promise<MermaidApi> {
  memo.mod ??= import("mermaid").then((m) => m.default)
  const mermaid = await memo.mod
  const theme = dark ? "dark" : "light"
  if (memo.theme !== theme) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: dark ? "dark" : "default",
    })
    memo.theme = theme
  }
  return mermaid
}

/** Tail of the render chain: mermaid's `render` is not reentrant. */
const queue: { tail: Promise<unknown> } = { tail: Promise.resolve() }

/**
 * Validate then render one diagram, one at a time.
 *
 * On a syntax error `mermaid.render` appends its "Syntax error" SVG inside a
 * temporary `#d<id>` div on `document.body` and never removes it, so every
 * invalid or half-streamed fence used to leak a visible bomb below the app.
 * `parse` with `suppressErrors` rejects bad input without touching the DOM, and
 * the `finally` sweeps the temp div should `render` itself still fail.
 */
function renderSerialized(dark: boolean, id: string, code: string): Promise<string> {
  const prev = queue.tail
  const run = (async () => {
    await prev
    try {
      const mermaid = await loadMermaid(dark)
      if (!(await mermaid.parse(code, { suppressErrors: true }))) {
        throw new Error("Invalid mermaid syntax")
      }
      const { svg } = await mermaid.render(id, code)
      return svg
    } finally {
      document.querySelector(`#d${id}`)?.remove()
    }
  })()
  queue.tail = run.catch(() => null)
  return run
}

/**
 * Render a ```mermaid``` fenced block as an actual diagram.
 *
 * `mermaid` is a heavy dependency (it pulls in d3, dagre, cytoscape), so it is
 * **lazy-loaded** via dynamic `import()` on first mount — the initial bundle
 * never pays for it, only agents that actually contain a mermaid diagram do.
 *
 * Rendering is async: `mermaid.render(id, code)` returns an SVG string that we
 * inject as inner HTML (the same pattern as the Finder's highlight.js preview).
 * `securityLevel: "strict"` makes mermaid sanitise the output (DOMPurify
 * internally) and forbid inline scripts/click handlers, so the injected markup
 * is safe by construction. The theme tracks the app's light/dark class so the
 * diagram reads on either background. A parse/render failure degrades to the
 * raw code in a muted box rather than throwing.
 *
 * `onAccent` is true inside the coloured user bubble — the error fallback then
 * uses a translucent-dark box instead of the muted surface so it stays legible.
 */
export function Mermaid({ code, onAccent }: { code: string; onAccent: boolean }): ReactNode {
  const dark =
    typeof document !== "undefined" && document.documentElement.classList.contains("dark")
  const cacheKey = `${dark ? "dark" : "light"}:${code}`
  // Seed from the cache during render (not via setState in the effect): a repeat
  // diagram or a re-mount paints its SVG on the FIRST frame with no flash and no
  // cascading effect render. A miss seeds `null` → the effect renders it async.
  const [svg, setSvg] = useState<string | null>(() => svgCache.get(cacheKey) ?? null)
  const [error, setError] = useState<string | null>(null)
  const [open, setOpen] = useState(false)
  const [origin, setOrigin] = useState<HTMLButtonElement | null>(null)
  // `useId` gives a stable, globally-unique id per component instance; strip the
  // framework's `:` delimiters so it is a valid DOM/SVG id (mermaid uses it in
  // selectors internally).
  const rawId = useId()
  const id = `cp-mermaid-${rawId.replaceAll(/[^a-z0-9]/gi, "")}`

  useEffect(() => {
    // Already rendered (seeded from cache at mount, or filled on a prior pass) —
    // nothing to do. Layout + serialize only ever runs on a genuine miss.
    if (svgCache.has(cacheKey)) {
      return
    }
    // Object-held flag (not a bare `let`): the async cleanup mutates it, and a
    // plain boolean would be narrowed to its literal init by the type-checker,
    // making the `alive.current` guards read as "always truthy".
    const alive = { current: true }
    void (async () => {
      try {
        const out = await renderSerialized(dark, id, code)
        if (alive.current) {
          svgCache.set(cacheKey, out)
          setSvg(out)
          setError(null)
        }
      } catch (err) {
        if (alive.current) {
          setError(err instanceof Error ? err.message : "Mermaid render failed")
          setSvg(null)
        }
      }
    })()

    return () => {
      alive.current = false
    }
  }, [cacheKey, dark, id, code])

  if (error !== null) {
    return (
      <pre
        className={cn(
          "my-2 overflow-x-auto rounded-lg p-3 font-mono text-[12px] leading-relaxed",
          onAccent ? "bg-black/20" : "border border-border bg-muted/60 text-foreground/90",
        )}
      >
        {code}
      </pre>
    )
  }

  if (svg === null) {
    return (
      <div className="my-2 flex justify-center">
        <span className="text-[12px] text-muted-foreground/60">Rendering diagram…</span>
      </div>
    )
  }

  return (
    <>
      <button
        ref={setOrigin}
        type="button"
        title="Open diagram"
        aria-label="Open diagram full size"
        onClick={() => {
          setOpen(true)
        }}
        className="my-2 flex w-full cursor-zoom-in justify-center overflow-x-auto rounded-lg transition-colors hover:bg-foreground/3 [&_svg]:h-auto"
      >
        <span
          className="contents"
          // mermaid renders with securityLevel:"strict" (DOMPurify-sanitised SVG,
          // no inline scripts/handlers), so the markup is safe to inject.
          dangerouslySetInnerHTML={{ __html: resizeSvg(svg, "inline") }}
        />
      </button>
      {open && (
        <MermaidViewer
          svg={svg}
          origin={origin}
          onClose={() => {
            setOpen(false)
          }}
        />
      )}
    </>
  )
}

/** Inline diagrams render this much larger than mermaid's natural size (still capped at 100%). */
const INLINE_SCALE = 1.5

/**
 * Rewrite the `max-width: Npx` mermaid stamps on its root `<svg>` (alongside
 * `width="100%"`). That cap is what made diagrams read tiny: inline it grows by
 * `INLINE_SCALE` while staying within the message column; in the viewer the
 * SVG gets its intrinsic width and no cap, so the CSS transform owns the scale.
 */
function resizeSvg(svg: string, mode: "inline" | "viewer"): string {
  // The viewer mounts a SECOND copy of the inline SVG: give it its own root id
  // (mermaid scopes its <style> and arrow markers by that id), else both copies
  // share ids and the markers / styles resolve against the wrong element.
  const id = /<svg[^>]*\sid="([^"]+)"/.exec(svg)?.[1]
  const own = mode === "viewer" && id ? svg.split(id).join(`${id}-viewer`) : svg
  return own.replace(/max-width:\s*([\d.]+)px;?/, (_m, raw: string) => {
    const px = Number(raw)
    return mode === "inline"
      ? `max-width: min(100%, ${Math.round(px * INLINE_SCALE)}px);`
      : `width: ${px}px; max-width: none;`
  })
}

/** Zoom bounds and the per-step factor for buttons / keys. */
const ZOOM = { min: 0.1, max: 12, step: 1.25 } as const

/** Pan offset (px, from the stage centre) + scale of the viewer's diagram. */
interface View {
  x: number
  y: number
  k: number
}

const clampK = (k: number): number => Math.min(ZOOM.max, Math.max(ZOOM.min, k))

/** Centre-relative coordinates of a client point inside `el`. */
function centred(el: Element, clientX: number, clientY: number): { x: number; y: number } {
  const r = el.getBoundingClientRect()
  return { x: clientX - r.left - r.width / 2, y: clientY - r.top - r.height / 2 }
}

/**
 * Pan/zoom state + gesture handlers for the viewer stage: wheel and pinch zoom
 * anchored at the cursor, one-pointer drag pan, `+`/`-`/`0` keys, fit-to-stage.
 */
function useViewerGestures(
  stageRef: RefObject<HTMLDivElement | null>,
  contentRef: RefObject<HTMLDivElement | null>,
  stage: HTMLDivElement | null,
) {
  const [view, setView] = useState<View>({ x: 0, y: 0, k: 1 })
  const pointersRef = useRef(new Map<number, { x: number; y: number }>())
  const pinchRef = useRef<number | null>(null)

  /** Scale by `factor`, keeping the centre-relative stage point (`px`,`py`) fixed. */
  const zoomAt = useCallback((factor: number, px = 0, py = 0) => {
    setView((v) => {
      const k = clampK(v.k * factor)
      const f = k / v.k
      return { k, x: px - (px - v.x) * f, y: py - (py - v.y) * f }
    })
  }, [])

  /** Fit the whole diagram in the stage with a small margin, centred. */
  const fit = useCallback(() => {
    const stage = stageRef.current
    const svgEl = contentRef.current?.querySelector("svg")
    if (!stage || !svgEl) {
      return
    }
    const w = svgEl.clientWidth || 1
    const h = svgEl.clientHeight || 1
    setView({
      x: 0,
      y: 0,
      k: clampK(Math.min((stage.clientWidth * 0.92) / w, (stage.clientHeight * 0.92) / h)),
    })
  }, [stageRef, contentRef])

  // Fit once the popup is mounted and measurable.
  useLayoutEffect(() => {
    const raf = requestAnimationFrame(fit)
    return () => {
      cancelAnimationFrame(raf)
    }
  }, [fit])

  // Wheel zoom needs a NON-passive listener to preventDefault the page scroll;
  // React's onWheel is passive, hence the manual subscription. Trackpad pinch
  // arrives as ctrl+wheel with small deltas, hence the steeper factor.
  // Keyed on the `stage` ELEMENT (state, not the ref): the popup mounts through
  // a portal after this hook's first effect pass, when the ref is still null —
  // a ref-read effect never re-ran, so wheel/trackpad-pinch fell through to the
  // browser (page zoom) instead of zooming the diagram.
  useEffect(() => {
    if (!stage) {
      return
    }
    const onWheel = (e: WheelEvent): void => {
      e.preventDefault()
      const p = centred(stage, e.clientX, e.clientY)
      zoomAt(Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.002)), p.x, p.y)
    }
    stage.addEventListener("wheel", onWheel, { passive: false })
    return () => {
      stage.removeEventListener("wheel", onWheel)
    }
  }, [stage, zoomAt])

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>): void => {
    e.currentTarget.setPointerCapture(e.pointerId)
    pointersRef.current.set(e.pointerId, { x: e.clientX, y: e.clientY })
    pinchRef.current = null
  }

  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>): void => {
    const prev = pointersRef.current.get(e.pointerId)
    if (!prev) {
      return
    }
    const next = { x: e.clientX, y: e.clientY }
    pointersRef.current.set(e.pointerId, next)
    const [a, b] = pointersRef.current.values().toArray()
    if (a && b) {
      // Two fingers: pinch-zoom around their midpoint.
      const dist = Math.hypot(a.x - b.x, a.y - b.y)
      if (pinchRef.current !== null) {
        const mid = centred(e.currentTarget, (a.x + b.x) / 2, (a.y + b.y) / 2)
        zoomAt(dist / pinchRef.current, mid.x, mid.y)
      }
      pinchRef.current = dist
      return
    }
    setView((v) => ({ ...v, x: v.x + next.x - prev.x, y: v.y + next.y - prev.y }))
  }

  const onPointerUp = (e: ReactPointerEvent<HTMLDivElement>): void => {
    pointersRef.current.delete(e.pointerId)
    pinchRef.current = null
  }

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>): void => {
    const factor = { "+": ZOOM.step, "=": ZOOM.step, "-": 1 / ZOOM.step, _: 1 / ZOOM.step }[e.key]
    if (factor !== undefined) {
      zoomAt(factor)
    } else if (e.key === "0") {
      fit()
    } else {
      return
    }
    e.preventDefault()
  }

  return { view, zoomAt, fit, onPointerDown, onPointerMove, onPointerUp, onKeyDown }
}

const TOOL_BTN =
  "flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/6 hover:text-foreground"

/** Floating top-right toolbar: zoom out / level / zoom in / fit / close. */
function ViewerToolbar({
  k,
  zoomAt,
  fit,
}: {
  k: number
  zoomAt: (factor: number) => void
  fit: () => void
}): ReactNode {
  return (
    <div className="absolute top-3 right-3 z-10 flex items-center gap-0.5 rounded-lg bg-popover/90 p-1 shadow-(--shadow-pop) backdrop-blur-sm">
      <button
        type="button"
        className={TOOL_BTN}
        aria-label="Zoom out"
        onClick={() => {
          zoomAt(1 / ZOOM.step)
        }}
      >
        <ZoomOut className="size-4" />
      </button>
      <span className="w-12 text-center text-[12px] text-muted-foreground tabular-nums">
        {Math.round(k * 100)}%
      </span>
      <button
        type="button"
        className={TOOL_BTN}
        aria-label="Zoom in"
        onClick={() => {
          zoomAt(ZOOM.step)
        }}
      >
        <ZoomIn className="size-4" />
      </button>
      <button type="button" className={TOOL_BTN} aria-label="Fit to screen" onClick={fit}>
        <Maximize2 className="size-4" />
      </button>
      <DialogPrimitive.Close className={TOOL_BTN} aria-label="Close">
        <XIcon className="size-4" />
      </DialogPrimitive.Close>
    </div>
  )
}

/**
 * Full-screen diagram viewer: a dialog covering 90% of the viewport with the
 * classic image-viewer gestures (see `useViewerGestures`); Esc (Base UI) or the
 * cross closes it.
 *
 * Built on the Base UI primitive directly: `lib/` may not import from
 * `components/` (boundaries rule). The diagram is laid out once at intrinsic
 * size and moved with one CSS transform, so zoom stays crisp (vector) and never
 * re-runs mermaid's layout.
 */
function MermaidViewer({
  svg,
  origin,
  onClose,
}: {
  svg: string
  origin: HTMLElement | null
  onClose: () => void
}): ReactNode {
  const popupRef = useRef<HTMLDivElement>(null)
  const backdropRef = useRef<HTMLDivElement>(null)
  const closingRef = useRef(false)
  const stageRef = useRef<HTMLDivElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const [stage, setStage] = useState<HTMLDivElement | null>(null)
  const g = useViewerGestures(stageRef, contentRef, stage)

  return (
    <DialogPrimitive.Root
      open
      onOpenChange={(next) => {
        // Stay mounted (open) while the close flourish plays; Esc / ✕ /
        // backdrop clicks all land here, the ref ignores repeats mid-flight.
        if (next || closingRef.current) {
          return
        }

        closingRef.current = true
        animateCloseToOrigin(popupRef.current, backdropRef.current, origin, onClose)
      }}
    >
      <DialogPrimitive.Portal>
        <DialogPrimitive.Backdrop
          ref={backdropRef}
          className="fixed inset-0 z-50 bg-black/40 backdrop-blur-[3px] transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0"
        />
        <DialogPrimitive.Popup
          ref={popupRef}
          className="fixed top-1/2 left-1/2 z-50 flex h-[90vh] w-[90vw] -translate-1/2 animate-[modal-pop_.2s_cubic-bezier(.16,1,.3,1)] flex-col overflow-hidden rounded-2xl bg-popover text-popover-foreground shadow-(--shadow-pop) outline-none"
          onKeyDown={g.onKeyDown}
        >
          <DialogPrimitive.Title className="sr-only">Diagram</DialogPrimitive.Title>
          <ViewerToolbar k={g.view.k} zoomAt={g.zoomAt} fit={g.fit} />
          <div
            ref={(el) => {
              stageRef.current = el
              setStage(el)
            }}
            className="relative flex-1 cursor-grab touch-none overflow-hidden select-none active:cursor-grabbing"
            onPointerDown={g.onPointerDown}
            onPointerMove={g.onPointerMove}
            onPointerUp={g.onPointerUp}
            onPointerCancel={g.onPointerUp}
            onDoubleClick={g.fit}
          >
            <div
              ref={contentRef}
              className="absolute top-1/2 left-1/2"
              style={{
                transform: `translate(-50%, -50%) translate(${g.view.x}px, ${g.view.y}px) scale(${g.view.k})`,
              }}
              // Same strict-sanitised mermaid SVG as the inline copy.
              dangerouslySetInnerHTML={{ __html: resizeSvg(svg, "viewer") }}
            />
          </div>
        </DialogPrimitive.Popup>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  )
}
