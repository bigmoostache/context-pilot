// Gestion de la connexion : login HTTP, WebSocket, reconnexion, requêtes corrélées.

import { useNestor } from './store'
import type { ServerFrame, WebCommand, WebQuery } from './types'

const TOKEN_KEY = 'nestor.token'

let socket: WebSocket | null = null
let reconnectTimer: number | null = null
let backoffMs = 1000
let reqCounter = 0
const pending = new Map<string, (data: unknown) => void>()

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY)
}

export async function login(password: string, deviceName: string): Promise<string> {
  const res = await fetch('/api/login', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ password, device_name: deviceName }),
  })
  if (res.status === 429) throw new Error('Trop d’essais — réessaie dans une seconde.')
  if (res.status === 401) throw new Error('Mot de passe incorrect.')
  if (!res.ok) throw new Error(`Erreur serveur (${res.status}).`)
  const body = (await res.json()) as { token: string }
  localStorage.setItem(TOKEN_KEY, body.token)
  return body.token
}

export function logout() {
  localStorage.removeItem(TOKEN_KEY)
  socket?.close()
  socket = null
  useNestor.getState().reset()
}

/** Ouvre (ou ré-ouvre) la connexion WebSocket avec le token stocké. */
export function connect() {
  const token = getToken()
  if (!token) {
    useNestor.getState().setConn('login')
    return
  }
  if (socket && (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)) return

  useNestor.getState().setConn('connecting')
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const ws = new WebSocket(`${proto}://${location.host}/ws?token=${encodeURIComponent(token)}`)
  socket = ws

  ws.onopen = () => {
    backoffMs = 1000
  }
  ws.onmessage = (event) => {
    const frame = JSON.parse(event.data as string) as ServerFrame
    if (frame.t === 'result') {
      pending.get(frame.req_id)?.(frame.data)
      pending.delete(frame.req_id)
      return
    }
    useNestor.getState().applyFrame(frame)
  }
  ws.onclose = async () => {
    if (socket !== ws) return
    socket = null
    // Bascule de projet en cours : le process redémarre, on retente vite
    // sans vérifier le token (l'API est down pendant ~2 s).
    if (useNestor.getState().switchingTo !== null) {
      useNestor.getState().setConn('connecting')
      if (reconnectTimer) clearTimeout(reconnectTimer)
      reconnectTimer = window.setTimeout(() => {
        reconnectTimer = null
        connect()
      }, 1000)
      return
    }
    // Token révoqué ? On vérifie via l'API ; sinon reconnexion avec backoff.
    const check = await fetch('/api/devices', { headers: { authorization: `Bearer ${token}` } }).catch(() => null)
    if (check && check.status === 401) {
      logout()
      return
    }
    useNestor.getState().setConn('offline')
    if (reconnectTimer) clearTimeout(reconnectTimer)
    reconnectTimer = window.setTimeout(() => {
      reconnectTimer = null
      connect()
    }, backoffMs)
    backoffMs = Math.min(backoffMs * 2, 15000)
  }
}

/** Envoie une commande (fire-and-forget). */
export function send(cmd: WebCommand) {
  if (socket?.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify({ t: 'cmd', ...cmd }))
  }
}

/** Envoie une requête corrélée ; résout avec `data` de la trame result. */
export function query<T = unknown>(payload: WebQuery, timeoutMs = 5000): Promise<T> {
  return new Promise((resolve, reject) => {
    if (socket?.readyState !== WebSocket.OPEN) {
      reject(new Error('Hors ligne'))
      return
    }
    reqCounter += 1
    const reqId = `q${reqCounter}`
    pending.set(reqId, (data) => resolve(data as T))
    socket.send(JSON.stringify({ t: 'query', req_id: reqId, ...payload }))
    window.setTimeout(() => {
      if (pending.delete(reqId)) reject(new Error('Délai dépassé'))
    }, timeoutMs)
  })
}

export async function fetchDevices() {
  const token = getToken()
  const res = await fetch('/api/devices', { headers: { authorization: `Bearer ${token}` } })
  if (!res.ok) throw new Error(`Erreur ${res.status}`)
  return res.json()
}

