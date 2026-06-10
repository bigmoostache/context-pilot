import { useEffect, useState } from 'react'
import { useNestor } from '@/lib/store'
import { fetchDevices, logout, revokeDevice, send } from '@/lib/ws'
import { fmtAgo, fmtTokens } from '@/lib/utils'
import type { DeviceInfo } from '@/lib/types'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import { Button } from '@/components/ui/button'

/** Configuration (parité Ctrl+H de la TUI) : modèles, thème, budgets,
    garde-fous, appareils connectés. Volet latéral droit. */
export function ConfigSheet({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const state = useNestor((s) => s.state)
  if (!state) return null
  const { status, meta } = state

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent side="right" aria-describedby={undefined}>
        <DialogTitle className="font-display text-2xl italic">Configuration</DialogTitle>

        <Section title="Modèle principal">
          <ModelPicker scope="primary" provider={status.provider} model={status.model} providers={meta.providers} />
          <ApiCheckBadge />
        </Section>

        <Section title="Modèle secondaire (reverie)">
          <ModelPicker
            scope="secondary"
            provider={status.secondary_provider}
            model={status.secondary_model}
            providers={meta.providers}
          />
        </Section>

        <Section title="Agent">
          <ToggleRow
            label="Auto-continuation"
            hint="Le spine relance le travail tant que des todos restent"
            checked={status.auto_continue}
            onChange={() => send({ cmd: 'toggle_auto_continue' })}
          />
          <ToggleRow
            label="Reverie"
            hint="Optimiseur de contexte en arrière-plan"
            checked={status.reverie_enabled}
            onChange={() => send({ cmd: 'toggle_reverie' })}
          />
          <SliderRow
            label="Garde-fou de coût"
            value={status.max_cost ?? 0}
            min={0}
            max={20}
            step={0.5}
            display={status.max_cost ? `$${status.max_cost.toFixed(2)}` : 'désactivé'}
            onCommit={(v) => send({ cmd: 'set_max_cost', value: v <= 0 ? null : v })}
          />
          <StepperRow
            label="Rappel « think »"
            value={status.think_threshold ?? -1}
            onChange={(v) => send({ cmd: 'set_think_threshold', value: Math.min(v, -1) })}
          />
        </Section>

        <Section title="Budget de contexte">
          <SliderRow
            label="Budget"
            value={status.context_budget ?? status.context_window}
            min={Math.round(status.context_window * 0.1)}
            max={status.context_window}
            step={1000}
            display={`${fmtTokens(status.context_budget ?? status.context_window)} / ${fmtTokens(status.context_window)}`}
            onCommit={(v) => send({ cmd: 'set_context_budget', tokens: v >= status.context_window ? null : v })}
          />
          <SliderRow
            label="Seuil de nettoyage"
            value={status.cleaning_threshold}
            min={0.3}
            max={0.95}
            step={0.05}
            display={`${Math.round(status.cleaning_threshold * 100)} %`}
            onCommit={(v) => send({ cmd: 'set_cleaning_threshold', value: v })}
          />
          <SliderRow
            label="Cible après nettoyage"
            value={status.cleaning_target}
            min={0.3}
            max={0.95}
            step={0.05}
            display={`${Math.round(status.cleaning_target * 100)} %`}
            onCommit={(v) => send({ cmd: 'set_cleaning_target', value: v })}
          />
        </Section>

        <Section title="Thème TUI">
          <Select value={status.theme} onValueChange={(theme) => send({ cmd: 'set_theme', theme })}>
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {meta.themes.map((theme) => (
                <SelectItem key={theme} value={theme}>
                  {theme}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Section>

        <Section title="Session">
          <div className="flex flex-wrap gap-2">
            <Button variant="secondary" size="sm" onClick={() => send({ cmd: 'reset_costs' })}>
              Remettre les compteurs à zéro
            </Button>
            <Button variant="secondary" size="sm" onClick={() => send({ cmd: 'new_context' })}>
              Nouveau contexte
            </Button>
            <Button variant="danger" size="sm" onClick={() => send({ cmd: 'clear_conversation' })}>
              Vider la conversation
            </Button>
            <Button variant="danger" size="sm" onClick={() => send({ cmd: 'reload' })}>
              Recharger Nestor
            </Button>
          </div>
        </Section>

        <Devices />

        <Section title="">
          <div className="flex items-center justify-between text-xs text-parchment-700">
            <span>
              Nestor v{meta.version} · {meta.workspace}
            </span>
            <Button variant="ghost" size="sm" onClick={logout}>
              Se déconnecter
            </Button>
          </div>
        </Section>
      </DialogContent>
    </Dialog>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mt-5">
      {title && (
        <h3 className="mb-2 text-[0.7rem] font-medium uppercase tracking-widest text-parchment-700">{title}</h3>
      )}
      <div className="space-y-3">{children}</div>
    </section>
  )
}

function ModelPicker({
  scope,
  provider,
  model,
  providers,
}: {
  scope: 'primary' | 'secondary'
  provider: string
  model: string
  providers: { id: string; label: string; models: { id: string; label: string }[] }[]
}) {
  const current = providers.find((p) => p.id === provider)
  // Le modèle courant arrive en id serde (primaire : nom API). On matche par
  // id OU par label pour rester tolérant.
  const currentModel =
    current?.models.find((m) => m.id === model || m.label === model)?.id ?? current?.models[0]?.id ?? ''
  return (
    <div className="grid grid-cols-2 gap-2">
      <Select value={provider} onValueChange={(p) => send({ cmd: 'set_provider', scope, provider: p })}>
        <SelectTrigger>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {providers.map((p) => (
            <SelectItem key={p.id} value={p.id}>
              {p.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select value={currentModel} onValueChange={(m) => send({ cmd: 'set_model', scope, model: m })}>
        <SelectTrigger>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {(current?.models ?? []).map((m) => (
            <SelectItem key={m.id} value={m.id}>
              {m.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

function ApiCheckBadge() {
  const status = useNestor((s) => s.state?.status)
  if (!status) return null
  if (status.api_check_in_progress) {
    return <p className="font-mono text-xs text-parchment-500 animate-pulse">Vérification de l’API…</p>
  }
  if (!status.api_check) return null
  const check = status.api_check
  return (
    <p className={`font-mono text-xs ${check.ok ? 'text-sage-400' : 'text-ember-400'}`}>
      {check.ok ? '✓ API opérationnelle' : `✗ ${check.error ?? 'API en erreur'}`}
    </p>
  )
}

function ToggleRow({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string
  hint: string
  checked: boolean
  onChange: () => void
}) {
  return (
    <div className="flex items-center justify-between gap-3">
      <div>
        <div className="text-sm text-parchment-100">{label}</div>
        <div className="text-xs text-parchment-700">{hint}</div>
      </div>
      <Switch checked={checked} onCheckedChange={onChange} />
    </div>
  )
}

/** Slider contrôlé localement, commit au relâchement (évite le spam WS). */
function SliderRow({
  label,
  value,
  min,
  max,
  step,
  display,
  onCommit,
}: {
  label: string
  value: number
  min: number
  max: number
  step: number
  display: string
  onCommit: (v: number) => void
}) {
  const [local, setLocal] = useState(value)
  useEffect(() => setLocal(value), [value])
  return (
    <div>
      <div className="mb-1.5 flex justify-between text-sm">
        <span className="text-parchment-100">{label}</span>
        <span className="font-mono text-xs text-brass-300">{display}</span>
      </div>
      <Slider
        value={[local]}
        min={min}
        max={max}
        step={step}
        onValueChange={([v]) => setLocal(v ?? min)}
        onValueCommit={([v]) => onCommit(v ?? min)}
      />
    </div>
  )
}

function StepperRow({ label, value, onChange }: { label: string; value: number; onChange: (v: number) => void }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="text-sm text-parchment-100">{label}</span>
      <div className="flex items-center gap-2">
        <Button variant="secondary" size="icon" onClick={() => onChange(value - 1)}>
          −
        </Button>
        <span className="w-10 text-center font-mono text-sm text-brass-300">{value}</span>
        <Button variant="secondary" size="icon" onClick={() => onChange(value + 1)}>
          +
        </Button>
      </div>
    </div>
  )
}

/** Appareils authentifiés — révocation des tokens (sécurité du contrat). */
function Devices() {
  const [devices, setDevices] = useState<DeviceInfo[] | null>(null)

  async function refresh() {
    try {
      setDevices((await fetchDevices()) as DeviceInfo[])
    } catch {
      setDevices([])
    }
  }
  useEffect(() => {
    void refresh()
  }, [])

  return (
    <Section title="Appareils connectés">
      {(devices ?? []).map((device) => (
        <div key={device.id} className="flex items-center justify-between gap-2 text-sm">
          <div>
            <div className="text-parchment-100">{device.name}</div>
            <div className="font-mono text-[0.65rem] text-parchment-700">vu {fmtAgo(device.last_seen_ms)}</div>
          </div>
          <Button
            variant="danger"
            size="sm"
            onClick={async () => {
              await revokeDevice(device.id)
              void refresh()
            }}
          >
            Révoquer
          </Button>
        </div>
      ))}
      {devices?.length === 0 && <p className="text-xs text-parchment-700">Aucun appareil.</p>}
    </Section>
  )
}
