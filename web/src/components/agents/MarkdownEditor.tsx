import { useEffect, useRef, useState } from "react"
import {
  Bold,
  Code2,
  Heading1,
  Heading2,
  Italic,
  Link2,
  List,
  ListOrdered,
  Quote,
  Strikethrough,
} from "lucide-react"
import { cn } from "@/lib/utils"

/**
 * A lightweight **WYSIWYG markdown editor** for the prompt library (design-only).
 *
 * Rather than a raw textarea, this renders an inline-formatted editing surface
 * (a `contentEditable` region) with a sticky formatting toolbar — bold, italic,
 * headings, lists, quote, code, link. Formatting is applied live via the
 * browser's rich-text commands, so what you type is what you see.
 *
 * The initial markdown is converted to HTML once on mount; thereafter the
 * surface is uncontrolled (so the caret never jumps). When `onChange` is
 * supplied, every edit serializes the live DOM back to markdown (the inverse of
 * the seeding `mdToHtml`) and reports it, so a host can save the result. With no
 * `onChange` the surface is a pure design-only maquette (the prompt library).
 */
export function MarkdownEditor({
  initialMarkdown,
  placeholder,
  className,
  onChange,
}: {
  initialMarkdown: string
  placeholder?: string
  className?: string
  /** Fired with the current markdown after every edit (enables save-back). */
  onChange?: (markdown: string) => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  const [, force] = useState(0)
  const [empty, setEmpty] = useState(false)

  // Seed the surface once — re-seeding would fight the caret. Intentionally a
  // mount-only effect: `initialMarkdown` is deliberately NOT a dependency (a
  // prop change must not re-seed and blow away in-progress edits). The
  // exhaustive-deps warning this raises is a known, roadmapped react-hooks item
  // (P6) — the inline `eslint-disable` that used to silence it is banned by the
  // P4 anti-suppression layer, so the honest warning stands until P6 restructures.
  useEffect(() => {
    if (ref.current) {
      ref.current.innerHTML = mdToHtml(initialMarkdown)
      setEmpty(ref.current.textContent.trim().length === 0)
    }
  }, [])

  // Serialize the live DOM back to markdown and report it (when a host wants to
  // save). Cheap (a read-only walk of a small doc) and never touches the DOM, so
  // the caret is safe to call after any edit.
  const emit = () => {
    if (onChange && ref.current) onChange(htmlToMarkdown(ref.current))
  }

  const exec = (cmd: string, arg?: string) => {
    ref.current?.focus()
    document.execCommand(cmd, false, arg)
    force((n) => n + 1)
    emit()
  }

  const block = (tag: string) => exec("formatBlock", tag)

  const active = (cmd: string) => {
    try {
      return document.queryCommandState(cmd)
    } catch {
      return false
    }
  }

  return (
    <div
      className={cn(
        "flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-border bg-card card-shadow",
        className,
      )}
    >
      {/* toolbar */}
      <div className="flex shrink-0 flex-wrap items-center gap-0.5 border-b border-border bg-muted/40 px-2 py-1.5">
        <Tool icon={Bold} title="Bold" on={active("bold")} onClick={() => exec("bold")} />
        <Tool icon={Italic} title="Italic" on={active("italic")} onClick={() => exec("italic")} />
        <Tool
          icon={Strikethrough}
          title="Strikethrough"
          on={active("strikeThrough")}
          onClick={() => exec("strikeThrough")}
        />
        <Sep />
        <Tool icon={Heading1} title="Heading 1" onClick={() => block("<h1>")} />
        <Tool icon={Heading2} title="Heading 2" onClick={() => block("<h2>")} />
        <Tool icon={Quote} title="Quote" onClick={() => block("<blockquote>")} />
        <Tool icon={Code2} title="Code block" onClick={() => block("<pre>")} />
        <Sep />
        <Tool
          icon={List}
          title="Bulleted list"
          on={active("insertUnorderedList")}
          onClick={() => exec("insertUnorderedList")}
        />
        <Tool
          icon={ListOrdered}
          title="Numbered list"
          on={active("insertOrderedList")}
          onClick={() => exec("insertOrderedList")}
        />
        <Sep />
        <Tool
          icon={Link2}
          title="Link"
          onClick={() => {
            const url = window.prompt("Link URL", "https://")
            if (url) exec("createLink", url)
          }}
        />
        <span className="ml-auto pr-1 text-[10.5px] text-muted-foreground/55">
          Markdown · WYSIWYG
        </span>
      </div>

      {/* editing surface */}
      <div className="relative min-h-0 flex-1 overflow-y-auto">
        {empty && placeholder && (
          <span className="pointer-events-none absolute left-5 top-4 text-[13px] text-muted-foreground/40">
            {placeholder}
          </span>
        )}
        <div
          ref={ref}
          contentEditable
          suppressContentEditableWarning
          spellCheck={false}
          onInput={() => {
            setEmpty(ref.current?.textContent.trim().length === 0)
            emit()
          }}
          onKeyUp={() => force((n) => n + 1)}
          onMouseUp={() => force((n) => n + 1)}
          className={cn(
            "prose-editor min-h-full px-5 py-4 text-[13.5px] leading-relaxed text-foreground/90 outline-none",
            "[&_h1]:mb-2 [&_h1]:mt-3 [&_h1]:text-[20px] [&_h1]:font-bold [&_h1]:tracking-tight [&_h1]:text-foreground",
            "[&_h2]:mb-1.5 [&_h2]:mt-3 [&_h2]:text-[16px] [&_h2]:font-semibold [&_h2]:text-foreground",
            "[&_p]:my-1.5",
            "[&_ul]:my-1.5 [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:my-1.5 [&_ol]:list-decimal [&_ol]:pl-5 [&_li]:my-0.5",
            "[&_blockquote]:my-2 [&_blockquote]:border-l-2 [&_blockquote]:border-[var(--signal)]/60 [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground",
            "[&_pre]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-lg [&_pre]:bg-[var(--surface-2)]/60 [&_pre]:px-3 [&_pre]:py-2 [&_pre]:font-mono [&_pre]:text-[12px]",
            "[&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_code]:font-mono [&_code]:text-[12.5px] [&_code]:text-[var(--signal)]",
            "[&_a]:text-[var(--interactive)] [&_a]:underline [&_a]:underline-offset-2",
            "[&_strong]:font-semibold [&_strong]:text-foreground",
          )}
        />
      </div>
    </div>
  )
}

function Tool({
  icon: Icon,
  title,
  on,
  onClick,
}: {
  icon: typeof Bold
  title: string
  on?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      title={title}
      // keep selection in the editor while clicking the toolbar
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className={cn(
        "flex size-7 items-center justify-center rounded-md transition-colors",
        on
          ? "bg-[var(--signal)]/15 text-[var(--signal)]"
          : "text-muted-foreground/80 hover:bg-muted/70 hover:text-foreground",
      )}
    >
      <Icon className="size-[15px]" />
    </button>
  )
}

function Sep() {
  return <span className="mx-1 h-4 w-px bg-border" />
}

/** Minimal markdown → HTML for seeding the editable surface (design-only). */
function mdToHtml(md: string): string {
  const lines = md.replace(/\r/g, "").split("\n")
  const out: string[] = []
  let list: "ul" | "ol" | null = null
  let fence: string[] | null = null

  const closeList = () => {
    if (list) {
      out.push(`</${list}>`)
      list = null
    }
  }

  for (const raw of lines) {
    const line = raw

    if (line.trim().startsWith("```")) {
      if (fence) {
        out.push(`<pre>${esc(fence.join("\n"))}</pre>`)
        fence = null
      } else {
        closeList()
        fence = []
      }
      continue
    }
    if (fence) {
      fence.push(line)
      continue
    }

    if (line.startsWith("## ")) {
      closeList()
      out.push(`<h2>${inline(line.slice(3))}</h2>`)
    } else if (line.startsWith("# ")) {
      closeList()
      out.push(`<h1>${inline(line.slice(2))}</h1>`)
    } else if (line.startsWith("> ")) {
      closeList()
      out.push(`<blockquote>${inline(line.slice(2))}</blockquote>`)
    } else if (/^[-*] /.test(line)) {
      if (list !== "ul") {
        closeList()
        out.push("<ul>")
        list = "ul"
      }
      out.push(`<li>${inline(line.slice(2))}</li>`)
    } else if (/^\d+\. /.test(line)) {
      if (list !== "ol") {
        closeList()
        out.push("<ol>")
        list = "ol"
      }
      out.push(`<li>${inline(line.replace(/^\d+\. /, ""))}</li>`)
    } else if (line.trim() === "") {
      closeList()
    } else {
      closeList()
      out.push(`<p>${inline(line)}</p>`)
    }
  }
  if (fence) out.push(`<pre>${esc(fence.join("\n"))}</pre>`)
  closeList()
  return out.join("")
}

function inline(s: string): string {
  return esc(s)
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/`([^`]+)`/g, "<code>$1</code>")
}

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
}

/**
 * Serialize the editable surface's DOM back to markdown — the inverse of
 * {@link mdToHtml}. The tag set is bounded by what the toolbar + seeding produce
 * (h1/h2, p, blockquote, pre, ul/ol/li, and the inline strong·em·code·link·del),
 * so a focused walk covers every case; an unrecognized element degrades to its
 * inline text. Block elements are separated by a blank line so the result
 * re-parses cleanly. `execCommand` emits either semantic (`<strong>`) or
 * presentational (`<b>`) tags depending on the browser — both are handled.
 */
function htmlToMarkdown(root: HTMLElement): string {
  const blocks: string[] = []
  for (const node of Array.from(root.childNodes)) {
    const md = serializeBlock(node)
    if (md.trim().length > 0) blocks.push(md)
  }
  return (
    blocks
      .join("\n\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim() + "\n"
  )
}

/** Serialize one top-level (block) node to its markdown line(s). */
function serializeBlock(node: ChildNode): string {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? ""
  if (!(node instanceof HTMLElement)) return ""

  const tag = node.tagName.toLowerCase()
  switch (tag) {
    case "h1":
      return `# ${serializeInline(node)}`
    case "h2":
      return `## ${serializeInline(node)}`
    case "h3":
      return `### ${serializeInline(node)}`
    case "blockquote":
      return serializeInline(node)
        .split("\n")
        .map((l) => `> ${l}`)
        .join("\n")
    case "pre":
      return `\`\`\`\n${node.textContent}\n\`\`\``
    case "ul":
      return listItems(node)
        .map((li) => `- ${serializeInline(li)}`)
        .join("\n")
    case "ol":
      return listItems(node)
        .map((li, i) => `${i + 1}. ${serializeInline(li)}`)
        .join("\n")
    case "br":
      return ""
    default:
      // p / div / unknown block → its inline content as a paragraph.
      return serializeInline(node)
  }
}

/** The `<li>` children of a list element. */
function listItems(list: HTMLElement): HTMLElement[] {
  return Array.from(list.children).filter(
    (c): c is HTMLElement => c instanceof HTMLElement && c.tagName.toLowerCase() === "li",
  )
}

/** Serialize an element's inline content (recursively) to markdown. */
function serializeInline(el: Node): string {
  let out = ""
  for (const node of Array.from(el.childNodes)) {
    if (node.nodeType === Node.TEXT_NODE) {
      out += node.textContent ?? ""
      continue
    }
    if (!(node instanceof HTMLElement)) continue
    const tag = node.tagName.toLowerCase()
    const inner = serializeInline(node)
    switch (tag) {
      case "strong":
      case "b":
        out += `**${inner}**`
        break
      case "em":
      case "i":
        out += `*${inner}*`
        break
      case "code":
        out += `\`${inner}\``
        break
      case "del":
      case "s":
      case "strike":
        out += `~~${inner}~~`
        break
      case "a": {
        const href = node.getAttribute("href") ?? ""
        out += href ? `[${inner}](${href})` : inner
        break
      }
      case "br":
        out += "\n"
        break
      default:
        out += inner
    }
  }
  return out
}
