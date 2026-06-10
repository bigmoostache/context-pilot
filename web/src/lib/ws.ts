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
