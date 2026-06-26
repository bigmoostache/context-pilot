// ── IT maintenance-plane API client ──────────────────────────────────
//
// The maintenance wizard (served on :9090, same-origin via Caddy) talks to the
// orchestrator's `/api/maint/*` routes. These are NOT in the OpenAPI contract
// (a separate Admin-gated plane), so this is a small hand-written fetch client
// rather than the generated SDK. Calls are SAME-ORIGIN (relative paths): the
// wizard bundle is served by the maintenance plane itself, so there is no
// cross-origin base URL — and the plane deliberately sends no CORS headers.

import { getToken, setToken } from "./client"

export interface MaintStatus {
  plane?: string
  bootstrapped: boolean
  provisioned: boolean
  identity_set: boolean
}

export interface MaintUser {
  id: string
  email: string
  name: string
  role: string
  must_change_password: boolean
}

export interface Identity {
  name: string
  ip: string
}

/** Same-origin JSON fetch with the Bearer token, throwing the backend's
 *  `error` message on a non-2xx response. */
async function maintFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers)
  if (init?.body) headers.set("Content-Type", "application/json")
  const token = getToken()
  if (token) headers.set("Authorization", `Bearer ${token}`)
  const res = await fetch(path, { ...init, headers })
  if (!res.ok) {
    const body = (await res.json().catch(() => ({}))) as { error?: string }
    throw new Error(body.error ?? `HTTP ${res.status}`)
  }
  return (await res.json()) as T
}

/** Probe the maintenance status (public — works before login). Returns null
 *  when this origin is NOT the maintenance plane (the route 404s on the product
 *  cockpit), which is how the SPA decides whether to render the wizard. */
export async function probeMaintPlane(): Promise<MaintStatus | null> {
  try {
    const res = await fetch("/api/maint/status")
    if (!res.ok) return null
    const data = (await res.json()) as MaintStatus
    return data.plane === "maintenance" ? data : null
  } catch {
    return null
  }
}

export function fetchMaintStatus(): Promise<MaintStatus> {
  return maintFetch<MaintStatus>("/api/maint/status")
}

export async function maintLogin(email: string, password: string): Promise<MaintUser> {
  const res = await maintFetch<{ token: string; user: MaintUser }>("/api/maint/login", {
    method: "POST",
    body: JSON.stringify({ email, password }),
  })
  setToken(res.token)
  return res.user
}

export function fetchMaintMe(): Promise<MaintUser> {
  return maintFetch<MaintUser>("/api/maint/me")
}

export function maintChangePassword(current: string, next: string): Promise<unknown> {
  return maintFetch("/api/maint/password", {
    method: "POST",
    body: JSON.stringify({ current, new: next }),
  })
}

export function maintUpdateProfile(name: string, email: string): Promise<unknown> {
  return maintFetch("/api/maint/me", {
    method: "PATCH",
    body: JSON.stringify({ name, email }),
  })
}

export function fetchIdentity(): Promise<{ identity: Identity | null }> {
  return maintFetch<{ identity: Identity | null }>("/api/maint/identity")
}

export function setIdentity(name: string, ip: string): Promise<{ identity: Identity; reloaded: boolean }> {
  return maintFetch("/api/maint/identity", {
    method: "POST",
    body: JSON.stringify({ name, ip }),
  })
}

export function fetchCaFingerprint(): Promise<{ fingerprint: string; algorithm: string }> {
  return maintFetch("/api/maint/ca/fingerprint")
}

/** Download the CA root via an authenticated fetch (a plain <a> can't carry the
 *  Bearer token), then trigger a browser "save as" for `root.crt`. */
export async function downloadCaCert(): Promise<void> {
  const token = getToken()
  const headers = new Headers()
  if (token) headers.set("Authorization", `Bearer ${token}`)
  const res = await fetch("/api/maint/ca.crt", { headers })
  if (!res.ok) throw new Error(`CA download failed (HTTP ${res.status})`)
  const blob = await res.blob()
  const url = URL.createObjectURL(blob)
  const a = document.createElement("a")
  a.href = url
  a.download = "root.crt"
  document.body.appendChild(a)
  a.click()
  a.remove()
  // Defer revoke: some browsers cancel the download if the blob URL is revoked
  // synchronously right after click().
  window.setTimeout(() => URL.revokeObjectURL(url), 10_000)
}

export function finalizeProvisioning(): Promise<{ provisioned: boolean; reloaded: boolean }> {
  return maintFetch("/api/maint/finalize", { method: "POST" })
}

export function maintLogout(): Promise<unknown> {
  const p = maintFetch("/api/maint/logout", { method: "POST" }).catch(() => undefined)
  setToken(null)
  return p
}
