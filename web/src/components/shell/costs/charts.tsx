import type { Slice, CostRow } from "./parse"
import { fmtDollar } from "./format"

// ── Shared constants ────────────────────────────────────────────────────────

const FONT = "system-ui, -apple-system, sans-serif"

// ── Donut chart ─────────────────────────────────────────────────────────────

interface DonutProps {
  data: Slice[]
  size?: number
  title?: string | undefined
}

/**
 * SVG donut (ring) chart with labels. Gracefully shows "No data" when empty.
 */
export function DonutChart({ data, size = 220, title }: DonutProps) {
  const total = data.reduce((s, d) => s + d.value, 0)
  if (total === 0) return <EmptyChart title={title} />

  const cx = size / 2
  const cy = size / 2
  const r = size * 0.36
  const stroke = size * 0.16
  const circumference = 2 * Math.PI * r

  let offset = 0
  const arcs = data.map((d) => {
    const ratio = d.value / total
    const dashArray = `${circumference * ratio} ${circumference * (1 - ratio)}`
    const dashOffset = -offset
    offset += circumference * ratio
    return { ...d, dashArray, dashOffset, ratio }
  })

  return (
    <div className="flex flex-col items-center gap-3">
      {title && <span className="text-[13px] font-semibold text-foreground/80">{title}</span>}
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        {arcs.map((a) => (
          <circle
            key={a.label}
            cx={cx}
            cy={cy}
            r={r}
            fill="none"
            stroke={a.color}
            strokeWidth={stroke}
            strokeDasharray={a.dashArray}
            strokeDashoffset={a.dashOffset}
            transform={`rotate(-90 ${cx} ${cy})`}
            className="transition-all duration-500"
          />
        ))}
        <text
          x={cx}
          y={cy - 6}
          textAnchor="middle"
          dominantBaseline="middle"
          fill="currentColor"
          fontSize={14}
          fontWeight={700}
          fontFamily={FONT}
          className="text-foreground"
        >
          {total < 1000 ? total.toFixed(0) : `${(total / 1000).toFixed(1)}K`}
        </text>
        <text
          x={cx}
          y={cy + 12}
          textAnchor="middle"
          dominantBaseline="middle"
          fill="currentColor"
          fontSize={10}
          fontFamily={FONT}
          className="text-muted-foreground"
        >
          total
        </text>
      </svg>
      <Legend items={arcs.map((a) => ({ label: a.label, color: a.color, value: a.value, pct: a.ratio }))} />
    </div>
  )
}

// ── Horizontal bar chart ────────────────────────────────────────────────────

interface HBarProps {
  data: Slice[]
  title?: string | undefined
  format?: (v: number) => string
}


export function HBarChart({ data, title, format = fmtDollar }: HBarProps) {
  const max = Math.max(...data.map((d) => d.value), 1)
  if (data.length === 0) return <EmptyChart title={title} />

  return (
    <div className="flex flex-col gap-3">
      {title && <span className="text-[13px] font-semibold text-foreground/80">{title}</span>}
      <div className="flex flex-col gap-2">
        {data.map((d) => {
          const pct = (d.value / max) * 100
          return (
            <div key={d.label} className="flex items-center gap-2">
              <span className="w-28 shrink-0 truncate text-right text-[11px] text-muted-foreground">{d.label}</span>
              <div className="relative h-5 min-w-0 flex-1 overflow-hidden rounded-md bg-muted/40">
                <div
                  className="absolute inset-y-0 left-0 rounded-md transition-all duration-500"
                  style={{ width: `${Math.max(pct, 1)}%`, backgroundColor: d.color }}
                />
              </div>
              <span className="w-16 shrink-0 text-right text-[11px] tabular-nums text-foreground/70">
                {format(d.value)}
              </span>
            </div>
          )
        })}
      </div>
    </div>
  )
}

// ── Stacked area timeline ───────────────────────────────────────────────────

interface TimelineProps {
  rows: CostRow[]
  title?: string | undefined
  width?: number
  height?: number
}

/**
 * Stacked area chart showing hit/miss/output cost over time (by tick index).
 */
