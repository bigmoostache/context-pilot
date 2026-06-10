import { useEffect, useRef, useState } from 'react'
import { CircleStop, CornerDownLeft, Folder, FileText } from 'lucide-react'
import { useNestor } from '@/lib/store'
import { query, send } from '@/lib/ws'
import { cn } from '@/lib/utils'
import type { DirEntry } from '@/lib/types'
import { Button } from '@/components/ui/button'

/** Zone de saisie — possédée par le navigateur (décision n°8 du contrat).
    Enter envoie, Shift+Enter saute une ligne, Esc stoppe le stream,
    ↑/↓ naviguent l'historique, `@` ouvre l'autocomplete de fichiers. */
export function Composer() {
  const phase = useNestor((s) => s.state?.status.stream_phase ?? 'idle')
  const [text, setText] = useState('')
  const [history, setHistory] = useState<string[]>([])
  const [histIdx, setHistIdx] = useState(-1)
  const draftRef = useRef('')
  const areaRef = useRef<HTMLTextAreaElement>(null)

  // Autocomplete @
  const [acEntries, setAcEntries] = useState<DirEntry[] | null>(null)
  const [acSel, setAcSel] = useState(0)
  const acAnchor = useRef(0)

  const streaming = phase !== 'idle'

  useEffect(() => {
    // Auto-redimensionnement du textarea
    const el = areaRef.current
    if (el) {
      el.style.height = 'auto'
      el.style.height = `${Math.min(el.scrollHeight, 220)}px`
    }
  }, [text])

  function submit() {
    const trimmed = text.trim()
    if (!trimmed || streaming) return
    send({ cmd: 'submit', text: trimmed })
    setHistory((h) => [...h, trimmed])
    setHistIdx(-1)
    setText('')
    closeAc()
  }

  // ─── Autocomplete ────────────────────────────────────────────────────
  function closeAc() {
    setAcEntries(null)
    setAcSel(0)
  }

  async function refreshAc(value: string, cursor: number) {
    const at = value.lastIndexOf('@', cursor - 1)
    if (at < 0 || (at > 0 && !/[\s\n]/.test(value[at - 1] ?? ''))) {
      closeAc()
      return
    }
    const q = value.slice(at + 1, cursor)
    if (/[\s\n]/.test(q)) {
      closeAc()
      return
    }
    acAnchor.current = at
    const slash = q.lastIndexOf('/')
    const dir = slash >= 0 ? q.slice(0, slash) : ''
    const prefix = slash >= 0 ? q.slice(slash + 1) : q
    try {
      const res = await query<{ entries: DirEntry[] }>({ q: 'list_dir', dir, prefix })
      setAcEntries(res.entries.slice(0, 10))
      setAcSel(0)
    } catch {
      closeAc()
    }
  }

  function acceptAc(entry: DirEntry) {
    const el = areaRef.current
    if (!el) return
    const cursor = el.selectionStart
    const at = acAnchor.current
    const q = text.slice(at + 1, cursor)
    const slash = q.lastIndexOf('/')
    const dir = slash >= 0 ? q.slice(0, slash) : ''
    const full = dir ? `${dir}/${entry.name}` : entry.name
    if (entry.is_dir) {
      const next = `${text.slice(0, at)}@${full}/${text.slice(cursor)}`
      setText(next)
      requestAnimationFrame(() => {
        const pos = at + 1 + full.length + 1
        el.setSelectionRange(pos, pos)
        void refreshAc(next, pos)
      })
    } else {
      const next = `${text.slice(0, at)}${full} ${text.slice(cursor)}`
      setText(next)
      closeAc()
      requestAnimationFrame(() => {
        const pos = at + full.length + 1
        el.setSelectionRange(pos, pos)
      })
    }
  }

  // ─── Historique ──────────────────────────────────────────────────────
  async function ensureHistory(): Promise<string[]> {
    if (history.length > 0) return history
    try {
      const res = await query<{ entries: string[] }>({ q: 'prompt_history', limit: 100 })
      const entries = [...res.entries].reverse() // plus ancien → plus récent
      setHistory(entries)
      return entries
    } catch {
      return history
    }
  }

  async function navigateHistory(dir: -1 | 1) {
    const entries = await ensureHistory()
    if (entries.length === 0) return
    let idx = histIdx
    if (dir === -1) {
      if (idx === -1) {
        draftRef.current = text
        idx = entries.length - 1
      } else if (idx > 0) idx -= 1
    } else {
      if (idx === -1) return
      idx += 1
      if (idx >= entries.length) {
        setHistIdx(-1)
        setText(draftRef.current)
        return
      }
    }
    setHistIdx(idx)
    setText(entries[idx] ?? '')
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (acEntries && acEntries.length > 0) {
      if (e.key === 'ArrowDown') { e.preventDefault(); setAcSel((s) => Math.min(s + 1, acEntries.length - 1)); return }
      if (e.key === 'ArrowUp') { e.preventDefault(); setAcSel((s) => Math.max(s - 1, 0)); return }
      if (e.key === 'Tab' || e.key === 'Enter') { e.preventDefault(); acceptAc(acEntries[acSel]!); return }
      if (e.key === 'Escape') { e.preventDefault(); closeAc(); return }
    }
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); submit(); return }
    if (e.key === 'Escape' && streaming) { e.preventDefault(); send({ cmd: 'stop' }); return }
    const el = areaRef.current
    const atLineEdge = el && el.selectionStart === el.selectionEnd
    if (e.key === 'ArrowUp' && atLineEdge && (e.ctrlKey || cursorOnFirstLine(el))) {
      e.preventDefault()
      void navigateHistory(-1)
      return
    }
    if (e.key === 'ArrowDown' && atLineEdge && (e.ctrlKey || cursorOnLastLine(el))) {
      e.preventDefault()
      void navigateHistory(1)
    }
  }

  function onChange(e: React.ChangeEvent<HTMLTextAreaElement>) {
    setText(e.target.value)
    setHistIdx(-1)
    void refreshAc(e.target.value, e.target.selectionStart)
  }

  return (
    <div className="border-t border-coal-800 bg-coal-900/70 backdrop-blur">
      <div className="relative mx-auto max-w-3xl px-6 py-4">
        {/* Popup autocomplete */}
        {acEntries && acEntries.length > 0 && (
          <div className="absolute bottom-full left-6 right-6 z-10 mb-1 overflow-hidden rounded-lg border border-coal-700 bg-coal-850 shadow-xl shadow-black/50">
            {acEntries.map((entry, i) => (
              <button
                key={entry.name}
                onClick={() => acceptAc(entry)}
                className={cn(
                  'flex w-full items-center gap-2 px-3 py-1.5 text-left font-mono text-xs cursor-pointer',
                  i === acSel ? 'bg-brass-500/15 text-brass-300' : 'text-parchment-300 hover:bg-coal-800',
                )}
              >
                {entry.is_dir ? <Folder className="size-3.5" /> : <FileText className="size-3.5" />}
                {entry.name}
                {entry.is_dir && '/'}
              </button>
            ))}
          </div>
        )}

        <div className="flex items-end gap-2 rounded-xl border border-coal-700 bg-coal-900 p-2 focus-within:border-brass-600/60 transition-colors">
          <textarea
            ref={areaRef}
            value={text}
            onChange={onChange}
            onKeyDown={onKeyDown}
            rows={1}
            placeholder="Une tâche pour Nestor…  (@fichier pour référencer, ↑ pour l’historique)"
            className="max-h-[220px] flex-1 resize-none bg-transparent px-2 py-1.5 text-[0.925rem] text-parchment-100 placeholder:text-parchment-700 focus:outline-none"
          />
          {streaming ? (
            <Button variant="danger" size="sm" onClick={() => send({ cmd: 'stop' })} title="Stopper (Esc)">
              <CircleStop className="size-4" />
              Stop
            </Button>
          ) : (
            <Button size="sm" onClick={submit} disabled={!text.trim()} title="Envoyer (Enter)">
              <CornerDownLeft className="size-4" />
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}

function cursorOnFirstLine(el: HTMLTextAreaElement | null): boolean {
  return !!el && !el.value.slice(0, el.selectionStart).includes('\n')
}

function cursorOnLastLine(el: HTMLTextAreaElement | null): boolean {
  return !!el && !el.value.slice(el.selectionEnd).includes('\n')
}
