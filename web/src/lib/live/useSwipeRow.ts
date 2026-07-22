import { useRef, useState } from "react"
import { animate, createSpring } from "animejs"
import { prefersReducedMotion } from "@/lib/utils"

// ── Swipe-to-reveal gesture engine (T630) ────────────────────────────
//
// The interaction behind the mobile ThreadList's iOS-Messages-style
// swipe-left-to-reveal-actions row, factored out of the component so the
// presentation file stays a thin renderer (design rule M141 — logic out of the
// component; here the "logic" is a self-contained pointer-gesture state machine).
//
// WHY it's a hand-rolled Pointer-Events drag and NOT React state per move: the
// naive version called `setState(dx)` on every `pointermove`, re-rendering the
// row (and its `Message` child) 60–120×/s — React's reconciliation lagged the
// finger (the "clicker" / unresponsive drag). This runs the gesture OFF the
// React render path, like a native list row:
//   • Direct DOM writes — the drag writes `el.style.transform` straight onto the
//     node (ref `dxRef`); zero React renders while the finger moves. The bound
//     element carries no `style` prop, so React never clobbers the inline
//     transform on an unrelated re-render (the two-writer bug). `open` state
//     flips only at REST.
//   • `touch-action: pan-y` (set by the consumer's className) — the browser
//     keeps vertical scroll, we own the horizontal axis, so scroll and swipe
//     never fight (no preventDefault / non-passive-listener gymnastics).
//   • Pointer capture + axis lock — the first ~8px decide the axis; a vertical
//     intent bails to native scroll, a horizontal one `setPointerCapture`s so
//     the drag survives the finger leaving the row. Pointer Events unify touch +
//     mouse in one path.
//   • Velocity-aware spring snap — on release a fast flick honours its direction
//     regardless of distance; otherwise it snaps to the nearer end (anime.js
//     spring; reduced-motion jumps).

/** Axis-lock threshold: how far a pointer must move before we commit to a
 *  horizontal swipe (below this the gesture is undecided and a mostly-vertical
 *  move yields to native scroll). */
const AXIS_LOCK_PX = 8
/** Flick speed (px/ms) above which release honours the flick DIRECTION over the
 *  drag distance — a fast short left-flick opens before the halfway point. */
const FLICK_VEL = 0.4

/** Mutable per-gesture bookkeeping, kept in a ref so the pointer handlers read
 *  live values without stale closures and without triggering re-renders. */
interface Gesture {
  startX: number
  startY: number
  lastX: number
  lastT: number
  vel: number
  base: number
  axis: "undecided" | "h" | "v"
  active: boolean
}

/** The pointer-handler props the consumer spreads onto the sliding element. */
export interface SwipeBindings {
  onPointerDown: (e: React.PointerEvent) => void
  onPointerMove: (e: React.PointerEvent) => void
  onPointerUp: () => void
  onPointerCancel: () => void
  onClickCapture: (e: React.MouseEvent) => void
}

/** What {@link useSwipeRow} hands back to a row component. */
export interface SwipeRow {
  /** Attach to the sliding element (the hook writes its transform directly). */
  rowRef: React.RefObject<HTMLDivElement | null>
  /** True once the row has settled open (action strip revealed). */
  open: boolean
  /** Force the row shut — for an action tap that should also close the row. */
  close: () => void
  /** Pointer handlers to spread onto the sliding element. */
  bind: SwipeBindings
}

/**
 * Drive a swipe-left-to-reveal row. `actionWidth` is the pixel width of the
 * revealed trailing action strip (the row slides at most this far left). See the
 * file header for the feel rationale.
 */
export function useSwipeRow(actionWidth: number): SwipeRow {
  const rowRef = useRef<HTMLDivElement>(null)
  const dxRef = useRef(0)
  const [open, setOpen] = useState(false)
  const gestureRef = useRef<Gesture>({
    startX: 0,
    startY: 0,
    lastX: 0,
    lastT: 0,
    vel: 0,
    base: 0,
    axis: "undecided",
    active: false,
  })

  /** Paint the current translate straight onto the node (no React render). */
  const paint = (x: number) => {
    dxRef.current = x
    if (rowRef.current) rowRef.current.style.transform = `translateX(${x}px)`
  }

  /** Snap to a resting position (0 or -actionWidth): spring there, then sync the
   *  `open` flag. Reduced-motion jumps straight to the target. */
  const settle = (target: number) => {
    if (prefersReducedMotion()) {
      paint(target)
      setOpen(target !== 0)
      return
    }
    const o = { x: dxRef.current }
    animate(o, {
      x: target,
      ease: createSpring({ stiffness: 420, damping: 38 }),
      onUpdate: () => paint(o.x),
      onComplete: () => setOpen(target !== 0),
    })
  }

  const close = () => {
    paint(0)
    setOpen(false)
  }

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.pointerType === "mouse" && e.button !== 0) return
    gestureRef.current = {
      startX: e.clientX,
      startY: e.clientY,
      lastX: e.clientX,
      lastT: performance.now(),
      vel: 0,
      base: dxRef.current,
      axis: "undecided",
      active: true,
    }
  }

  const onPointerMove = (e: React.PointerEvent) => {
    const s = gestureRef.current
    if (!s.active) return
    const dx = e.clientX - s.startX
    const dy = e.clientY - s.startY

    // First meaningful movement decides the axis: a vertical intent bails so the
    // native list scroll takes over; a horizontal one grabs the pointer so the
    // drag survives the finger leaving the row.
    if (s.axis === "undecided") {
      if (Math.abs(dx) < AXIS_LOCK_PX && Math.abs(dy) < AXIS_LOCK_PX) return
      if (Math.abs(dx) > Math.abs(dy)) {
        s.axis = "h"
        rowRef.current?.setPointerCapture(e.pointerId)
      } else {
        s.axis = "v"
        s.active = false
        return
      }
    }
    if (s.axis !== "h") return

    paint(Math.min(0, Math.max(-actionWidth, s.base + dx)))

    // Track instantaneous velocity (px/ms) for the flick decision on release.
    const now = performance.now()
    const dt = now - s.lastT || 1
    s.vel = (e.clientX - s.lastX) / dt
    s.lastX = e.clientX
    s.lastT = now
  }

  const onPointerEnd = () => {
    const s = gestureRef.current
    s.active = false
    if (s.axis !== "h") return
    // Fast flick → honour its direction; otherwise snap to the nearer end.
    let target: number
    if (s.vel <= -FLICK_VEL) target = -actionWidth
    else if (s.vel >= FLICK_VEL) target = 0
    else target = dxRef.current < -actionWidth / 2 ? -actionWidth : 0
    settle(target)
  }

  /** Swallow the tap that ENDS an open swipe so it closes the row instead of
   *  selecting the thread beneath. A tap on a closed row (dx 0) passes through. */
  const onClickCapture = (e: React.MouseEvent) => {
    if (dxRef.current === 0) return
    e.preventDefault()
    e.stopPropagation()
    close()
  }

  return {
    rowRef,
    open,
    close,
    bind: {
      onPointerDown,
      onPointerMove,
      onPointerUp: onPointerEnd,
      onPointerCancel: onPointerEnd,
      onClickCapture,
    },
  }
}
