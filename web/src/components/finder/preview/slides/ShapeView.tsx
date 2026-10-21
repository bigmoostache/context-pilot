import { useId, type CSSProperties } from "react"
import type { SlideShape, SlideStroke } from "@/lib/api"
import { dashArray, px } from "./units"
import { TextBox } from "./TextBox"
import { TableBox } from "./TableBox"

/** Rotation unit: 60 000ths of a degree. */
const ROT_UNIT = 60_000

/** A positioned shape: geometry (fill, picture, outline), then text or table. */
export function ShapeView({ shape, scale }: { shape: SlideShape; scale: number }) {
  const style: CSSProperties = {
    position: "absolute",
    left: px(shape.x, scale),
    top: px(shape.y, scale),
    width: px(shape.w, scale),
    height: px(shape.h, scale),
    transform: shape.rot ? `rotate(${shape.rot / ROT_UNIT}deg)` : undefined,
  }
  return (
    <div style={style} data-shape={shape.name}>
      {shape.path && <Geometry shape={shape} scale={scale} />}
      {shape.table && <TableBox table={shape.table} scale={scale} />}
      {shape.text && <TextBox body={shape.text} scale={scale} />}
    </div>
  )
}

/**
 * SVG outline of a shape in its own EMU box, stretched to the wrapper. Strokes
 * use `non-scaling-stroke`, so their width is given in screen px.
 */
function Geometry({ shape, scale }: { shape: SlideShape; scale: number }) {
  const clipId = useId()
  const w = Math.max(shape.w, 1)
  const h = Math.max(shape.h, 1)
  const flip = `translate(${shape.flipH ? w : 0} ${shape.flipV ? h : 0}) scale(${shape.flipH ? -1 : 1} ${shape.flipV ? -1 : 1})`
  const solid = shape.fill && !shape.fill.includes("gradient") ? shape.fill : undefined
  const painted = Boolean((shape.fill && !solid) || shape.image)
  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      preserveAspectRatio="none"
      style={{ position: "absolute", inset: 0, width: "100%", height: "100%", overflow: "visible" }}
    >
      <g transform={flip}>
        {painted && (
          <>
            <clipPath id={clipId}>
              <path d={shape.path} />
            </clipPath>
            <g clipPath={`url(#${clipId})`}>
              {shape.fill && !solid && (
                <foreignObject width={w} height={h}>
                  <div style={{ width: "100%", height: "100%", background: shape.fill }} />
                </foreignObject>
              )}
              {shape.image && (
                <image href={shape.image} width={w} height={h} preserveAspectRatio="none" />
              )}
            </g>
          </>
        )}
        <Outline d={shape.path ?? ""} fill={solid} stroke={shape.stroke} scale={scale} />
      </g>
    </svg>
  )
}

/** The path itself: solid fill and/or stroke with dash and arrow ends. */
function Outline(props: {
  d: string
  fill: string | undefined
  stroke: SlideStroke | undefined
  scale: number
}) {
  const { d, fill, stroke, scale } = props
  const markerId = useId()
  const width = stroke ? Math.max(px(stroke.width, scale), 0.75) : 0
  const arrows = Boolean(stroke && (stroke.head || stroke.tail))
  return (
    <>
      {stroke && arrows && (
        <defs>
          <marker
            id={markerId}
            viewBox="0 0 10 10"
            refX="5"
            refY="5"
            markerWidth="4"
            markerHeight="4"
            orient="auto-start-reverse"
          >
            <path d="M0,0 L10,5 L0,10 z" fill={stroke.color} />
          </marker>
        </defs>
      )}
      <path
        d={d}
        fill={fill ?? "none"}
        stroke={stroke?.color ?? "none"}
        strokeWidth={width}
        strokeDasharray={dashArray(stroke?.dash, width)}
        markerEnd={stroke?.head ? `url(#${markerId})` : undefined}
        markerStart={stroke?.tail ? `url(#${markerId})` : undefined}
        vectorEffect="non-scaling-stroke"
      />
    </>
  )
}
