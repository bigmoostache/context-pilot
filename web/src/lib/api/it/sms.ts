// ── SMS cockpit logic ───────────────────────────────────────────────
//
// Everything the SMS panel knows that has nothing to do with how it looks: the
// React-Query key, the poll interval, the page size and its cursor arithmetic,
// the compose draft and its request body, the client-side mirror of the
// server's validation rules, and the per-message view model the rows render.
//
// It lives here for the same reason `./network.ts` does: the panel exists TWICE
// — `components/shell/config/it/ItSmsPane.tsx` and its `mobile-components`
// twin — and the two differ ONLY in Tailwind classes (design-mobile §3.3).
// Without this module every rule below would be written out twice and every
// cockpit fix made twice (review finding C8). Nothing in this file may import a
// component or emit JSX, and it must not import the `@/lib/api` barrel — that
// barrel re-exports `./index.ts` from this very directory.
//
// **The server is the authority.** `sms/mod.rs::validate` runs on every send,
// including the ones this file would have allowed; the predicates below exist
// so the operator gets a specific message instead of a round trip. And note
// what they deliberately do NOT attempt: the hourly/daily rate limit and the
// modem's own refusal are facts about the box and the carrier that no client
// can anticipate, so the panel prints the server's words for them verbatim
// rather than guessing ahead of it.

import type { ItSmsList, ItSmsMessage, PostApiItSmsData } from "../generated/types.gen"
import { codePointCount } from "./network"
import { agoLabel } from "./networkStatus"

type SendBody = PostApiItSmsData["body"]

/** React-Query key for `GET /api/it/sms`. Shared so the list query, the three
 *  mutations and their invalidations cannot drift apart. */
export const IT_SMS_KEY = ["it-sms"] as const

/**
 * How often the archive re-reads itself while the panel is open.
 *
 * Slower than the uplink pane's 5 s (`network.ts::POLL_MS`) on purpose: a
 * message cannot appear in the archive faster than the background ingester
 * sweeps for it (`CP_SMS_POLL_S`, 30 s by default), so polling at uplink speed
 * would be six requests per sweep with nothing new in five of them. Half a
 * sweep is the useful floor — it keeps the cockpit's own added latency well
 * under the ingester's, which is what the hardware recipe actually measures —
 * and 15 s is that half.
 *
 * The unread badge does NOT ride on this query — it comes from `status.sms` on
 * the 5 s network poll, which is the one probe the backend promises is a single
 * source of truth.
 */
export const SMS_POLL_MS = 15_000

/**
 * Messages per page.
 *
 * The server clamps `limit` to 200 and defaults to 50; asking for fewer than it
 * would give makes the "Load older" affordance meaningful on a box that has
 * been running for months, without ever holding a five-thousand-row archive in
 * a browser tab.
 */
export const SMS_PAGE = 25

// ── Pagination (`?before=<id>&limit=<n>`) ────────────────────────────

/**
 * The `before` cursor for the page after this one, or null when the archive is
 * exhausted.
 *
 * A SHORT page is the end-of-archive signal — the server has no total to report
 * and inventing one would mean a second query. It stays a reliable signal under
 * the soft delete, because the route drops removed rows in its `WHERE` and only
 * then applies `LIMIT`: a page is short because the archive ran out, never
 * because something in it was hidden.
 *
 * The cursor is the last row's id, which is exactly what the route paginates on
 * (`id < before`, ordered by `id DESC`). Not a timestamp: the network's clock is
 * missing on some messages and wrong on others, which is why neither the
 * ordering nor the cursor touches it.
 */
export function nextSmsCursor(page: ItSmsList): number | null {
  if (page.messages.length < SMS_PAGE) return null
  const last = page.messages.at(-1)
  return last?.id ?? null
}

/** Every page fetched so far, flattened into the single newest-first list the
 *  panel renders. Pages are disjoint by construction (each starts strictly
 *  before the previous one's last id), so no de-duplication is needed. */
export function flattenSmsPages(pages: readonly ItSmsList[]): ItSmsMessage[] {
  return pages.flatMap((page) => page.messages)
}

// ── Compose draft ───────────────────────────────────────────────────

/** The compose form's editable shape. Two fields, both free text: the number is
 *  validated rather than masked, because a masked input cannot express the
 *  optional `+` without guessing at a country. */
export interface SmsDraft {
  to: string
  body: string
}

/** A blank compose form — also what the panel snaps back to after a send. */
export const EMPTY_SMS_DRAFT: SmsDraft = { to: "", body: "" }

/**
 * The `POST /api/it/sms` body for a draft.
 *
 * The number is trimmed, the text is not: leading and trailing whitespace in a
 * number is a slip, whereas in a message it is the operator's own text and the
 * carrier will carry it. {@link sendProblem} validates the same trimmed number,
 * so the client and the server can never disagree about what was checked.
 */
export function smsBody(draft: SmsDraft): SendBody {
  return { to: draft.to.trim(), body: draft.body }
}

// ── Validation: a transcription of `sms/mod.rs::validate`, rule for rule ──

/** The server's `MAX_BODY_CHARS` — ten UCS-2 segments of 67 characters, which
 *  is where carriers stop being reliable about reassembling a concatenated
 *  message. Exported because the panel shows a live counter against it. */
