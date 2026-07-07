import { useId } from "react"
import type { FinderKind } from "@/lib/types"

/**
 * macOS-faithful file & folder icons (T27).
 *
 * Apple's real artwork is proprietary, so this is the legal next-best thing:
 * hand-built SVGs in the Big Sur / Sonoma icon language. The defining UX goal
 * of this version is *differentiation* — in list and column views every typed
 * document used to collapse into the same white sheet, told apart only by a
 * tag label that's illegible at 16px. Now identity is carried two ways:
 *
 *   • a distinct, type-colored **body glyph** for each kind — `</>` for code,
 *     a dense bar block for PDF, a grid for spreadsheets, `{ }` for JSON, the
 *     "M↓" mark for markdown, a photo thumb, a play triangle, etc. The glyph is
 *     recognizable even when the tag text is too small to read.
 *   • the colored **type tag** (PDF, JS, MD…), shown only at larger sizes.
 *
 * Below `LABEL_MIN_SIZE` the tag is dropped and the glyph is scaled up to fill
 * the sheet, exactly as macOS does — clean and glyph-forward in tight list
 * rows. At or above it, the compact glyph sits in the upper region with the
 * labeled tag beneath, for gallery / column hero use.
 *
 * One self-contained `<FileIcon kind ext size />`, full color, no external
 * tint. Drawn in a 28×28 viewBox and scaled by `size`. All gradient/filter ids
 * are `useId`-scoped to avoid collisions when many icons render at once.
 */

// Below this rendered size, drop the tag label and show an enlarged glyph.
const LABEL_MIN_SIZE = 32

// ── language / extension → tag color ──────────────────────────────
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

// ── kind → tag color + default extension label + glyph id ─────────
type GlyphKind =
  | "code"
  | "doc"
  | "pdf"
  | "sheet"
  | "slides"
  | "photo"
  | "markdown"
  | "json"
  | "zip"
  | "audio"
  | "video"
  | "binary"

const KIND_TAG: Record<FinderKind, { color: string; ext: string; glyph: GlyphKind }> = {
  folder: { color: "#1577E0", ext: "", glyph: "doc" },
  code: { color: "#2F74C0", ext: "CODE", glyph: "code" },
  doc: { color: "#5B95E0", ext: "DOC", glyph: "doc" },
  pdf: { color: "#F0453A", ext: "PDF", glyph: "pdf" },
  sheet: { color: "#2FB457", ext: "XLS", glyph: "sheet" },
  slides: { color: "#FF9F0A", ext: "KEY", glyph: "slides" },
  image: { color: "#34A4D0", ext: "IMG", glyph: "photo" },
  markdown: { color: "#0A84FF", ext: "MD", glyph: "markdown" },
  json: { color: "#14B8A6", ext: "JSON", glyph: "json" },
  archive: { color: "#8E8E93", ext: "ZIP", glyph: "zip" },
  audio: { color: "#FF2D55", ext: "AAC", glyph: "audio" },
  video: { color: "#4B7BEC", ext: "MP4", glyph: "video" },
  binary: { color: "#8E8E93", ext: "BIN", glyph: "binary" },
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
  ext?: string | undefined
  size?: number | undefined
  className?: string | undefined
}) {
  if (kind === "folder") return <MacFolder size={size} className={className} />
  return <MacDocument kind={kind} ext={ext} size={size} className={className} />
}

