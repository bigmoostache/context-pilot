// ── Internet-uplink cockpit logic ───────────────────────────────────
//
// Everything the uplink + Wi-Fi pane knows that has nothing to do with how it
// looks: the mode table, the poll interval, the form drafts and their request
// bodies, the client-side mirror of the server's validation rules, and the
// status / probe / supervisor formatting.
//
// It lives here rather than beside the pane because the pane exists TWICE —
// `components/shell/config/it/**` and its `mobile-components` twin — and the
// two differ ONLY in Tailwind classes (design-mobile §3.3). Before this module
// every rule below was written out twice and every cockpit fix had to be made
// twice (review finding C8). It sits beside `./index.ts`, the hand-written
// client for the routes it describes, in the api layer that already claims
// presentation reshaping as its business. Nothing in this file may import a
// component or emit JSX, and it must not import the `@/lib/api` barrel — that
// barrel re-exports `./index.ts` from this very directory.
//
// **The server is the authority.** The predicates below exist so the operator
// gets a specific message instead of a round-trip and an opaque failure; they
// are not a guarantee. `state.rs::validate` is, and it runs on every write —
// including the ones this file would have allowed.

import type {
  ItNetworkAp,
  ItNetworkConfig,
  ItNetworkResponse,
  ItNetworkWwan,
  PostApiItNetworkApData,
  PostApiItNetworkWwanData,
} from "../generated/types.gen"

/** React-Query key for `GET /api/it/network`. Shared so the pane, the three
 *  mutations and the cache patches below cannot drift apart. */
export const IT_NETWORK_KEY = ["it-network"] as const

/** How often the status card re-reads the box while the pane is open. Fast
 *  enough that a failover — or a mode change taking effect — is visible within
 *  about ten seconds, without a manual refresh.
 *
 *  Nothing may be keyed off this query's identity — see {@link apDraftFrom}. */
export const POLL_MS = 5000

type Mode = ItNetworkConfig["mode"]
type ApBody = PostApiItNetworkApData["body"]
type WwanBody = PostApiItNetworkWwanData["body"]

// ── Uplink modes ─────────────────────────────────────────────────────

/** Uplink modes, in the order an admin escalates through them. `needsModem`
 *  marks the two that a box without the 5G module cannot honour — picking `5g`
 *  there would suppress the ethernet default route with nothing to replace it. */
const MODES: { id: Mode; label: string; blurb: string; needsModem: boolean }[] = [
  { id: "wan", label: "Ethernet", blurb: "Cable only — the modem stays off", needsModem: false },
  {
    id: "wan_5g",
    label: "Ethernet + 5G",
    blurb: "5G takes over when the cable stops working",
    needsModem: true,
  },
  {
    id: "5g",
    label: "5G only",
    blurb: "The cable's default route is suppressed",
    needsModem: true,
  },
]

/** One row of the mode selector. */
export interface ModeRow {
  id: Mode
  label: string
  blurb: string
  /** The persisted mode — the row to highlight. */
  selected: boolean
  /** Why this row cannot be picked, or null when it can. */
  unavailable: string | null
}

/**
 * The mode rows to render for a box, given what the persisted document says and
 * whether the modem is actually there.
 *
 * A modem-less box hides the two 5G modes — **unless the document already says
 * one of them** (B12). That is reachable: a box seeded `5g` and later stripped
 * of its M.2 module, or one sysfs probe that misses. Filtering it out silently
 * left the list with no row highlighted and no hint of what the box was doing.
 * The backend now coerces such a document to `wan` at apply time and logs a
 * WARN, but the *document* still says `5g`, so the row is rendered — selected,
 * not selectable, and saying why.
 */
export function modeRows(mode: Mode, modemPresent: boolean): ModeRow[] {
  return MODES.filter((option) => modemPresent || !option.needsModem || option.id === mode).map(
    (option) => ({
      id: option.id,
      label: option.label,
      blurb: option.blurb,
      selected: option.id === mode,
      unavailable:
        option.needsModem && !modemPresent
          ? "Saved on this box, but it has no 5G module — ethernet is what it actually routes through."
          : null,
    }),
  )
}

// ── Form drafts (B8: a poll tick must never destroy user input) ──────
//
// Both forms hold a PARTIAL draft — only the fields the operator has actually
// touched — which is merged over the server's latest value at render time:
//
//   • a field the operator typed in is theirs, and a poll tick cannot take it
//     back (the bug: the forms used to be REMOUNTED off the 5 s-polled query,
//     so any persisted change from another session, the boot applier or a
//     concurrent save wiped everything typed, half-typed passphrase included);
//   • a field they have NOT touched keeps tracking the server, so a change made
//     elsewhere still shows up;
//   • clearing the draft after a successful save snaps the whole form back onto
//     the server's own response — which is also what empties the write-only
//     secret fields, and it no longer depends on a remount that destroyed the
//     "Saved" confirmation on its way past (B9).