export const SMS_MAX_BODY = 670

/** How long a body the server will consider it, in the units it counts:
 *  Unicode code points, matching Rust's `chars().count()`. `String.length`
 *  would count UTF-16 units and disagree on every emoji — the single most
 *  likely astral character in a text message, and precisely the disagreement
 *  that produces an unexplained rejection. */
export function smsBodyLength(text: string): number {
  return codePointCount(text)
}

/**
 * Why this draft cannot be sent, or null.
 *
 * `sms/mod.rs::validate` in full, in the server's own order and returning the
 * server's own messages — including its two-step number check, which separates
 * "that is not a number" from "that is a number of the wrong length" because
 * the two are different mistakes.
 *
 * Note what is NOT checked here: an empty-looking body of spaces passes, exactly
 * as it passes on the server (`req.body.is_empty()`, not a trim). Tightening it
 * would refuse something the box would have sent, which is a worse failure than
 * sending a space.
 */
export function sendProblem(draft: SmsDraft): string | null {
  const recipient = draft.to.trim()
  const digits = recipient.startsWith("+") ? recipient.slice(1) : recipient
  if (digits === "" || !/^\d+$/.test(digits)) {
    return "number must be digits, optionally prefixed with +"
  }
  // `.length` is safe here and only here: every character is an ASCII digit, so
  // UTF-16 units and code points are the same count.
  if (digits.length < 6 || digits.length > 15) {
    return "number must be 6 to 15 digits (E.164)"
  }
  if (draft.body === "") return "body must not be empty"
  if (smsBodyLength(draft.body) > SMS_MAX_BODY) {
    return `body is too long (max ${SMS_MAX_BODY} characters)`
  }
  return null
}

// ── Per-message view model ──────────────────────────────────────────

/** How loudly a row should read. Same vocabulary as `networkStatus`'s
 *  supervisor card, so both panes map tone to colour the same way. */
export type SmsTone = "muted" | "warn" | "ok" | "danger"

/** One archived message, as a row renders it. Every string here is final: the
 *  panel prints them, it does not compose them. */
export interface SmsView {
  /** The other end. Not always a dialling number — carriers send from
   *  alphanumeric short names, which is why it is never validated on the way
   *  in. */
  peer: string
  /** The text itself, untouched. */
  body: string
  /** True for a message this box received, false for one it sent. The primary
   *  visual distinction; a row must never leave it ambiguous who spoke. */
  inbound: boolean
  /** An inbound message nobody has opened yet. */
  unread: boolean
  /** Delivery in words: "Received", "Sending…", "Sent", "Failed". */
  delivery: string
  /** The tone that delivery deserves. */
  tone: SmsTone
  /** How long ago, from `ingested_at` — the timestamp this box can vouch for. */
  when: string
  /** The full timestamp, with its provenance, for a `title` tooltip. */
  exact: string
  /** The modem's own words when a send failed, else null. Never collapsed into
   *  "Failed": the reason is the only actionable half. */
  error: string | null
  /** "Sent by <user>" — the audit trail, since sending spends the vendor's data
   *  plan. Null for inbound, and for an outbound message from god-mode. */
  sentBy: string | null
}

const DELIVERY: Record<ItSmsMessage["delivery"], { label: string; tone: SmsTone }> = {
  received: { label: "Received", tone: "muted" },
  // In flight. Not "ok": the row is a promise the modem has not kept yet, and a
  // send that dies mid-flight leaves this row exactly as it is.
  sending: { label: "Sending…", tone: "warn" },
  sent: { label: "Sent", tone: "ok" },
  failed: { label: "Failed", tone: "danger" },
}

/**
 * The absolute timestamp a row shows on hover, and where it came from.
 *
 * `sent_at` is the NETWORK's clock and is null whenever the modem reported none
 * or reported something unparseable; `ingested_at` is this box's own and is
 * never null. When the two exist they can differ by minutes — a message queued
 * by the carrier — so the label says which one it is rather than presenting a
 * guess as a fact.
 */
function exactTime(message: ItSmsMessage): string {
  const box = new Date(message.ingested_at * 1000).toLocaleString()
  if (message.sent_at === null) return `${box} (seen by this box; the network sent no timestamp)`
  return `${new Date(message.sent_at * 1000).toLocaleString()} (network) · ${box} (seen by this box)`
}

/** Everything a row needs, computed once. */
export function smsView(message: ItSmsMessage): SmsView {
  const state = DELIVERY[message.delivery]
  const inbound = message.direction === "received"
  return {
    peer: message.peer,
    body: message.body,
    inbound,
    unread: inbound && !message.read,
    delivery: state.label,
    tone: state.tone,
    when: agoLabel(message.ingested_at),
    exact: exactTime(message),
    error: message.error,
    sentBy: message.sent_by === null ? null : `Sent by ${message.sent_by}`,
  }
}

/** The unread badge, or null when there is nothing to badge. Counts the
 *  ARCHIVE, not the modem — the ingester empties the modem on every sweep. */
export function unreadLabel(unread: number): string | null {
  if (unread <= 0) return null
  return unread === 1 ? "1 unread" : `${unread} unread`
}
