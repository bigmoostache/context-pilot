import { type ReactNode, type Ref } from "react"
import { cn } from "@/lib/utils"

// ── Frosted floating bottom bar (mobile chrome) ──────────────────────
//
// The shared primitive behind every mobile surface's floating bottom control
// strip (the thread composer, the thread-sidebar search/create bar, the agents
// list search/create bar). Factored to ONE implementation (design rule M141) so
// the gradient-blur look + fixed-bottom behaviour is defined once and every
// consumer inherits it identically.
//
// It renders as an ABSOLUTE overlay pinned to the bottom of its (relatively
// positioned) parent, so the scroll content flows BEHIND it and is frosted by
// the ProgressiveBlur layer. Because it overlays rather than sits in-flow, each
// consumer must reserve a bottom spacer in its own scroll content so the last
// item can scroll clear of the bar — measure this bar's height with
// `useElementHeight` and render a `1.5×` spacer at the end of the scroll area.
//
// NOTE ON PLACEMENT: like `CornerButton`, this is mobile chrome that lives in
// the desktop `components/` tree because that is the mirror's source-of-truth
// side (design §5 — every mobile file has a path-parity desktop twin). It is
// consumed only through its `mobile-components/shell/FrostedBottomBar` stub;
// desktop surfaces don't render it.

/**
 * True *progressive* backdrop-blur — max at the bottom fading to zero at the top.
 * CSS has no gradient `backdrop-filter`, so this is the layered-mask trick:
 * stacked layers, each a STRONGER `backdrop-blur` masked with a `linear-gradient`
 * reaching LESS far up, so the heaviest blur pools at the bottom and ramps
 * smoothly to none at the top. `-webkit-` prefixes on `backdropFilter` +
 * `maskImage` are load-bearing on iOS Safari.
 */
export function ProgressiveBlur() {
  // Each layer: [blur px, how far up the mask stays opaque]. Stronger blur =
  // shorter reach, so heavier blur pools at the bottom.
  const layers: [number, number][] = [
    [0.5, 100],
    [1.5, 75],
    [3, 50],
    [6, 25],
  ]
  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
      {layers.map(([blur, reach], i) => {
        const grad = `linear-gradient(to top, black 0%, transparent ${reach}%)`
        return (
          <div
            key={i}
            className="absolute inset-0"
            style={{
              backdropFilter: `blur(${blur}px) saturate(150%)`,
              WebkitBackdropFilter: `blur(${blur}px) saturate(150%)`,
              maskImage: grad,
              WebkitMaskImage: grad,
            }}
          />
        )
      })}
    </div>
  )
}

/**
 * A floating frosted bottom bar. Renders the {@link ProgressiveBlur} layer + a
 * gradient tint (stronger at the bottom) behind `children`, positioned as an
 * `absolute inset-x-0 bottom-0` overlay — so the parent must be `relative` and
 * the scroll content must reserve a spacer (see the file header). `ref` forwards
 * to the outer element so the consumer can measure its height for that spacer.
 * `className` is merged last for per-surface padding overrides.
 */
export function FrostedBottomBar({
  children,
  className,
  ref,
}: {
  children: ReactNode
  className?: string
  ref?: Ref<HTMLDivElement>
}) {
  return (
    <div
      ref={ref}
      className={cn(
        "absolute inset-x-0 bottom-0 z-10",
        "bg-linear-to-t from-background/90 via-background/45 to-transparent",
        className,
      )}
    >
      <ProgressiveBlur />
      {children}
    </div>
  )
}