// ── Folder ────────────────────────────────────────────────────────
function MacFolder({ size, className }: { size: number; className?: string | undefined }) {
  const id = useId().replaceAll(":", "")
  return (
    <svg width={size} height={size} viewBox="0 0 28 28" className={className} aria-hidden>
      <defs>
        <linearGradient id={`back${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#2C96F0" />
          <stop offset="1" stopColor="#0E6FD6" />
        </linearGradient>
        <linearGradient id={`front${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#6FBEFB" />
          <stop offset="0.55" stopColor="#34A1F6" />
          <stop offset="1" stopColor="#1A82EC" />
        </linearGradient>
        <radialGradient id={`glow${id}`} cx="0.5" cy="0.12" r="0.9">
          <stop offset="0" stopColor="#9FD4FF" stopOpacity="0.85" />
          <stop offset="0.5" stopColor="#9FD4FF" stopOpacity="0" />
        </radialGradient>
        <linearGradient id={`sheen${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#FFFFFF" stopOpacity="0.7" />
          <stop offset="1" stopColor="#FFFFFF" stopOpacity="0" />
        </linearGradient>
        <filter id={`ds${id}`} x="-30%" y="-30%" width="160%" height="160%">
          <feDropShadow
            dx="0"
            dy="0.5"
            stdDeviation="0.65"
            floodColor="#0A4FA0"
            floodOpacity="0.32"
          />
        </filter>
      </defs>
      <g filter={`url(#ds${id})`}>
        <path
          d="M2.6 9.4c0-1.16.94-2.1 2.1-2.1h4.6c.66 0 1.29.31 1.69.84l1.04 1.36c.4.53 1.03.84 1.69.84H23.3c1.16 0 2.1.94 2.1 2.1v6.7c0 1.16-.94 2.1-2.1 2.1H4.7c-1.16 0-2.1-.94-2.1-2.1V9.4Z"
          fill={`url(#back${id})`}
        />
        <path
          d="M2.6 12.2c0-1.16.94-2.1 2.1-2.1H23.3c1.16 0 2.1.94 2.1 2.1v7.6c0 1.16-.94 2.1-2.1 2.1H4.7c-1.16 0-2.1-.94-2.1-2.1v-7.6Z"
          fill={`url(#front${id})`}
        />
        <path
          d="M2.6 12.2c0-1.16.94-2.1 2.1-2.1H23.3c1.16 0 2.1.94 2.1 2.1v7.6c0 1.16-.94 2.1-2.1 2.1H4.7c-1.16 0-2.1-.94-2.1-2.1v-7.6Z"
          fill={`url(#glow${id})`}
        />
        <path
          d="M4.7 10.1h18.6c1 0 1.85.7 2.05 1.63-.5-.36-1.12-.56-1.8-.56H4.45c-.68 0-1.3.2-1.8.56C2.85 10.8 3.7 10.1 4.7 10.1Z"
          fill={`url(#sheen${id})`}
        />
      </g>
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
  ext?: string | undefined
  size: number
  className?: string | undefined
}) {
  const id = useId().replaceAll(":", "")
  const meta = KIND_TAG[kind]
  const tagColor = (ext && EXT_COLOR[ext.toLowerCase()]) || meta.color
  const label = (ext ? ext.toUpperCase() : meta.ext).slice(0, 4)
  const showLabel = size >= LABEL_MIN_SIZE

  return (
    <svg width={size} height={size} viewBox="0 0 28 28" className={className} aria-hidden>
      <defs>
        <linearGradient id={`sheet${id}`} x1="0.1" y1="0" x2="0.4" y2="1">
          <stop offset="0" stopColor="#FFFFFF" />
          <stop offset="1" stopColor="#ECEEF2" />
        </linearGradient>
        <linearGradient id={`fold${id}`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#E4E7ED" />
          <stop offset="1" stopColor="#CFD4DC" />
        </linearGradient>
        <linearGradient id={`tag${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={tagColor} />
          <stop offset="1" stopColor={tagColor} stopOpacity="0.88" />
        </linearGradient>
        <filter id={`ds${id}`} x="-30%" y="-30%" width="160%" height="160%">
          <feDropShadow
            dx="0"
            dy="0.5"
            stdDeviation="0.55"
            floodColor="#485060"
            floodOpacity="0.5"
          />
        </filter>
      </defs>
      <g filter={`url(#ds${id})`}>
        {/* sheet: taller-than-wide with a folded top-right corner */}
        <path
          d="M5.4 3.5c0-.83.67-1.5 1.5-1.5h10l5.2 5.2v15.8c0 .83-.67 1.5-1.5 1.5H6.9c-.83 0-1.5-.67-1.5-1.5V3.5Z"
          fill={`url(#sheet${id})`}
          stroke="#D2D7DF"
          strokeWidth="0.4"
        />
        <path d="M16.9 2l5.2 5.2h-3.7c-.83 0-1.5-.67-1.5-1.5V2Z" fill={`url(#fold${id})`} />
        <path
          d="M16.9 2v3.7c0 .83.67 1.5 1.5 1.5h3.7"
          fill="none"
          stroke="#BFC5CE"
          strokeWidth="0.35"
        />

        {/* type identity glyph — compact (with label) or enlarged (without) */}
        <DocGlyph glyph={meta.glyph} color={tagColor} compact={showLabel} />

        {/* colored type tag, only when large enough to read */}
        {showLabel && (
          <g>
            <rect x="5.4" y="16" width="13.4" height="5" rx="1.2" fill={`url(#tag${id})`} />
            <text
              x="12.1"
              y="19.65"
              textAnchor="middle"
              fontFamily="'SF Pro Text', -apple-system, system-ui, sans-serif"
              fontSize="3.7"
              fontWeight="700"
              letterSpacing="0.15"
              fill="#FFFFFF"
            >
              {label}
            </text>
          </g>
        )}
      </g>
    </svg>
  )
}

/**
 * Type-identity art. `compact` (large icon) draws a small glyph in the upper
 * region above the tag; non-compact (small icon) draws an enlarged centered
 * glyph that fills the sheet for instant recognition in list rows.
 */
function DocGlyph({
  glyph,
  color,
  compact,
}: {
  glyph: GlyphKind
  color: string
  compact: boolean
}) {
  // Photo / video render their own framed thumbnail at both sizes.
  if (glyph === "photo") {
    return compact ? (
      <g>
        <rect x="7.4" y="6.6" width="9.4" height="7.2" rx="1" fill="#EAF4FB" />
        <circle cx="9.9" cy="9" r="1.05" fill="#F5C24B" />
        <path d="M7.7 12.9l2.4-2.6 1.8 1.7 1.7-2.1 2.3 3H7.7Z" fill="#5CB85C" />
      </g>
    ) : (
      <g>
        <rect x="6.8" y="7" width="14.4" height="11" rx="1.4" fill="#EAF4FB" />
        <circle cx="10.6" cy="10.5" r="1.6" fill="#F5C24B" />
        <path d="M7.4 16.8l3.6-4 2.7 2.6 2.6-3.2 3.5 4.6H7.4Z" fill="#5CB85C" />
      </g>
    )
  }
  if (glyph === "video") {
    return compact ? (
      <g>
        <rect x="7.4" y="6.6" width="9.4" height="7.2" rx="1" fill="#E7ECF6" />
        <path d="M10.9 8.6l3.5 1.9-3.5 1.9V8.6Z" fill="#4B7BEC" />
      </g>
    ) : (
      <g>
        <rect x="6.8" y="7" width="14.4" height="11" rx="1.4" fill="#E7ECF6" />
        <path d="M12 10l4.6 2.5L12 15z" fill="#4B7BEC" />
      </g>
    )
  }
  if (glyph === "audio") {
    return compact ? (
      <g fill="#FF2D55">
        <rect x="10.9" y="6.7" width="1.3" height="5.6" rx="0.45" />
        <path d="M10.9 6.7l3.9-.9v1.5l-3.9.9V6.7Z" />
        <circle cx="10.5" cy="12.3" r="1.5" />
        <circle cx="14.4" cy="11.4" r="1.5" />
      </g>
    ) : (
      <g fill="#FF2D55">
        <rect x="12" y="7" width="1.7" height="8" rx="0.6" />
        <path d="M12 7l6-1.4v2.2L12 9.2V7Z" />
        <circle cx="11.3" cy="15.2" r="2.1" />
        <circle cx="17.3" cy="13.8" r="2.1" />
      </g>
    )
  }

  // Stroke / fill glyphs colored by type.
  const c = color
  if (glyph === "code") {
    return compact ? (
      <g fill="none" stroke={c} strokeWidth="1.15" strokeLinecap="round" strokeLinejoin="round">
        <path d="M10.3 7.6 8 10.2l2.3 2.6" />
        <path d="M13.9 7.6l2.3 2.6-2.3 2.6" />
      </g>
    ) : (
      <g fill="none" stroke={c} strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
        <path d="M11 8.5 7.6 12l3.4 3.5" />
        <path d="M16 8.5 19.4 12 16 15.5" />
      </g>
    )
  }
  if (glyph === "pdf") {
    return compact ? (
      <g fill={c} opacity="0.9">
        <rect x="8" y="7.2" width="8" height="1.5" rx="0.4" />
        <rect x="8" y="9.6" width="8" height="1.5" rx="0.4" />
        <rect x="8" y="12" width="5" height="1.5" rx="0.4" />
      </g>
    ) : (
      <g fill={c}>
        <rect x="8" y="8" width="11" height="2" rx="0.6" />
        <rect x="8" y="11.5" width="11" height="2" rx="0.6" />
        <rect x="8" y="15" width="7" height="2" rx="0.6" />
      </g>
    )
  }
  if (glyph === "markdown") {
    return compact ? (
      <g fill="none" stroke={c} strokeWidth="1.1" strokeLinecap="round" strokeLinejoin="round">
        <path d="M7.6 12.4V7.8l2 2.3 2-2.3v4.6" />
        <path d="M14.6 7.8v4.6M13.2 11l1.4 1.6 1.4-1.6" />
      </g>
    ) : (
      <g fill="none" stroke={c} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M7.5 16V9l2.6 3 2.6-3v7" />
        <path d="M17 9v7M14.8 13.5 17 16l2.2-2.5" />
      </g>
    )
  }
  if (glyph === "sheet") {
    return compact ? (
      <g fill="none" stroke={c} strokeWidth="0.9">
        <rect x="7.6" y="7.2" width="8.8" height="6.4" rx="0.6" />
        <path d="M7.6 9.5h8.8M7.6 11.4h8.8M10.5 7.2v6.4M13.5 7.2v6.4" />
      </g>
    ) : (
      <g fill="none" stroke={c} strokeWidth="1.3">
        <rect x="7.5" y="8" width="13" height="9" rx="0.8" />
        <path d="M7.5 11.3h13M7.5 14h13M11.8 8v9M16 8v9" />
      </g>
    )
  }
  if (glyph === "slides") {
    return compact ? (
      <g fill="none" stroke={c} strokeWidth="1">
        <rect x="7.6" y="7.2" width="8.8" height="6" rx="0.8" />
        <path d="M9.4 10.2h5.2" strokeWidth="1.2" />
      </g>
    ) : (
      <g fill="none" stroke={c} strokeWidth="1.4">
        <rect x="7.4" y="8" width="13.2" height="9" rx="1" />
        <path d="M10 12.5h7.4" strokeWidth="1.8" />
      </g>
    )
  }
  if (glyph === "json") {
    return compact ? (
      <g fill="none" stroke={c} strokeWidth="1.1" strokeLinecap="round" strokeLinejoin="round">
        <path d="M10.4 7.4c-1.2 0-1.2 1.4-1.2 2.4 0 .8-.6 1-1 1 .4 0 1 .2 1 1 0 1 0 2.4 1.2 2.4" />
        <path d="M13.6 7.4c1.2 0 1.2 1.4 1.2 2.4 0 .8.6 1 1 1-.4 0-1 .2-1 1 0 1 0 2.4-1.2 2.4" />
      </g>
    ) : (
      <g fill="none" stroke={c} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M11 7.5c-1.8 0-1.8 2-1.8 3.5 0 1.2-.9 1.5-1.5 1.5.6 0 1.5.3 1.5 1.5 0 1.5 0 3.5 1.8 3.5" />
        <path d="M17 7.5c1.8 0 1.8 2 1.8 3.5 0 1.2.9 1.5 1.5 1.5-.6 0-1.5.3-1.5 1.5 0 1.5 0 3.5-1.8 3.5" />
      </g>
    )
  }
  if (glyph === "zip") {
    return compact ? (
      <g fill={c}>
        <rect x="11.4" y="6.4" width="1.4" height="1.4" />
        <rect x="11.4" y="9" width="1.4" height="1.4" />
        <rect x="11.4" y="11.6" width="1.4" height="1.4" />
      </g>
    ) : (
      <g fill={c}>
        <rect x="13" y="7" width="2" height="2" />
        <rect x="13" y="10.5" width="2" height="2" />
        <rect x="13" y="14" width="2" height="2" />
      </g>
    )
  }
  if (glyph === "binary") {
    return compact ? (
      <g fill="none" stroke={c} strokeWidth="1">
        <circle cx="12" cy="10.2" r="2.1" />
        <path d="M12 6.6v1.2M12 12.6v1.2M8.4 10.2h1.2M14.4 10.2h1.2" />
      </g>
    ) : (
      <g fill="none" stroke={c} strokeWidth="1.4">
        <circle cx="14" cy="12.5" r="3" />
        <path d="M14 6.5v2M14 16.5v2M8 12.5h2M18 12.5h2M9.8 8.3l1.4 1.4M16.8 15.3l1.4 1.4M18.2 8.3l-1.4 1.4M11.2 15.3l-1.4 1.4" />
      </g>
    )
  }

  // generic document: faint text lines
  return compact ? (
    <g fill="#CDD2DA">
      <rect x="7.6" y="7.2" width="9.2" height="1" rx="0.5" />
      <rect x="7.6" y="9.4" width="9.2" height="1" rx="0.5" />
      <rect x="7.6" y="11.6" width="6.2" height="1" rx="0.5" />
    </g>
  ) : (
    <g fill="#C2C8D2">
      <rect x="8" y="8" width="11" height="1.4" rx="0.6" />
      <rect x="8" y="11" width="11" height="1.4" rx="0.6" />
      <rect x="8" y="14" width="7.5" height="1.4" rx="0.6" />
    </g>
  )
}
