import { Download, Share2, X } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { fmtBytes } from "@/lib/finderFs"
import { kindMeta, kindTint } from "./kind"
import { cn } from "@/lib/utils"

/**
 * QuickLook preview pane — the Finder's centerpiece. Renders a rich, kind-aware
 * preview of the selected file: code, markdown, JSON, spreadsheets, slide decks,
 * PDFs and images each get a bespoke, beautiful treatment.
 */
export function FinderPreview({
  node,
  onClose,
}: {
  node: FinderNode | null
  onClose: () => void
}) {
  return (
    <aside className="flex w-[420px] shrink-0 flex-col border-l border-border bg-surface">
      <div className="flex h-11 shrink-0 items-center gap-2 border-b border-border px-3">
        <span className="text-[12px] font-semibold text-muted-foreground">Preview</span>
        <div className="ml-auto flex items-center gap-1">
          {node && node.kind !== "folder" && (
            <>
              <IconBtn icon={Download} title="Download" />
              <IconBtn icon={Share2} title="Share" />
            </>
          )}
          <IconBtn icon={X} title="Close" onClick={onClose} />
        </div>
      </div>

      {!node ? (
        <Empty />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="min-h-0 flex-1 overflow-auto p-4">
            <Body node={node} />
          </div>
          <Meta node={node} />
        </div>
      )}
    </aside>
  )
}

function IconBtn({
  icon: Icon,
  title,
  onClick,
}: {
  icon: typeof X
  title: string
  onClick?: () => void
}) {
  return (
    <button
      title={title}
      onClick={onClick}
      className="flex size-7 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted/70 hover:text-foreground"
    >
      <Icon className="size-3.5" />
    </button>
  )
}

function Body({ node }: { node: FinderNode }) {
  if (node.kind === "folder") return <FolderPreview node={node} />
  if (node.code) return <CodePreview lang={node.code.lang} lines={node.code.lines} />
  if (node.sheet) return <SheetPreview sheet={node.sheet} />
  if (node.slides) return <SlidesPreview slides={node.slides} />
  if (node.pdf) return <PdfPreview pdf={node.pdf} />
  if (node.image) return <ImagePreview image={node.image} />
  if (node.text) return <TextPreview kind={node.kind} text={node.text} />
  return <Generic node={node} />
}

// ── code ──────────────────────────────────────────────────────────
const KEYWORDS = new Set([
  "pub", "fn", "let", "mut", "if", "else", "for", "match", "struct", "enum",
  "impl", "use", "return", "const", "import", "export", "function", "interface",
  "type", "from", "members", "name", "edition",
])

function CodePreview({ lang, lines }: { lang: string; lines: string[] }) {
  return (
    <div className="overflow-hidden rounded-lg border border-border bg-card card-shadow">
      <div className="flex items-center gap-2 border-b border-border bg-muted/50 px-3 py-1.5">
        <span className="flex gap-1.5">
          <span className="size-2.5 rounded-full bg-[#ff5f57]" />
          <span className="size-2.5 rounded-full bg-[#febc2e]" />
          <span className="size-2.5 rounded-full bg-[#28c840]" />
        </span>
        <span className="ml-1 font-mono text-[10.5px] uppercase tracking-wide text-muted-foreground">
          {lang}
        </span>
      </div>
      <pre className="overflow-x-auto px-3 py-2.5 font-mono text-[11px] leading-relaxed">
        {lines.map((line, i) => (
          <div key={i} className="flex gap-3">
            <span className="w-6 shrink-0 select-none text-right text-muted-foreground/35">
              {i + 1}
            </span>
            <code className="whitespace-pre text-foreground/85">{tint(line)}</code>
          </div>
        ))}
      </pre>
    </div>
  )
}

