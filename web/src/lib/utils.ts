import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"
import { zip as fflateZip } from "fflate"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// ── Client-side zip-on-drop (T367) ────────────────────────────────────
//
// Bundle the file(s) a user drops onto the thread conversation into a SINGLE
// `.zip` (built entirely in the browser via fflate) before they're uploaded, so
// what lands in the realm is one compressed archive instead of N loose files.
// Only the drag-and-drop path uses this; the paperclip picker still uploads raw
// files unchanged.

/**
 * De-duplicate an entry name within a zip: if `name` is already a key in `taken`,
 * append ` (1)`, ` (2)`… before the extension until it's unique. Two files
 * dragged from different folders can share a basename — without this the second
 * would silently overwrite the first inside the archive.
 */
function uniqueZipEntry(taken: Record<string, unknown>, name: string): string {
  if (!(name in taken)) return name
  const dot = name.lastIndexOf(".")
  const stem = dot > 0 ? name.slice(0, dot) : name
  const ext = dot > 0 ? name.slice(dot) : ""
  for (let i = 1; ; i++) {
    const candidate = `${stem} (${i})${ext}`
    if (!(candidate in taken)) return candidate
  }
}

/**
 * Zip the dropped `files` into one archive `File`, built client-side with
 * fflate (DEFLATE, level 6). The archive is named after the lone file when a
 * single one is dropped (`report.pdf` → `report.pdf.zip`, matching the macOS
 * Finder "Compress" convention) or `dropped-<n>-files.zip` for several. Each
 * entry keeps its original filename (de-duplicated on collision). Rejects if
 * fflate fails or a file can't be read.
 */
export function zipFiles(files: File[]): Promise<File> {
  return Promise.all(
    files.map(async (f) => [f.name, new Uint8Array(await f.arrayBuffer())] as const),
  ).then(
    (entries) =>
      new Promise<File>((resolve, reject) => {
        const data: Record<string, Uint8Array> = {}
        for (const [name, bytes] of entries) data[uniqueZipEntry(data, name)] = bytes
        fflateZip(data, { level: 6 }, (err, out) => {
          if (err) {
            reject(err)
            return
          }
          const zipName =
            files.length === 1 && files[0] ? `${files[0].name}.zip` : `dropped-${files.length}-files.zip`
          resolve(new File([out], zipName, { type: "application/zip" }))
        })
      }),
  )
}

// ── List Enter / Tab behaviour (T359) ────────────────────────────────
//
// Web port of the TUI input area (src/modules/conversation/list.rs +
// panel.rs Enter branch), used by the thread composer, extended with
// nesting (Tab / Shift+Tab) and depth-styled unordered bullets.
//
// Plain Enter:
//   1. continues an ordered/unordered list — `2. foo` → newline + `3. ` (the
//      marker auto-increments: numeric 2→3, single-letter a→b … z→aa with the
//      Rust bijective base-26 scheme, case preserved; unordered bullets repeat
//      and are beautified to the depth's pretty glyph, see below);
//   2. on an EMPTY list item (`3. ` / `• ` with nothing after) removes the
//      marker, collapsing the line to blank instead of spawning another item;
//   3. sends only when the caret sits on an empty trailing line (so a stray
//      Enter mid-message inserts a newline rather than firing too early);
//   4. otherwise inserts a newline.
// Shift+Enter always inserts a newline (handled in the keydown handler).
//
// Tab / Shift+Tab (on a list line):
//   - Tab indents the current item one level (a *sublist*): the leading indent
//     grows by [`INDENT`], an unordered marker becomes the next depth's pretty
//     glyph, and an ordered marker resets to a fresh `1.` / `a.` sublist count.
//   - Shift+Tab outdents one level (no-op at depth 0).
//
// Pretty bullets: the user types the plain `- ` / `* ` they know; the FIRST
// continuation (or any indent) rewrites the marker to a depth-appropriate
// Unicode glyph from [`BULLETS`] — every one a single UTF-16 code unit, so the
// swap is width-preserving and never disturbs caret offsets.

/** Spaces per nesting level (one indent step). */
const INDENT = "  "
/** Depth-styled unordered bullet glyphs (clamped past the last). */
const BULLETS = ["•", "◦", "▪", "‣"]
/** Plain unordered markers the user might type (normalised to a pretty glyph). */
const PLAIN_BULLETS = ["-", "*", "+"]
/** Every char accepted as an unordered marker: plain inputs + pretty glyphs. */
const UL_MARKERS = new Set([...PLAIN_BULLETS, ...BULLETS])

/** The depth's pretty bullet (clamped to the last glyph for deep nesting). */
function bulletForDepth(depth: number): string {
  return BULLETS[Math.min(Math.max(depth, 0), BULLETS.length - 1)] ?? "•"
}

/**
 * Increment a single alphabetical list marker, porting the TUI's bijective
 * base-26 scheme: `a`→`b`, `z`→`aa`, `A`→`B`, `Z`→`AA`. Case follows the input.
 */
