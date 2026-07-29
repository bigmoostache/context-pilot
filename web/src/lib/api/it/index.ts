// ── IT-infrastructure API client (design §13.5) ──────────────────────
//
// The IT Settings section (`can_manage_it`, admin+) talks to the product REST
// routes `/api/it/*` on `:443` — the maintenance-plane functions re-homed onto
// the product API. Everything routes through the generated SDK except the raw
// CA-root download, which is a binary blob (irreducible manual fetch, like
// finder's `downloadFile`).

import type {
  ItFingerprint,
  ItIdentityResponse,
  ItSetIdentityResponse,
  ItNetworkResponse,
  ItNetworkApResult,
  ItNetworkModeResult,
  ItNetworkWwanResult,
  PostApiItNetworkApData,
  PostApiItNetworkModeData,
  PostApiItNetworkWwanData,
} from "../generated/types.gen"
import {
  getApiItCaFingerprint,
  getApiItIdentity,
  postApiItIdentity,
  getApiItProvisioned,
  getApiItNetwork,
  postApiItNetworkAp,
  postApiItNetworkMode,
  postApiItNetworkWwan,
} from "../generated"
import { sdk, getToken, BASE } from "../client"

// Body shapes for the three network writes. Local aliases, not exports: they
// are only the parameter types of the three functions below — every caller
// passes an object literal or a body built in `./network`, so
// nothing ever named them (C7). The `NonNullable<…>` wrapper they used to carry
// was a no-op too: `body` is non-optional on all three `…Data` types.
type ItNetworkModeBody = PostApiItNetworkModeData["body"]
type ItNetworkApBody = PostApiItNetworkApData["body"]
type ItNetworkWwanBody = PostApiItNetworkWwanData["body"]

// ── Endpoints (SDK) ──────────────────────────────────────────────────

/** CA-root SHA-256 fingerprint for out-of-band verification (`can_manage_it`). */
export function fetchItCaFingerprint(): Promise<ItFingerprint> {
  return sdk(getApiItCaFingerprint())
}

/** Current box network identity (name/IP), or `{ identity: null }` when unset. */
export function fetchItIdentity(): Promise<ItIdentityResponse> {
  return sdk(getApiItIdentity())
}

/** Set the box name/IP — the backend re-issues the private-CA leaf and reloads
 *  Caddy. Rejects an invalid name/IP with a 400 (thrown by the SDK). */
export function setItIdentity(name: string, ip: string): Promise<ItSetIdentityResponse> {
  return sdk(postApiItIdentity({ body: { name, ip } }))
}

/** Whether the box has been provisioned (`can_manage_it`). */
export function fetchItProvisioned(): Promise<{ provisioned: boolean }> {
  return sdk(getApiItProvisioned())
}

// ── Internet uplink + Wi-Fi access point (design-network-uplink §10) ─

/** Uplink + AP configuration (secrets elided) plus live status. Polled while
 *  the network pane is open, so the status card tracks a failover without a
 *  manual refresh. */
export function fetchItNetwork(): Promise<ItNetworkResponse> {
  return sdk(getApiItNetwork())
}

/** Select the uplink mode. `5g` suppresses the ethernet default route even with
 *  a cable plugged in; a failed apply is rolled back and surfaces as a 502. */
export function setItNetworkMode(body: ItNetworkModeBody): Promise<ItNetworkModeResult> {
  return sdk(postApiItNetworkMode({ body }))
}

/** Set the access-point configuration. `passphrase` is write-only: omit the
 *  field to keep the stored PSK, send null to clear it. Enabling the AP without
 *  a regulatory country is a 400 (FR-NET-14). */
export function setItNetworkAp(body: ItNetworkApBody): Promise<ItNetworkApResult> {
  return sdk(postApiItNetworkAp({ body }))
}

/** Set the 5G bearer configuration. `password` and `pin` are write-only, with
 *  the same omit-to-keep semantics as the AP passphrase. */
export function setItNetworkWwan(body: ItNetworkWwanBody): Promise<ItNetworkWwanResult> {
  return sdk(postApiItNetworkWwan({ body }))
}

// ── CA-root download (irreducible binary blob) ───────────────────────

/** Download the CA root via an authenticated fetch (a plain `<a>` can't carry
 *  the Bearer token), then trigger a browser "save as" for `root.crt`. Mirrors
 *  finder's `downloadFile` auth + revoke-defer handling. */
export async function downloadItCaCert(): Promise<void> {
  const headers: Record<string, string> = {}
  const token = getToken()
  if (token) headers["Authorization"] = `Bearer ${token}`
  const res = await fetch(`${BASE}/api/it/ca.crt`, { headers }) // ok:manual — binary CA-root blob, irreducible
  if (!res.ok) throw new Error(`CA download failed (HTTP ${res.status})`)
  const blob = await res.blob()
  const url = URL.createObjectURL(blob)
  const a = document.createElement("a")
  a.href = url
  a.download = "root.crt"
  document.body.append(a)
  a.click()
  a.remove()
  // Defer revoke: revoking synchronously after click() can abort the download
  // in some browsers.
  window.setTimeout(() => URL.revokeObjectURL(url), 10_000)
}
