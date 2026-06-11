import { useEffect, useState } from 'react'
import {
  ArrowLeft,
  Cpu,
  KeyRound,
  Lock,
  Pencil,
  Power,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Wifi,
} from 'lucide-react'
import { useNestor } from '@/lib/store'
import {
  changePassword,
  connectWifi,
  fetchDefaults,
  fetchEnvKeys,
  fetchSystemInfo,
  fetchWifi,
  rebootPi,
  restartService,
  saveDefaults,
  setEnvKey,
  type EnvKey,
  type SystemInfo,
  type WifiNetwork,
} from '@/lib/ws'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'

/** Paramètres généraux — niveau machine/installation, pas projet :
    WiFi, clés API, défauts des nouveaux projets, système, sécurité. */
export function Settings() {
  const setScreen = useNestor((s) => s.setScreen)
  const [restarting, setRestarting] = useState(false)

  /** Redémarre le service (relit le .env) et attend son retour. */
  async function restartAndWait() {
    setRestarting(true)
    try {
      await restartService()
    } catch {
      /* le process meurt pendant la requête — attendu */
    }
    for (let i = 0; i < 30; i++) {
      await new Promise((s) => setTimeout(s, 1000))
      try {
        await fetchSystemInfo()
        break
      } catch {
        /* pas encore revenu */
      }
    }
    setRestarting(false)
  }

  if (restarting) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2">
        <p className="font-display text-3xl italic text-brass-300 animate-pulse">Redémarrage…</p>
        <p className="text-sm text-parchment-700">Nestor relit sa configuration.</p>
      </div>
    )
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl px-6 py-8">
        <header className="mb-8 flex items-center gap-3 animate-rise">
          <button
            onClick={() => setScreen('projects')}
            className="rounded-md p-2 text-parchment-500 hover:bg-coal-800 hover:text-parchment-100 cursor-pointer"
            title="Retour aux projets"
          >
            <ArrowLeft className="size-5" />
          </button>
          <h1 className="font-display text-3xl italic">Paramètres généraux</h1>
        </header>

        <div className="space-y-10">
          <WifiSection />
          <ApiKeysSection onRestart={restartAndWait} />
          <DefaultsSection />
          <SystemSection onRestart={restartAndWait} />
          <SecuritySection />
        </div>
      </div>
    </div>
  )
}

function Section({ icon, title, children }: { icon: React.ReactNode; title: string; children: React.ReactNode }) {
  return (
    <section className="animate-rise">
      <h2 className="mb-3 flex items-center gap-2 text-[0.7rem] font-medium uppercase tracking-widest text-parchment-700">
        {icon}
        {title}
      </h2>
      <div className="rounded-xl border border-coal-700 bg-coal-900/50 p-4">{children}</div>
    </section>
  )
}

// ─── WiFi ───────────────────────────────────────────────────────────────────

function WifiSection() {
  const [data, setData] = useState<{ ip: string; current: string | null; networks: WifiNetwork[] } | null>(null)
  const [target, setTarget] = useState<WifiNetwork | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [scanning, setScanning] = useState(false)

  async function refresh() {
    setScanning(true)
    setError(null)
    try {
      setData(await fetchWifi())
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Erreur')
    } finally {
      setScanning(false)
    }
  }
  useEffect(() => {
    void refresh()
  }, [])

  return (
    <Section icon={<Wifi className="size-3.5" />} title="Connexion">
      {!data && !error && <p className="font-mono text-xs text-parchment-700 animate-pulse">Scan…</p>}
      {error && <p className="text-sm text-ember-400">{error}</p>}
      {data && (
        <>
          <div className="mb-3 flex items-baseline justify-between">
            <div>
              <span className="text-sm text-parchment-100">{data.current ?? 'Non connecté'}</span>
              <span className="ml-2 font-mono text-xs text-parchment-700">{data.ip}</span>
            </div>
            <Button variant="ghost" size="sm" onClick={refresh} disabled={scanning}>
              <RefreshCw className={cn('size-3.5', scanning && 'animate-spin')} />
              {scanning ? 'Scan…' : 'Scanner'}
            </Button>
          </div>
          <div className="space-y-1">
            {data.networks.map((network) => (
              <button
                key={network.ssid}
                onClick={() => !network.active && setTarget(network)}
                disabled={network.active}
                className={cn(
                  'flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-sm',
                  network.active
                    ? 'bg-brass-500/10 text-brass-300'
                    : 'text-parchment-300 hover:bg-coal-800 cursor-pointer',
                )}
              >
                <SignalBars signal={network.signal} />
                <span className="flex-1 truncate">{network.ssid}</span>
                {network.security && <Lock className="size-3 text-parchment-700" />}
                {network.active && <span className="font-mono text-[0.6rem] uppercase">connecté</span>}
              </button>
            ))}
          </div>
          <p className="mt-3 text-xs text-parchment-700">
            ⚠ Changer de réseau peut rendre la Pi inaccessible si ton appareil n’est pas sur le même réseau.
          </p>
        </>
      )}
      {target && (
        <WifiConnectDialog
          network={target}
          onClose={() => setTarget(null)}
          onDone={() => {
            setTarget(null)
            void refresh()
          }}
        />
      )}
    </Section>
  )
}

