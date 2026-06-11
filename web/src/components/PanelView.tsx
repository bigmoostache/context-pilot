import { useMemo } from 'react'
import { X } from 'lucide-react'
import { useNestor } from '@/lib/store'
import { fmtAgo, fmtTokens, langFromPath, panelIcon } from '@/lib/utils'
import { CodeBlock } from './CodeBlock'
import { Markdown } from './Markdown'

/** Volet du panneau actif (à droite). La conversation vit au centre ;
    tout autre panneau sélectionné s'affiche ici. */
export function PanelView() {
  const active = useNestor((s) => s.state?.active_panel)
  const panels = useNestor((s) => s.state?.panels)
  const panelOpen = useNestor((s) => s.panelOpen)
  const setPanelOpen = useNestor((s) => s.setPanelOpen)
  const meta = useMemo(() => panels?.find((p) => p.selected), [panels])

  if (!panelOpen || !active || active.kind === 'conversation') return null

  const filePath = typeof active.metadata['file_path'] === 'string' ? (active.metadata['file_path'] as string) : null

  return (
    <section className="flex w-[34rem] shrink-0 flex-col border-r border-coal-800 bg-coal-900/40 animate-rise">
      <header className="flex items-center gap-2 border-b border-coal-800 px-4 py-2.5">
        <span>{panelIcon(active.kind)}</span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium text-parchment-100">{active.name}</div>
          <div className="font-mono text-[0.65rem] text-parchment-700">
            {active.id} · {active.kind}
            {meta && <> · {fmtTokens(meta.token_count)} tokens · {fmtAgo(meta.last_refresh_ms)}</>}
          </div>
        </div>
        <button
          onClick={() => setPanelOpen(false)}
          title="Fermer l'écran (rouvrir via le menu)"
          className="rounded-md p-1.5 text-parchment-500 hover:bg-coal-800 hover:text-parchment-100 cursor-pointer"
        >
          <X className="size-4" />
        </button>
      </header>
      <div className="flex-1 overflow-y-auto p-4">
        {filePath ? (
          <CodeBlock code={active.content ?? ''} lang={langFromPath(filePath)} />
        ) : active.kind === 'scratchpad' || active.kind === 'memory' || active.kind === 'library' ? (
          <Markdown>{active.content ?? ''}</Markdown>
        ) : (
          <pre className="whitespace-pre-wrap font-mono text-[0.78rem] leading-relaxed text-parchment-300">
            {active.content ?? ''}
          </pre>
        )}
      </div>
    </section>
  )
}
