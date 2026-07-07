/**
 * Cost-dashboard value formatters. Kept in their own module (not beside the
 * chart components) so importing them never trips Fast Refresh's
 * component-only-export rule.
 */

/** Format a dollar amount: 4 decimals below a cent, else 2. */
export const fmtDollar = (v: number): string => (v < 0.01 ? `$${v.toFixed(4)}` : `$${v.toFixed(2)}`)

/** Format a token count with K/M suffixes. */
export const fmtTokens = (v: number): string =>
  v >= 1_000_000
    ? `${(v / 1_000_000).toFixed(1)}M`
    : v >= 1000
      ? `${(v / 1000).toFixed(1)}K`
      : String(v)
