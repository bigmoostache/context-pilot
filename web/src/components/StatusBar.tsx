import { Activity, Bone, Moon, ShieldAlert, Wifi, WifiOff } from 'lucide-react'
import { useNestor } from '@/lib/store'
import { cn, fmtTokens } from '@/lib/utils'

/** Bandeau du bas : l'état de la machine en un coup d'œil (cockpit). */
export function StatusBar() {
  const conn = useNestor((s) => s.conn)
  const status = useNestor((s) => s.state?.status)
  if (!status) return null

  const phaseLabel = {
    idle: 'PRÊT',
    receiving: 'EN RÉPONSE',
    executing_tools: 'OUTILS',
  }[status.stream_phase]

  return (
    <footer className="flex h-8 items-center gap-4 border-t border-coal-800 bg-coal-900/80 px-4 font-mono text-[0.7rem] text-parchment-500">
      <span
        className={cn(
          'flex items-center gap-1.5 font-medium',
          status.stream_phase === 'idle' ? 'text-sage-400' : 'text-brass-400',
        )}
      >
        <Activity className={cn('size-3', status.stream_phase !== 'idle' && 'animate-pulse')} />
        {phaseLabel}
      </span>

      <span title="Modèle actif">
        {status.provider} · {status.model}
      </span>

      <span title="Auto-continuation (spine)" className="flex items-center gap-1">
        <Bone className="size-3" />
        {status.auto_continue ? 'auto' : 'manuel'}
        {status.spine_notifications > 0 && (
          <span className="rounded bg-brass-500/20 px-1 text-brass-300">{status.spine_notifications}</span>
        )}
      </span>

      {status.reverie_enabled && (
        <span className="flex items-center gap-1" title="Reverie (optimiseur de contexte)">
          <Moon className="size-3" /> reverie
        </span>
      )}

      {status.guard_rail_blocked && (
        <span className="flex items-center gap-1 text-ember-400" title={status.guard_rail_blocked}>
          <ShieldAlert className="size-3" /> garde-fou
        </span>
      )}

      <span className="ml-auto" title="Tokens de session (hit/miss/out)">
        {fmtTokens(status.session_tokens.cache_hit)} hit · {fmtTokens(status.session_tokens.cache_miss)} miss ·{' '}
        {fmtTokens(status.session_tokens.output)} out
      </span>

      <span
        className={cn('flex items-center gap-1', conn === 'online' ? 'text-sage-400' : 'text-ember-400')}
        title="Connexion à la Pi"
      >
        {conn === 'online' ? <Wifi className="size-3" /> : <WifiOff className="size-3" />}
        {conn === 'online' ? 'pi' : conn}
      </span>
    </footer>
  )
}
