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
  /** Cache break kind: "no_break" | "content_changed" | "panel_appeared" | "panel_disappeared". */
  breakKind: string
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
      if (c.length < 15) return null
      // Support both old 15-col and new 16-col format
      const has16 = c.length >= 16
      const o = has16 ? 1 : 0
      return {
        epoch: Number(c[0]),
        tools: c[1] ?? "",
        culprit: c[2] ?? "none",
        tokensBefore: Number(c[3]),
        tokensCulprit: Number(c[4]),
        tokensAfter: Number(c[5]),
        queueActive: c[6] === "true",
        tempoActive: c[7] === "true",
        breakKind: c[8] ?? "no_break",
        culpritMaxFreezes: has16 ? Number(c[9]) : 0,
        hitTokens: Number(c[9 + o]),
        hitCost: Number(c[10 + o]),
        missTokens: Number(c[11 + o]),
        missCost: Number(c[12 + o]),
        outTokens: Number(c[13 + o]),
        outCost: Number(c[14 + o]),
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
  const broken = rows.filter((r) => r.breakKind !== "no_break")
  const map = new Map<string, number>()
  for (const r of broken) {
    map.set(r.culprit, (map.get(r.culprit) ?? 0) + 1)
  }
  return [...map]
    .toSorted((a, b) => b[1] - a[1])
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
    if (r.breakKind === "no_break") clean++
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
    const tool = r.tools.split(",", 1)[0] ?? "(none)"
    if (!tool) continue
    const culprit = r.culprit || "none"
    toolSet.add(tool)
    culpritSet.add(culprit)
    const key = `${tool}\t${culprit}`
    cells.set(key, (cells.get(key) ?? 0) + 1)
  }
  // Sort tools by total count descending
  const toolTotals = [...toolSet]
    .map((t) => {
      let total = 0
      for (const c of culpritSet) total += cells.get(`${t}\t${c}`) ?? 0
      return { tool: t, total }
    })
    .toSorted((a, b) => b.total - a.total)
  // Sort culprits by total count descending
  const culpritTotals = [...culpritSet]
    .map((c) => {
      let total = 0
      for (const t of toolSet) total += cells.get(`${t}\t${c}`) ?? 0
      return { culprit: c, total }
    })
    .toSorted((a, b) => b.total - a.total)
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
  if (v >= 1000) return `${(v / 1000).toFixed(1)}K`
  return String(Math.round(v))
}

// ── Markdown report sections (one pure `CostRow[] → string[]` builder each) ──
//
// buildMarkdownReport was a single 86-statement function; each report section
// is now its own small pure builder returning the lines it contributes (an
// empty array when its section is omitted), so the top-level function is a thin
// concat and every piece stays within the P8 complexity/statement budgets.

/** Report title + optional active-filter line + tick count. */
function reportHeader(
  s: Summary,
  totalRows: number,
  filters: { tempo: string; queue: string; breakKind: string },
): string[] {
  const fParts: string[] = []
  if (filters.tempo !== "all") fParts.push(`Tempo=${filters.tempo === "1" ? "On" : "Off"}`)
  if (filters.queue !== "all") fParts.push(`Queue=${filters.queue === "1" ? "On" : "Off"}`)
  if (filters.breakKind !== "all") fParts.push(`Break=${filters.breakKind}`)
  const suffix = s.totalTicks === totalRows ? "" : ` (filtered from ${totalRows})`
  return [
    "# Cost Analysis Report",
    "",
    ...(fParts.length > 0 ? [`**Filters:** ${fParts.join(", ")}`] : []),
    `**Ticks:** ${s.totalTicks}${suffix}`,
    "",
  ]
}

/** Summary metrics table. */
function reportSummary(s: Summary): string[] {
  return [
    "## Summary",
    "",
    "| Metric | Value |",
    "|--------|-------|",
    `| Total cost | ${md$(s.totalCost)} |`,
    `| LLM ticks | ${s.totalTicks.toLocaleString()} |`,
    `| Avg cost / tick | ${md$(s.avgCostPerTick)} |`,
    `| Cache hit rate | ${(s.cacheHitRate * 100).toFixed(1)}% |`,
    `| Clean ticks | ${s.ticksClean} |`,
    `| Break ticks | ${s.ticksWithBreak} |`,
    `| Total hit tokens | ${mdTok(s.totalHitTokens)} |`,
    `| Total miss tokens | ${mdTok(s.totalMissTokens)} |`,
    `| Total output tokens | ${mdTok(s.totalOutTokens)} |`,
    "",
  ]
}

/** Cache-break culprit frequency table (omitted when no breaks). */
function reportCulprits(culprits: Slice[]): string[] {
  if (culprits.length === 0) return []
  const totalCount = culprits.reduce((a, b) => a + b.value, 0)
  return [
    "## Cache Break Culprits",
    "",
    "| Culprit | Count | % |",
    "|---------|-------|---|",
    ...culprits.map(
      (c) => `| ${c.label} | ${c.value} | ${((c.value / totalCount) * 100).toFixed(1)}% |`,
    ),
    "",
  ]
}

/** Cost-by-category table. */
function reportCostBreakdown(costs: Slice[]): string[] {
  return [
    "## Cost by Category",
    "",
    "| Category | Cost |",
    "|----------|------|",
    ...costs.map((c) => `| ${c.label} | ${md$(c.value)} |`),
    "",
  ]
}

