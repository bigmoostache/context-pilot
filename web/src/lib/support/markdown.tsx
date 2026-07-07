import { memo, useRef, useState, type ReactNode } from "react"
import ReactMarkdown, { type Components } from "react-markdown"
import remarkGfm from "remark-gfm"
import remarkMath from "remark-math"
import rehypeKatex from "rehype-katex"
import "katex/dist/katex.min.css"

import { CopyButton } from "@/components/conversation/Message"
import { cn, clipboard } from "@/lib/utils"

/**
 * Full GitHub-flavored markdown renderer for chat/thread messages.
 *
 * Replaces the old hand-rolled inline renderer (bold/code/linebreaks only),
 * which left headings (`#`/`##`), tables, lists, blockquotes and fenced code
 * blocks as literal text. We delegate parsing to `react-markdown` (CommonMark)
 * + `remark-gfm` (tables, task lists, strikethrough, autolinks) — a
 * battle-tested pipeline — and own only the *presentation*: every element is
 * mapped to the app's theme tokens so the output is typeset, not generic.
 *
 * No raw-HTML rehype plugin is wired in: message text is treated as markdown,
 * never HTML, so a message can't inject markup (XSS-safe by construction).
 *
 * ## Variants
 * - `default` — assistant/agent prose on the page background.
 * - `onAccent` — user prose inside the coloured (signal) bubble; all ink
 *   inherits `currentColor` (the bubble's `--primary-foreground`) and chrome
 *   (code chips, quote bars, table borders) switches to translucent-white so
 *   it stays legible on the accent fill.
 */
export type MarkdownVariant = "default" | "onAccent"

/**
 * Extract plain text from a React element tree (for copy). Walks the tree with
 * an explicit stack rather than recursion — a React node tree can nest
 * arbitrarily (arrays of elements whose children are arrays…), and the stack
 * keeps the traversal iterative while preserving document order (children are
 * pushed in reverse so they pop left-to-right).
 */
function extractText(root: ReactNode): string {
  let out = ""
  const stack: ReactNode[] = [root]
  while (stack.length > 0) {
    const node = stack.pop()
    if (node == null || typeof node === "boolean") continue
    if (typeof node === "string" || typeof node === "number") {
      out += String(node)
    } else if (Array.isArray(node)) {
      for (let i = node.length - 1; i >= 0; i--) stack.push(node[i] as ReactNode)
    } else if (typeof node === "object" && "props" in node) {
      stack.push((node as { props: { children?: ReactNode } }).props.children)
    }
  }
  return out
}

/**
 * Inline code chip with click-to-copy.
 *
 * Clicking copies the text content and briefly flashes the border/text to
 * `--ok` green as confirmation. Clicks inside a `<pre>` (fenced code blocks)
 * are ignored — those have a dedicated CopyButton beneath them.
 */
function ClickableCode({
  baseClass,
  children,
  ...rest
}: {
  baseClass: string
  children?: ReactNode
  [k: string]: unknown
}) {
  const [copied, setCopied] = useState(false)
  return (
    <code
      className={cn(
        baseClass,
        "cursor-pointer transition-colors duration-150",
        copied && "!border-[var(--ok)] !text-[var(--ok)]",
      )}
      onClick={(ev) => {
        if ((ev.target as HTMLElement).closest("pre")) return
        const text = typeof children === "string" ? children : extractText(children)
        // `clipboard()` is honestly typed `Clipboard | undefined` (absent on an
        // insecure origin), so the `?.` guard is real — a missing clipboard is a
        // silent no-op, not a throw.
        void clipboard()
          ?.writeText(text)
          .then(
            () => {
              setCopied(true)
              window.setTimeout(() => setCopied(false), 1500)
            },
            () => {
              /* clipboard write rejected — ignore, the copy tick simply won't flash */
            },
          )
      }}
      {...rest}
    >
      {children}
    </code>
  )
}

/**
 * Convert a rendered `<table>` DOM element back to a pipe-delimited markdown
 * table. Used by the "Copy table" button so the clipboard receives the
 * markdown source — not raw HTML — matching the user's expectation for a
 * markdown-native application.
 */
function tableToMarkdown(table: HTMLTableElement): string {
  const rows = [...table.rows]
  if (rows.length === 0) return ""
  const matrix = rows.map((row) => [...row.cells].map((cell) => cell.textContent.trim()))
  const colCount = Math.max(...matrix.map((r) => r.length))
  const colWidths = Array.from({ length: colCount }, (_, i) =>
    Math.max(3, ...matrix.map((r) => (r[i] ?? "").length)),
  )
  const fmtRow = (cells: string[]) =>
    "| " +
    Array.from({ length: colCount }, (_, i) => (cells[i] ?? "").padEnd(colWidths[i] ?? 3)).join(
      " | ",
    ) +
    " |"
  const sep = "| " + colWidths.map((w) => "-".repeat(w)).join(" | ") + " |"
  const [header, ...body] = matrix
  return [fmtRow(header ?? []), sep, ...body.map((row) => fmtRow(row))].join("\n")
}

