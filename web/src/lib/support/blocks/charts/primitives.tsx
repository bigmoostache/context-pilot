import { createContext, use, useId, type ComponentProps, type ReactNode } from "react"
import { ResponsiveContainer } from "recharts"

import { cn } from "@/lib/utils"

// shadcn/ui `chart` (https://ui.shadcn.com/docs/components/chart), adapted to
// recharts 3 and this repo's lint. A ChartConfig maps each series key to a
// label + colour; ChartContainer exposes them as `--color-<key>` CSS variables
// so recharts elements use `fill="var(--color-<key>)"`.

export type ChartConfig = Record<string, { label?: ReactNode; color?: string }>

const ChartContext = createContext<ChartConfig>({})

/** Recharts theming from design tokens: axis text, grid, cursor, polar grid. */
const THEME = [
  "flex justify-center text-[12px]",
  "[&_.recharts-cartesian-axis-tick_text]:fill-muted-foreground",
  "[&_.recharts-polar-angle-axis-tick_text]:fill-muted-foreground",
  "[&_.recharts-cartesian-grid_line[stroke='#ccc']]:stroke-foreground/10",
  "[&_.recharts-curve.recharts-tooltip-cursor]:stroke-foreground/15",
  "[&_.recharts-polar-grid_[stroke='#ccc']]:stroke-foreground/12",
  "[&_.recharts-radial-bar-background-sector]:fill-foreground/6",
  "[&_.recharts-rectangle.recharts-tooltip-cursor]:fill-foreground/5",
  "[&_.recharts-dot[stroke='#fff']]:stroke-transparent",
  "[&_.recharts-sector[stroke='#fff']]:stroke-transparent",
  "[&_.recharts-layer]:outline-hidden [&_.recharts-sector]:outline-hidden [&_.recharts-surface]:outline-hidden",
].join(" ")

/** Responsive frame + per-chart colour variables. */
export function ChartContainer({
  className,
  children,
  config,
  ...props
}: ComponentProps<"div"> & {
  config: ChartConfig
  children: ComponentProps<typeof ResponsiveContainer>["children"]
}): ReactNode {
  const chartId = `chart-${useId().replaceAll(":", "")}`
  return (
    <ChartContext value={config}>
      <div data-slot="chart" data-chart={chartId} className={cn(THEME, className)} {...props}>
        <ChartStyle id={chartId} config={config} />
        <ResponsiveContainer>{children}</ResponsiveContainer>
      </div>
    </ChartContext>
  )
}

/** `--color-<key>` variables scoped to one chart. */
function ChartStyle({ id, config }: { id: string; config: ChartConfig }): ReactNode {
  const vars = Object.entries(config)
    .flatMap(([key, c]) => (c.color ? [`  --color-${key}: ${c.color};`] : []))
    .join("\n")
  if (!vars) return null
  return <style>{`[data-chart=${id}] {\n${vars}\n}`}</style>
}

export { Tooltip as ChartTooltip, Legend as ChartLegend } from "recharts"

/** One series entry as recharts hands it to tooltip/legend content. */
interface Item {
  name?: string | number | undefined
  value?: unknown
  dataKey?: unknown
  color?: string | undefined
  payload?: Record<string, unknown> | undefined
}

type Indicator = "dot" | "line" | "dashed" | "none"
type Format = (value: number) => ReactNode

/** Config entry for `key`, or for the item's own payload value at `key` (pie slices). */
function configFor(config: ChartConfig, item: Item | undefined, key: string) {
  const inner = item?.payload?.[key]
  const byInner =
    typeof inner === "string" ? Object.entries(config).find(([k]) => k === inner) : undefined
  return byInner?.[1] ?? Object.entries(config).find(([k]) => k === key)?.[1]
}

/** Item key: `nameKey` from its payload, else name / dataKey. */
function itemKey(item: Item, i: number, nameKey: string | undefined): string {
  const raw = nameKey ? item.payload?.[nameKey] : (item.name ?? item.dataKey)
  return String(typeof raw === "string" || typeof raw === "number" ? raw : i)
}

