import { use, type ComponentProps, type ReactNode } from "react"
import {
  Area,
  AreaChart as RAreaChart,
  Bar,
  BarChart as RBarChart,
  CartesianGrid,
  Label,
  Line,
  LineChart as RLineChart,
  Pie,
  PieChart as RPieChart,
  PolarAngleAxis,
  PolarGrid,
  PolarRadiusAxis,
  Radar,
  RadarChart as RRadarChart,
  RadialBar,
  RadialBarChart,
  XAxis,
  YAxis,
} from "recharts"

import { cn } from "@/lib/utils"

import { StoreContext, numProp, numbersArg, str, stringLists, toneColor } from "../openuiKit"
import type { OpenUiArg, OpenUiNode } from "../openuiParse"
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "./primitives"

// ── OpenUI Lang — charts on shadcn/ui `chart` (Recharts) ─────────────────────
//
// Every chart takes its x categories as the first string list and one series
// per number list, in order:
//   AreaChart(["Mon","Tue","Wed"], [3,5,4], [1,2,2], series: "Runs, Passed")
// Series keys are `s0`, `s1`… (CSS-safe); `series:` names them and `tones:`
// colours them ("signal, ok"), defaulting to the palette below. Pie/Radial take
// one number list and colour each slice instead.

const PALETTE = ["signal", "interactive", "ok", "warn", "danger"]

/** Comma-separated string prop as a trimmed list (`"a, b"` → `["a","b"]`). */
const listProp = (node: OpenUiNode, key: string): string[] => {
  const v = node.props[key]
  return typeof v === "string"
    ? v
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean)
    : []
}

/** Every `numbers` list arg (one per series). */
const numberLists = (node: OpenUiNode): number[][] =>
  node.args
    .filter((a): a is Extract<OpenUiArg, { kind: "numbers" }> => a.kind === "numbers")
    .map((a) => a.values)

const flag = (node: OpenUiNode, key: string, fallback = false): boolean => {
  const v = node.props[key]
  return typeof v === "boolean" ? v : fallback
}

/** Colour for item `i`: the `tones:` list, else the single `tone:`, else the palette. */
function colourAt(node: OpenUiNode, i: number): string {
  const tones = listProp(node, "tones")
  const tone = tones[i] ?? (i === 0 ? node.props["tone"] : undefined) ?? PALETTE[i % PALETTE.length]
  return toneColor(tone)
}

/** Shape of a multi-series chart: rows for recharts + the series config. */
interface Series {
  data: Record<string, string | number>[]
  keys: string[]
  config: ChartConfig
}

function series(node: OpenUiNode): Series {
  const lists = numberLists(node)
  const labels = stringLists(node)[0] ?? []
  const names = listProp(node, "series")
  const keys = lists.map((_, i) => `s${String(i)}`)
  const len = Math.max(0, ...lists.map((l) => l.length))
  const data = Array.from({ length: len }, (_, r) => {
    const row: Record<string, string | number> = { label: labels[r] ?? String(r + 1) }
    for (const [k, l] of lists.entries()) row[`s${String(k)}`] = l[r] ?? 0
    return row
  })
  const config: ChartConfig = Object.fromEntries(
    keys.map((k, i) => [
      k,
      {
        label: names[i] ?? (keys.length > 1 ? `Series ${String(i + 1)}` : "Value"),
        color: colourAt(node, i),
      },
    ]),
  )
  return { data, keys, config }
}

/** Currency units go before the number, every other `unit:` after it. */
const PREFIX_UNITS = new Set(["$", "€", "£"])

/** Tooltip swatch styles accepted by `indicator:`. */
const INDICATORS = ["dot", "line", "dashed", "none"] as const

/** Value formatter from `unit:` ("%", " ms", "$"…). */
function formatter(node: OpenUiNode): (v: number) => string {
  const unit = str(node.props["unit"])
  const prefix = PREFIX_UNITS.has(unit)
  return (v) => (prefix ? `${unit}${v.toLocaleString()}` : `${v.toLocaleString()}${unit}`)
}