/** A generic labelled cost table (top tools / top culprits by cost). */
function reportCostTable(title: string, header: string, rows: Slice[]): string[] {
  if (rows.length === 0) return []
  return [
    `## ${title}`,
    "",
    header,
    "|------|------|",
    ...rows.map((r) => `| ${r.label} | ${md$(r.value)} |`),
    "",
  ]
}

/** Max-freezes-per-culprit table (omitted when no breaks). */
function reportFreezes(filtered: CostRow[]): string[] {
  const freezes = maxFreezePerCulprit(filtered)
  if (freezes.length === 0) return []
  return [
    "## Max Freezes per Culprit",
    "",
    "| Culprit | Max Freezes |",
    "|---------|-------------|",
    ...freezes.map((f) => `| ${f.label} | ${f.value} |`),
    "",
  ]
}

/** A three-row average-token table (mean of `pick` over the rows). */
function avgTokenTable(
  title: string,
  filtered: CostRow[],
  segments: [string, (r: CostRow) => number][],
): string[] {
  const rows = segments.map(([label, pick]) => {
    const avg = Math.round(filtered.reduce((a, r) => a + pick(r), 0) / filtered.length)
    return `| ${label} | ${mdTok(avg)} |`
  })
  return [
    `## ${title} (${filtered.length} ticks)`,
    "",
    "| Segment | Avg tokens |",
    "|---------|-----------|",
    ...rows,
    "",
  ]
}

/** Both token-layout tables (panel-based + API-reported); omitted when empty. */
function reportTokenLayout(filtered: CostRow[]): string[] {
  if (filtered.length === 0) return []
  return [
    ...avgTokenTable("Average Token Layout", filtered, [
      ["Before culprit", (r) => r.tokensBefore],
      ["Culprit panel", (r) => r.tokensCulprit],
      ["After culprit", (r) => r.tokensAfter],
    ]),
    ...avgTokenTable("API-Reported Token Layout", filtered, [
      ["Cache hit", (r) => r.hitTokens],
      ["Cache miss", (r) => r.missTokens],
      ["Output", (r) => r.outTokens],
    ]),
  ]
}

/** Tool × culprit cross-tabulation table (omitted when either axis is empty). */
function reportCrossTab(ct: CrossTab, filtered: CostRow[]): string[] {
  if (ct.tools.length === 0 || ct.culprits.length === 0) return []
  const hdr = ["Tool", ...ct.culprits, "Total"]
  const body = ct.tools.map((tool) => {
    const cells = ct.culprits.map((c) => String(ct.cells.get(`${tool}\t${c}`) ?? 0))
    const rowTotal = ct.culprits.reduce((a, c) => a + (ct.cells.get(`${tool}\t${c}`) ?? 0), 0)
    return `| ${tool} | ${cells.join(" | ")} | ${rowTotal} |`
  })
  const colTotals = ct.culprits.map((c) =>
    String(ct.tools.reduce((a, t) => a + (ct.cells.get(`${t}\t${c}`) ?? 0), 0)),
  )
  return [
    "## Tool × Culprit Cross-Tab",
    "",
    `| ${hdr.join(" | ")} |`,
    `|${hdr.map(() => "------").join("|")}|`,
    ...body,
    `| **Total** | ${colTotals.join(" | ")} | **${filtered.length}** |`,
    "",
  ]
}

/** Serialize the current filtered view as a clean markdown report. */
export function buildMarkdownReport(
  filtered: CostRow[],
  totalRows: number,
  filters: { tempo: string; queue: string; breakKind: string },
): string {
  const s = computeSummary(filtered)
  return [
    ...reportHeader(s, totalRows, filters),
    ...reportSummary(s),
    ...reportCulprits(culpritDistribution(filtered)),
    ...reportCostBreakdown(costBreakdown(filtered)),
    ...reportCostTable(
      "Top Tools by Associated Cost",
      "| Tool | Cost |",
      toolCostAttribution(filtered),
    ),
    ...reportCostTable(
      "Top Culprits by Associated Cost",
      "| Culprit | Cost |",
      culpritCostAttribution(filtered),
    ),
    ...reportFreezes(filtered),
    ...reportTokenLayout(filtered),
    ...reportCrossTab(crossTabToolCulprit(filtered), filtered),
  ].join("\n")
}

/** Per-culprit max_freezes (latest recorded value per panel type). */
export function maxFreezePerCulprit(rows: CostRow[]): Slice[] {
  const map = new Map<string, number>()
  for (const r of rows) {
    if (r.breakKind === "no_break") continue
    const key = r.culprit || "unknown"
    // Always take the latest value (overwrite) — reflects current code config
    map.set(key, r.culpritMaxFreezes)
  }
  return [...map]
    .toSorted((a, b) => b[1] - a[1])
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
    if (r.breakKind === "no_break") continue
    const totalCost = r.hitCost + r.missCost + r.outCost
    const key = r.culprit || "unknown"
    map.set(key, (map.get(key) ?? 0) + totalCost)
  }
  return [...map]
    .toSorted((a, b) => b[1] - a[1])
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
    const rowTools = r.tools.split(",").filter(Boolean)
    for (const tool of rowTools) {
      map.set(tool, (map.get(tool) ?? 0) + totalCost)
    }
  }
  return [...map]
    .toSorted((a, b) => b[1] - a[1])
    .slice(0, 10)
    .map(([label, value], i) => ({
      label,
      value,
      color: CULPRIT_PALETTE[i % CULPRIT_PALETTE.length] ?? "#94a3b8",
    }))
}
