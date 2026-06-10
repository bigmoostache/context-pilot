import { Anchor, FolderOpen, Settings2, TerminalSquare } from 'lucide-react'
import { useNestor } from '@/lib/store'
import { send } from '@/lib/ws'
import { cn, fmtTokens, panelIcon } from '@/lib/utils'
import type { WebPanel } from '@/lib/types'

/** Rail gauche : jauge de contexte + liste des panneaux (la « salle des machines »). */
export function Sidebar({ onOpenConfig }: { onOpenConfig: () => void }) {
  const state = useNestor((s) => s.state)
  const setScreen = useNestor((s) => s.setScreen)
  if (!state) return null
  const { status, panels, meta } = state

  // La conversation est traitée comme fixe : c'est le panneau central.
  const fixed = panels.filter((p) => p.is_fixed || p.kind === 'conversation')
  const dynamic = panels.filter((p) => !p.is_fixed && p.kind !== 'conversation')

  return (
    <aside className="flex w-64 shrink-0 flex-col border-r border-coal-800 bg-coal-900/60">
      {/* En-tête : wordmark + jauge de contexte */}
      <div className="border-b border-coal-800 p-4">
        <div className="mb-3 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Anchor className="size-4 text-brass-400" />
            <span className="font-display text-2xl italic leading-none">Nestor</span>
          </div>
          <button
            onClick={onOpenConfig}
            title="Configuration (Ctrl+H)"
            className="rounded-md p-1.5 text-parchment-500 hover:bg-coal-800 hover:text-parchment-100 cursor-pointer"
          >
            <Settings2 className="size-4" />
          </button>
        </div>
        {meta.project && (
          <button
            onClick={() => setScreen('projects')}
            title="Changer de projet"
            className="mb-3 flex w-full items-center gap-1.5 rounded-md border border-coal-700 bg-coal-850 px-2 py-1.5 font-mono text-xs text-parchment-300 hover:border-brass-600/50 hover:text-brass-300 cursor-pointer"
          >
            <FolderOpen className="size-3.5 shrink-0" />
            <span className="truncate">{meta.project}</span>
            <span className="ml-auto text-[0.6rem] uppercase tracking-wider text-parchment-700">changer</span>
          </button>
        )}
        <ContextGauge
          used={status.context_used_tokens}
          budget={status.context_budget}
          window={status.context_window}
          threshold={status.cleaning_threshold}
        />
      </div>

      {/* Panneaux */}
      <nav className="flex-1 overflow-y-auto p-2">
        {fixed.map((p, i) => (
          <PanelRow key={p.id} panel={p} delay={i} />
        ))}
        {dynamic.length > 0 && (
          <>
            <div className="mt-3 mb-1 px-2 text-[0.65rem] font-medium uppercase tracking-widest text-parchment-700">
              Panneaux ouverts
            </div>
            {dynamic.map((p, i) => (
              <PanelRow key={p.id} panel={p} delay={fixed.length + i} />
            ))}
          </>
        )}
      </nav>

      <div className="border-t border-coal-800 p-3 text-[0.7rem] text-parchment-700">
        <div className="flex items-center gap-1.5">
          <TerminalSquare className="size-3.5" />
          <span>TUI de secours : ssh → nestor-tui</span>
        </div>
      </div>
    </aside>
  )
}

function PanelRow({ panel, delay }: { panel: WebPanel; delay: number }) {
  return (
    <button
      onClick={() => send({ cmd: 'select_panel', id: panel.id })}
      style={{ animationDelay: `${Math.min(delay * 30, 400)}ms` }}
      className={cn(
        'group flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm animate-rise cursor-pointer',
        panel.selected
          ? 'bg-brass-500/10 text-brass-300 shadow-[inset_2px_0_0] shadow-brass-400'
          : 'text-parchment-300 hover:bg-coal-800',
      )}
    >
      <span className="w-5 text-center">{panelIcon(panel.kind)}</span>
      <span className="flex-1 truncate">{panel.name}</span>
      {panel.total_pages > 1 && (
        <span className="font-mono text-[0.65rem] text-parchment-700">
          {panel.page + 1}/{panel.total_pages}
        </span>
      )}
      <span className="font-mono text-xs text-parchment-500 group-hover:text-parchment-300">
        {fmtTokens(panel.token_count)}
      </span>
    </button>
  )
}

/** Jauge « carburant » : tokens utilisés / seuil de nettoyage / fenêtre. */
function ContextGauge({
  used,
  budget,
  window: win,
  threshold,
}: {
  used: number
  budget: number | null
  window: number
  threshold: number
}) {
  const max = budget ?? win
  const usedPct = Math.min((used / max) * 100, 100)
  const thresholdPct = threshold * 100
  return (
    <div>
      <div className="mb-1 flex justify-between font-mono text-[0.7rem] text-parchment-500">
        <span className="text-parchment-300">{fmtTokens(used)}</span>
        <span>
          {budget ? `${fmtTokens(budget)} budget` : ''} · {fmtTokens(win)}
        </span>
      </div>
      <div className="relative h-1.5 overflow-hidden rounded-full bg-coal-700">
        <div
          className="h-full rounded-full bg-gradient-to-r from-brass-600 to-brass-400 transition-[width] duration-500"
          style={{ width: `${usedPct}%` }}
        />
        {/* Repère du seuil de nettoyage */}
        <div className="absolute top-0 h-full w-px bg-parchment-500/70" style={{ left: `${thresholdPct}%` }} />
      </div>
    </div>
  )
}
