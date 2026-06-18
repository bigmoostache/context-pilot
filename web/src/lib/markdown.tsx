import { memo, type ReactNode } from "react"
import ReactMarkdown, { type Components } from "react-markdown"
import remarkGfm from "remark-gfm"

import { cn } from "@/lib/utils"

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

  return {
    // ── Headings — graded scale, tight top spacing, no top margin on first. ──
    h1: ({ children }) => (
      <h1 className="mt-3 mb-1.5 text-[17px] font-semibold leading-snug first:mt-0">{children}</h1>
    ),
    h2: ({ children }) => (
      <h2 className="mt-3 mb-1.5 text-[15px] font-semibold leading-snug first:mt-0">{children}</h2>
    ),
    h3: ({ children }) => (
      <h3 className="mt-2.5 mb-1 text-[13.5px] font-semibold leading-snug first:mt-0">{children}</h3>
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
      <ul className="my-1.5 list-disc space-y-0.5 pl-5 marker:text-muted-foreground/60 first:mt-0 last:mb-0">
        {children}
      </ul>
    ),
    ol: ({ children }) => (
      <ol className="my-1.5 list-decimal space-y-0.5 pl-5 marker:text-muted-foreground/60 first:mt-0 last:mb-0">
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
    code: ({ className, children, ...rest }) => (
      <code className={cn(inlineCode, className)} {...rest}>
        {children}
      </code>
    ),
    pre: ({ children }) => (
      <pre
        className={cn(
          preBox,
          "[&>code]:border-0 [&>code]:bg-transparent [&>code]:p-0 [&>code]:text-[inherit]",
        )}
      >
        {children}
      </pre>
    ),

    // ── Blockquote ──
    blockquote: ({ children }) => (
      <blockquote className={cn("my-2 border-l-2 pl-3 italic", quoteBar)}>{children}</blockquote>
    ),

    // ── Horizontal rule ──
    hr: () => <hr className={cn("my-3 border-t", onAccent ? "border-white/25" : "border-border")} />,

    // ── Tables (GFM) — wrapped so wide tables scroll instead of overflowing. ──
    table: ({ children }) => (
      <div className="my-2 max-w-full overflow-x-auto">
        <table className={cn("w-full border-collapse text-[12.5px]", "border", tableBorder)}>
          {children}
        </table>
      </div>
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
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components(variant)}>
        {text}
      </ReactMarkdown>
    </div>
  )
})
