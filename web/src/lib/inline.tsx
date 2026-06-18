import { createElement, Fragment, type ReactNode } from "react"

/**
 * Minimal inline renderer — handles **bold**, `code`, and line breaks.
 * Enough to make assistant/user prose feel typeset without a markdown dep.
 */
export function renderInline(text: string): ReactNode {
  const lines = text.split("\n")
  return lines.map((line, li) =>
    createElement(
      Fragment,
      { key: li },
      ...parseSegments(line),
      li < lines.length - 1 ? createElement("br", { key: `br-${li}` }) : null,
    ),
  )
}

function parseSegments(line: string): ReactNode[] {
  const out: ReactNode[] = []
  // tokenize on ** and `
  const re = /(\*\*[^*]+\*\*|`[^`]+`)/g
  let last = 0
  let m: RegExpExecArray | null
  let k = 0
  while ((m = re.exec(line)) !== null) {
    if (m.index > last) out.push(line.slice(last, m.index))
    const tok = m[0]
    if (tok.startsWith("**")) {
      out.push(
        createElement(
          "strong",
          { key: k++, className: "font-semibold text-foreground" },
          tok.slice(2, -2),
        ),
      )
    } else {
      out.push(
        createElement(
          "code",
          {
            key: k++,
            // Theme-aware chip: `bg-muted` adapts (light beige in the light
            // palette, warm-grey in dark) instead of a hardcoded near-black
            // oklch that was unreadable in light mode (T23). Faint border gives
            // it definition on both backgrounds; the interactive accent stays.
            className:
              "rounded-[3px] border border-foreground/10 bg-muted px-1 py-px font-mono text-[0.82em] text-[var(--interactive)]",
          },
          tok.slice(1, -1),
        ),
      )
    }
    last = m.index + tok.length
  }
  if (last < line.length) out.push(line.slice(last))
  return out
}