/** The AP form's editable shape: `ItNetworkAp` with the write-only passphrase
 *  added back, and `channel` as free text so a typo is a message rather than a
 *  silent `Number("abc") || 0` → "automatic" (B11). */
export interface ApDraft {
  enabled: boolean
  ssid: string
  passphrase: string
  band: ItNetworkAp["band"]
  channel: string
  country: string
  hidden: boolean
  shareInternet: boolean
}

/** Seed an AP draft from the server's projection. The passphrase always starts
 *  empty: no read route ever returns it — the projection carries only
 *  `passphrase_set` — and leaving the field empty is what keeps the stored
 *  key. */
export function apDraftFrom(ap: ItNetworkAp): ApDraft {
  return {
    enabled: ap.enabled,
    ssid: ap.ssid,
    passphrase: "",
    band: ap.band,
    channel: String(ap.channel),
    country: ap.country,
    hidden: ap.hidden,
    shareInternet: ap.share_internet,
  }
}

/** The `POST /api/it/network/ap` body for a draft. */
export function apBody(draft: ApDraft): ApBody {
  return {
    enabled: draft.enabled,
    ssid: draft.ssid.trim(),
    // Omitted entirely when untouched: absent means "keep the stored key",
    // which is the only way a UI that never receives the key can save the rest
    // of the form without wiping it.
    ...(draft.passphrase !== "" && { passphrase: draft.passphrase }),
    band: draft.band,
    channel: parseChannel(draft.channel) ?? 0,
    country: draft.country.trim().toUpperCase(),
    hidden: draft.hidden,
    share_internet: draft.shareInternet,
  }
}

/** The bearer form's editable shape: `ItNetworkWwan` with the two write-only
 *  credentials added back. */
export interface WwanDraft {
  apn: string
  username: string
  password: string
  pin: string
  roaming: boolean
  standby: ItNetworkWwan["standby"]
}

/** Seed a bearer draft from the server's projection. */
export function wwanDraftFrom(wwan: ItNetworkWwan): WwanDraft {
  return {
    apn: wwan.apn,
    username: wwan.username ?? "",
    password: "",
    pin: "",
    roaming: wwan.roaming,
    standby: wwan.standby,
  }
}

/** The `POST /api/it/network/wwan` body for a draft. */
export function wwanBody(draft: WwanDraft): WwanBody {
  const username = draft.username.trim()
  return {
    apn: draft.apn.trim(),
    username: username === "" ? null : username,
    ...(draft.password !== "" && { password: draft.password }),
    ...(draft.pin !== "" && { pin: draft.pin }),
    roaming: draft.roaming,
    standby: draft.standby,
  }
}

// ── Validation: a transcription of `state.rs`, rule for rule ─────────

/** `state.rs::valid_channel`'s 5 GHz list — the union of the UNII channels
 *  `nmcli` accepts. Which of them are usable is the regulatory domain's call at
 *  run time (`cp-regdom`), not this list's. */
const FIVE_GHZ_CHANNELS = new Set([
  36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144, 149,
  153, 157, 161, 165,
])

/** Unicode code points, which is what Rust's `chars().count()` counts on the
 *  other side of the wire. `String.length` counts UTF-16 code units, so it and
 *  the server would disagree on any astral character — rare in a passphrase or
 *  an APN, and precisely the kind of disagreement that produces an unexplained
 *  rejection. `/./gsu` is one match per code point, `s` so a newline counts.
 *
 *  Exported for `./sms.ts`, which counts a message body against the same Rust
 *  rule and where the astral character is an emoji rather than a rarity. One
 *  copy, because two would drift and the drift would be invisible. */
export function codePointCount(text: string): number {
  return text.match(/./gsu)?.length ?? 0
}

/** A channel as the server will parse it, or null when it is not a whole
 *  number. `channel: Number(x) || 0` used to turn `"abc"` into "automatic" with
 *  no feedback at all (B11). */
function parseChannel(raw: string): number | null {
  const text = raw.trim()
  if (!/^\d{1,5}$/.test(text)) return null
  const value = Number(text)
  return value <= 65_535 ? value : null
}

/** `state.rs::valid_channel`. `0` always means "let the driver pick". */
function validChannel(band: ItNetworkAp["band"], channel: number): boolean {
  if (channel === 0) return true
  return band === "bg" ? channel >= 1 && channel <= 14 : FIVE_GHZ_CHANNELS.has(channel)
}

/** `state.rs::valid_country` — two ASCII letters (`FR`, `DE`, …). */
function validCountry(country: string): boolean {
  return /^[a-z]{2}$/i.test(country)
}