/** Tooltip shared by every chart: `indicator: dot|line|dashed|none`, `total: true`. */
function tooltip(node: OpenUiNode, hideLabel = false): ReactNode {
  const raw = str(node.props["indicator"], "dot")
  const ind = INDICATORS.find((k) => k === raw) ?? "dot"
  return (
    <ChartTooltip
      cursor={ind !== "none"}
      content={
        <ChartTooltipContent
          indicator={ind}
          hideLabel={hideLabel}
          showTotal={flag(node, "total")}
          format={formatter(node)}
        />
      }
    />
  )
}

/** Legend when asked (`legend: true`), on by default for several series. */
const legend = (node: OpenUiNode, many: boolean): ReactNode =>
  flag(node, "legend", many) ? <ChartLegend content={<ChartLegendContent />} /> : null

/** Title / description header + the chart frame at `height:` px (default 220). */
function Frame({
  node,
  config,
  children,
  square = false,
}: {
  node: OpenUiNode
  config: ChartConfig
  children: ComponentProps<typeof ChartContainer>["children"]
  square?: boolean
}): ReactNode {
  const title = str(node.props["title"])
  const desc = str(node.props["description"])
  const height = numProp(node, "height") ?? (square ? 240 : 220)
  return (
    <div className="flex min-w-0 flex-col gap-3">
      {(title || desc) && (
        <div className="flex flex-col gap-0.5">
          {title && <span className="text-[14px] font-semibold text-foreground">{title}</span>}
          {desc && <span className="text-[12px] text-muted-foreground">{desc}</span>}
        </div>
      )}
      <ChartContainer
        config={config}
        className={cn("aspect-auto w-full", square && "mx-auto max-w-[320px]")}
        style={{ height }}
      >
        {children}
      </ChartContainer>
    </div>
  )
}

const X_AXIS = (
  <XAxis dataKey="label" tickLine={false} axisLine={false} tickMargin={8} minTickGap={16} />
)

/** `AreaChart(labels, series…, stacked, curve: natural|linear|step, gradient)`. */
export function AreaChart({ node }: { node: OpenUiNode }): ReactNode {
  const s = series(node)
  if (s.keys.length === 0) return null
  const curve = str(node.props["curve"], "natural") as "natural" | "linear" | "step"
  const stacked = flag(node, "stacked")
  const gradient = flag(node, "gradient", true)
  const gid = `ou-area-${String(node.args.length)}-${s.keys.join("")}`
  return (
    <Frame node={node} config={s.config}>
      <RAreaChart data={s.data} margin={{ left: 4, right: 12, top: 8 }}>
        <defs>
          {s.keys.map((k) => (
            <linearGradient key={k} id={`${gid}-${k}`} x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor={`var(--color-${k})`} stopOpacity={0.5} />
              <stop offset="95%" stopColor={`var(--color-${k})`} stopOpacity={0.05} />
            </linearGradient>
          ))}
        </defs>
        <CartesianGrid vertical={false} />
        {X_AXIS}
        {tooltip(node)}
        {s.keys.map((k) => (
          <Area
            key={k}
            dataKey={k}
            type={curve}
            stroke={`var(--color-${k})`}
            strokeWidth={2}
            fill={gradient ? `url(#${gid}-${k})` : `var(--color-${k})`}
            fillOpacity={gradient ? 1 : 0.25}
            {...(stacked ? { stackId: "a" } : {})}
          />
        ))}
        {legend(node, s.keys.length > 1)}
      </RAreaChart>
    </Frame>
  )
}

/** `BarChart(labels, series…, stacked, horizontal)`; legacy `BarChart([v…], [labels], tone)` too. */
export function BarChart({ node }: { node: OpenUiNode }): ReactNode {
  const s = series(node)
  if (s.keys.length === 0) return null
  const stacked = flag(node, "stacked")
  const horizontal = flag(node, "horizontal")
  const last = s.keys.length - 1
  return (
    <Frame node={node} config={s.config}>
      <RBarChart
        data={s.data}
        layout={horizontal ? "vertical" : "horizontal"}
        margin={{ left: horizontal ? 0 : 4, right: 12, top: 8 }}
      >
        <CartesianGrid vertical={horizontal} horizontal={!horizontal} />
        {horizontal ? (
          <>
            <YAxis
              dataKey="label"
              type="category"
              tickLine={false}
              axisLine={false}
              tickMargin={8}
              width={96}
            />
            <XAxis type="number" hide />
          </>
        ) : (
          X_AXIS
        )}
        {tooltip(node)}
        {s.keys.map((k, i) => (
          <Bar
            key={k}
            dataKey={k}
            fill={`var(--color-${k})`}
            radius={stacked ? (i === last ? [4, 4, 0, 0] : 0) : 4}
            {...(stacked ? { stackId: "a" } : {})}
          />
        ))}
        {legend(node, s.keys.length > 1)}
      </RBarChart>
    </Frame>
  )
}

