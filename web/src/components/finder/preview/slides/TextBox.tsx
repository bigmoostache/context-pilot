import type { CSSProperties, ReactElement } from "react"
import type { SlideParagraph, SlideRun, SlideTextBody } from "@/lib/api"
import { anchorJustify, ptPx, px, textAlign } from "./units"

/** PowerPoint's "single" line height, as a multiple of the font size. */
const SINGLE_LINE = 1.2

/** Shrink-on-overflow factor (`normAutofit`), 1 = none. */
function fontScaleOf(body: SlideTextBody): number {
  return body.fontScale > 0 ? body.fontScale / 100_000 : 1
}

/** A text body laid out inside its shape's box (insets, anchor, direction). */
export function TextBox({ body, scale }: { body: SlideTextBody; scale: number }) {
  const [left, top, right, bottom] = body.insets
  const vertical = body.vert === "vert" || body.vert === "eaVert" ? "vertical-rl" : undefined
  const style: CSSProperties = {
    position: "absolute",
    inset: 0,
    display: "flex",
    flexDirection: "column",
    justifyContent: anchorJustify(body.anchor),
    padding: `${px(top ?? 0, scale)}px ${px(right ?? 0, scale)}px ${px(bottom ?? 0, scale)}px ${px(left ?? 0, scale)}px`,
    whiteSpace: body.wrap ? "pre-wrap" : "pre",
    overflowWrap: "break-word",
    writingMode: vertical,
    transform: body.vert === "vert270" ? "rotate(180deg)" : undefined,
  }
  const fontScale = fontScaleOf(body)
  return (
    <div style={style}>
      {body.paragraphs.map((para, i) => (
        <Paragraph key={i} para={para} scale={scale} fontScale={fontScale} />
      ))}
    </div>
  )
}

/** Line height: positive = 1/1000 percent of single spacing, negative = exact 1/100 pt. */
function lineHeight(spacing: number, scale: number): string {
  if (spacing < 0) return `${ptPx(-spacing, scale)}px`
  return String((spacing / 100_000) * SINGLE_LINE)
}

/** One paragraph: margins, hanging indent, spacing, bullet, runs. */
function Paragraph(props: { para: SlideParagraph; scale: number; fontScale: number }) {
  const { para, scale, fontScale } = props
  const first = para.runs.find((run) => run.text !== "\n")
  const baseSize = ptPx((first?.size ?? para.endSize) * fontScale, scale)
  const style: CSSProperties = {
    margin: 0,
    paddingLeft: px(para.marginLeft, scale),
    textIndent: px(para.indent, scale),
    textAlign: textAlign(para.align),
    lineHeight: lineHeight(para.lineSpacing, scale),
    marginTop: ptPx(para.spaceBefore, scale),
    marginBottom: ptPx(para.spaceAfter, scale),
    fontSize: baseSize,
    minHeight:
      para.runs.length === 0 ? ptPx(para.endSize * fontScale, scale) * SINGLE_LINE : undefined,
  }
  return (
    <p style={style}>
      {para.bullet && first && (
        <span
          style={{
            color: para.bullet.color ?? first.color,
            fontFamily: para.bullet.font ?? first.font,
            fontSize: baseSize * (para.bullet.scale / 100_000),
            display: "inline-block",
            minWidth: px(-Math.min(para.indent, 0), scale),
            textIndent: 0,
          }}
        >
          {para.bullet.text}
        </span>
      )}
      {para.runs.map((run, i) => (
        <Run key={i} run={run} scale={scale} fontScale={fontScale} />
      ))}
    </p>
  )
}

/** One formatted run (or a line break). */
function Run(props: { run: SlideRun; scale: number; fontScale: number }): ReactElement {
  const { run, scale, fontScale } = props
  if (run.text === "\n") return <br />
  const shifted = run.baseline !== 0
  const decoration = [run.underline && "underline", run.strike && "line-through"]
    .filter(Boolean)
    .join(" ")
  const style: CSSProperties = {
    fontFamily: `"${run.font}", system-ui, sans-serif`,
    fontSize: ptPx(run.size * fontScale, scale) * (shifted ? 0.66 : 1),
    fontWeight: run.bold ? 700 : 400,
    fontStyle: run.italic ? "italic" : undefined,
    textDecoration: decoration || undefined,
    color: run.color,
    background: run.highlight,
    verticalAlign: shifted ? `${run.baseline / 1000}%` : undefined,
    textTransform: run.caps === "all" ? "uppercase" : undefined,
    fontVariant: run.caps === "small" ? "small-caps" : undefined,
  }
  if (run.link)
    return (
      <a href={run.link} target="_blank" rel="noreferrer" style={style}>
        {run.text}
      </a>
    )
  return <span style={style}>{run.text}</span>
}
