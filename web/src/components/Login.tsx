import { useState } from 'react'
import { Anchor } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { connect, login } from '@/lib/ws'

/** Écran d'authentification : mot de passe → token par appareil. */
export function Login() {
  const [password, setPassword] = useState('')
  const [device, setDevice] = useState(() => defaultDeviceName())
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await login(password, device)
      connect()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Erreur inconnue')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex h-full items-center justify-center p-6">
      <form onSubmit={submit} className="w-full max-w-sm animate-rise">
        <div className="mb-8 text-center">
          <div className="mx-auto mb-4 flex size-14 items-center justify-center rounded-2xl border border-brass-600/40 bg-coal-900 shadow-[0_0_40px_-8px] shadow-brass-500/30">
            <Anchor className="size-7 text-brass-400" />
          </div>
          <h1 className="font-display text-5xl italic text-parchment-100">Nestor</h1>
          <p className="mt-2 text-sm text-parchment-500">
            Context Pilot, qui veille sur la Pi pendant que tu dors.
          </p>
        </div>

        <label className="mb-1.5 block text-xs font-medium uppercase tracking-wider text-parchment-500">
          Mot de passe
        </label>
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          autoFocus
          className="mb-3 h-10 w-full rounded-md border border-coal-700 bg-coal-900 px-3 text-parchment-100 focus:outline-2 focus:outline-brass-400"
        />
        <label className="mb-1.5 block text-xs font-medium uppercase tracking-wider text-parchment-500">
          Nom de cet appareil
        </label>
        <input
          type="text"
          value={device}
          onChange={(e) => setDevice(e.target.value)}
          className="mb-4 h-10 w-full rounded-md border border-coal-700 bg-coal-900 px-3 text-parchment-100 focus:outline-2 focus:outline-brass-400"
        />
        {error && <p className="mb-3 text-sm text-ember-400">{error}</p>}
        <Button type="submit" disabled={busy || password.length === 0} className="w-full">
          {busy ? 'Connexion…' : 'Embarquer'}
        </Button>
      </form>
    </div>
  )
}

function defaultDeviceName(): string {
  const ua = navigator.userAgent
  const browser = ua.includes('Firefox') ? 'Firefox' : ua.includes('Chrome') ? 'Chrome' : 'Navigateur'
  const os = ua.includes('Windows') ? 'Windows' : ua.includes('Mac') ? 'macOS' : 'Linux'
  return `${os} — ${browser}`
}
