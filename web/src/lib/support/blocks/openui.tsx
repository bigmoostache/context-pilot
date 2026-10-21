import { useState, type ReactNode } from "react"

import { cn } from "@/lib/utils"

import { parseOpenUi, type OpenUiArg, type OpenUiNode, type OpenUiValue } from "./openuiParse"

// ── OpenUI Lang — renderer (see the ```openui``` block in the Send tool) ──────
//
// Turns the node table from `parseOpenUi` into a themed, interactive React
// tree. Every colour is an app design token (never a raw hex) so the UI the
// model describes inherits the current palette, light or dark. `onAccent` is
// true inside the coloured user bubble — chrome then switches to translucent
// white, mirroring markdown.tsx and mermaid.tsx.
//
// Interactivity is intentionally client-only for this POC: a `Button` fires an
// ephemeral toast. No backend round-trip, no business logic in the frontend —
// it is a live render of a declarative description, nothing more.

/** Tone → CSS token, for Badge / Button / Metric accents. */
const TONE_VAR: Record<string, string> = {
  signal: "var(--signal)",
  ok: "var(--ok)",
  warn: "var(--warn)",
  danger: "var(--danger)",
  interactive: "var(--interactive)",
}

const toneColor = (tone: OpenUiValue | undefined): string =>
  (typeof tone === "string" && TONE_VAR[tone]) || "var(--signal)"

const str = (v: OpenUiValue | undefined, fallback = ""): string =>
  v === undefined ? fallback : String(v)

/** First positional arg as a plain string literal (text components). */
function firstText(node: OpenUiNode): string {
  const a = node.args[0]
  return a?.kind === "value" ? String(a.value) : ""
}

/** Positional child-id list (container components): a `refs` arg or a lone `ref`. */
function childIds(node: OpenUiNode): string[] {
  for (const a of node.args) {
    if (a.kind === "refs") return a.ids
    if (a.kind === "ref") return [a.id]
  }
  return []
}

/** The `numbers`/`strings` list arg, if present (BarChart). */
function listArg(
  node: OpenUiNode,
): Extract<OpenUiArg, { kind: "numbers" | "strings" }> | undefined {
  for (const a of node.args) {
    if (a.kind === "numbers" || a.kind === "strings") return a
  }
  return undefined
}

/** The numeric prop `key`, or `undefined` when absent / not a number. */
const numProp = (node: OpenUiNode, key: string): number | undefined => {
  const v = node.props[key]
  return typeof v === "number" ? v : undefined
}

/** Render context threaded through the tree walk. */
interface Ctx {
  nodes: Map<string, OpenUiNode>
  onAccent: boolean
  /** Ids on the current path — a cycle short-circuits instead of recursing. */
  seen: Set<string>
}

/** A component renderer: node + context + a lazy child-list thunk. */
type Renderer = (node: OpenUiNode, ctx: Ctx, kids: () => ReactNode[]) => ReactNode

/** A tiny inline bar chart in pure SVG — no charting dependency. */
function BarChart({ node }: { node: OpenUiNode }): ReactNode {
  const data = listArg(node)
  const values = data?.kind === "numbers" ? data.values : []
  if (values.length === 0) return null
  const max = Math.max(...values, 1)
  const gap = 6
  const bw = 26
  const h = 96
  const w = values.length * (bw + gap) - gap
  const color = toneColor(node.props["tone"])
  return (
    <svg
      viewBox={`0 0 ${w} ${h + 18}`}
      className="my-1 h-28 w-full"
      preserveAspectRatio="xMidYMax meet"
      role="img"
      aria-label="bar chart"
    >
      {values.map((v, i) => {
        const bh = Math.max(2, (v / max) * h)
        const x = i * (bw + gap)
        return (
          <g key={i}>
            <rect x={x} y={h - bh} width={bw} height={bh} rx={3} fill={color} opacity={0.85} />
            <text
              x={x + bw / 2}
              y={h + 13}
              textAnchor="middle"
              className="fill-current text-[9px] opacity-60"
            >
              {v}
            </text>
          </g>
        )
      })}
    </svg>
  )
}

/** Big-number stat: label on top, value large, optional delta chip. */
function Metric({ node, onAccent }: { node: OpenUiNode; onAccent: boolean }): ReactNode {
  const deltaNum = numProp(node, "delta")
  return (
    <div
      className={cn(
        "flex min-w-28 flex-col gap-0.5 rounded-lg px-3 py-2",
        onAccent ? "bg-white/10" : "bg-muted/50",
      )}
    >
      <span className="text-[11px] tracking-wide uppercase opacity-60">
        {str(node.props["label"])}
      </span>
      <span className="text-[22px] leading-none font-semibold tabular-nums">
        {str(node.props["value"]) || firstText(node)}
      </span>
      {deltaNum !== undefined && (
        <span
          className="text-[11px] font-medium tabular-nums"
          style={{ color: deltaNum >= 0 ? "var(--ok)" : "var(--danger)" }}
        >
          {deltaNum >= 0 ? "▲" : "▼"} {Math.abs(deltaNum)}%
        </span>
      )}
    </div>
  )
}

