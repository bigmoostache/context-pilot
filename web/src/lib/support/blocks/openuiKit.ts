import { createContext } from "react"

import type { OpenUiArg, OpenUiNode, OpenUiValue } from "./openuiParse"

// ── OpenUI Lang — shared kit (non-component exports) ───────────────────────────
//
// The block-local input store, the send event name, and the small node/tone
// helpers used by both the layout renderer (openui.tsx) and the leaf widgets
// (openuiWidgets.tsx). Kept apart from the components so react-refresh can
// hot-swap those files (it only handles component-only modules).

/** Block-local input values keyed by the input's binding id. */
export interface Store {
  vals: Record<string, string>
  set: (id: string, value: string) => void
  onAccent: boolean
}

/** Placeholder setter for a widget rendered outside an `OpenUi` block. */
const noopSet = (): undefined => undefined

export const StoreContext = createContext<Store>({ vals: {}, set: noopSet, onAccent: false })

/** Window event a send-button fires; `useAgentForms` relays it to `send_message`. */
export const OPENUI_SEND_EVENT = "openui:send"

const TONE_VAR: Record<string, string> = {
  signal: "var(--signal)",
  ok: "var(--ok)",
  warn: "var(--warn)",
  danger: "var(--danger)",
  interactive: "var(--interactive)",
}

export const toneColor = (tone: OpenUiValue | undefined): string =>
  (typeof tone === "string" && TONE_VAR[tone]) || "var(--signal)"

/** A tone washed to `pct`% over transparent — soft fills that work on both palettes. */
export const tint = (color: string, pct: number): string =>
  `color-mix(in oklab, ${color} ${String(pct)}%, transparent)`

export const str = (v: OpenUiValue | undefined, fallback = ""): string =>
  v === undefined ? fallback : String(v)

export function numProp(node: OpenUiNode, key: string): number | undefined {
  const v = node.props[key]
  return typeof v === "number" ? v : undefined
}

/** First positional arg as a plain string literal (text components). */
export function firstText(node: OpenUiNode): string {
  const a = node.args[0]
  return a?.kind === "value" ? String(a.value) : ""
}

/** The first `numbers` list arg (charts, trends). */
export function numbersArg(node: OpenUiNode): number[] {
  const a = node.args.find(
    (x): x is Extract<OpenUiArg, { kind: "numbers" }> => x.kind === "numbers",
  )
  return a?.values ?? []
}

/** Every `strings` list arg, in order (table rows, tab labels, select options). */
export function stringLists(node: OpenUiNode): string[][] {
  return node.args
    .filter((a): a is Extract<OpenUiArg, { kind: "strings" }> => a.kind === "strings")
    .map((a) => a.values)
}

/** Fill `{inputId}` placeholders from the block's input values. */
export const interpolate = (text: string, vals: Record<string, string>): string =>
  text.replaceAll(/\{([\w-]+)\}/g, (_m, k: string) => vals[k] ?? `{${k}}`)