/**
 * GFM table with a "Copy table" button beneath it. Mirrors the code-block
 * pattern (`<pre>` + `<CopyButton label="Copy code">`). The table content is
 * read from the DOM on click via {@link tableToMarkdown} so the clipboard
 * receives pipe-delimited markdown, not HTML.
 *
 * Defined at module scope (stable identity) so `useRef` is safe — the
 * `components()` factory delegates to this without re-creating it each render.
 */
function CopyableTable({
  children,
  tableBorder,
  onAccent,
}: {
  children: ReactNode
  tableBorder: string
  onAccent: boolean
}) {
  const ref = useRef<HTMLTableElement>(null)
  const getText = () => (ref.current ? tableToMarkdown(ref.current) : "")
  return (
    <div className="my-2 max-w-full overflow-x-auto">
      <table
        ref={ref}
        className={cn("w-full border-collapse text-[12.5px]", "border", tableBorder)}
      >
        {children}
      </table>
      <CopyButton
        getText={getText}
        align="start"
        label="Copy table"
        className={onAccent ? "text-current/60 hover:text-current" : undefined}
      />
    </div>
  )
}

/** Build the element→component style map for a given variant. */
function components(variant: MarkdownVariant): Components {
  const onAccent = variant === "onAccent"

  // Inline code chip — themed pill on the default surface, translucent-white
  // on the accent bubble.
  const inlineCode = onAccent
    ? "rounded-[3px] bg-white/20 px-1 py-px font-mono text-[0.85em]"
    : "rounded-[3px] border border-foreground/10 bg-muted px-1 py-px font-mono text-[0.82em] text-[var(--interactive)]"

  // Block-level code fence container.
  const preBox = onAccent
    ? "my-2 overflow-x-auto rounded-lg bg-black/20 p-3 font-mono text-[12px] leading-relaxed"
    : "my-2 overflow-x-auto rounded-lg border border-border bg-muted/60 p-3 font-mono text-[12px] leading-relaxed text-foreground/90"

  const linkColor = onAccent
    ? "underline decoration-white/40 underline-offset-2 hover:decoration-white"
    : "text-[var(--interactive)] underline decoration-[var(--interactive)]/30 underline-offset-2 hover:decoration-[var(--interactive)]"

  const quoteBar = onAccent
    ? "border-white/40 text-current/85"
    : "border-[var(--signal)]/40 text-muted-foreground"

  const tableBorder = onAccent ? "border-white/25" : "border-border"
  const tableHeadBg = onAccent ? "bg-white/10" : "bg-muted/60"

  // List bullet/number ink. On the accent bubble the marker MUST inherit the
  // bubble's foreground (`currentColor` = `--primary-foreground`) — the old
  // hardcoded `text-muted-foreground/60` resolved to a near-background tint in
  // dark mode, rendering bullets and numbers white-on-white (invisible). The
  // default surface keeps the deliberately-muted marker.
  const markerColor = onAccent ? "marker:text-current" : "marker:text-muted-foreground/60"

  return {
    // ── Headings — graded scale, tight top spacing, no top margin on first. ──
    h1: ({ children }) => (
      <h1 className="mt-3 mb-1.5 text-[17px] font-semibold leading-snug first:mt-0">{children}</h1>
    ),
    h2: ({ children }) => (
      <h2 className="mt-3 mb-1.5 text-[15px] font-semibold leading-snug first:mt-0">{children}</h2>
    ),
    h3: ({ children }) => (
      <h3 className="mt-2.5 mb-1 text-[13.5px] font-semibold leading-snug first:mt-0">
        {children}
      </h3>
    ),
    h4: ({ children }) => (
      <h4 className="mt-2 mb-1 text-[12.5px] font-semibold uppercase tracking-wide opacity-80 first:mt-0">
        {children}
      </h4>
    ),
    h5: ({ children }) => (
      <h5 className="mt-2 mb-1 text-[12px] font-semibold first:mt-0">{children}</h5>
    ),
    h6: ({ children }) => (
      <h6 className="mt-2 mb-1 text-[11.5px] font-semibold opacity-70 first:mt-0">{children}</h6>
    ),

    // ── Paragraph + inline emphasis ──
    p: ({ children }) => <p className="my-1.5 leading-relaxed first:mt-0 last:mb-0">{children}</p>,
    strong: ({ children }) => (
      <strong className={cn("font-semibold", !onAccent && "text-foreground")}>{children}</strong>
    ),
    em: ({ children }) => <em className="italic">{children}</em>,
    del: ({ children }) => <del className="opacity-70">{children}</del>,
    a: ({ children, href }) => (
      <a href={href} target="_blank" rel="noopener noreferrer" className={linkColor}>
        {children}
      </a>
    ),

    // ── Lists ──
    ul: ({ children }) => (
      <ul className={cn("my-1.5 list-disc space-y-0.5 pl-5 first:mt-0 last:mb-0", markerColor)}>
        {children}
      </ul>
    ),
    ol: ({ children }) => (
      <ol className={cn("my-1.5 list-decimal space-y-0.5 pl-5 first:mt-0 last:mb-0", markerColor)}>
        {children}
      </ol>
    ),
    li: ({ children }) => <li className="leading-relaxed">{children}</li>,

    // ── Code: inline chip vs. fenced block ──
    //
    // react-markdown v10 dropped the legacy `inline` flag, so we can't branch
    // on it. Instead `code` *always* wears the inline chip, and the fenced-block
    // `pre` neutralises that chip on its direct `code` child (transparent bg, no
    // border/padding, inherit colour) — the `pre` owns the block frame. This is
    // robust regardless of whether a fence declares a language.
    code: ({ className, children, ...rest }) => {
      // Bare URLs inside backticks → render as clickable links, not code chips.
      const raw = typeof children === "string" ? children : extractText(children)
      const trimmed = raw.trim()
      if (/^https?:\/\/\S+$/.test(trimmed)) {
        return (
          <a href={trimmed} target="_blank" rel="noopener noreferrer" className={linkColor}>
            {trimmed}
          </a>
        )
      }
      return (
        <ClickableCode baseClass={cn(inlineCode, className)} {...rest}>
          {children}
        </ClickableCode>
      )
    },
    pre: ({ children }) => (
      <div>
        <pre
          className={cn(
            preBox,
            "[&>code]:border-0 [&>code]:bg-transparent [&>code]:p-0 [&>code]:text-[inherit]",
          )}
        >
          {children}
        </pre>
        <CopyButton
          text={extractText(children)}
          align="start"
          label="Copy code"
          className={onAccent ? "text-current/60 hover:text-current" : undefined}
        />
      </div>
    ),

    // ── Blockquote ──
    blockquote: ({ children }) => (
      <blockquote className={cn("my-2 border-l-2 pl-3 italic", quoteBar)}>{children}</blockquote>
    ),

    // ── Horizontal rule ──
    hr: () => (
      <hr className={cn("my-3 border-t", onAccent ? "border-white/25" : "border-border")} />
    ),

    // ── Tables (GFM) — wrapped so wide tables scroll instead of overflowing. ──
    table: ({ children }) => (
      <CopyableTable tableBorder={tableBorder} onAccent={onAccent}>
        {children}
      </CopyableTable>
    ),
    thead: ({ children }) => <thead className={tableHeadBg}>{children}</thead>,
    tr: ({ children }) => <tr className={cn("border-b last:border-0", tableBorder)}>{children}</tr>,
    th: ({ children }) => (
      <th className={cn("border px-2.5 py-1 text-left font-semibold", tableBorder)}>{children}</th>
    ),
    td: ({ children }) => (
      <td className={cn("border px-2.5 py-1 align-top", tableBorder)}>{children}</td>
    ),

    // ── GFM task-list checkbox ──
    input: ({ checked, type }) =>
      type === "checkbox" ? (
        <input
          type="checkbox"
          checked={checked}
          readOnly
          className="mr-1.5 translate-y-px accent-[var(--signal)]"
        />
      ) : null,
  }
}

