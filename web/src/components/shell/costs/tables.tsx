import { useMemo, useState } from "react"
import type { CrossTab, CostRow } from "./parse"
import { fmtTokens } from "./format"

// ── Heat-map helpers ────────────────────────────────────────────────────────

/** HSL heat-map: 0 → transparent, low → green, high → red. */
function heatBg(count: number, max: number): string | undefined {
  if (count === 0 || max === 0) return undefined
  const t = count / max // 0..1
  const hue = 120 - t * 120 // 120 (green) → 0 (red)
  const sat = 50 + t * 15 // 50% → 65%
  const lit = 93 - t * 18 // 93% → 75%
  return `hsl(${hue}, ${sat}%, ${lit}%)`
}

/** Dark-mode heat-map — lower lightness, slightly higher saturation. */
function heatBgDark(count: number, max: number): string | undefined {
  if (count === 0 || max === 0) return undefined
  const t = count / max
  const hue = 120 - t * 120
  const sat = 40 + t * 20
  const lit = 20 + t * 10
  return `hsl(${hue}, ${sat}%, ${lit}%)`
}

// ── Section wrapper (shared) ────────────────────────────────────────────────

function Section({ children }: { children: React.ReactNode }) {
  return <div className="rounded-xl border border-border bg-card p-5 card-shadow">{children}</div>
}

// ── Cross-tab table ─────────────────────────────────────────────────────────

export function CrossTabTable({
  crossTab,
  totalTicks,
}: {
  crossTab: CrossTab
  totalTicks: number
}) {
  const [hover, setHover] = useState<{ tool: string; culprit: string } | null>(null)
  const isDark = document.documentElement.classList.contains("dark")
  const bgFn = isDark ? heatBgDark : heatBg

  // Find max cell value for heat-map normalization
  const maxCount = useMemo(() => {
    let mx = 0
    for (const v of crossTab.cells.values()) if (v > mx) mx = v
    return mx
  }, [crossTab.cells])

  return (
    <Section>
      <div className="flex items-baseline justify-between">
        <span className="text-[13px] font-semibold text-foreground/80">
          Tool × Culprit (occurrence count)
        </span>
        {/* Hover info bar */}
        {hover && (
          <span className="text-[11px] text-muted-foreground">
            <strong className="text-foreground/80">{hover.tool}</strong>
            {" × "}
            <strong className="text-foreground/80">{hover.culprit}</strong>
            {" = "}
            <strong className="tabular-nums text-foreground">
              {crossTab.cells.get(`${hover.tool}\t${hover.culprit}`) ?? 0}
            </strong>
          </span>
        )}
      </div>
      <div className="mt-3 overflow-x-auto">
        <table className="w-full border-collapse text-[11px]">
          <thead>
            <tr>
              <th className="sticky left-0 z-10 bg-card px-2 py-1.5 text-left font-medium text-muted-foreground">
                Tool
              </th>
              {crossTab.culprits.map((c) => (
                <th
                  key={c}
                  className={`px-2 py-1.5 text-center font-medium transition-colors ${
                    hover?.culprit === c ? "text-foreground" : "text-muted-foreground"
                  }`}
                >
                  {c}
                </th>
              ))}
              <th className="px-2 py-1.5 text-center font-semibold text-foreground/70">Total</th>
            </tr>
          </thead>
          <tbody>
            {crossTab.tools.map((tool) => {
              const rowTotal = crossTab.culprits.reduce(
                (s, c) => s + (crossTab.cells.get(`${tool}\t${c}`) ?? 0),
                0,
              )
              const isRowHovered = hover?.tool === tool
              return (
                <tr key={tool} className="border-t border-border/40">
                  <td
                    className={`sticky left-0 z-10 bg-card px-2 py-1 font-medium transition-colors ${
                      isRowHovered ? "text-foreground" : "text-foreground/80"
                    }`}
                  >
                    {tool}
                  </td>
                  {crossTab.culprits.map((c) => {
                    const v = crossTab.cells.get(`${tool}\t${c}`) ?? 0
                    const isActive = hover !== null && hover.tool === tool && hover.culprit === c
                    const isDimmed =
                      hover !== null && !isActive && hover.tool !== tool && hover.culprit !== c
                    return (
                      <td
                        key={c}
                        className={`px-2 py-1 text-center tabular-nums transition-all duration-150 ${
                          isActive
                            ? "ring-2 ring-foreground/30 ring-inset font-bold text-foreground"
                            : isDimmed
                              ? "text-foreground/25"
                              : "text-foreground/70"
                        }`}
                        style={{
                          backgroundColor: v > 0 ? bgFn(v, maxCount) : undefined,
                          borderRadius: isActive ? "4px" : undefined,
                        }}
                        onMouseEnter={() => setHover({ tool, culprit: c })}
                        onMouseLeave={() => setHover(null)}
                      >
                        {v > 0 ? v : <span className="text-muted-foreground/30">·</span>}
                      </td>
                    )
                  })}
                  <td className="px-2 py-1 text-center tabular-nums font-semibold text-foreground/70">
                    {rowTotal}
                  </td>
                </tr>
              )
            })}
          </tbody>
          <tfoot>
            <tr className="border-t border-border">
              <td className="sticky left-0 z-10 bg-card px-2 py-1.5 font-semibold text-foreground/70">
                Total
              </td>
              {crossTab.culprits.map((c) => {
                const colTotal = crossTab.tools.reduce(
                  (s, t) => s + (crossTab.cells.get(`${t}\t${c}`) ?? 0),
                  0,
                )
                return (
                  <td
                    key={c}
                    className={`px-2 py-1.5 text-center tabular-nums font-semibold transition-colors ${
                      hover?.culprit === c ? "text-foreground" : "text-foreground/70"
                    }`}
                  >
                    {colTotal}
                  </td>
                )
              })}
              <td className="px-2 py-1.5 text-center tabular-nums font-bold text-foreground">
                {totalTicks}
              </td>
            </tr>
          </tfoot>
        </table>
      </div>
    </Section>
  )
}

