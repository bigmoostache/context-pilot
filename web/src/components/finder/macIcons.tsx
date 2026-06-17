import { useId } from "react"
import type { FinderKind } from "@/lib/types"

/**
 * macOS-faithful file & folder icons (T27).
 *
 * The user asked for the *exact* icons macOS Finder uses. Apple's real artwork
 * is proprietary and can't be redistributed, so this is the next best, fully
 * legal thing: hand-built SVGs that reproduce the macOS Big Sur / Sonoma icon
 * language pixel-for-feel —
 *   • the blue gradient **folder** (back tab + front body + top sheen), and
 *   • the white **document** sheet with a folded top-right corner and a colored
 *     uppercase **type tag** (PDF, JS, MD…), plus small QuickLook-style art for
 *     images / audio / video.
 *
 * Everything is a single self-contained `<FileIcon kind ext size />` — full
 * color, no external tint or gradient chip required. Drawn in a 28×28 viewBox
 * and scaled by `size`, so the same component serves the 14px tab glyph and the
 * 128px gallery hero identically. Gradient ids are `useId`-scoped to avoid
 * collisions when many icons render at once.
 */

// ── language / extension → tag color ──────────────────────────────
// macOS tints a typed document by its app/UTI; we approximate with a compact
// map keyed on the common extensions, falling back to the kind's tag color.
const EXT_COLOR: Record<string, string> = {
  ts: "#2F74C0",
  tsx: "#2F74C0",
  js: "#E8A33D",
  jsx: "#E8A33D",
  rs: "#D2691E",
  py: "#3B72A6",
  go: "#00ADD8",
  rb: "#CC342D",
  c: "#5C6BC0",
  cpp: "#5C6BC0",
  h: "#5C6BC0",
  java: "#E76F00",
  swift: "#FF6F47",
  sh: "#4E5D6C",
  css: "#2F74C0",
  html: "#E8662B",
}

// ── kind → tag color + default extension label + glyph hint ───────
type Glyph = "none" | "photo" | "note" | "play" | "zip"
const KIND_TAG: Record<FinderKind, { color: string; ext: string; glyph: Glyph }> = {
  folder: { color: "#1577E0", ext: "", glyph: "none" },
  code: { color: "#2F74C0", ext: "CODE", glyph: "none" },
  doc: { color: "#5B95E0", ext: "DOC", glyph: "none" },
  pdf: { color: "#F0453A", ext: "PDF", glyph: "none" },
  sheet: { color: "#2FB457", ext: "XLS", glyph: "none" },
  slides: { color: "#FF9F0A", ext: "KEY", glyph: "none" },
  image: { color: "#34A4D0", ext: "IMG", glyph: "photo" },
  markdown: { color: "#0A84FF", ext: "MD", glyph: "none" },
  json: { color: "#14B8A6", ext: "JSON", glyph: "none" },
  archive: { color: "#8E8E93", ext: "ZIP", glyph: "zip" },
  audio: { color: "#FF2D55", ext: "AAC", glyph: "note" },
  video: { color: "#4B7BEC", ext: "MP4", glyph: "play" },
  binary: { color: "#8E8E93", ext: "BIN", glyph: "none" },
}

/** Public icon — folder or typed document, full macOS-style color. */
export function FileIcon({
  kind,
  ext,
  size = 28,
  className,
}: {
  kind: FinderKind
  /** real file extension (overrides the kind's default tag text) */
  ext?: string
  size?: number
  className?: string
}) {
  if (kind === "folder") return <MacFolder size={size} className={className} />
  return <MacDocument kind={kind} ext={ext} size={size} className={className} />
}