/** `LineChart(labels, series…, curve, dots)`. */
export function LineChart({ node }: { node: OpenUiNode }): ReactNode {
  const s = series(node)
  if (s.keys.length === 0) return null
  const curve = str(node.props["curve"], "natural") as "natural" | "linear" | "step"
  const dots = flag(node, "dots")
  return (
    <Frame node={node} config={s.config}>
      <RLineChart data={s.data} margin={{ left: 12, right: 12, top: 8 }}>
        <CartesianGrid vertical={false} />
        {X_AXIS}
        {tooltip(node)}
        {s.keys.map((k) => (
          <Line
            key={k}
            dataKey={k}
            type={curve}
            stroke={`var(--color-${k})`}
            strokeWidth={2}
            dot={dots ? { fill: `var(--color-${k})` } : false}
            activeDot={{ r: 5 }}
          />
        ))}
        {legend(node, s.keys.length > 1)}
      </RLineChart>
    </Frame>
  )
}

/** `RadarChart(axes, series…, dots)`. */
export function RadarChart({ node }: { node: OpenUiNode }): ReactNode {
  const s = series(node)
  if (s.keys.length === 0) return null
  const many = s.keys.length > 1
  return (
    <Frame node={node} config={s.config} square>
      <RRadarChart data={s.data}>
        {tooltip(node)}
        <PolarAngleAxis dataKey="label" />
        <PolarGrid />
        {s.keys.map((k, i) => (
          <Radar
            key={k}
            dataKey={k}
            fill={`var(--color-${k})`}
            fillOpacity={many && i > 0 ? 0.3 : 0.55}
            stroke={`var(--color-${k})`}
            dot={flag(node, "dots") ? { r: 3, fillOpacity: 1 } : false}
          />
        ))}
        {legend(node, many)}
      </RRadarChart>
    </Frame>
  )
}

/** Slices: one number list + labels, each item its own colour (`slice0`…). */
function slices(node: OpenUiNode): {
  data: Record<string, string | number>[]
  config: ChartConfig
} {
  const values = numbersArg(node)
  const labels = stringLists(node)[0] ?? []
  const data = values.map((v, i) => ({
    key: `slice${String(i)}`,
    value: v,
    fill: `var(--color-slice${String(i)})`,
  }))
  const config: ChartConfig = Object.fromEntries(
    values.map((_, i) => [
      `slice${String(i)}`,
      { label: labels[i] ?? String(i + 1), color: colourAt(node, i) },
    ]),
  )
  return { data, config: { value: { label: str(node.props["unit"]) || "Value" }, ...config } }
}

/** Big centred number + caption inside a donut / radial chart. */
function centre(value: string, caption: string): ReactNode {
  return (
    <Label
      content={({ viewBox }) => {
        if (!viewBox || !("cx" in viewBox)) return null
        const { cx, cy } = viewBox
        return (
          <text x={cx} y={cy} textAnchor="middle" dominantBaseline="middle">
            <tspan x={cx} y={cy} className="fill-foreground text-[24px] font-bold">
              {value}
            </tspan>
            {caption && (
              <tspan x={cx} y={cy + 22} className="fill-muted-foreground text-[12px]">
                {caption}
              </tspan>
            )}
          </text>
        )
      }}
    />
  )
}

/** `PieChart(values, labels, donut: true, center: "1,045", caption: "lints")`. */
export function PieChart({ node }: { node: OpenUiNode }): ReactNode {
  const { data, config } = slices(node)
  if (data.length === 0) return null
  const donut = flag(node, "donut", true)
  const center = str(node.props["center"])
  return (
    <Frame node={node} config={config} square>
      <RPieChart>
        {tooltip(node, true)}
        <Pie
          data={data}
          dataKey="value"
          nameKey="key"
          innerRadius={donut ? "58%" : 0}
          strokeWidth={4}
          stroke="var(--card)"
        >
          {donut && center && centre(center, str(node.props["caption"]))}
        </Pie>
        {flag(node, "legend", true) && (
          <ChartLegend content={<ChartLegendContent nameKey="key" />} />
        )}
      </RPieChart>
    </Frame>
  )
}

