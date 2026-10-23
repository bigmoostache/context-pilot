import {
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react"

import { cn } from "@/lib/utils"

import { parseOpenUi, type OpenUiNode } from "./openuiParse"
import {
  AreaChart,
  BarChart,
  LineChart,
  PieChart,
  RadarChart,
  RadialChart,
  Ring,
  SparklineNode,
} from "./charts/openuiCharts"
import {
  StoreContext,
  firstText,
  numProp,
  str,
  stringLists,
  toneColor,
  type Store,
} from "./openuiKit"
import {
  ActionButton,
  Badge,
  Input,
  Metric,
  Progress,
  Select,
  Slider,
  Table,
} from "./openuiWidgets"
import { SqlTable } from "./sqlTable/SqlTable"

// ── OpenUI Lang — layout + entry (see the ```openui``` block in the Send tool) ──
//
// Turns the node table from `parseOpenUi` into a themed React tree. The root
// renders BARE (no outer frame): only `Card`s are surfaces, and they use a soft
// shadow instead of a border (the app's "shadows, not borders" rule). Inputs
// keep their values in a block-local store so a send-button can interpolate
// them into the message it posts back to the agent (see openuiWidgets.tsx).

/** Render context threaded through the tree walk. */
interface Ctx {
  nodes: Map<string, OpenUiNode>
  /** Ids on the current path — a cycle short-circuits instead of recursing. */
  seen: Set<string>
}

/** A component renderer: binding id + node + context + lazy children. */
type Renderer = (id: string, node: OpenUiNode, kids: () => ReactNode[], ctx: Ctx) => ReactNode

/** Child-id list (containers): a `refs` arg or a lone `ref`. */
function childIds(node: OpenUiNode): string[] {
  for (const a of node.args) {
    if (a.kind === "refs") return a.ids
    if (a.kind === "ref") return [a.id]
  }
  return []
}

/** Tab strip over child panes (`Tabs(["A","B"], [paneA, paneB])`). */
function Tabs({ labels, panes }: { labels: string[]; panes: ReactNode[] }): ReactNode {
  const [active, setActive] = useState(0)
  return (
    <div className="flex flex-col gap-4">
      <div className="flex gap-5 text-[13px] shadow-[inset_0_-1px_0_color-mix(in_oklab,var(--foreground)_10%,transparent)]">
        {labels.map((l, i) => (
          <button
            key={l}
            type="button"
            onClick={() => setActive(i)}
            className={cn(
              "pb-2 font-medium transition-[color,box-shadow]",
              i === active
                ? "text-foreground shadow-[inset_0_-2px_0_var(--signal)]"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {l}
          </button>
        ))}
      </div>
      {panes[active]}
    </div>
  )
}

const RENDERERS: Record<string, Renderer> = {
  Stack: (_id, node, kids) => (
    <div className="flex flex-col" style={{ gap: numProp(node, "gap") ?? 12 }}>
      {kids()}
    </div>
  ),
  Row: (_id, node, kids) => (
    <div className="flex flex-wrap items-center" style={{ gap: numProp(node, "gap") ?? 8 }}>
      {kids()}
    </div>
  ),
  Grid: (_id, node, kids) => (
    <div
      className="grid gap-4"
      style={{
        gridTemplateColumns: `repeat(auto-fit, minmax(${String(numProp(node, "min") ?? 180)}px, 1fr))`,
      }}
    >
      {kids()}
    </div>
  ),
  Card: (_id, node, kids) => {
    const tone = node.props["tone"]
    return (
      <section className="flex min-w-0 flex-col gap-4 rounded-lg bg-card p-5 shadow-(--shadow-card) ring-1 ring-foreground/6">
        {node.props["title"] !== undefined && (
          <header className="flex items-center gap-2 text-[13px] font-medium text-muted-foreground">
            {tone !== undefined && (
              <span className="size-2 rounded-xs" style={{ background: toneColor(tone) }} />
            )}
            {str(node.props["title"])}
          </header>
        )}
        {kids()}
      </section>
    )
  },
  Tabs: (_id, node, kids) => <Tabs labels={stringLists(node)[0] ?? []} panes={kids()} />,
  Heading: (_id, node) => (
    <div
      className={cn(
        "leading-tight font-semibold tracking-tight text-foreground",
        (numProp(node, "level") ?? 2) <= 2 ? "text-[18px]" : "text-[15px]",
      )}
    >
      {firstText(node)}
    </div>
  ),
  Text: (_id, node) => (
    <p
      className={cn(
        "text-[13px] leading-relaxed",
        node.props["muted"] === true && "text-muted-foreground",
      )}
    >
      {firstText(node)}
    </p>
  ),
  Divider: () => <div className="h-px w-full bg-foreground/10" />,
  Badge: (_id, node) => <Badge node={node} />,
  Metric: (_id, node) => <Metric node={node} />,
  Progress: (_id, node) => <Progress node={node} />,
  Ring: (_id, node) => <Ring node={node} />,
  Sparkline: (_id, node) => <SparklineNode node={node} />,
  BarChart: (_id, node) => <BarChart node={node} />,
  AreaChart: (_id, node) => <AreaChart node={node} />,
  LineChart: (_id, node) => <LineChart node={node} />,
  RadarChart: (_id, node) => <RadarChart node={node} />,
  RadialChart: (_id, node) => <RadialChart node={node} />,
  PieChart: (_id, node) => <PieChart node={node} />,
  Table: (_id, node) => <Table node={node} />,
  SqlTable: (_id, node) => <SqlTable node={node} />,
  Button: (_id, node) => <ActionButton node={node} />,
  Input: (id, node) => <Input id={id} node={node} />,
  Select: (id, node) => <Select id={id} node={node} />,
  Slider: (id, node) => <Slider id={id} node={node} />,
}

/** Keyed wrapper so a child list doesn't warn on reorder. */
function Keyed({ children }: { children: ReactNode }): ReactNode {
  return <>{children}</>
}

/** Render one node by component name, recursing into children via `kids`. */
function renderNode(id: string, ctx: Ctx): ReactNode {
  const node = ctx.nodes.get(id)
  if (ctx.seen.has(id) || !node) return null
  const kids = (): ReactNode[] => {
    const next: Ctx = { ...ctx, seen: new Set(ctx.seen).add(id) }
    return childIds(node).map((c) => <Keyed key={c}>{renderNode(c, next)}</Keyed>)
  }
  const render = RENDERERS[node.comp]
  if (render) return render(id, node, kids, ctx)
  return <span className="text-[12px] text-(--danger)">Unknown component “{node.comp}”</span>
}

/** Widest a dashboard grows, and the margin it keeps from the pane edges (px). */
const BREAKOUT_MAX = 1100
const BREAKOUT_GAP = 24

/**
 * Only the dashboard breaks out of the 720px reading column (prose keeps it).
 * Its message isn't necessarily centred in the conversation pane, so CSS
 * centring can push it under the left sidebar: measure instead, widen up to
 * {@link BREAKOUT_MAX}, centre on the message, then clamp inside the pane
 * (`[data-ou-pane]`, the conversation column).
 */
function useBreakout(active: boolean): [RefObject<HTMLDivElement | null>, CSSProperties] {
  const ref = useRef<HTMLDivElement>(null)
  const [style, setStyle] = useState<CSSProperties>({})
  useLayoutEffect(() => {
    const el = ref.current
    const host = el?.parentElement
    const pane = el?.closest("[data-ou-pane]")
    if (!active || !host || !pane) return
    const fit = (): void => {
      const p = pane.getBoundingClientRect()
      const h = host.getBoundingClientRect()
      const room = p.width - 2 * BREAKOUT_GAP
      const w = Math.max(h.width, Math.min(BREAKOUT_MAX, room))
      const centred = h.left + (h.width - w) / 2
      const left = Math.max(p.left + BREAKOUT_GAP, Math.min(centred, p.right - BREAKOUT_GAP - w))
      setStyle({ width: w, marginLeft: Math.round(left - h.left) })
    }
    const ro = new ResizeObserver(fit)
    ro.observe(pane)
    ro.observe(host)
    return () => ro.disconnect()
  }, [active])
  return [ref, style]
}

/** Initial input values: each input's `value:` prop, else its first option / min. */
function initialValues(nodes: Map<string, OpenUiNode>): Record<string, string> {
  const vals: Record<string, string> = {}
  for (const [id, n] of nodes) {
    const v = n.props["value"]
    if (n.comp === "Input" || n.comp === "Slider")
      vals[id] = str(v, n.comp === "Slider" ? str(n.props["min"], "0") : "")
    else if (n.comp === "Select") vals[id] = str(v, stringLists(n)[0]?.[0] ?? "")
  }
  return vals
}

/**
 * Render an ```openui``` fenced block as live interactive UI.
 *
 * On any parse error it degrades to the raw source in a muted code box (same
 * contract as a failed mermaid diagram) so a half-streamed block never throws.
 */
export function OpenUi({ source, onAccent }: { source: string; onAccent: boolean }): ReactNode {
  const program = useMemo(() => parseOpenUi(source), [source])
  const [vals, setVals] = useState<Record<string, string>>(() =>
    program.ok ? initialValues(program.nodes) : {},
  )
  const [ref, breakout] = useBreakout(program.ok)
  if (!program.ok) {
    return (
      <pre
        className={cn(
          "my-2 overflow-x-auto rounded-lg p-3 font-mono text-[12px] leading-relaxed",
          onAccent ? "bg-black/20" : "bg-muted/60 text-foreground/90",
        )}
        title={program.error}
      >
        {source}
      </pre>
    )
  }
  const store: Store = {
    vals,
    set: (id, value) => setVals((v) => ({ ...v, [id]: value })),
    onAccent,
  }
  return (
    <StoreContext value={store}>
      <div ref={ref} className="my-3 text-[13px]" style={breakout}>
        {renderNode(program.root, { nodes: program.nodes, seen: new Set() })}
      </div>
    </StoreContext>
  )
}
