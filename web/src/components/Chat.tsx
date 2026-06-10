import { useEffect, useMemo, useRef, useState } from 'react'
import { AlertTriangle, ChevronRight, Hammer } from 'lucide-react'
import { useNestor } from '@/lib/store'
import { cn } from '@/lib/utils'
import type { WebMessage } from '@/lib/types'
import { Markdown } from './Markdown'

/** Colonne centrale : le fil de conversation, en flux. */
export function Chat() {
  const conversation = useNestor((s) => s.state?.conversation)
  const phase = useNestor((s) => s.state?.status.stream_phase ?? 'idle')
  const streamingTool = useNestor((s) => s.state?.status.streaming_tool ?? null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const pinnedRef = useRef(true)

  const visible = useMemo(
    () => (conversation ?? []).filter((m) => m.status === 'full'),
    [conversation],
  )

  // Auto-scroll tant que l'utilisateur est « épinglé » en bas.
  useEffect(() => {
    const el = scrollRef.current
    if (el && pinnedRef.current) el.scrollTop = el.scrollHeight
  }, [visible, phase])

  function onScroll() {
    const el = scrollRef.current
    if (!el) return
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80
  }

  return (
    <div ref={scrollRef} onScroll={onScroll} className="flex-1 overflow-y-auto">
      <div className="mx-auto max-w-3xl px-6 py-6">
        {visible.length === 0 && (
          <div className="mt-24 text-center animate-rise">
            <p className="font-display text-3xl italic text-parchment-500">À l’écoute.</p>
            <p className="mt-2 text-sm text-parchment-700">
              Donne une tâche à Nestor — il continuera de travailler une fois l’onglet fermé.
            </p>
          </div>
        )}
        {visible.map((msg, i) => (
          <MessageRow key={msg.id} msg={msg} isLast={i === visible.length - 1} phase={phase} />
        ))}
        {streamingTool && (
          <div className="my-2 flex items-center gap-2 font-mono text-xs text-brass-400 animate-rise">
            <Hammer className="size-3.5 animate-pulse" />
            <span>{streamingTool.name}…</span>
          </div>
        )}
      </div>
    </div>
  )
}

function MessageRow({ msg, isLast, phase }: { msg: WebMessage; isLast: boolean; phase: string }) {
  if (msg.kind === 'tool_call') return <ToolCallCard msg={msg} />
  if (msg.kind === 'tool_result') return <ToolResultCard msg={msg} />
  if (msg.role === 'user') return <UserBubble msg={msg} />
  return <AssistantBlock msg={msg} streaming={isLast && phase !== 'idle'} />
}

function UserBubble({ msg }: { msg: WebMessage }) {
  return (
    <div className="my-4 flex justify-end animate-rise">
      <div className="max-w-[85%] rounded-2xl rounded-br-sm border border-brass-600/30 bg-brass-500/10 px-4 py-2.5">
        <div className="prose-nestor whitespace-pre-wrap">{msg.content}</div>
      </div>
    </div>
  )
}

function AssistantBlock({ msg, streaming }: { msg: WebMessage; streaming: boolean }) {
  return (
    <div className="my-4 animate-rise">
      <div className="mb-1 flex items-baseline gap-2">
        <span className="font-display text-lg italic text-brass-300">Nestor</span>
        <span className="font-mono text-[0.65rem] text-parchment-700">{msg.id}</span>
      </div>
      <Markdown>{msg.content}</Markdown>
      {streaming && (
        <span className="ml-0.5 inline-block h-4 w-2 translate-y-0.5 bg-brass-400 animate-caret" />
      )}
    </div>
  )
}

/** Appel d'outil : carte compacte repliée par défaut (divulgation progressive). */
function ToolCallCard({ msg }: { msg: WebMessage }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="my-1.5">
      {msg.content && <Markdown>{msg.content}</Markdown>}
      {msg.tool_uses.map((tu) => (
        <button
          key={tu.id}
          onClick={() => setOpen(!open)}
          className="group flex w-full items-center gap-2 rounded-md border border-coal-800 bg-coal-900/50 px-3 py-1.5 text-left font-mono text-xs text-parchment-500 hover:border-coal-700 cursor-pointer"
        >
          <ChevronRight className={cn('size-3 transition-transform', open && 'rotate-90')} />
          <Hammer className="size-3 text-tide-400" />
          <span className="text-parchment-300">{tu.name}</span>
          {!open && <span className="flex-1 truncate text-parchment-700">{JSON.stringify(tu.input)}</span>}
        </button>
      ))}
      {open &&
        msg.tool_uses.map((tu) => (
          <pre
            key={`${tu.id}-detail`}
            className="mt-1 overflow-x-auto rounded-md border border-coal-800 bg-coal-900/50 p-2.5 font-mono text-[0.7rem] leading-relaxed text-parchment-300"
          >
            {JSON.stringify(tu.input, null, 2)}
          </pre>
        ))}
    </div>
  )
}

/** Résultat d'outil : replié par défaut, erreurs signalées. */
function ToolResultCard({ msg }: { msg: WebMessage }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="my-1.5">
      {msg.tool_results.map((tr) => {
        const summary = tr.tldr ?? firstLine(tr.content)
        return (
          <div key={tr.tool_use_id}>
            <button
              onClick={() => setOpen(!open)}
              className={cn(
                'flex w-full items-center gap-2 rounded-md border px-3 py-1.5 text-left font-mono text-xs cursor-pointer',
                tr.is_error
                  ? 'border-ember-400/30 bg-ember-400/5 text-ember-400'
                  : 'border-coal-800 bg-coal-900/30 text-parchment-500 hover:border-coal-700',
              )}
            >
              <ChevronRight className={cn('size-3 shrink-0 transition-transform', open && 'rotate-90')} />
              {tr.is_error && <AlertTriangle className="size-3 shrink-0" />}
              <span className="truncate">{summary || `${tr.tool_name} → ok`}</span>
            </button>
            {open && (
              <div className="mt-1 rounded-md border border-coal-800 bg-coal-900/50 p-3">
                <Markdown>{tr.content}</Markdown>
              </div>
            )}
          </div>
        )
      })}
    </div>
  )
}

function firstLine(text: string): string {
  const line = text.split('\n', 1)[0] ?? ''
  return line.length > 120 ? `${line.slice(0, 120)}…` : line
}