// ── Token distribution bars ─────────────────────────────────────────────────

export function ApiTokenDistribution({ rows }: { rows: CostRow[] }) {
  if (rows.length === 0) return null

  const avgHit = Math.round(rows.reduce((s, r) => s + r.hitTokens, 0) / rows.length)
  const avgMiss = Math.round(rows.reduce((s, r) => s + r.missTokens, 0) / rows.length)
  const avgOut = Math.round(rows.reduce((s, r) => s + r.outTokens, 0) / rows.length)
  const total = avgHit + avgMiss + avgOut

  const segments = [
    { label: "Cache hit", value: avgHit, color: "var(--ok, #4ade80)" },
    { label: "Cache miss", value: avgMiss, color: "var(--danger, #ef4444)" },
    { label: "Output", value: avgOut, color: "#60a5fa" },
  ]

  return (
    <Section>
      <span className="text-[13px] font-semibold text-foreground/80">
        API-reported token layout ({rows.length} ticks)
      </span>
      <div className="mt-3 flex h-7 overflow-hidden rounded-lg">
        {segments.map((s) => (
          <div
            key={s.label}
            className="flex items-center justify-center text-[10px] font-medium text-white transition-all duration-500"
            style={{
              width: `${(s.value / total) * 100}%`,
              backgroundColor: s.color,
              minWidth: s.value > 0 ? "2%" : 0,
            }}
          >
            {(s.value / total) * 100 > 8 ? fmtTokens(s.value) : ""}
          </div>
        ))}
      </div>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
        {segments.map((s) => (
          <span key={s.label} className="flex items-center gap-1.5">
            <span
              className="inline-block size-2.5 rounded-sm"
              style={{ backgroundColor: s.color }}
            />
            {s.label}: {fmtTokens(s.value)} avg
          </span>
        ))}
      </div>
    </Section>
  )
}

export function TokenDistribution({ rows }: { rows: CostRow[] }) {
  if (rows.length === 0) return null

  const avgBefore = Math.round(rows.reduce((s, r) => s + r.tokensBefore, 0) / rows.length)
  const avgCulprit = Math.round(rows.reduce((s, r) => s + r.tokensCulprit, 0) / rows.length)
  const avgAfter = Math.round(rows.reduce((s, r) => s + r.tokensAfter, 0) / rows.length)
  const total = avgBefore + avgCulprit + avgAfter

  const segments = [
    { label: "Before culprit", value: avgBefore, color: "var(--ok, #4ade80)" },
    { label: "Culprit panel", value: avgCulprit, color: "var(--danger, #ef4444)" },
    { label: "After culprit", value: avgAfter, color: "#60a5fa" },
  ]

  return (
    <Section>
      <span className="text-[13px] font-semibold text-foreground/80">
        Average token layout ({rows.length} ticks)
      </span>
      <div className="mt-3 flex h-7 overflow-hidden rounded-lg">
        {segments.map((s) => (
          <div
            key={s.label}
            className="flex items-center justify-center text-[10px] font-medium text-white transition-all duration-500"
            style={{
              width: `${(s.value / total) * 100}%`,
              backgroundColor: s.color,
              minWidth: s.value > 0 ? "2%" : 0,
            }}
          >
            {(s.value / total) * 100 > 8 ? fmtTokens(s.value) : ""}
          </div>
        ))}
      </div>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
        {segments.map((s) => (
          <span key={s.label} className="flex items-center gap-1.5">
            <span
              className="inline-block size-2.5 rounded-sm"
              style={{ backgroundColor: s.color }}
            />
            {s.label}: {fmtTokens(s.value)} avg
          </span>
        ))}
      </div>
    </Section>
  )
}
