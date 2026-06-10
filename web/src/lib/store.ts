// Store Zustand : l'état miroir (snapshot + deltas) et l'état de connexion.

import { create } from 'zustand'
import type { ServerFrame, WebMessage, WebState } from './types'

export type ConnState = 'login' | 'connecting' | 'online' | 'offline'

interface NestorStore {
  conn: ConnState
  state: WebState | null
  lastError: string | null
  setConn: (conn: ConnState) => void
  setError: (message: string | null) => void
  applyFrame: (frame: ServerFrame) => void
  reset: () => void
}

/** Upsert des messages par id (l'ordre d'arrivée suit l'ordre serveur). */
function upsertMessages(current: WebMessage[], upserts: WebMessage[]): WebMessage[] {
  const byId = new Map(current.map((m) => [m.id, m]))
  const next = [...current]
  for (const msg of upserts) {
    if (byId.has(msg.id)) {
      const idx = next.findIndex((m) => m.id === msg.id)
      if (idx >= 0) next[idx] = msg
    } else {
      next.push(msg)
    }
  }
  return next
}

export const useNestor = create<NestorStore>((set) => ({
  conn: 'login',
  state: null,
  lastError: null,
  setConn: (conn) => set({ conn }),
  setError: (lastError) => set({ lastError }),
  reset: () => set({ state: null, conn: 'login' }),

  applyFrame: (frame) =>
    set((store) => {
      switch (frame.t) {
        case 'snapshot':
          return { state: frame.state, conn: 'online' as const }
        case 'delta': {
          if (!store.state) return {}
          const next: WebState = { ...store.state }
          if (frame.status !== undefined) next.status = frame.status
          if (frame.panels !== undefined) next.panels = frame.panels
          if (frame.active_panel !== undefined) next.active_panel = frame.active_panel
          if (frame.question_form !== undefined) next.question_form = frame.question_form
          if (frame.input_draft !== undefined) next.input_draft = frame.input_draft
          if (frame.conversation_upsert) {
            next.conversation = upsertMessages(next.conversation, frame.conversation_upsert)
          }
          if (frame.conversation_remove?.length) {
            const gone = new Set(frame.conversation_remove)
            next.conversation = next.conversation.filter((m) => !gone.has(m.id))
          }
          return { state: next }
        }
        case 'append': {
          if (!store.state) return {}
          const conversation = store.state.conversation.map((m) =>
            m.id === frame.id ? { ...m, content: m.content + frame.text } : m,
          )
          return { state: { ...store.state, conversation } }
        }
        case 'error':
          return { lastError: frame.message }
        default:
          return {}
      }
    }),
}))