function nextAlphaMarker(marker: string): string {
  const first = marker[0] ?? "a"
  const isUpper = first >= "A" && first <= "Z"
  const base = isUpper ? 65 : 97 // 'A' / 'a'
  // Decode bijective base-26 to a 1-indexed number (a=1, z=26, aa=27, …).
  let num = 0
  for (const c of marker) num = num * 26 + (c.toLowerCase().charCodeAt(0) - 96)
  num += 1
  // Re-encode, peeling least-significant "digit" each step.
  let out = ""
  for (let n = num; n > 0; n = Math.floor((n - 1) / 26)) {
    out = String.fromCharCode(base + ((n - 1) % 26)) + out
  }
  return out
}

/** A parsed list line, or null when the line is not a list item. */
interface ParsedLine {
  /** Leading whitespace (the indent). */
  indent: string
  /** Nesting depth = floor(indent / INDENT). */
  depth: number
  /** Ordered (`ol`) or unordered (`ul`). */
  kind: "ol" | "ul"
  /** The marker token: a bullet glyph (`ul`) or the count `1`/`a`/`II` (`ol`). */
  marker: string
  /** Text after the `marker + space` prefix (empty for a bare item). */
  rest: string
}

/**
 * Parse one line into its list shape (indent / depth / kind / marker / rest),
 * or null when it isn't a recognised ordered or unordered item. Shared by the
 * Enter and Tab resolvers so both agree on what a list line is.
 */
function parseListLine(line: string): ParsedLine | null {
  const trimmed = line.replace(/^\s+/, "")
  if (trimmed.length === 0) return null
  const indent = line.slice(0, line.length - trimmed.length)
  const depth = Math.floor(indent.length / INDENT.length)

  // Unordered: a single marker char followed by a space.
  const head = trimmed[0] ?? ""
  if (UL_MARKERS.has(head) && trimmed[1] === " ") {
    return { indent, depth, kind: "ul", marker: head, rest: trimmed.slice(2) }
  }

  // Ordered: `<count>. ` where count is digits or a single letter.
  const dot = trimmed.indexOf(". ")
  if (dot > 0) {
    const marker = trimmed.slice(0, dot)
    const isNumeric = /^\d+$/.test(marker)
    const isAlpha = marker.length === 1 && /^[a-zA-Z]$/.test(marker)
    if (isNumeric || isAlpha) {
      return { indent, depth, kind: "ol", marker, rest: trimmed.slice(dot + 2) }
    }
  }
  return null
}

/** Locate the line containing `pos`: its [start, end) offsets in `value`. */
export function lineBounds(value: string, pos: number): { start: number; end: number } {
  const start = value.lastIndexOf("\n", pos - 1) + 1
  const end = value.indexOf("\n", pos)
  return { start, end: end === -1 ? value.length : end }
}

/** A fresh ordered sublist marker matching the parent's numeric/alpha style. */
function resetOrderedMarker(marker: string): string {
  if (/^\d+$/.test(marker)) return "1"
  return marker === marker.toUpperCase() ? "A" : "a"
}

/** Increment an ordered marker preserving its style: numeric `+1`, else the
 *  bijective base-26 letter step. */
function incrementOrderedMarker(marker: string): string {
  return /^\d+$/.test(marker) ? String(Number.parseInt(marker, 10) + 1) : nextAlphaMarker(marker)
}

/**
 * Find the ordered marker of the nearest *preceding sibling* at `targetIndentLen`
 * — the last list item directly above `lineStart` whose indent matches the target
 * depth — so a depth change (indent / outdent) can RESUME that list's count
 * instead of restarting at 1. Returns null when there is no such sibling (the
 * enclosing list was just entered, a shallower line ends it, a blank/non-list
 * line breaks it, or the sibling is unordered), in which case the caller resets
 * to a fresh `1.` / `a.`.
 *
 * Walks upward skipping deeper (nested-child) lines; stops the moment it leaves
 * the enclosing context so numbering never bleeds across separate lists.
 */
function siblingOrderedMarkerAtDepth(value: string, lineStart: number, targetIndentLen: number): string | null {
  if (lineStart === 0) return null // current line is the first — no sibling above
  const before = value.slice(0, lineStart - 1) // drop the \n preceding the current line
  const lines = before.split("\n")
  for (let i = lines.length - 1; i >= 0; i--) {
    const ln = lines[i] ?? ""
    if (ln.trim().length === 0) return null // blank line breaks the list
    const indentLen = ln.length - ln.replace(/^\s+/, "").length
    if (indentLen > targetIndentLen) continue // nested child of a sibling — skip
    if (indentLen < targetIndentLen) return null // left the enclosing list
    const parsed = parseListLine(ln)
    if (!parsed) return null // non-list line at this depth breaks it
    return parsed.kind === "ol" ? parsed.marker : null // unordered → no count
  }
  return null
}

