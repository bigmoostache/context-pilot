import { Command } from 'cmdk'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { useNestor } from '@/lib/store'
import { send } from '@/lib/ws'
import { panelIcon } from '@/lib/utils'

/** Palette de commandes (parité Ctrl+P) — cmdk, fait pour ça. */
export function Palette({
  open,
  onOpenChange,
  onOpenConfig,
  onOpenIndex,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  onOpenConfig: () => void
  onOpenIndex: () => void
}) {
  const panels = useNestor((s) => s.state?.panels ?? [])
  const streaming = useNestor((s) => (s.state?.status.stream_phase ?? 'idle') !== 'idle')

  function run(action: () => void) {
    action()
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent hideClose className="max-w-xl p-0 overflow-hidden" aria-describedby={undefined}>
        <DialogTitle className="sr-only">Palette de commandes</DialogTitle>
        <Command label="Palette de commandes" className="bg-transparent">
          <Command.Input
            autoFocus
            placeholder="Panneau, action…"
            className="h-12 w-full border-b border-coal-700 bg-transparent px-4 text-parchment-100 placeholder:text-parchment-700 focus:outline-none"
          />
          <Command.List className="max-h-80 overflow-y-auto p-2">
            <Command.Empty className="py-6 text-center text-sm text-parchment-700">Rien à bord.</Command.Empty>

            <Command.Group heading="Panneaux" className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1 [&_[cmdk-group-heading]]:text-[0.65rem] [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-widest [&_[cmdk-group-heading]]:text-parchment-700">
              {panels.map((panel) => (
                <Item key={panel.id} onSelect={() => run(() => send({ cmd: 'select_panel', id: panel.id }))}>
                  <span className="w-5 text-center">{panelIcon(panel.kind)}</span>
                  {panel.name}
                  <span className="ml-auto font-mono text-[0.65rem] text-parchment-700">{panel.id}</span>
                </Item>
              ))}
            </Command.Group>

            <Command.Group heading="Actions" className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1 [&_[cmdk-group-heading]]:text-[0.65rem] [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-widest [&_[cmdk-group-heading]]:text-parchment-700">
              {streaming && (
                <Item onSelect={() => run(() => send({ cmd: 'stop' }))}>⏹ Stopper le stream</Item>
              )}
              <Item onSelect={() => run(onOpenConfig)}>⚙ Configuration</Item>
              <Item onSelect={() => run(onOpenIndex)}>🔎 Statut de l’index</Item>
              <Item onSelect={() => run(() => send({ cmd: 'new_context' }))}>✦ Nouveau contexte</Item>
              <Item onSelect={() => run(() => send({ cmd: 'clear_conversation' }))}>🧹 Vider la conversation</Item>
              <Item onSelect={() => run(() => send({ cmd: 'reset_costs' }))}>Ø Remettre les compteurs à zéro</Item>
              <Item onSelect={() => run(() => send({ cmd: 'reload' }))}>↻ Recharger Nestor</Item>
            </Command.Group>
          </Command.List>
        </Command>
      </DialogContent>
    </Dialog>
  )
}

function Item({ children, onSelect }: { children: React.ReactNode; onSelect: () => void }) {
  return (
    <Command.Item
      onSelect={onSelect}
      className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-2 text-sm text-parchment-300 data-[selected=true]:bg-brass-500/15 data-[selected=true]:text-brass-300"
    >
      {children}
    </Command.Item>
  )
}
