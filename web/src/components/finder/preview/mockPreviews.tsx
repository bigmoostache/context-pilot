import { useState } from "react"
import { Check, Copy, Pause, Play } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { cn } from "@/lib/utils"

// ── code (mock) ───────────────────────────────────────────────────
const KEYWORDS = new Set([
  "pub", "fn", "let", "mut", "if", "else", "for", "match", "struct", "enum",
  "impl", "use", "return", "const", "import", "export", "function", "interface",
  "type", "from", "members", "name", "edition",
])

export function CodePreview({ lang, lines }: { lang: string; lines: string[] }) {
  const [copied, setCopied] = useState(false)
  const copy = () => {
    navigator.clipboard?.writeText(lines.join("\n")).catch(() => {})
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1400)
  }
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
        <button
          onClick={copy}
          className="ml-auto flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[10.5px] text-muted-foreground/80 transition-colors hover:bg-muted hover:text-foreground"
        >
          {copied ? <Check className="size-3 text-[var(--ok)]" /> : <Copy className="size-3" />}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="overflow-x-auto px-3 py-2.5 font-mono text-[11px] leading-relaxed">
        {lines.map((line, i) => (
          <div key={i} className="group flex gap-3 rounded hover:bg-[var(--signal)]/6">
            <span className="w-6 shrink-0 select-none text-right text-muted-foreground/35">{i + 1}</span>
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

// ── spreadsheet (mock) ────────────────────────────────────────────
export function SheetPreview({ sheet }: { sheet: NonNullable<FinderNode["sheet"]> }) {
  const colLetter = (i: number) => String.fromCharCode(65 + i)
  return (
    <div className="flex flex-col overflow-hidden rounded-lg border border-border card-shadow">
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-[11px]">
          <thead className="sticky top-0">
            <tr>
              <th className="w-7 border border-border bg-muted/70 px-1 py-1 text-muted-foreground/50" />
              {sheet.columns.map((_, i) => (
                <th
                  key={i}
                  className="border border-border bg-muted/70 px-2 py-1 text-center font-medium text-muted-foreground/70"
                >
                  {colLetter(i)}
                </th>
              ))}
            </tr>
            <tr>
              <th className="border border-border bg-muted/50 px-1 py-1 text-center text-[10px] text-muted-foreground/50">
                1
              </th>
              {sheet.columns.map((c, i) => (
                <th
                  key={i}
                  className="border border-border bg-[var(--ok)]/10 px-2 py-1.5 text-left font-semibold text-foreground/85"
                >
                  {c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sheet.rows.map((row, r) => (
              <tr key={r} className={cn(r % 2 === 1 && "bg-muted/20", "hover:bg-[var(--signal)]/8")}>
                <td className="border border-border bg-muted/50 px-1 py-1 text-center text-[10px] text-muted-foreground/50">
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
      {/* sheet tabs */}
      <div className="flex items-center gap-1 border-t border-border bg-muted/40 px-2 py-1">
        <span className="rounded-t-md border-x border-t border-border bg-card px-2.5 py-0.5 text-[10.5px] font-medium text-foreground/80">
          Sheet 1
        </span>
        <span className="px-2.5 py-0.5 text-[10.5px] text-muted-foreground/60">Sheet 2</span>
        <span className="px-2.5 py-0.5 text-[10.5px] text-muted-foreground/40">+</span>
      </div>
    </div>
  )
}

// ── slides (mock) ─────────────────────────────────────────────────
export function SlidesPreview({ slides }: { slides: NonNullable<FinderNode["slides"]> }) {
  const [active, setActive] = useState(0)
  const cur = slides[active]
  return (
    <div className="flex flex-col gap-3">
      <div className="flex aspect-video flex-col justify-center gap-2 rounded-xl border border-border bg-gradient-to-br from-[var(--surface-2)] to-card p-5 card-shadow">
        <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--signal)]">
          Slide {active + 1} / {slides.length}
        </span>
        <h3 className="text-[18px] font-semibold tracking-tight text-foreground">{cur?.title}</h3>
        <ul className="mt-1 flex flex-col gap-1">
          {cur?.bullets.map((b, i) => (
            <li key={i} className="flex items-start gap-2 text-[12px] text-muted-foreground">
              <span className="mt-1.5 size-1 rounded-full bg-[var(--signal)]" />
              {b}
            </li>
          ))}
        </ul>
      </div>
      <div className="grid grid-cols-4 gap-2">
        {slides.map((s, i) => (
          <button
            key={i}
            onClick={() => setActive(i)}
            className={cn(
              "flex aspect-video flex-col gap-0.5 rounded-md border p-1.5 text-left text-[8px] transition-colors",
              i === active ? "border-[var(--signal)]/70 bg-card" : "border-border bg-muted/40 hover:border-border",
            )}
          >
            <span className="font-semibold leading-tight text-foreground/80 line-clamp-3">{s.title}</span>
          </button>
        ))}
      </div>
    </div>
  )
}

// ── pdf (mock) ────────────────────────────────────────────────────
export function PdfPreview({ pdf }: { pdf: NonNullable<FinderNode["pdf"]> }) {
  return (
    <div className="flex flex-col gap-3">
      <div className="mx-auto flex aspect-[1/1.3] w-[78%] flex-col gap-3 rounded-md border border-border bg-white px-5 py-6 shadow-lg">
        <span className="text-[9px] uppercase tracking-widest text-neutral-400">Specification</span>
        <h3 className="text-[14px] font-bold leading-snug text-neutral-800">{pdf.title}</h3>
        <div className="mt-1 flex flex-col gap-2">
          {pdf.excerpt.map((line, i) => (
            <p key={i} className="text-[9.5px] leading-relaxed text-neutral-600">{line}</p>
          ))}
        </div>
        <div className="mt-auto h-px bg-neutral-200" />
        <span className="text-center text-[8px] text-neutral-400">Page 1 of {pdf.pages}</span>
      </div>
      {/* page thumbnail strip */}
      <div className="flex items-center justify-center gap-1.5">
        {Array.from({ length: Math.min(pdf.pages, 8) }, (_, i) => (
          <span
            key={i}
            className={cn(
              "h-7 w-[22px] rounded-[2px] border",
              i === 0 ? "border-[var(--signal)]/70 bg-white" : "border-border bg-muted/50",
            )}
          />
        ))}
        {pdf.pages > 8 && <span className="text-[10px] text-muted-foreground/60">+{pdf.pages - 8}</span>}
      </div>
      <span className="text-center text-[11px] text-muted-foreground">{pdf.pages} pages · PDF</span>
    </div>
  )
}

// ── image (mock) ──────────────────────────────────────────────────
export function ImagePreview({ image }: { image: NonNullable<FinderNode["image"]> }) {
  const [zoom, setZoom] = useState(100)
  return (
    <div className="flex flex-col gap-2">
      <div className="checker overflow-hidden rounded-lg border border-border">
        <div className="flex items-center justify-center p-4">
          <div
            className="aspect-video w-full rounded-md card-shadow transition-transform"
            style={{ background: image.gradient, transform: `scale(${zoom / 100})` }}
          />
        </div>
      </div>
      <div className="flex items-center gap-2">
        <span className="font-mono text-[11px] text-muted-foreground">{image.w} × {image.h}</span>
        <div className="ml-auto flex items-center gap-1.5">
          <button
            onClick={() => setZoom((z) => Math.max(50, z - 25))}
            className="flex size-5 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground"
          >
            −
          </button>
          <span className="w-9 text-center text-[11px] tabular-nums text-muted-foreground">{zoom}%</span>
          <button
            onClick={() => setZoom((z) => Math.min(200, z + 25))}
            className="flex size-5 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground"
          >
            +
          </button>
        </div>
      </div>
    </div>
  )
}

// ── audio (mock) ──────────────────────────────────────────────────
export function AudioPreview({ media }: { media: NonNullable<FinderNode["media"]> }) {
  const [playing, setPlaying] = useState(false)
  const peaks = media.peaks ?? []
  return (
    <div className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4 card-shadow">
      <div className="flex h-20 items-center justify-center gap-[2px]">
        {peaks.map((p, i) => (
          <span
            key={i}
            className="w-[3px] rounded-full"
            style={{
              height: `${Math.max(8, p * 100)}%`,
              background: i < peaks.length * 0.35 ? "var(--signal)" : "color-mix(in oklab, var(--muted-foreground) 50%, transparent)",
            }}
          />
        ))}
      </div>
      <div className="flex items-center gap-3">
        <button
          onClick={() => setPlaying((p) => !p)}
          className="flex size-9 items-center justify-center rounded-full bg-[var(--signal)] text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
        >
          {playing ? <Pause className="size-4" /> : <Play className="size-4 translate-x-0.5" />}
        </button>
        <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
          <div className="h-full w-[35%] rounded-full bg-[var(--signal)]" />
        </div>
        <span className="font-mono text-[11px] tabular-nums text-muted-foreground">{media.duration}</span>
      </div>
    </div>
  )
}

// ── video (mock) ──────────────────────────────────────────────────
export function VideoPreview({ media }: { media: NonNullable<FinderNode["media"]> }) {
  return (
    <div className="flex flex-col gap-2">
      <div
        className="relative flex aspect-video w-full items-center justify-center rounded-lg border border-border card-shadow"
        style={{ background: media.poster ?? "var(--surface-2)" }}
      >
        <button className="flex size-14 items-center justify-center rounded-full bg-black/45 text-white backdrop-blur transition-transform hover:scale-105">
          <Play className="size-6 translate-x-0.5" />
        </button>
        <span className="absolute bottom-2 right-2 rounded bg-black/55 px-1.5 py-0.5 font-mono text-[10px] text-white">
          {media.duration}
        </span>
      </div>
      <div className="flex items-center gap-2">
        <Play className="size-3.5 text-muted-foreground" />
        <div className="h-1 flex-1 overflow-hidden rounded-full bg-muted">
          <div className="h-full w-[15%] rounded-full bg-[var(--signal)]" />
        </div>
        <span className="font-mono text-[10.5px] text-muted-foreground">0:20 / {media.duration}</span>
      </div>
    </div>
  )
}