const num = (v: unknown): number => (typeof v === "number" ? v : Number(v))

/** Colour swatch: square dot, vertical bar, or dashed bar. */
function Swatch({ kind, color }: { kind: Indicator; color: string | undefined }): ReactNode {
  if (kind === "none") return null
  return (
    <div
      className={cn(
        "shrink-0 rounded-[2px]",
        kind === "dot" && "size-2.5",
        kind === "line" && "h-3 w-1",
        kind === "dashed" && "h-3 w-0 border-[1.5px] border-dashed bg-transparent",
      )}
      style={{ background: kind === "dashed" ? undefined : color, borderColor: color }}
    />
  )
}

/** Label/value row with the mono tabular value on the right. */
function Row({
  name,
  value,
  strong = false,
}: {
  name: ReactNode
  value: ReactNode
  strong?: boolean
}): ReactNode {
  return (
    <div className="flex flex-1 items-center justify-between gap-4 leading-none">
      <span className={strong ? "font-medium text-foreground" : "text-muted-foreground"}>
        {name}
      </span>
      <span className="font-mono font-medium text-foreground tabular-nums">{value}</span>
    </div>
  )
}

/** Tooltip card: label, one row per series, optional total. */
export function ChartTooltipContent({
  active,
  payload,
  label,
  indicator = "dot",
  hideLabel = false,
  format = (v) => v.toLocaleString(),
  nameKey,
  showTotal = false,
}: {
  active?: boolean | undefined
  payload?: readonly Item[] | undefined
  label?: ReactNode
  indicator?: Indicator | undefined
  hideLabel?: boolean | undefined
  format?: Format | undefined
  nameKey?: string | undefined
  showTotal?: boolean | undefined
}): ReactNode {
  const config = use(ChartContext)
  if (!active || !payload?.length) return null
  const title = hideLabel || label === undefined || label === "" ? null : label
  const total = payload.reduce((s, p) => s + (Number.isFinite(num(p.value)) ? num(p.value) : 0), 0)
  return (
    <div className="grid min-w-32 items-start gap-1.5 rounded-lg bg-popover px-2.5 py-1.5 text-[12px] shadow-xl ring-1 ring-foreground/10">
      {title !== null && <div className="font-medium text-foreground">{title}</div>}
      <div className="grid gap-1.5">
        {payload.map((item, i) => {
          const key = itemKey(item, i, nameKey)
          const fill = item.payload?.["fill"]
          const color = typeof fill === "string" ? fill : item.color
          return (
            <div key={key} className="flex w-full items-center gap-2">
              <Swatch kind={indicator} color={color} />
              <Row
                name={configFor(config, item, key)?.label ?? key}
                value={format(num(item.value))}
              />
            </div>
          )
        })}
        {showTotal && payload.length > 1 && (
          <div className="mt-0.5 flex border-t border-t-foreground/10 pt-1.5">
            <Row name="Total" value={format(total)} strong />
          </div>
        )}
      </div>
    </div>
  )
}

/** Centred row of colour swatches + series labels. */
export function ChartLegendContent({
  payload,
  nameKey,
}: {
  payload?: readonly Item[] | undefined
  nameKey?: string | undefined
}): ReactNode {
  const config = use(ChartContext)
  if (!payload?.length) return null
  return (
    <div className="flex flex-wrap items-center justify-center gap-4 pt-3">
      {payload.map((item, i) => {
        const key = itemKey(item, i, nameKey)
        return (
          <div key={key} className="flex items-center gap-1.5 text-muted-foreground">
            <div
              className="size-2 shrink-0 rounded-[2px]"
              style={{ backgroundColor: item.color }}
            />
            {configFor(config, item, key)?.label ?? key}
          </div>
        )
      })}
    </div>
  )
}
