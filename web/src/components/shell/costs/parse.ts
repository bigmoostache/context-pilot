/** A single row from the cost-tracking TSV. */
export interface CostRow {
  /** Epoch milliseconds at tick start. */
  epoch: number
  /** Comma-separated last 3 tool names. */
  tools: string
  /** Panel type that broke the cache (or "none"). */
  culprit: string
  /** Tokens strictly before the culprit (incl. system + tools prefix). */
  tokensBefore: number
  /** Tokens of the culprit panel itself. */
  tokensCulprit: number
  /** Tokens strictly after the culprit. */
  tokensAfter: number
  queueActive: boolean
  tempoActive: boolean
  noPanelBroken: boolean
  hitTokens: number
  hitCost: number
  missTokens: number
  missCost: number
  outTokens: number
  outCost: number
}

/** Parse the raw TSV content into typed rows. Skips the header line. */
export function parseCostTsv(content: string): CostRow[] {
  const lines = content.trim().split("\n")
  if (lines.length < 2) return []
  return lines
    .slice(1)
    .map((line) => {
      const c = line.split("\t")
      if (c.length < 15) return null
      return {
        epoch: Number(c[0]),
        tools: c[1] ?? "",
        culprit: c[2] ?? "none",
        tokensBefore: Number(c[3]),
        tokensCulprit: Number(c[4]),
        tokensAfter: Number(c[5]),
        queueActive: c[6] === "true",
        tempoActive: c[7] === "true",
        noPanelBroken: c[8] === "true",
        hitTokens: Number(c[9]),
        hitCost: Number(c[10]),
        missTokens: Number(c[11]),
        missCost: Number(c[12]),
        outTokens: Number(c[13]),
        outCost: Number(c[14]),
      } satisfies CostRow
    })
    .filter((r): r is CostRow => r !== null)
}

// ── Aggregation helpers ─────────────────────────────────────────────────────

export interface Slice {
  label: string
  value: number
  color: string
}

const CULPRIT_PALETTE = [
  "#60a5fa", // blue
  "#f97316", // orange
  "#a78bfa", // purple
  "#34d399", // emerald
  "#fb7185", // rose
  "#fbbf24", // amber
  "#38bdf8", // sky
  "#e879f9", // fuchsia
]

/** Culprit panel type → frequency + total miss cost. */
export function culpritDistribution(rows: CostRow[]): Slice[] {
  const broken = rows.filter((r) => !r.noPanelBroken)
  const map = new Map<string, number>()
  for (const r of broken) {
    map.set(r.culprit, (map.get(r.culprit) ?? 0) + 1)
  }
  return [...map.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([label, value], i) => ({
      label,
      value,
      color: CULPRIT_PALETTE[i % CULPRIT_PALETTE.length] ?? "#94a3b8",
    }))
}

/** Cost-by-category breakdown for the entire dataset. */
export function costBreakdown(rows: CostRow[]): Slice[] {
  let hit = 0
  let miss = 0
  let out = 0
  for (const r of rows) {
    hit += r.hitCost
    miss += r.missCost
    out += r.outCost
  }
  return [
    { label: "Cache hit", value: hit, color: "var(--ok, #4ade80)" },
    { label: "Cache miss", value: miss, color: "var(--warn, #fbbf24)" },
    { label: "Output", value: out, color: "#60a5fa" },
  ]
}

/** Summary statistics. */
export interface Summary {
  totalTicks: number
  totalCost: number
  avgCostPerTick: number
  cacheHitRate: number
  totalHitTokens: number
  totalMissTokens: number
  totalOutTokens: number
  ticksWithBreak: number
  ticksClean: number
}

export function computeSummary(rows: CostRow[]): Summary {
  let totalCost = 0
  let totalHit = 0
  let totalMiss = 0
  let totalOut = 0
  let clean = 0
  for (const r of rows) {
    totalCost += r.hitCost + r.missCost + r.outCost
    totalHit += r.hitTokens
    totalMiss += r.missTokens
    totalOut += r.outTokens
    if (r.noPanelBroken) clean++
  }
  const totalInput = totalHit + totalMiss
  return {
    totalTicks: rows.length,
    totalCost,
    avgCostPerTick: rows.length > 0 ? totalCost / rows.length : 0,
    cacheHitRate: totalInput > 0 ? totalHit / totalInput : 0,
    totalHitTokens: totalHit,
    totalMissTokens: totalMiss,
    totalOutTokens: totalOut,
    ticksWithBreak: rows.length - clean,
    ticksClean: clean,
  }
}

/** Tool × culprit cross-tabulation (count matrix). */
export interface CrossTab {
  /** Sorted unique tool names (rows). */
  tools: string[]
  /** Sorted unique culprit types (columns). */
  culprits: string[]
  /** Map "tool\tculprit" → count. */
  cells: Map<string, number>
}

export function crossTabToolCulprit(rows: CostRow[]): CrossTab {
  const cells = new Map<string, number>()
  const toolSet = new Set<string>()
  const culpritSet = new Set<string>()
  for (const r of rows) {
    // First tool in the comma-separated list is the most recent (the one that ran at this tick)
    const tool = r.tools.split(",")[0] ?? "(none)"
    if (!tool) continue
    const culprit = r.culprit || "none"
    toolSet.add(tool)
    culpritSet.add(culprit)
    const key = `${tool}\t${culprit}`
    cells.set(key, (cells.get(key) ?? 0) + 1)
  }
  // Sort tools by total count descending
  const toolTotals = [...toolSet].map((t) => {
    let total = 0
    for (const c of culpritSet) total += cells.get(`${t}\t${c}`) ?? 0
    return { tool: t, total }
  })
  toolTotals.sort((a, b) => b.total - a.total)
  // Sort culprits by total count descending
  const culpritTotals = [...culpritSet].map((c) => {
    let total = 0
    for (const t of toolSet) total += cells.get(`${t}\t${c}`) ?? 0
    return { culprit: c, total }
  })
  culpritTotals.sort((a, b) => b.total - a.total)
  return {
    tools: toolTotals.map((t) => t.tool),
    culprits: culpritTotals.map((c) => c.culprit),
    cells,
  }
}

/** Per-tool cost attribution (from the three_last_tools column). */
export function toolCostAttribution(rows: CostRow[]): Slice[] {
  const map = new Map<string, number>()
  for (const r of rows) {
    const totalCost = r.hitCost + r.missCost + r.outCost
    for (const tool of r.tools.split(",").filter(Boolean)) {
      map.set(tool, (map.get(tool) ?? 0) + totalCost)
    }
  }
  return [...map.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 10)
    .map(([label, value], i) => ({
      label,
      value,
      color: CULPRIT_PALETTE[i % CULPRIT_PALETTE.length] ?? "#94a3b8",
    }))
}
