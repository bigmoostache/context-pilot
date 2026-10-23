import { ChevronDown } from "lucide-react"
import { use, useId, useState, type CSSProperties, type ReactNode } from "react"

import { cn } from "@/lib/utils"

import {
  OPENUI_SEND_EVENT,
  StoreContext,
  firstText,
  interpolate,
  numProp,
  numbersArg,
  str,
  stringLists,
  tint,
  toneColor,
} from "./openuiKit"
import type { OpenUiNode } from "./openuiParse"

// ── OpenUI Lang — leaf widgets (charts, stats, inputs, send buttons) ────────────
//
// Every colour is a design token (never a raw hex), tinted with `color-mix` so a
// tone reads as a soft wash in light AND dark palettes. Inputs write into the
// block-local store ({@link StoreContext}); a `Button` with `send:` interpolates
// `{inputId}` placeholders from that store and posts the result as a real user
// message (via the `openui:send` window event that `useAgentForms` relays to the
// existing send path) — the frontend only renders and forwards, no logic.

/** Points of a polyline through `values`, scaled into a `w`×`h` box. */
function linePoints(values: number[], w: number, h: number): [number, number][] {
  const min = Math.min(...values)
  const span = Math.max(...values) - min || 1
  const step = values.length > 1 ? w / (values.length - 1) : 0
  return values.map((v, i) => [i * step, h - ((v - min) / span) * (h - 4) - 2])
}

/** Smooth area sparkline (gradient fill + stroke + end dot). Pure SVG. */
export function Sparkline({
  values,
  color,
  height = 44,
}: {
  values: number[]
  color: string
  height?: number
}): ReactNode {
  const gradientId = `ou-g${useId().replaceAll(":", "")}`
  const w = 200
  const pts = linePoints(values, w, height)
  const line = pts.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ")
  const last = pts.at(-1) ?? [0, 0]
  return (
    <svg
      viewBox={`0 0 ${String(w)} ${String(height)}`}
      className="w-full overflow-visible"
      style={{ height }}
      preserveAspectRatio="none"
      aria-hidden
    >
      <defs>
        <linearGradient id={gradientId} x1="0" x2="0" y1="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity={0.18} />
          <stop offset="100%" stopColor={color} stopOpacity={0} />
        </linearGradient>
      </defs>
      <polygon
        points={`0,${String(height)} ${line} ${String(w)},${String(height)}`}
        fill={`url(#${gradientId})`}
      />
      <polyline
        points={line}
        fill="none"
        stroke={color}
        strokeWidth={2}
        strokeLinejoin="round"
        strokeLinecap="round"
        vectorEffect="non-scaling-stroke"
      />
      <circle cx={last[0]} cy={last[1]} r={3} fill={color} />
    </svg>
  )
}

/** Big-number stat with optional delta chip and trend sparkline (`Metric(…, [1,2,3])`). */
export function Metric({ node }: { node: OpenUiNode }): ReactNode {
  const delta = numProp(node, "delta")
  const trend = numbersArg(node)
  const up = delta === undefined || delta >= 0
  const color = toneColor(node.props["tone"])
  return (
    <div className="flex min-w-40 flex-1 flex-col gap-1.5 rounded-lg p-4 ring-1 ring-foreground/8">
      <span className="text-[13px] text-muted-foreground">{str(node.props["label"])}</span>
      <div className="flex items-baseline gap-2">
        <span className="text-[26px] leading-none font-semibold tracking-tight text-foreground tabular-nums">
          {str(node.props["value"]) || firstText(node)}
        </span>
        {delta !== undefined && (
          <span
            className="rounded-md px-1.5 py-0.5 text-[12px] font-medium tabular-nums ring-1 ring-inset"
            style={{
              color: up ? "var(--ok)" : "var(--danger)",
              background: tint(up ? "var(--ok)" : "var(--danger)", 10),
              ["--tw-ring-color" as string]: tint(up ? "var(--ok)" : "var(--danger)", 25),
            }}
          >
            {up ? "+" : "−"}
            {Math.abs(delta)}%
          </span>
        )}
      </div>
      {trend.length > 1 && <Sparkline values={trend} color={color} height={36} />}
    </div>
  )
}

/** Radial gauge (`Ring(value: 0..100, label, tone)`). */
export function Ring({ node }: { node: OpenUiNode }): ReactNode {
  const pct = Math.max(0, Math.min(100, numProp(node, "value") ?? 0))
  const color = toneColor(node.props["tone"])
  const r = 34
  const c = 2 * Math.PI * r
  return (
    <div className="flex flex-col items-center gap-1">
      <svg
        viewBox="0 0 84 84"
        className="size-24 -rotate-90"
        aria-label={`${String(pct)}%`}
        role="img"
      >
        <circle
          cx={42}
          cy={42}
          r={r}
          fill="none"
          stroke={tint("var(--foreground)", 8)}
          strokeWidth={6}
        />
        <circle
          cx={42}
          cy={42}
          r={r}
          fill="none"
          stroke={color}
          strokeWidth={6}
          strokeLinecap="round"
          strokeDasharray={`${String((pct / 100) * c)} ${String(c)}`}
          className="transition-[stroke-dasharray] duration-700"
        />
        <text
          x={42}
          y={42}
          dy="0.35em"
          textAnchor="middle"
          className="rotate-90 fill-current text-[17px] font-semibold"
          style={{ transformOrigin: "42px 42px" }}
        >
          {Math.round(pct)}%
        </text>
      </svg>
      <span className="text-[12px] text-muted-foreground">{str(node.props["label"])}</span>
    </div>
  )
}

