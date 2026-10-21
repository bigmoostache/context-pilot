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
          <stop offset="0%" stopColor={color} stopOpacity={0.35} />
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
    <div
      className="flex min-w-36 flex-1 flex-col gap-1 rounded-xl p-3"
      style={{ background: tint(color, 7) }}
    >
      <span className="text-[11px] font-medium tracking-wider uppercase opacity-55">
        {str(node.props["label"])}
      </span>
      <div className="flex items-baseline gap-2">
        <span className="text-[26px] leading-none font-semibold tracking-tight tabular-nums">
          {str(node.props["value"]) || firstText(node)}
        </span>
        {delta !== undefined && (
          <span
            className="rounded-full px-1.5 py-0.5 text-[11px] font-semibold tabular-nums"
            style={{
              color: up ? "var(--ok)" : "var(--danger)",
              background: tint(up ? "var(--ok)" : "var(--danger)", 14),
            }}
          >
            {up ? "↑" : "↓"} {Math.abs(delta)}%
          </span>
        )}
      </div>
      {trend.length > 1 && <Sparkline values={trend} color={color} height={32} />}
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
        <circle cx={42} cy={42} r={r} fill="none" stroke={tint(color, 16)} strokeWidth={8} />
        <circle
          cx={42}
          cy={42}
          r={r}
          fill="none"
          stroke={color}
          strokeWidth={8}
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
      <span className="text-[11px] opacity-60">{str(node.props["label"])}</span>
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
      <div className="flex justify-between text-[12px]">
        <span className="opacity-70">{label}</span>
        <span className="font-medium tabular-nums">{Math.round(pct)}%</span>
      </div>
      <div
        className="h-1.5 w-full overflow-hidden rounded-full"
        style={{ background: tint(color, 15) }}
      >
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
    <div className="flex h-36 items-end gap-1.5 pt-5">
      {values.map((v, i) => (
        <div
          key={i}
          className="relative flex h-full flex-1 flex-col items-center justify-end gap-1"
          onPointerEnter={() => setHover(i)}
          onPointerLeave={() => setHover(null)}
        >
          {hover === i && (
            <span className="absolute -top-5 text-[11px] font-semibold tabular-nums">{v}</span>
          )}
          <div
            className="w-full rounded-md transition-opacity duration-150"
            style={{
              height: `${String(Math.max(3, (v / max) * 100))}%`,
              background: color,
              opacity: hover === null || hover === i ? 0.9 : 0.35,
            }}
          />
          <span className="text-[10px] opacity-50">{labels[i] ?? ""}</span>
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
      <table className="w-full text-[12.5px]">
        <thead>
          <tr>
            {head.map((h) => (
              <th
                key={h}
                className="px-2 pb-2 text-left text-[11px] font-medium tracking-wider uppercase opacity-50"
              >
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} className="transition-colors odd:bg-foreground/3 hover:bg-foreground/6">
              {r.map((c, j) => (
                <td
                  key={j}
                  className="px-2 py-1.5 tabular-nums first:rounded-l-md last:rounded-r-md"
                >
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
      className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[11.5px] font-medium"
      style={{ color, background: tint(color, 14) }}
    >
      <span className="size-1.5 rounded-full" style={{ background: color }} />
      {firstText(node)}
    </span>
  )
}

const FIELD =
  "w-full rounded-lg bg-foreground/5 px-3 py-1.5 text-[13px] outline-none transition-shadow focus:ring-2 focus:ring-(--signal)/40"

/** Labelled wrapper shared by every input. */
function Field({ label, children }: { label: string; children: ReactNode }): ReactNode {
  return (
    <label className="flex min-w-40 flex-1 flex-col gap-1">
      {label && <span className="text-[11px] font-medium opacity-60">{label}</span>}
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

/** Segmented choice bound to its id (`env = Select(["prod","staging"], label)`). */
export function Select({ id, node }: { id: string; node: OpenUiNode }): ReactNode {
  const s = use(StoreContext)
  const options = stringLists(node)[0] ?? []
  const color = toneColor(node.props["tone"])
  return (
    <Field label={str(node.props["label"])}>
      <div className="flex gap-1 rounded-lg bg-foreground/5 p-1">
        {options.map((o) => {
          const on = s.vals[id] === o
          return (
            <button
              key={o}
              type="button"
              onClick={() => s.set(id, o)}
              className={cn(
                "flex-1 rounded-md px-2.5 py-1 text-[12.5px] font-medium transition-colors",
                !on && "opacity-60 hover:opacity-100",
              )}
              style={on ? { background: color, color: "white" } : undefined}
            >
              {o}
            </button>
          )
        })}
      </div>
    </Field>
  )
}

/** Range slider bound to its id (`pct = Slider(min: 0, max: 100, value: 25, label)`). */
export function Slider({ id, node }: { id: string; node: OpenUiNode }): ReactNode {
  const s = use(StoreContext)
  const value = s.vals[id] ?? "0"
  const style = { accentColor: toneColor(node.props["tone"]) } satisfies CSSProperties
  return (
    <Field label={`${str(node.props["label"])} · ${value}${str(node.props["unit"])}`}>
      <input
        type="range"
        className="w-full"
        style={style}
        min={numProp(node, "min") ?? 0}
        max={numProp(node, "max") ?? 100}
        step={numProp(node, "step") ?? 1}
        value={value}
        onChange={(e) => s.set(id, e.target.value)}
      />
    </Field>
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
      className="rounded-lg px-3.5 py-1.5 text-[13px] font-semibold transition-[transform,filter] hover:brightness-110 active:scale-95"
      style={
        ghost
          ? { color, background: tint(color, 12) }
          : { color: "white", background: color, boxShadow: `0 4px 14px ${tint(color, 35)}` }
      }
    >
      {flash ?? (firstText(node) || "Go")}
    </button>
  )
}