/** A themed progress bar (`value` 0–100). */
function Progress({ node, onAccent }: { node: OpenUiNode; onAccent: boolean }): ReactNode {
  const pct = Math.max(0, Math.min(100, numProp(node, "value") ?? 0))
  const label = str(node.props["label"])
  return (
    <div className="my-1 flex flex-col gap-1">
      {label && <span className="text-[11px] opacity-70">{label}</span>}
      <div
        className={cn(
          "h-2 w-full overflow-hidden rounded-full",
          onAccent ? "bg-white/15" : "bg-muted",
        )}
      >
        <div
          className="h-full rounded-full transition-[width] duration-500"
          style={{ width: `${pct}%`, background: toneColor(node.props["tone"]) }}
        />
      </div>
    </div>
  )
}

/** A clickable button that pops an ephemeral toast (POC interactivity). */
function ActionButton({ node, onAccent }: { node: OpenUiNode; onAccent: boolean }): ReactNode {
  const [toast, setToast] = useState<string | null>(null)
  const action = str(node.props["action"], "Clicked")
  return (
    <span className="relative inline-flex">
      <button
        type="button"
        onClick={() => {
          setToast(action)
          window.setTimeout(() => setToast(null), 1800)
        }}
        className={cn(
          "rounded-md px-3 py-1.5 text-[13px] font-medium transition-transform active:scale-95",
          onAccent ? "bg-white/20 hover:bg-white/30" : "text-white hover:opacity-90",
        )}
        style={onAccent ? undefined : { background: toneColor(node.props["tone"]) }}
      >
        {firstText(node) || "Button"}
      </button>
      {toast !== null && (
        <span className="absolute -top-8 left-1/2 -translate-x-1/2 rounded-md bg-popover px-2 py-1 text-[11px] whitespace-nowrap text-popover-foreground shadow-(--shadow-pop)">
          {toast}
        </span>
      )}
    </span>
  )
}

/** Container/leaf renderers, keyed by component name. */
const RENDERERS: Record<string, Renderer> = {
  Stack: (node, _ctx, kids) => (
    <div className="flex flex-col gap-2" style={{ gap: numProp(node, "gap") }}>
      {kids()}
    </div>
  ),
  Row: (node, _ctx, kids) => (
    <div className="flex flex-wrap items-center gap-2" style={{ gap: numProp(node, "gap") }}>
      {kids()}
    </div>
  ),
  Card: (node, ctx, kids) => (
    <div
      className={cn(
        "flex flex-col gap-2 rounded-xl p-3",
        ctx.onAccent ? "bg-white/10" : "border border-(--border-strong) bg-surface/60",
      )}
    >
      {node.props["title"] !== undefined && (
        <div className="text-[13px] font-semibold opacity-90">{str(node.props["title"])}</div>
      )}
      {kids()}
    </div>
  ),
  Heading: (node) => {
    const level = numProp(node, "level") ?? 2
    return (
      <div className={cn("leading-snug font-semibold", level <= 2 ? "text-[16px]" : "text-[14px]")}>
        {firstText(node)}
      </div>
    )
  },
  Text: (node) => (
    <p className={cn("text-[13px] leading-relaxed", node.props["muted"] === true && "opacity-60")}>
      {firstText(node)}
    </p>
  ),
  Badge: (node) => (
    <span
      className="inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-medium text-white"
      style={{ background: toneColor(node.props["tone"]) }}
    >
      {firstText(node)}
    </span>
  ),
  Divider: (_node, ctx) => (
    <hr className={cn("my-1 border-t", ctx.onAccent ? "border-white/20" : "border-border")} />
  ),
  Metric: (node, ctx) => <Metric node={node} onAccent={ctx.onAccent} />,
  Progress: (node, ctx) => <Progress node={node} onAccent={ctx.onAccent} />,
  Button: (node, ctx) => <ActionButton node={node} onAccent={ctx.onAccent} />,
  BarChart: (node) => <BarChart node={node} />,
}

/** Stable keyed wrapper so a child list doesn't warn on reorder. */
function Fragment({ children }: { children: ReactNode }): ReactNode {
  return <>{children}</>
}

/** Render one node by component name, recursing into children via `kids`. */
function renderNode(id: string, ctx: Ctx): ReactNode {
  if (ctx.seen.has(id) || !ctx.nodes.has(id)) return null
  const node = ctx.nodes.get(id)
  if (!node) return null
  const kids = (): ReactNode[] => {
    const next: Ctx = { ...ctx, seen: new Set(ctx.seen).add(id) }
    return childIds(node).map((c) => <Fragment key={c}>{renderNode(c, next)}</Fragment>)
  }
  const render = RENDERERS[node.comp]
  if (render) return render(node, ctx, kids)
  return <span className="text-[12px] text-(--danger)">Unknown component “{node.comp}”</span>
}

/**
 * Render an ```openui``` fenced block as live interactive UI.
 *
 * Parses the OpenUI Lang source; on any parse error it degrades to the raw
 * source in a muted code box (same contract as a failed mermaid diagram) so a
 * half-streamed or malformed block never throws.
 */
export function OpenUi({ source, onAccent }: { source: string; onAccent: boolean }): ReactNode {
  const program = parseOpenUi(source)
  if (!program.ok) {
    return (
      <pre
        className={cn(
          "my-2 overflow-x-auto rounded-lg p-3 font-mono text-[12px] leading-relaxed",
          onAccent ? "bg-black/20" : "border border-border bg-muted/60 text-foreground/90",
        )}
        title={program.error}
      >
        {source}
      </pre>
    )
  }
  return (
    <div
      className={cn(
        "my-2 rounded-xl p-3 text-[13px]",
        onAccent ? "bg-black/10" : "border border-border bg-surface/40",
      )}
    >
      {renderNode(program.root, { nodes: program.nodes, onAccent, seen: new Set() })}
    </div>
  )
}
