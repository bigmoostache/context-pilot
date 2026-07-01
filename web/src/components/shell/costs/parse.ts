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
  /** Configured max_freezes for the culprit panel (0 when no culprit). */
  culpritMaxFreezes: number
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
      if (c.length < 16) return null
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
        culpritMaxFreezes: Number(c[9]),
        hitTokens: Number(c[10]),
        hitCost: Number(c[11]),
        missTokens: Number(c[12]),
        missCost: Number(c[13]),
        outTokens: Number(c[14]),
        outCost: Number(c[15]),
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

// ── Markdown export ─────────────────────────────────────────────────────────

/** Minimal dollar formatter for markdown. */
function md$(v: number): string {
  return v < 0.01 ? `$${v.toFixed(4)}` : `$${v.toFixed(2)}`
}

/** Minimal token formatter for markdown. */
function mdTok(v: number): string {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(2)}M`
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`
  return String(Math.round(v))
}

/** Serialize the current filtered view as a clean markdown report. */
export function buildMarkdownReport(
  filtered: CostRow[],
  totalRows: number,
  filters: { tempo: string; queue: string; noBreak: string },
): string {
  const s = computeSummary(filtered)
  const culprits = culpritDistribution(filtered)
  const costs = costBreakdown(filtered)
  const tools = toolCostAttribution(filtered)
  const ct = crossTabToolCulprit(filtered)

  const lines: string[] = []
  const push = (...l: string[]) => lines.push(...l)

  // Header
  push("# Cost Analysis Report", "")
  const fParts: string[] = []
  if (filters.tempo !== "all") fParts.push(`Tempo=${filters.tempo === "1" ? "On" : "Off"}`)
  if (filters.queue !== "all") fParts.push(`Queue=${filters.queue === "1" ? "On" : "Off"}`)
  if (filters.noBreak !== "all") fParts.push(`NoBreak=${filters.noBreak === "1" ? "On" : "Off"}`)
  if (fParts.length > 0) push(`**Filters:** ${fParts.join(", ")}`)
  push(`**Ticks:** ${s.totalTicks}${s.totalTicks !== totalRows ? ` (filtered from ${totalRows})` : ""}`, "")

  // Summary
  push("## Summary", "")
  push("| Metric | Value |", "|--------|-------|")
  push(`| Total cost | ${md$(s.totalCost)} |`)
  push(`| LLM ticks | ${s.totalTicks.toLocaleString()} |`)
  push(`| Avg cost / tick | ${md$(s.avgCostPerTick)} |`)
  push(`| Cache hit rate | ${(s.cacheHitRate * 100).toFixed(1)}% |`)
  push(`| Clean ticks | ${s.ticksClean} |`)
  push(`| Break ticks | ${s.ticksWithBreak} |`)
  push(`| Total hit tokens | ${mdTok(s.totalHitTokens)} |`)
  push(`| Total miss tokens | ${mdTok(s.totalMissTokens)} |`)
  push(`| Total output tokens | ${mdTok(s.totalOutTokens)} |`)
  push("")

  // Culprit distribution
  if (culprits.length > 0) {
    const totalCount = culprits.reduce((a, b) => a + b.value, 0)
    push("## Cache Break Culprits", "")
    push("| Culprit | Count | % |", "|---------|-------|---|")
    for (const c of culprits) {
      push(`| ${c.label} | ${c.value} | ${((c.value / totalCount) * 100).toFixed(1)}% |`)
    }
    push("")
  }

  // Cost breakdown
  push("## Cost by Category", "")
  push("| Category | Cost |", "|----------|------|")
  for (const c of costs) push(`| ${c.label} | ${md$(c.value)} |`)
  push("")

  // Tool attribution
  if (tools.length > 0) {
    push("## Top Tools by Associated Cost", "")
    push("| Tool | Cost |", "|------|------|")
    for (const t of tools) push(`| ${t.label} | ${md$(t.value)} |`)
    push("")
  }

  // Culprit cost attribution
  const culpritCosts = culpritCostAttribution(filtered)
  if (culpritCosts.length > 0) {
    push("## Top Culprits by Associated Cost", "")
    push("| Culprit | Cost |", "|---------|------|")
    for (const c of culpritCosts) push(`| ${c.label} | ${md$(c.value)} |`)
    push("")
  }

  // Max freeze per culprit
  const freezes = maxFreezePerCulprit(filtered)
  if (freezes.length > 0) {
    push("## Max Freezes per Culprit", "")
    push("| Culprit | Max Freezes |", "|---------|-------------|")
    for (const f of freezes) push(`| ${f.label} | ${f.value} |`)
    push("")
  }

  // Token distribution (panel-based)
  if (filtered.length > 0) {
    const avgBefore = Math.round(filtered.reduce((a, r) => a + r.tokensBefore, 0) / filtered.length)
    const avgCulprit = Math.round(filtered.reduce((a, r) => a + r.tokensCulprit, 0) / filtered.length)
    const avgAfter = Math.round(filtered.reduce((a, r) => a + r.tokensAfter, 0) / filtered.length)
    push(`## Average Token Layout (${filtered.length} ticks)`, "")
    push("| Segment | Avg tokens |", "|---------|-----------|")
    push(`| Before culprit | ${mdTok(avgBefore)} |`)
    push(`| Culprit panel | ${mdTok(avgCulprit)} |`)
    push(`| After culprit | ${mdTok(avgAfter)} |`)
    push("")

    // Token distribution (API-reported)
    const avgHit = Math.round(filtered.reduce((a, r) => a + r.hitTokens, 0) / filtered.length)
    const avgMiss = Math.round(filtered.reduce((a, r) => a + r.missTokens, 0) / filtered.length)
    const avgOut = Math.round(filtered.reduce((a, r) => a + r.outTokens, 0) / filtered.length)
    push(`## API-Reported Token Layout (${filtered.length} ticks)`, "")
    push("| Segment | Avg tokens |", "|---------|-----------|")
    push(`| Cache hit | ${mdTok(avgHit)} |`)
    push(`| Cache miss | ${mdTok(avgMiss)} |`)
    push(`| Output | ${mdTok(avgOut)} |`)
    push("")
  }

  // Cross-tab
  if (ct.tools.length > 0 && ct.culprits.length > 0) {
    push("## Tool × Culprit Cross-Tab", "")
    const hdr = ["Tool", ...ct.culprits, "Total"]
    push(`| ${hdr.join(" | ")} |`)
    push(`|${hdr.map(() => "------").join("|")}|`)
    for (const tool of ct.tools) {
      const cells = ct.culprits.map((c) => String(ct.cells.get(`${tool}\t${c}`) ?? 0))
      const rowTotal = ct.culprits.reduce((a, c) => a + (ct.cells.get(`${tool}\t${c}`) ?? 0), 0)
      push(`| ${tool} | ${cells.join(" | ")} | ${rowTotal} |`)
    }
    // Totals row
    const colTotals = ct.culprits.map((c) =>
      String(ct.tools.reduce((a, t) => a + (ct.cells.get(`${t}\t${c}`) ?? 0), 0)),
    )
    push(`| **Total** | ${colTotals.join(" | ")} | **${filtered.length}** |`)
    push("")
  }

  return lines.join("\n")
}

/** Per-culprit max_freezes (latest recorded value per panel type). */
export function maxFreezePerCulprit(rows: CostRow[]): Slice[] {
  const map = new Map<string, number>()
  for (const r of rows) {
    if (r.noPanelBroken) continue
    const key = r.culprit || "unknown"
    // Always take the latest value (overwrite) — reflects current code config
    map.set(key, r.culpritMaxFreezes)
  }
  return [...map.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([label, value], i) => ({
      label,
      value,
      color: CULPRIT_PALETTE[i % CULPRIT_PALETTE.length] ?? "#94a3b8",
    }))
}

/** Per-culprit cost attribution (break ticks only, grouped by culprit type). */
export function culpritCostAttribution(rows: CostRow[]): Slice[] {
  const map = new Map<string, number>()
  for (const r of rows) {
    if (r.noPanelBroken) continue
    const totalCost = r.hitCost + r.missCost + r.outCost
    const key = r.culprit || "unknown"
    map.set(key, (map.get(key) ?? 0) + totalCost)
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