export function CostTimeline({ rows, title, width = 600, height = 200 }: TimelineProps) {
  if (rows.length < 2) return <EmptyChart title={title} />

  const pad = { top: 20, right: 16, bottom: 24, left: 48 }
  const w = width - pad.left - pad.right
  const h = height - pad.top - pad.bottom

  // Stack: output on top, miss in middle, hit at bottom
  const maxCost = Math.max(...rows.map((r) => r.hitCost + r.missCost + r.outCost), 0.0001)

  const xScale = (i: number) => pad.left + (i / (rows.length - 1)) * w
  const yScale = (v: number) => pad.top + h - (v / maxCost) * h

  const buildArea = (accessor: (r: CostRow, base: number) => [number, number]) => {
    const upper: string[] = []
    const lower: string[] = []
    rows.forEach((r, i) => {
      const [lo, hi] = accessor(r, 0)
      upper.push(`${xScale(i)},${yScale(hi)}`)
      lower.unshift(`${xScale(i)},${yScale(lo)}`)
    })
    return `M${upper.join("L")}L${lower.join("L")}Z`
  }

  const hitArea = buildArea((r) => [0, r.hitCost])
  const missArea = buildArea((r) => [r.hitCost, r.hitCost + r.missCost])
  const outArea = buildArea((r) => [r.hitCost + r.missCost, r.hitCost + r.missCost + r.outCost])

  // Y-axis ticks (4 divisions)
  const yTicks = [0, 0.25, 0.5, 0.75, 1].map((f) => f * maxCost)

  return (
    <div className="flex flex-col gap-2">
      {title && <span className="text-[13px] font-semibold text-foreground/80">{title}</span>}
      <svg width="100%" viewBox={`0 0 ${width} ${height}`} className="overflow-visible">
        {/* Grid lines */}
        {yTicks.map((t) => (
          <g key={t}>
            <line
              x1={pad.left}
              x2={width - pad.right}
              y1={yScale(t)}
              y2={yScale(t)}
              stroke="currentColor"
              strokeOpacity={0.08}
            />
            <text
              x={pad.left - 6}
              y={yScale(t)}
              textAnchor="end"
              dominantBaseline="middle"
              fontSize={9}
              fill="currentColor"
              fillOpacity={0.4}
              fontFamily={FONT}
            >
              {fmtDollar(t)}
            </text>
          </g>
        ))}

        {/* Areas */}
        <path d={hitArea} fill="var(--ok, #4ade80)" fillOpacity={0.5} />
        <path d={missArea} fill="var(--warn, #fbbf24)" fillOpacity={0.5} />
        <path d={outArea} fill="#60a5fa" fillOpacity={0.5} />

        {/* X-axis label */}
        <text
          x={width / 2}
          y={height - 2}
          textAnchor="middle"
          fontSize={10}
          fill="currentColor"
          fillOpacity={0.4}
          fontFamily={FONT}
        >
          tick index ({rows.length} ticks)
        </text>
      </svg>
      <div className="flex items-center gap-4 text-[11px] text-muted-foreground">
        <span className="flex items-center gap-1">
          <span className="inline-block size-2.5 rounded-sm" style={{ backgroundColor: "var(--ok, #4ade80)" }} />
          Hit
        </span>
        <span className="flex items-center gap-1">
          <span className="inline-block size-2.5 rounded-sm" style={{ backgroundColor: "var(--warn, #fbbf24)" }} />
          Miss
        </span>
        <span className="flex items-center gap-1">
          <span className="inline-block size-2.5 rounded-sm" style={{ backgroundColor: "#60a5fa" }} />
          Output
        </span>
      </div>
    </div>
  )
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function EmptyChart({ title }: { title?: string | undefined }) {
  return (
    <div className="flex flex-col items-center gap-2 py-8 text-muted-foreground">
      {title && <span className="text-[13px] font-semibold text-foreground/80">{title}</span>}
      <span className="text-[12px]">No data yet</span>
    </div>
  )
}

function Legend({
  items,
}: {
  items: { label: string; color: string; value: number; pct: number }[]
}) {
  return (
    <div className="flex flex-wrap justify-center gap-x-4 gap-y-1">
      {items.map((it) => (
        <span key={it.label} className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
          <span className="inline-block size-2.5 shrink-0 rounded-sm" style={{ backgroundColor: it.color }} />
          <span className="truncate">{it.label}</span>
          <span className="tabular-nums text-foreground/60">{(it.pct * 100).toFixed(0)}%</span>
        </span>
      ))}
    </div>
  )
}
