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
} from "./generated/types.gen"
import {
  getApiItCaFingerprint,
  getApiItIdentity,
  postApiItIdentity,
  getApiItProvisioned,
} from "./generated"
import { sdk, getToken, BASE } from "./client"

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