function SignalBars({ signal }: { signal: number }) {
  const bars = signal > 75 ? 4 : signal > 50 ? 3 : signal > 25 ? 2 : 1
  return (
    <span className="flex items-end gap-px" title={`${signal} %`}>
      {[1, 2, 3, 4].map((bar) => (
        <span
          key={bar}
          className={cn('w-1 rounded-sm', bar <= bars ? 'bg-brass-400' : 'bg-coal-600')}
          style={{ height: `${3 + bar * 3}px` }}
        />
      ))}
    </span>
  )
}

function WifiConnectDialog({
  network,
  onClose,
  onDone,
}: {
  network: WifiNetwork
  onClose: () => void
  onDone: () => void
}) {
  const [password, setPassword] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await connectWifi(network.ssid, password || undefined)
      onDone()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Erreur')
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent>
        <DialogTitle className="font-display text-2xl italic">Se connecter à « {network.ssid} »</DialogTitle>
        <DialogDescription className="text-ember-400">
          Si ton appareil n’est pas sur ce réseau, tu perdras l’accès à Nestor jusqu’à retrouver la Pi.
        </DialogDescription>
        <form onSubmit={submit} className="mt-4 space-y-3">
          {network.security ? (
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoFocus
              placeholder="Mot de passe du réseau"
              className="h-10 w-full rounded-md border border-coal-700 bg-coal-900 px-3 text-sm text-parchment-100 placeholder:text-parchment-700 focus:outline-2 focus:outline-brass-400"
            />
          ) : (
            <p className="text-sm text-parchment-500">Réseau ouvert — pas de mot de passe.</p>
          )}
          {error && <p className="text-sm text-ember-400">{error}</p>}
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" onClick={onClose} disabled={busy}>
              Annuler
            </Button>
            <Button type="submit" variant="danger" disabled={busy || (!!network.security && !password)}>
              {busy ? 'Connexion…' : 'Changer de réseau'}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}

// ─── Clés API ───────────────────────────────────────────────────────────────

function ApiKeysSection({ onRestart }: { onRestart: () => void }) {
  const [keys, setKeys] = useState<EnvKey[] | null>(null)
  const [oauth, setOauth] = useState(false)
  const [editing, setEditing] = useState<EnvKey | null>(null)
  const [dirty, setDirty] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function refresh() {
    try {
      const res = await fetchEnvKeys()
      setKeys(res.keys)
      setOauth(res.claude_oauth)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Erreur')
    }
  }
  useEffect(() => {
    void refresh()
  }, [])

  async function remove(key: string) {
    await setEnvKey(key, null)
    setDirty(true)
    void refresh()
  }

  return (
    <Section icon={<KeyRound className="size-3.5" />} title="Clés API">
      {error && <p className="mb-2 text-sm text-ember-400">{error}</p>}
      <div
        className={cn(
          'mb-3 flex items-center gap-2 rounded-md border px-3 py-2 text-xs',
          oauth ? 'border-sage-400/30 bg-sage-400/5 text-sage-400' : 'border-coal-700 text-parchment-700',
        )}
      >
        <ShieldCheck className="size-3.5" />
        OAuth Claude Code : {oauth ? 'configuré (~/.claude/.credentials.json)' : 'absent'}
      </div>
      <div className="space-y-1">
        {(keys ?? []).map((entry) => (
          <div key={entry.key} className="group flex items-center gap-2 rounded-md px-2.5 py-1.5 text-sm">
            <span className="w-44 truncate text-parchment-300">{entry.label}</span>
            <span className="flex-1 font-mono text-xs text-parchment-700">
              {entry.set ? entry.masked : 'non définie'}
            </span>
            <button
              onClick={() => setEditing(entry)}
              title={entry.set ? 'Modifier' : 'Définir'}
              className="rounded-md p-1.5 text-parchment-500 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-coal-800 hover:text-parchment-100 cursor-pointer"
            >
              <Pencil className="size-3.5" />
            </button>
            {entry.set && (
              <button
                onClick={() => void remove(entry.key)}
                title="Supprimer"
                className="rounded-md p-1.5 text-parchment-500 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-ember-400/15 hover:text-ember-400 cursor-pointer"
              >
                <Trash2 className="size-3.5" />
              </button>
            )}
          </div>
        ))}
      </div>
      {dirty && (
        <div className="mt-3 flex items-center justify-between rounded-md border border-brass-600/40 bg-brass-500/10 px-3 py-2">
          <span className="text-xs text-brass-300">Modifications en attente — appliquées au redémarrage.</span>
          <Button size="sm" onClick={onRestart}>
            <RefreshCw className="size-3.5" /> Redémarrer
          </Button>
        </div>
      )}
      {editing && (
        <EnvKeyDialog
          entry={editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null)
            setDirty(true)
            void refresh()
          }}
        />
      )}
    </Section>
  )
}