/** `state.rs::valid_pin` — 4–8 decimal digits. Anything else is refused by the
 *  modem and, worse, could burn one of the three unlock retries, which is why
 *  the client checks it before the PIN ever leaves the browser. */
function validPin(pin: string): boolean {
  return /^\d{4,8}$/.test(pin)
}

/** The unconditional half of `state.rs::validate_ap`, in the server's own order
 *  and returning the server's own messages. */
function apFieldProblem(draft: ApDraft): string | null {
  const ssidBytes = new TextEncoder().encode(draft.ssid.trim()).length
  if (ssidBytes < 1 || ssidBytes > 32) return "ssid must be 1–32 bytes"
  // Only a passphrase being SENT is checked: an empty field means "keep the
  // stored key", and the stored key was validated when it was set.
  const passphraseLength = codePointCount(draft.passphrase)
  if (passphraseLength > 0 && (passphraseLength < 8 || passphraseLength > 63)) {
    return "passphrase must be 8–63 characters"
  }
  const country = draft.country.trim()
  if (country !== "" && !validCountry(country)) {
    return "country must be a two-letter ISO-3166 code"
  }
  const channel = parseChannel(draft.channel)
  // Client-only: the server never sees a non-numeric channel because this form
  // is what turns the field into a number.
  if (channel === null) return "channel must be a whole number (0 = automatic)"
  if (!validChannel(draft.band, channel)) {
    return "channel is not valid for the selected band (0 = automatic)"
  }
  return null
}

/** `state.rs::enabled_ap_prerequisites` — the two things an AP needs before it
 *  can legally beacon. `passphraseSet` stands in for the stored key the client
 *  is never sent. */
function apEnabledProblem(draft: ApDraft, passphraseSet: boolean): string | null {
  if (draft.country.trim() === "") {
    return "a regulatory country code is required before the access point can be enabled"
  }
  if (!passphraseSet && draft.passphrase === "") {
    return "a passphrase is required before the access point can be enabled"
  }
  return null
}

/**
 * `state.rs::validate_ap` in full: why this AP form cannot be saved, or null.
 *
 * Note what is deliberately NOT conditional on `enabled`: a malformed country,
 * an out-of-range SSID and an illegal channel are refused on a disabled AP too,
 * exactly as the server refuses them.
 */
export function apProblem(draft: ApDraft, passphraseSet: boolean): string | null {
  return apFieldProblem(draft) ?? (draft.enabled ? apEnabledProblem(draft, passphraseSet) : null)
}

/** `state.rs::validate_wwan`: why this bearer form cannot be saved, or null. */
export function wwanProblem(draft: WwanDraft): string | null {
  const apn = draft.apn.trim()
  if (codePointCount(apn) > 100) return "apn is too long"
  if (!/^[\w.-]*$/.test(apn)) {
    return "apn may only contain letters, digits, '.', '-' and '_'"
  }
  // As with the passphrase: only a PIN being sent is checked.
  if (draft.pin !== "" && !validPin(draft.pin)) return "pin must be 4–8 digits"
  return null
}

// ── Save confirmation (C4) ───────────────────────────────────────────

/**
 * What "Saved" should actually say, given the `applied` flag all three POSTs
 * return and nothing used to read.
 *
 * The server answers `applied: false` when the applier is inert — no `nmcli`
 * gate in this environment — which is the difference between a working access
 * point and a stored intention. Printing the same "Saved" for both was the
 * cockpit claiming something it had not been told.
 */
export function savedLabel(applied: boolean): string {
  return applied ? "Saved & applied" : "Saved — stored only, not applied to the hardware"
}

// ── Cache patches ────────────────────────────────────────────────────
//
// Each POST answers with the server's own view of what it just persisted, so
// the form can snap straight onto it instead of showing the previous value for
// the length of a refetch. Pure functions, so the pane's mutation callback is
// one line and identical on both twins.

/** Fold a mode POST's result into the cached `GET /api/it/network`. */
export function withMode(previous: ItNetworkResponse | undefined, mode: Mode) {
  if (previous === undefined) return previous
  return { ...previous, config: { ...previous.config, mode } }
}

/** Fold an AP POST's result into the cached `GET /api/it/network`. */
export function withAp(previous: ItNetworkResponse | undefined, ap: ItNetworkAp) {
  if (previous === undefined) return previous
  return { ...previous, config: { ...previous.config, ap } }
}

/** Fold a bearer POST's result into the cached `GET /api/it/network`. */
export function withWwan(previous: ItNetworkResponse | undefined, wwan: ItNetworkWwan) {
  if (previous === undefined) return previous
  return { ...previous, config: { ...previous.config, wwan } }
}