/** Cheap, decorative keyword tinting — not a real tokenizer. */
function tint(line: string) {
  return line.split(/(\s+|[(){}<>;:,])/).map((tok, i) => {
    if (KEYWORDS.has(tok)) return <span key={i} style={{ color: "var(--signal)" }}>{tok}</span>
    if (/^".*"$/.test(tok) || /^'.*'$/.test(tok))
      return <span key={i} style={{ color: "var(--ok)" }}>{tok}</span>
    if (/^\d+$/.test(tok)) return <span key={i} style={{ color: "var(--interactive)" }}>{tok}</span>
    return <span key={i}>{tok}</span>
  })
}

// ── spreadsheet ───────────────────────────────────────────────────
function SheetPreview({ sheet }: { sheet: NonNullable<FinderNode["sheet"]> }) {
  const colLetter = (i: number) => String.fromCharCode(65 + i)
  return (
    <div className="overflow-hidden rounded-lg border border-border card-shadow">
      <table className="w-full border-collapse text-[11px]">
        <thead>
          <tr>
            <th className="w-7 border border-border bg-muted/60 px-1 py-1 text-muted-foreground/50" />
            {sheet.columns.map((_, i) => (
              <th
                key={i}
                className="border border-border bg-muted/60 px-2 py-1 text-center font-medium text-muted-foreground/70"
              >
                {colLetter(i)}
              </th>
            ))}
          </tr>
          <tr>
            <th className="border border-border bg-muted/40 px-1 py-1 text-center text-[10px] text-muted-foreground/50">
              1
            </th>
            {sheet.columns.map((c, i) => (
              <th
                key={i}
                className="border border-border bg-[var(--ok)]/8 px-2 py-1.5 text-left font-semibold text-foreground/85"
              >
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {sheet.rows.map((row, r) => (
            <tr key={r} className="hover:bg-muted/30">
              <td className="border border-border bg-muted/40 px-1 py-1 text-center text-[10px] text-muted-foreground/50">
                {r + 2}
              </td>
              {row.map((cell, c) => (
                <td
                  key={c}
                  className={cn(
                    "border border-border px-2 py-1 tabular-nums text-foreground/80",
                    c === 0 && "font-medium",
                  )}
                >
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

// ── slides ────────────────────────────────────────────────────────
function SlidesPreview({ slides }: { slides: NonNullable<FinderNode["slides"]> }) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex aspect-video flex-col justify-center gap-2 rounded-xl border border-border bg-gradient-to-br from-[var(--surface-2)] to-card p-5 card-shadow">
        <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--signal)]">
          Slide 1
        </span>
        <h3 className="text-[19px] font-semibold tracking-tight text-foreground">
          {slides[0]?.title}
        </h3>
        <ul className="mt-1 flex flex-col gap-1">
          {slides[0]?.bullets.map((b, i) => (
            <li key={i} className="flex items-start gap-2 text-[12px] text-muted-foreground">
              <span className="mt-1.5 size-1 rounded-full bg-[var(--signal)]" />
              {b}
            </li>
          ))}
        </ul>
      </div>
      <div className="grid grid-cols-3 gap-2">
        {slides.map((s, i) => (
          <div
            key={i}
            className={cn(
              "flex aspect-video flex-col gap-0.5 rounded-md border p-2 text-[8px]",
              i === 0 ? "border-[var(--signal)]/60 bg-card" : "border-border bg-muted/40",
            )}
          >
            <span className="font-semibold leading-tight text-foreground/80 line-clamp-2">
              {s.title}
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}

// ── pdf ───────────────────────────────────────────────────────────
function PdfPreview({ pdf }: { pdf: NonNullable<FinderNode["pdf"]> }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="mx-auto flex aspect-[1/1.3] w-[78%] flex-col gap-3 rounded-md border border-border bg-white px-5 py-6 shadow-lg">
        <span className="text-[9px] uppercase tracking-widest text-neutral-400">
          Specification
        </span>
        <h3 className="text-[14px] font-bold leading-snug text-neutral-800">{pdf.title}</h3>
        <div className="mt-1 flex flex-col gap-2">
          {pdf.excerpt.map((line, i) => (
            <p key={i} className="text-[9.5px] leading-relaxed text-neutral-600">
              {line}
            </p>
          ))}
        </div>
        <div className="mt-auto h-px bg-neutral-200" />
        <span className="text-center text-[8px] text-neutral-400">Page 1 of {pdf.pages}</span>
      </div>
      <span className="text-center text-[11px] text-muted-foreground">{pdf.pages} pages · PDF</span>
    </div>
  )
}

// ── image ─────────────────────────────────────────────────────────
function ImagePreview({ image }: { image: NonNullable<FinderNode["image"]> }) {
  return (
    <div className="flex flex-col gap-2">
      <div
        className="aspect-video w-full rounded-lg border border-border card-shadow"
        style={{ background: image.gradient }}
      />
      <span className="text-center font-mono text-[11px] text-muted-foreground">
        {image.w} × {image.h}
      </span>
    </div>
  )
}

// ── text / markdown / json ────────────────────────────────────────
function TextPreview({ kind, text }: { kind: FinderNode["kind"]; text: string }) {
  const mono = kind === "json"
  return (
    <div className="overflow-hidden rounded-lg border border-border bg-card p-3.5 card-shadow">
      <pre
        className={cn(
          "whitespace-pre-wrap break-words text-[11.5px] leading-relaxed text-foreground/85",
          mono ? "font-mono" : "font-sans",
        )}
      >
        {text}
      </pre>
    </div>
  )
}

// ── folder ────────────────────────────────────────────────────────
function FolderPreview({ node }: { node: FinderNode }) {
  const kids = node.children ?? []
  const folders = kids.filter((k) => k.kind === "folder").length
  const files = kids.length - folders
  return (
    <div className="flex flex-col items-center gap-3 py-6 text-center">
      <span
        className="flex size-16 items-center justify-center rounded-2xl"
        style={{ background: kindTint("folder", 16), color: kindMeta.folder.accent }}
      >
        <kindMeta.folder.icon className="size-8" />
      </span>
      <div className="flex flex-col gap-0.5">
        <span className="text-[14px] font-semibold text-foreground/90">{node.name}</span>
        <span className="text-[12px] text-muted-foreground">
          {folders} folder{folders === 1 ? "" : "s"} · {files} file{files === 1 ? "" : "s"}
        </span>
      </div>
    </div>
  )
}

function Generic({ node }: { node: FinderNode }) {
  const Meta_ = kindMeta[node.kind]
  return (
    <div className="flex flex-col items-center gap-3 py-8 text-center">
      <span
        className="flex size-16 items-center justify-center rounded-2xl"
        style={{ background: kindTint(node.kind, 16), color: Meta_.accent }}
      >
        <Meta_.icon className="size-8" />
      </span>
      <span className="text-[13px] text-muted-foreground">No preview available</span>
    </div>
  )
}

// ── metadata footer ───────────────────────────────────────────────
function Meta({ node }: { node: FinderNode }) {
  return (
    <div className="shrink-0 border-t border-border bg-card/60 px-4 py-3">
      <div className="mb-2 flex items-center gap-2">
        <span
          className="flex size-7 items-center justify-center rounded-md"
          style={{ background: kindTint(node.kind), color: kindMeta[node.kind].accent }}
        >
          {(() => {
            const I = kindMeta[node.kind].icon
            return <I className="size-4" />
          })()}
        </span>
        <span className="truncate text-[12.5px] font-medium text-foreground/90">{node.name}</span>
      </div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[11px]">
        <Row k="Kind" v={kindMeta[node.kind].label} />
        <Row k="Size" v={fmtBytes(node.size)} />
        <Row k="Modified" v={node.modified} />
        <Row k="Where" v={node.path} mono />
      </dl>
    </div>
  )
}

function Row({ k, v, mono }: { k: string; v: string; mono?: boolean }) {
  return (
    <>
      <dt className="text-muted-foreground">{k}</dt>
      <dd className={cn("truncate text-right text-foreground/80", mono && "font-mono text-[10.5px]")}>
        {v}
      </dd>
    </>
  )
}

function Empty() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
      <span className="text-[12.5px] text-muted-foreground/60">
        Select a file to preview it here.
      </span>
    </div>
  )
}