/** Themed progress bar (`Progress(value: 0..100, label, tone)`). */
export function Progress({ node }: { node: OpenUiNode }): ReactNode {
  const pct = Math.max(0, Math.min(100, numProp(node, "value") ?? 0))
  const color = toneColor(node.props["tone"])
  const label = str(node.props["label"])
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex justify-between text-[13px]">
        <span className="text-muted-foreground">{label}</span>
        <span className="font-medium text-foreground tabular-nums">{Math.round(pct)}%</span>
      </div>
      <div className="h-2 w-full overflow-hidden rounded-full bg-foreground/8">
        <div
          className="h-full rounded-full transition-[width] duration-700"
          style={{ width: `${String(pct)}%`, background: color }}
        />
      </div>
    </div>
  )
}

/** Bar chart with per-bar hover readout (`BarChart([…], labels…)`). */
export function BarChart({ node }: { node: OpenUiNode }): ReactNode {
  const [hover, setHover] = useState<number | null>(null)
  const values = numbersArg(node)
  if (values.length === 0) return null
  const labels = stringLists(node)[0] ?? []
  const max = Math.max(...values, 1)
  const color = toneColor(node.props["tone"])
  return (
    <div className="relative flex h-44 items-end gap-2 pt-6">
      {[0, 1, 2, 3].map((g) => (
        <div
          key={g}
          aria-hidden
          className="pointer-events-none absolute inset-x-0 h-px bg-foreground/6"
          style={{ bottom: `calc(${String(g * 33.3)}% * 0.82 + 18px)` }}
        />
      ))}
      {values.map((v, i) => (
        <div
          key={i}
          className="relative flex h-full flex-1 flex-col items-center justify-end gap-1.5"
          onPointerEnter={() => setHover(i)}
          onPointerLeave={() => setHover(null)}
        >
          {hover === i && (
            <span className="absolute -top-6 rounded-md bg-popover px-1.5 py-0.5 text-[11px] font-medium tabular-nums shadow-sm ring-1 ring-foreground/10">
              {v}
            </span>
          )}
          <div
            className="w-full max-w-10 rounded-t-sm transition-opacity duration-150"
            style={{
              height: `${String(Math.max(2, (v / max) * 82))}%`,
              background: color,
              opacity: hover === null || hover === i ? 1 : 0.4,
            }}
          />
          <span className="text-[11px] text-muted-foreground">{labels[i] ?? ""}</span>
        </div>
      ))}
    </div>
  )
}