// ── Pre-processor ─────────────────────────────────────────────────────
//
// The thread composer's Tab-nesting (`resolveTab` in utils.ts) emits Unicode
// bullet characters `• ◦ ▪ ‣` as depth-styled list markers. CommonMark only
// recognises `-`, `*`, `+` — so the Unicode bullets land as plain inline text
// and collapse onto a single line (the "bullets on one line" bug, T369).
//
// `normalizeMarkdown` converts those characters back to the CommonMark `- `
// marker, preserving leading whitespace (for nesting depth).  Applied BEFORE
// react-markdown so the parser sees valid list syntax.

const BULLET_RE = /^([ \t]*)[•◦▪‣][ \t]/gm

function normalizeMarkdown(text: string): string {
  return text.replaceAll(BULLET_RE, "$1- ")
}

/**
 * Render `text` as themed markdown. Memoised on `text`+`variant` so an
 * unrelated re-render of the conversation doesn't re-parse every message.
 */
export const Markdown = memo(function Markdown({
  text,
  variant = "default",
  className,
}: {
  text: string
  variant?: MarkdownVariant
  className?: string
}): ReactNode {
  return (
    <div className={cn("break-words", className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm, [remarkMath, { singleDollarTextMath: false }]]}
        rehypePlugins={[rehypeKatex]}
        components={components(variant)}
      >
        {normalizeMarkdown(text)}
      </ReactMarkdown>
    </div>
  )
})