/** `RadialChart(values 0..max, labels, max: 100)`: one concentric bar per value. */
export function RadialChart({ node }: { node: OpenUiNode }): ReactNode {
  const { data, config } = slices(node)
  if (data.length === 0) return null
  const max = numProp(node, "max") ?? 100
  const center = str(node.props["center"])
  return (
    <Frame node={node} config={config} square>
      <RadialBarChart
        data={data}
        innerRadius={data.length > 1 ? "30%" : "70%"}
        outerRadius="100%"
        startAngle={90}
        endAngle={-270}
      >
        <PolarAngleAxis type="number" domain={[0, max]} tick={false} />
        {tooltip(node, true)}
        <RadialBar dataKey="value" background cornerRadius={10} />
        <PolarRadiusAxis tick={false} tickLine={false} axisLine={false}>
          {center && centre(center, str(node.props["caption"]))}
        </PolarRadiusAxis>
        {data.length > 1 && flag(node, "legend", true) && (
          <ChartLegend content={<ChartLegendContent nameKey="key" />} />
        )}
      </RadialBarChart>
    </Frame>
  )
}

/** `Ring(value: 0..100, label, tone)`: a single radial gauge with the % in the middle. */
export function Ring({ node }: { node: OpenUiNode }): ReactNode {
  const pct = Math.max(0, Math.min(100, numProp(node, "value") ?? 0))
  const config: ChartConfig = {
    value: { label: str(node.props["label"]), color: toneColor(node.props["tone"]) },
  }
  return (
    <div className="flex flex-col items-center gap-1">
      <ChartContainer config={config} className="aspect-square h-28">
        <RadialBarChart
          data={[{ value: pct, fill: "var(--color-value)" }]}
          innerRadius="78%"
          outerRadius="100%"
          startAngle={90}
          endAngle={-270}
        >
          <PolarAngleAxis type="number" domain={[0, 100]} tick={false} />
          <RadialBar dataKey="value" background cornerRadius={10} />
          <PolarRadiusAxis tick={false} tickLine={false} axisLine={false}>
            {centre(`${String(Math.round(pct))}%`, "")}
          </PolarRadiusAxis>
        </RadialBarChart>
      </ChartContainer>
      <span className="text-[12px] text-muted-foreground">{str(node.props["label"])}</span>
    </div>
  )
}

/** Axis-less trend area (Metric trends, `Sparkline([…], tone)`), with a value tooltip. */
export function Sparkline({
  values,
  color,
  height = 44,
}: {
  values: number[]
  color: string
  height?: number
}): ReactNode {
  const s = use(StoreContext)
  const config: ChartConfig = { v: { label: "Value", color } }
  const data = values.map((v, i) => ({ i: i + 1, v }))
  const gid = `ou-spark-${String(values.length)}-${String(values[0] ?? 0)}-${s.onAccent ? "a" : "d"}`
  return (
    <ChartContainer config={config} className="aspect-auto w-full" style={{ height }}>
      <RAreaChart data={data} margin={{ top: 4, bottom: 2, left: 2, right: 2 }}>
        <defs>
          <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor="var(--color-v)" stopOpacity={0.35} />
            <stop offset="95%" stopColor="var(--color-v)" stopOpacity={0} />
          </linearGradient>
        </defs>
        <ChartTooltip cursor={false} content={<ChartTooltipContent hideLabel />} />
        <Area
          dataKey="v"
          type="natural"
          stroke="var(--color-v)"
          strokeWidth={2}
          fill={`url(#${gid})`}
          dot={false}
        />
      </RAreaChart>
    </ChartContainer>
  )
}

/** Renderer-table entry for `Sparkline([…], tone)`. */
export function SparklineNode({ node }: { node: OpenUiNode }): ReactNode {
  const values = numbersArg(node)
  return values.length > 1 ? (
    <Sparkline values={values} color={toneColor(node.props["tone"])} />
  ) : null
}