// ── Folder ────────────────────────────────────────────────────────
function MacFolder({ size, className }: { size: number; className?: string }) {
  const id = useId().replace(/:/g, "")
  return (
    <svg width={size} height={size} viewBox="0 0 28 28" className={className} aria-hidden>
      <defs>
        <linearGradient id={`fb${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#2B93EE" />
          <stop offset="1" stopColor="#1471DA" />
        </linearGradient>
        <linearGradient id={`ff${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#54AEF8" />
          <stop offset="1" stopColor="#1E84EC" />
        </linearGradient>
      </defs>
      {/* back panel + raised left tab */}
      <path
        d="M3 8.5c0-1.1.9-2 2-2h5.2c.6 0 1.1.25 1.5.7l1.3 1.5c.38.43.93.68 1.5.68H23c1.1 0 2 .9 2 2v9.4c0 1.1-.9 2-2 2H5c-1.1 0-2-.9-2-2V8.5Z"
        fill={`url(#fb${id})`}
      />
      {/* front flap */}
      <path
        d="M3 11.4c0-1.1.9-2 2-2h18c1.1 0 2 .9 2 2v8.4c0 1.1-.9 2-2 2H5c-1.1 0-2-.9-2-2v-8.4Z"
        fill={`url(#ff${id})`}
      />
      {/* top sheen */}
      <path
        d="M5 10.4h18c.74 0 1.4.4 1.74.99C24.2 11 23.6 10.9 23 10.9H5c-.6 0-1.2.1-1.74.49.35-.59 1-.99 1.74-.99Z"
        fill="#FFFFFF"
        opacity="0.45"
      />
    </svg>
  )
}

// ── Document ──────────────────────────────────────────────────────
function MacDocument({
  kind,
  ext,
  size,
  className,
}: {
  kind: FinderKind
  ext?: string
  size: number
  className?: string
}) {
  const id = useId().replace(/:/g, "")
  const meta = KIND_TAG[kind]
  const tagColor = (ext && EXT_COLOR[ext.toLowerCase()]) || meta.color
  const label = (ext ? ext.toUpperCase() : meta.ext).slice(0, 4)

  return (
    <svg width={size} height={size} viewBox="0 0 28 28" className={className} aria-hidden>
      <defs>
        <linearGradient id={`sh${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#FFFFFF" />
          <stop offset="1" stopColor="#EFF1F4" />
        </linearGradient>
      </defs>
      {/* sheet with folded top-right corner */}
      <path
        d="M6.5 3.6c0-.55.45-1 1-1h9.3l5.2 5.2v16.6c0 .55-.45 1-1 1H7.5c-.55 0-1-.45-1-1V3.6Z"
        fill={`url(#sh${id})`}
        stroke="#D5D9E0"
        strokeWidth="0.7"
      />
      {/* fold underside */}
      <path d="M16.8 2.6l5.2 5.2h-4.2c-.55 0-1-.45-1-1V2.6Z" fill="#DCE0E7" />
      <path d="M16.8 2.6l5.2 5.2h-4.2c-.55 0-1-.45-1-1V2.6Z" fill="#000" opacity="0.04" />

      {/* QuickLook-style art */}
      <DocGlyph glyph={meta.glyph} />

      {/* colored type tag */}
      <g>
        <rect x="6.5" y="16.4" width="15" height="5.4" rx="1.4" fill={tagColor} />
        <text
          x="14"
          y="20.25"
          textAnchor="middle"
          fontFamily="-apple-system, system-ui, sans-serif"
          fontSize="4.3"
          fontWeight="700"
          letterSpacing="0.2"
          fill="#FFFFFF"
        >
          {label}
        </text>
      </g>
    </svg>
  )
}

/** Small QuickLook-style art layered in the sheet's upper region. */
function DocGlyph({ glyph }: { glyph: Glyph }) {
  if (glyph === "photo") {
    return (
      <g>
        <rect x="8.6" y="6" width="10.8" height="8.4" rx="1.1" fill="#EAF4FB" />
        <circle cx="11.4" cy="8.9" r="1.2" fill="#F5C24B" />
        <path d="M9 13.6l2.8-3 2.1 2 2-2.4 2.6 3.4H9Z" fill="#5CB85C" />
      </g>
    )
  }
  if (glyph === "note") {
    return (
      <g fill="#FF2D55">
        <rect x="12.6" y="6.2" width="1.5" height="6.4" rx="0.5" />
        <path d="M12.6 6.2l4.4-1v1.7l-4.4 1V6.2Z" />
        <circle cx="12.2" cy="12.6" r="1.7" />
        <circle cx="16.6" cy="11.6" r="1.7" />
      </g>
    )
  }
  if (glyph === "play") {
    return (
      <g>
        <rect x="8.6" y="6" width="10.8" height="8.4" rx="1.1" fill="#E7ECF6" />
        <path d="M12.6 8.2l4 2.2-4 2.2V8.2Z" fill="#4B7BEC" />
      </g>
    )
  }
  if (glyph === "zip") {
    return (
      <g fill="#B9BEC8">
        <rect x="13.2" y="6" width="1.6" height="1.6" />
        <rect x="13.2" y="9" width="1.6" height="1.6" />
        <rect x="13.2" y="12" width="1.6" height="1.6" />
      </g>
    )
  }
  // generic document: faint text lines
  return (
    <g fill="#C7CCD4">
      <rect x="9" y="7.4" width="10" height="1.1" rx="0.55" />
      <rect x="9" y="9.8" width="10" height="1.1" rx="0.55" />
      <rect x="9" y="12.2" width="7" height="1.1" rx="0.55" />
    </g>
  )
}
