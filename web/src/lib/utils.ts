import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// ── Ordered-list Enter behaviour (T359) ──────────────────────────────
//
// Faithful web port of the TUI input area (src/modules/conversation/list.rs +
// panel.rs Enter branch), used by the thread composer. On a plain Enter the
// composer:
//   1. continues an ordered/unordered list — `2. foo` → newline + `3. ` (the
//      marker auto-increments: numeric 2→3, single-letter a→b … z→aa with the
//      Rust bijective base-26 scheme, case preserved; bullets `- `/`* ` repeat);
//   2. on an EMPTY list item (`3. ` with nothing after) removes the marker,
//      collapsing the line to blank instead of spawning another item;
//   3. sends only when the caret sits on an empty trailing line (so a stray
//      Enter mid-message inserts a newline rather than firing too early);
//   4. otherwise inserts a newline.
// Shift+Enter always inserts a newline (handled in the keydown handler).

/** The decision Enter yields, mirroring the TUI's three list outcomes + send. */
export type EnterAction =
  | { kind: "send" }
  | { kind: "newline" }
  | { kind: "continue"; text: string }
  | { kind: "remove" }

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

/**
 * Detect the list action for a single line — direct port of the TUI
 * `detect_list_action`. Returns a continuation string, an empty-item removal,
 * or null (no list ⇒ plain newline).
 */
function detectListAction(line: string): { kind: "continue"; text: string } | { kind: "remove" } | null {
  const trimmed = line.replace(/^\s+/, "")
  if (trimmed.length === 0) return null
  const indent = " ".repeat(line.length - trimmed.length)

  // Empty unordered item.
  if (trimmed === "- " || trimmed === "* ") return { kind: "remove" }

  const dot = trimmed.indexOf(". ")
  if (dot >= 0) {
    const marker = trimmed.slice(0, dot)
    const after = trimmed.slice(dot + 2)
    const isNumeric = marker.length > 0 && /^\d+$/.test(marker)
    const isAlpha = marker.length === 1 && (/^[a-z]$/.test(marker) || /^[A-Z]$/.test(marker))
    if (after.length === 0 && (isNumeric || isAlpha)) return { kind: "remove" }
  }

  // Non-empty unordered item → repeat the bullet.
  if (trimmed.startsWith("- ") || trimmed.startsWith("* ")) {
    return { kind: "continue", text: `\n${indent}${trimmed.slice(0, 2)}` }
  }

  // Non-empty ordered item → next marker.
  if (dot >= 0) {
    const marker = trimmed.slice(0, dot)
    if (/^\d+$/.test(marker)) {
      const n = Number.parseInt(marker, 10)
      return { kind: "continue", text: `\n${indent}${n + 1}. ` }
    }
    if (marker.length === 1 && (/^[a-z]$/.test(marker) || /^[A-Z]$/.test(marker))) {
      return { kind: "continue", text: `\n${indent}${nextAlphaMarker(marker)}. ` }
    }
  }
  return null
}

/**
 * Resolve what a plain Enter should do given the textarea value + caret,
 * mirroring the TUI panel.rs Enter branch: send only when the caret is at the
 * very end on an empty trailing line, else continue/remove a list or newline.
 * The list decision keys off the line the caret sits on (equivalent to the
 * TUI's last-line check when the caret is at the end, the dominant case).
 */
export function resolveEnter(value: string, selStart: number, selEnd: number): EnterAction {
  const atEnd = selStart === selEnd && selStart === value.length
  const lastLine = value.split("\n").at(-1) ?? ""
  const endsEmptyLine = value.endsWith("\n") || lastLine.trim().length === 0
  if (atEnd && endsEmptyLine) return { kind: "send" }

  const lineStart = value.lastIndexOf("\n", selStart - 1) + 1
  const lineEnd = value.indexOf("\n", selStart)
  const curLine = value.slice(lineStart, lineEnd === -1 ? value.length : lineEnd)
  const action = detectListAction(curLine)
  if (!action) return { kind: "newline" }
  return action
}
