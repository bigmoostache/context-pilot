// ── Relative-time formatting ───────────────────────────────────────────
//
// Shared by the thread conversation and the cockpit conversation renderers so
// the two no longer keep near-duplicate `formatTs`/`ago` copies (L21). Split out
// of `utils.ts` (L23) so importing `cn` no longer drags this in.

/** Format an epoch-ms instant as a compact relative age ("just now", "5m ago",
 *  "2h ago", "3d ago"). */
export function relativeTime(ms: number): string {
  const s = Math.max(0, Math.floor((Date.now() - ms) / 1000))
  if (s < 5) return "just now"
  if (s < 60) return `${s}s ago`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ago`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ago`
  return `${Math.floor(h / 24)}d ago`
}

/**
 * Normalise a thread message's `ts` into a human relative age. The field
 * arrives as either an epoch-ms number (REST backstop poll), an ISO 8601 string
 * (SSE delta reducer), or an already-formatted relative string — this collapses
 * all three into a single "Xm ago" label. Anything that isn't a recognisable
 * timestamp passes through unchanged.
 */
export function formatTs(ts: string | number | undefined): string {
  if (ts === undefined) return ""
  const n = typeof ts === "number" ? ts : Number(ts)
  // Epoch-ms: any number above 2020-01-01 00:00:00 UTC.
  if (!Number.isNaN(n) && n > 1_577_836_800_000) return relativeTime(n)
  // ISO 8601 string (from the SSE reducer).
  if (typeof ts === "string") {
    const d = new Date(ts)
    if (!Number.isNaN(d.getTime()) && d.getTime() > 1_577_836_800_000) {
      return relativeTime(d.getTime())
    }
  }
  // Already formatted or unknown — pass through.
  return String(ts)
}