export async function revokeDevice(deviceId: string) {
  const token = getToken()
  await fetch('/api/devices/revoke', {
    method: 'POST',
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: JSON.stringify({ device_id: deviceId }),
  })
}

// ─── Projets (workspaces) ────────────────────────────────────────────────────

function authHeaders(): Record<string, string> {
  return { authorization: `Bearer ${getToken()}`, 'content-type': 'application/json' }
}

async function expectOk(res: Response): Promise<void> {
  if (!res.ok) throw new Error((await res.text()) || `Erreur ${res.status}`)
}

export async function fetchProjects(): Promise<{ projects: import('./types').ProjectInfo[]; current: string | null }> {
  const res = await fetch('/api/projects', { headers: authHeaders() })
  await expectOk(res)
  return res.json()
}

/** Crée un projet (clone git optionnel — la requête dure le temps du clone). */
export async function createProject(name: string, gitUrl?: string): Promise<void> {
  const res = await fetch('/api/projects', {
    method: 'POST',
    headers: authHeaders(),
    body: JSON.stringify({ name, ...(gitUrl?.trim() ? { git_url: gitUrl.trim() } : {}) }),
  })
  await expectOk(res)
}

/** Demande la bascule ; l'overlay d'attente est armé tout de suite
    (le cœur enverra aussi un « bye » avant de redémarrer). */
export async function switchProject(name: string): Promise<void> {
  useNestor.getState().setSwitching(name)
  const res = await fetch('/api/projects/switch', {
    method: 'POST',
    headers: authHeaders(),
    body: JSON.stringify({ name }),
  })
  if (!res.ok) {
    useNestor.getState().setSwitching(null)
    throw new Error((await res.text()) || `Erreur ${res.status}`)
  }
}

export async function archiveProject(name: string): Promise<void> {
  const res = await fetch('/api/projects/archive', {
    method: 'POST',
    headers: authHeaders(),
    body: JSON.stringify({ name }),
  })
  await expectOk(res)
}

export async function deleteProject(name: string, confirm: string): Promise<void> {
  const res = await fetch('/api/projects/delete', {
    method: 'POST',
    headers: authHeaders(),
    body: JSON.stringify({ name, confirm }),
  })
  await expectOk(res)
}

// ─── Paramètres généraux ─────────────────────────────────────────────────────

async function getJson<T>(url: string): Promise<T> {
  const res = await fetch(url, { headers: authHeaders() })
  await expectOk(res)
  return res.json()
}

async function postJson(url: string, body: unknown): Promise<void> {
  const res = await fetch(url, { method: 'POST', headers: authHeaders(), body: JSON.stringify(body) })
  await expectOk(res)
}

export interface SystemInfo {
  hostname: string
  ip: string
  version: string
  uptime_s: number
  mem_total_kb: number
  mem_available_kb: number
  cpu_temp_milli_c: number
  disk_total_bytes: number
  disk_avail_bytes: number
  projects_root: string | null
}

export interface WifiNetwork {
  ssid: string
  signal: number
  security: string
  active: boolean
}

export interface EnvKey {
  key: string
  label: string
  set: boolean
  masked: string | null
}

export interface ProjectDefaults {
  provider?: string | null
  model?: string | null
}

export const fetchSystemInfo = () => getJson<SystemInfo>('/api/system/info')
export const fetchWifi = () =>
  getJson<{ ip: string; current: string | null; networks: WifiNetwork[] }>('/api/system/wifi')
export const connectWifi = (ssid: string, password?: string) =>
  postJson('/api/system/wifi/connect', { ssid, ...(password ? { password } : {}) })
export const fetchEnvKeys = () => getJson<{ keys: EnvKey[]; claude_oauth: boolean }>('/api/system/env')
export const setEnvKey = (key: string, value: string | null) => postJson('/api/system/env', { key, value })
export const fetchDefaults = () => getJson<ProjectDefaults>('/api/projects/defaults')
export const saveDefaults = (defaults: ProjectDefaults) => postJson('/api/projects/defaults', defaults)
export const restartService = () => postJson('/api/system/restart', {})
export const rebootPi = () => postJson('/api/system/reboot', {})
export const changePassword = (current: string, newPassword: string, revokeOthers: boolean) =>
  postJson('/api/auth/password', { current, new_password: newPassword, revoke_others: revokeOthers })