/** Header row + body rows (`Table(["h1","h2"], ["a","b"], …)`). */
export function Table({ node }: { node: OpenUiNode }): ReactNode {
  const [head = [], ...rows] = stringLists(node)
  return (
    <div className="overflow-x-auto">
      <table className="w-full border-separate border-spacing-0 text-[13px]">
        <thead>
          <tr>
            {head.map((h) => (
              <th
                key={h}
                className="border-b border-b-foreground/10 px-3 py-2 text-left text-[12px] font-medium text-muted-foreground"
              >
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} className="transition-colors hover:bg-foreground/4">
              {r.map((c, j) => (
                <td key={j} className="border-b border-b-foreground/6 px-3 py-2 tabular-nums">
                  {c}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

/** Soft tinted pill (`Badge("text", tone)`). */
export function Badge({ node }: { node: OpenUiNode }): ReactNode {
  const color = toneColor(node.props["tone"])
  return (
    <span
      className="inline-flex items-center rounded-md px-2 py-0.5 text-[12px] font-medium ring-1 ring-inset"
      style={{
        color,
        background: tint(color, 10),
        ["--tw-ring-color" as string]: tint(color, 25),
      }}
    >
      {firstText(node)}
    </span>
  )
}

const FIELD =
  "h-9 w-full rounded-md bg-transparent px-3 text-[13px] ring-1 ring-foreground/12 outline-none transition-shadow focus:ring-2 focus:ring-(--signal)/50"

/** Labelled wrapper shared by every input. */
function Field({ label, children }: { label: string; children: ReactNode }): ReactNode {
  return (
    <label className="flex min-w-40 flex-1 flex-col gap-1.5">
      {label && <span className="text-[13px] font-medium text-foreground">{label}</span>}
      {children}
    </label>
  )
}

/** Free-text input bound to its id (`name = Input("placeholder", label, value)`). */
export function Input({ id, node }: { id: string; node: OpenUiNode }): ReactNode {
  const s = use(StoreContext)
  return (
    <Field label={str(node.props["label"])}>
      <input
        className={FIELD}
        placeholder={firstText(node)}
        value={s.vals[id] ?? ""}
        onChange={(e) => s.set(id, e.target.value)}
      />
    </Field>
  )
}

/** Segmented control only fits a few short options; anything longer is a dropdown. */
const fitsSegmented = (options: string[]): boolean =>
  options.length <= 4 && options.join("").length <= 28

/** Choice bound to its id (`env = Select(["prod","staging"], label)`). */
export function Select({ id, node }: { id: string; node: OpenUiNode }): ReactNode {
  const s = use(StoreContext)
  const options = stringLists(node)[0] ?? []
  if (!fitsSegmented(options)) {
    return (
      <Field label={str(node.props["label"])}>
        <span className="relative">
          <select
            className={cn(FIELD, "cursor-pointer appearance-none pr-8")}
            value={s.vals[id] ?? ""}
            onChange={(e) => s.set(id, e.target.value)}
          >
            {options.map((o) => (
              <option key={o} value={o}>
                {o}
              </option>
            ))}
          </select>
          <ChevronDown className="pointer-events-none absolute top-1/2 right-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
        </span>
      </Field>
    )
  }
  return (
    <Field label={str(node.props["label"])}>
      <div className="flex gap-1 rounded-md bg-foreground/6 p-1">
        {options.map((o) => {
          const on = s.vals[id] === o
          return (
            <button
              key={o}
              type="button"
              onClick={() => s.set(id, o)}
              className={cn(
                "flex-1 rounded-sm px-2.5 py-1 text-[13px] font-medium transition-colors",
                on
                  ? "bg-card text-foreground shadow-sm ring-1 ring-foreground/8"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {o}
            </button>
          )
        })}
      </div>
    </Field>
  )
}

/** Thumb styling for both engines: a card-coloured knob ringed in the tone. */
const THUMB =
  "[&::-webkit-slider-thumb]:size-4 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-card [&::-webkit-slider-thumb]:shadow-sm [&::-webkit-slider-thumb]:ring-2 [&::-webkit-slider-thumb]:ring-(--ou-tone) [&::-moz-range-thumb]:size-4 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:border-0 [&::-moz-range-thumb]:bg-card [&::-moz-range-thumb]:shadow-sm [&::-moz-range-thumb]:ring-2 [&::-moz-range-thumb]:ring-(--ou-tone)"

/** Range slider bound to its id (`pct = Slider(min: 0, max: 100, value: 25, label)`). */
export function Slider({ id, node }: { id: string; node: OpenUiNode }): ReactNode {
  const s = use(StoreContext)
  const value = s.vals[id] ?? "0"
  const min = numProp(node, "min") ?? 0
  const max = numProp(node, "max") ?? 100
  const pct = max > min ? ((Number(value) - min) / (max - min)) * 100 : 0
  const tone = toneColor(node.props["tone"])
  const track = tint("var(--foreground)", 10)
  const style = {
    "--ou-tone": tone,
    background: `linear-gradient(to right, ${tone} ${String(pct)}%, ${track} ${String(pct)}%)`,
  } as CSSProperties
  return (
    <div className="flex min-w-40 flex-1 flex-col gap-2">
      <div className="flex items-baseline justify-between text-[13px]">
        <span className="font-medium text-foreground">{str(node.props["label"])}</span>
        <span className="text-muted-foreground tabular-nums">
          {value}
          {str(node.props["unit"])}
        </span>
      </div>
      <input
        type="range"
        aria-label={str(node.props["label"])}
        className={cn("mt-1.5 h-2 w-full cursor-pointer appearance-none rounded-full", THUMB)}
        style={style}
        min={min}
        max={max}
        step={numProp(node, "step") ?? 1}
        value={value}
        onChange={(e) => s.set(id, e.target.value)}
      />
    </div>
  )
}

/**
 * A button. With `send:` it posts the interpolated text back to the agent as a
 * real user message (the agent answers, typically with a fresh UI); otherwise
 * it pops a toast (`action:`).
 */
export function ActionButton({ node }: { node: OpenUiNode }): ReactNode {
  const s = use(StoreContext)
  const [flash, setFlash] = useState<string | null>(null)
  const send = node.props["send"]
  const color = toneColor(node.props["tone"])
  const ghost = node.props["variant"] === "ghost"
  const click = (): void => {
    if (typeof send === "string") {
      window.dispatchEvent(
        new CustomEvent(OPENUI_SEND_EVENT, { detail: interpolate(send, s.vals) }),
      )
      setFlash("Sent ✓")
    } else {
      setFlash(str(node.props["action"], "Done"))
    }
    window.setTimeout(() => setFlash(null), 1600)
  }
  return (
    <button
      type="button"
      onClick={click}
      className={cn(
        "h-9 rounded-md px-4 text-[13px] font-medium transition-[filter,background-color] active:brightness-95",
        ghost
          ? "bg-card text-foreground shadow-sm ring-1 ring-foreground/12 hover:bg-foreground/4"
          : "text-white shadow-sm hover:brightness-110",
      )}
      style={ghost ? undefined : { background: color }}
    >
      {flash ?? (firstText(node) || "Go")}
    </button>
  )
}
