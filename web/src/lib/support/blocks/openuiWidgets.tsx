import { ChevronDown } from "lucide-react"
import { use, useState, type CSSProperties, type ReactNode } from "react"

import { cn } from "@/lib/utils"

import { Sparkline } from "./charts/openuiCharts"
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
