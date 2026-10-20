import { useLayoutEffect, useRef, useState } from "react"
import type { Slide, SlideDeck as Deck } from "@/lib/api"
import { ShapeView } from "./ShapeView"

/** Horizontal breathing room around each slide, px. */
const GUTTER = 24

/**
 * A whole presentation, slides stacked vertically and scaled to the pane
 * width. Every shape arrives already resolved through master → layout → slide
 * (positions, fills, fonts, bullets), so this only places boxes.
 */
export function SlideDeck({ deck }: { deck: Deck }) {
  const hostRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  useLayoutEffect(() => {
    const el = hostRef.current
    if (!el) return
    // ResizeObserver fires once on observe, so the initial width flows through
    // the same path — no synchronous setState in the effect body.
    const observer = new ResizeObserver(() => setWidth(el.clientWidth))
    observer.observe(el)
    return () => observer.disconnect()
  }, [])
  const scale = deck.width > 0 ? Math.max(width - GUTTER * 2, 0) / deck.width : 0
  return (
    <div ref={hostRef} className="min-h-0 flex-1 overflow-auto bg-muted/30 py-6">
      {scale > 0 &&
        deck.slides.map((slide) => (
          <SlideFrame
            key={slide.index}
            slide={slide}
            scale={scale}
            deck={deck}
            total={deck.slides.length}
          />
        ))}
      {deck.truncated && (
        <p className="px-6 text-center text-xs text-foreground/80">
          Preview truncated — later slides are not shown.
        </p>
      )}
    </div>
  )
}

/** One slide card: number, canvas, speaker notes. */
function SlideFrame(props: { slide: Slide; scale: number; deck: Deck; total: number }) {
  const { slide, scale, deck, total } = props
  return (
    <section className="mx-auto mb-8" style={{ width: deck.width * scale }}>
      <header className="mb-1.5 flex items-center gap-2 text-xs text-foreground/80">
        <span className="font-medium tabular-nums">
          {slide.index} / {total}
        </span>
        {slide.hidden && <span>· hidden</span>}
      </header>
      <div
        className="relative overflow-hidden rounded-sm shadow-md"
        style={{
          width: deck.width * scale,
          height: deck.height * scale,
          background: slide.background ?? "#ffffff",
        }}
      >
        {slide.shapes.map((shape, i) => (
          <ShapeView key={i} shape={shape} scale={scale} />
        ))}
      </div>
      {slide.notes && (
        <p className="mt-2 text-xs whitespace-pre-wrap text-foreground/80">{slide.notes}</p>
      )}
    </section>
  )
}