/** The decision Enter yields: send the message, or splice a new value + caret. */
export type EnterAction = { kind: "send" } | { kind: "edit"; value: string; caret: number }

/**
 * Resolve what a plain Enter should do given the textarea value + caret,
 * mirroring the TUI panel.rs Enter branch: send only when the caret is at the
 * very end on an empty trailing line, else continue/remove a list or newline.
 * Returns a full `{value, caret}` splice so the caller applies it in one shot.
 */
export function resolveEnter(value: string, selStart: number, selEnd: number): EnterAction {
  const atEnd = selStart === selEnd && selStart === value.length
  const lastLine = value.split("\n").at(-1) ?? ""
  const endsEmptyLine = value.endsWith("\n") || lastLine.trim().length === 0
  if (atEnd && endsEmptyLine) return { kind: "send" }

  const { start: lineStart, end: lineEnd } = lineBounds(value, selStart)
  const curLine = value.slice(lineStart, lineEnd)
  const parsed = parseListLine(curLine)

  // Not a list line → plain newline at the caret.
  if (!parsed) {
    const next = `${value.slice(0, selStart)}\n${value.slice(selEnd)}`
    return { kind: "edit", value: next, caret: selStart + 1 }
  }

  // Empty item → strip it, collapsing the line to blank (caret to line start).
  if (parsed.rest.length === 0) {
    const next = value.slice(0, lineStart) + value.slice(selStart)
    return { kind: "edit", value: next, caret: lineStart }
  }

  // Non-empty unordered → beautify this line's marker to the depth glyph (a
  // width-preserving 1-char swap, caret unaffected) and continue with it.
  if (parsed.kind === "ul") {
    const pretty = bulletForDepth(parsed.depth)
    const markerIdx = lineStart + parsed.indent.length
    const swapped = value.slice(0, markerIdx) + pretty + value.slice(markerIdx + 1)
    const ins = `\n${parsed.indent}${pretty} `
    const next = swapped.slice(0, selStart) + ins + swapped.slice(selEnd)
    return { kind: "edit", value: next, caret: selStart + ins.length }
  }

  // Non-empty ordered → next marker (numeric +1, or bijective base-26 letter).
  const nextMarker = incrementOrderedMarker(parsed.marker)
  const ins = `\n${parsed.indent}${nextMarker}. `
  const next = value.slice(0, selStart) + ins + value.slice(selEnd)
  return { kind: "edit", value: next, caret: selStart + ins.length }
}

/**
 * Resolve a Tab / Shift+Tab on the current line — indent or outdent a list item
 * by one level. Returns the new `{value, caret}` splice, or null when the line
 * is not a list item (or Shift+Tab at depth 0), letting the textarea's default
 * Tab behaviour stand.
 *
 * Indenting an unordered item restyles its bullet to the next depth's glyph;
 * indenting an ordered item starts a fresh `1.` / `a.` sublist count. Outdenting
 * reverses both. The caret tracks the content as the indent/marker width shifts.
 */
export function resolveTab(value: string, selStart: number, _selEnd: number, shift: boolean): { value: string; caret: number } | null {
  const { start: lineStart, end: lineEnd } = lineBounds(value, selStart)
  const oldLine = value.slice(lineStart, lineEnd)
  const parsed = parseListLine(oldLine)
  if (!parsed) return null

  let newIndent: string
  let newDepth: number
  if (shift) {
    if (parsed.indent.length < INDENT.length) return null // already top-level
    newIndent = parsed.indent.slice(INDENT.length)
    newDepth = parsed.depth - 1
  } else {
    newIndent = parsed.indent + INDENT
    newDepth = parsed.depth + 1
  }

  // Ordered items resume the count of the nearest preceding sibling at the NEW
  // depth (so Shift+Tab back out of a sublist continues the parent list — `1.`
  // above ⇒ `2.` — instead of restarting at 1, and Tab into an existing sublist
  // continues it too); with no such sibling we start a fresh `1.` / `a.`. The
  // sibling's own style drives numeric-vs-alpha; the current marker is the
  // fallback when starting fresh.
  let markerToken: string
  if (parsed.kind === "ul") {
    markerToken = bulletForDepth(newDepth)
  } else {
    const sib = siblingOrderedMarkerAtDepth(value, lineStart, newIndent.length)
    markerToken = sib ? `${incrementOrderedMarker(sib)}.` : `${resetOrderedMarker(parsed.marker)}.`
  }
  const rebuilt = `${newIndent}${markerToken} ${parsed.rest}`
  const next = value.slice(0, lineStart) + rebuilt + value.slice(lineEnd)

  // Shift the caret by the change in line length so a caret in the content
  // region stays put relative to the text; clamp inside the rebuilt line.
  const delta = rebuilt.length - oldLine.length
  const caret = Math.max(lineStart, Math.min(lineStart + rebuilt.length, selStart + delta))
  return { value: next, caret }
}