function EnvKeyDialog({ entry, onClose, onSaved }: { entry: EnvKey; onClose: () => void; onSaved: () => void }) {
  const [value, setValue] = useState('')
  const [error, setError] = useState<string | null>(null)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    try {
      await setEnvKey(entry.key, value.trim())
      onSaved()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Erreur')
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogTitle className="font-display text-2xl italic">{entry.label}</DialogTitle>
        <DialogDescription>
          <span className="font-mono text-xs">{entry.key}</span>
          {entry.set && ' — une valeur existe déjà, elle sera remplacée.'}
        </DialogDescription>
        <form onSubmit={submit} className="mt-4 space-y-3">
          <input
            type="password"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            autoFocus
            placeholder="Colle la clé ici"
            className="h-10 w-full rounded-md border border-coal-700 bg-coal-900 px-3 font-mono text-sm text-parchment-100 placeholder:text-parchment-700 focus:outline-2 focus:outline-brass-400"
          />
          {error && <p className="text-sm text-ember-400">{error}</p>}
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" onClick={onClose}>
              Annuler
            </Button>
            <Button type="submit" disabled={!value.trim()}>
              Enregistrer
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}

// ─── Défauts des nouveaux projets ───────────────────────────────────────────

function DefaultsSection() {
  const meta = useNestor((s) => s.state?.meta)
  const [provider, setProvider] = useState<string>('')
  const [model, setModel] = useState<string>('')
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    fetchDefaults()
      .then((d) => {
        setProvider(d.provider ?? '')
        setModel(d.model ?? '')
      })
      .catch(() => {})
  }, [])

  const providers = meta?.providers ?? []
  const models = providers.find((p) => p.id === provider)?.models ?? []

  async function save(nextProvider: string, nextModel: string) {
    setProvider(nextProvider)
    setModel(nextModel)
    await saveDefaults({ provider: nextProvider || null, model: nextModel || null }).catch(() => {})
    setSaved(true)
    window.setTimeout(() => setSaved(false), 2000)
  }

  return (
    <Section icon={<Cpu className="size-3.5" />} title="Défauts des nouveaux projets">
      <p className="mb-3 text-xs text-parchment-700">
        Provider et modèle appliqués au premier démarrage d’un projet créé depuis le sélecteur.
      </p>
      <div className="grid grid-cols-2 gap-2">
        <Select value={provider} onValueChange={(p) => void save(p, '')}>
          <SelectTrigger>
            <SelectValue placeholder="Provider (défaut binaire)" />
          </SelectTrigger>
          <SelectContent>
            {providers.map((p) => (
              <SelectItem key={p.id} value={p.id}>
                {p.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={model} onValueChange={(m) => void save(provider, m)} disabled={!provider}>
          <SelectTrigger>
            <SelectValue placeholder="Modèle" />
          </SelectTrigger>
          <SelectContent>
            {models.map((m) => (
              <SelectItem key={m.id} value={m.id}>
                {m.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {saved && <p className="mt-2 font-mono text-xs text-sage-400">✓ Enregistré</p>}
      {providers.length === 0 && (
        <p className="mt-2 text-xs text-parchment-700">Catalogue indisponible (connexion à la session en cours…).</p>
      )}
    </Section>
  )
}

// ─── Système ────────────────────────────────────────────────────────────────

function SystemSection({ onRestart }: { onRestart: () => void }) {
  const [info, setInfo] = useState<SystemInfo | null>(null)
  const [confirmReboot, setConfirmReboot] = useState(false)

  useEffect(() => {
    fetchSystemInfo().then(setInfo).catch(() => {})
  }, [])

  function fmtUptime(s: number) {
    const days = Math.floor(s / 86400)
    const hours = Math.floor((s % 86400) / 3600)
    const min = Math.floor((s % 3600) / 60)
    return days > 0 ? `${days} j ${hours} h` : hours > 0 ? `${hours} h ${min} min` : `${min} min`
  }

  return (
    <Section icon={<Cpu className="size-3.5" />} title="Système">
      {info && (
        <dl className="mb-4 grid grid-cols-2 gap-x-4 gap-y-1.5 font-mono text-xs sm:grid-cols-3">
          <InfoCell label="Machine" value={`${info.hostname} · ${info.ip}`} />
          <InfoCell label="Nestor" value={`v${info.version}`} />
          <InfoCell label="Uptime" value={fmtUptime(info.uptime_s)} />
          <InfoCell
            label="RAM libre"
            value={`${Math.round(info.mem_available_kb / 1024)} / ${Math.round(info.mem_total_kb / 1024)} Mo`}
          />
          <InfoCell
            label="Disque libre"
            value={`${(info.disk_avail_bytes / 1e9).toFixed(1)} / ${(info.disk_total_bytes / 1e9).toFixed(1)} Go`}
          />
          <InfoCell label="CPU" value={`${(info.cpu_temp_milli_c / 1000).toFixed(1)} °C`} />
          {info.projects_root && <InfoCell label="Projets" value={info.projects_root} />}
        </dl>
      )}
      <div className="flex gap-2">
        <Button variant="secondary" size="sm" onClick={onRestart}>
          <RefreshCw className="size-3.5" /> Redémarrer Nestor
        </Button>
        <Button variant="danger" size="sm" onClick={() => setConfirmReboot(true)}>
          <Power className="size-3.5" /> Redémarrer la Pi
        </Button>
      </div>
      {confirmReboot && (
        <Dialog open onOpenChange={(o) => !o && setConfirmReboot(false)}>
          <DialogContent>
            <DialogTitle className="font-display text-2xl italic text-ember-400">Redémarrer la Pi ?</DialogTitle>
            <DialogDescription>
              Toute la machine redémarre — Nestor revient en ~1 minute (service au boot).
            </DialogDescription>
            <div className="mt-4 flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setConfirmReboot(false)}>
                Annuler
              </Button>
              <Button
                variant="danger"
                onClick={() => {
                  void rebootPi().catch(() => {})
                  setConfirmReboot(false)
                }}
              >
                Redémarrer
              </Button>
            </div>
          </DialogContent>
        </Dialog>
      )}
    </Section>
  )
}

function InfoCell({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-[0.6rem] uppercase tracking-wider text-parchment-700">{label}</dt>
      <dd className="truncate text-parchment-300" title={value}>
        {value}
      </dd>
    </div>
  )
}

// ─── Sécurité ───────────────────────────────────────────────────────────────

function SecuritySection() {
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [confirm, setConfirm] = useState('')
  const [revokeOthers, setRevokeOthers] = useState(true)
  const [status, setStatus] = useState<{ ok: boolean; msg: string } | null>(null)

  const valid = current.length > 0 && next.length >= 8 && next === confirm

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    try {
      await changePassword(current, next, revokeOthers)
      setStatus({ ok: true, msg: 'Mot de passe changé.' })
      setCurrent('')
      setNext('')
      setConfirm('')
    } catch (err) {
      setStatus({ ok: false, msg: err instanceof Error ? err.message : 'Erreur' })
    }
  }

  const inputClass =
    'h-9 w-full rounded-md border border-coal-700 bg-coal-900 px-3 text-sm text-parchment-100 placeholder:text-parchment-700 focus:outline-2 focus:outline-brass-400'

  return (
    <Section icon={<Lock className="size-3.5" />} title="Sécurité — mot de passe web">
      <form onSubmit={submit} className="space-y-2.5">
        <input
          type="password"
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
          placeholder="Mot de passe actuel"
          className={inputClass}
        />
        <div className="grid grid-cols-2 gap-2">
          <input
            type="password"
            value={next}
            onChange={(e) => setNext(e.target.value)}
            placeholder="Nouveau (8 min)"
            className={inputClass}
          />
          <input
            type="password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            placeholder="Confirmer"
            className={inputClass}
          />
        </div>
        <label className="flex items-center justify-between gap-3 text-sm text-parchment-300">
          Révoquer tous les autres appareils
          <Switch checked={revokeOthers} onCheckedChange={setRevokeOthers} />
        </label>
        {status && <p className={cn('text-sm', status.ok ? 'text-sage-400' : 'text-ember-400')}>{status.msg}</p>}
        <div className="flex justify-end">
          <Button type="submit" disabled={!valid}>
            Changer le mot de passe
          </Button>
        </div>
      </form>
    </Section>
  )
}
