// ── Day-0 provisioning (first login on a fresh box) ──────────────────
//
// Shown by AuthGuard when the backend reports `next_action === "set_identity"`:
// an `can_manage_it` operator has logged in over cleartext :80 on an
// unprovisioned box (design §13.4). Two local phases replace the removed
// maintenance wizard's IdentityStep + TrustStep:
//
//   1. "identity" — name the box (DNS name + LAN IP). Saving posts to
//      `POST /api/it/identity`, which provisions the box, issues the private-CA
//      leaf, and brings :443 up.
//   2. "trust" — distribute the CA root so clients trust the new https cockpit,
//      then continue. `refreshMe()` re-pulls /me; the box is now provisioned so
//      `next_action` advances past the day-0 step.

import { useEffect, useState, type SyntheticEvent } from "react"
import { setItIdentity, fetchItCaFingerprint, downloadItCaCert } from "@/lib/api"
import { useAuth } from "@/lib/providers/auth"

export function DayZeroSetup() {
  const { refreshMe } = useAuth()
  const [phase, setPhase] = useState<"identity" | "trust">("identity")

  return (
    <div className="flex min-h-screen w-screen items-center justify-center overflow-auto bg-background p-4">
      <div className="w-full max-w-md py-8">
        <div className="mb-8 text-center">
          <div className="mb-2 font-mono text-2xl font-bold tracking-tight text-foreground">
            <span className="text-signal">▌</span> Context Pilot
          </div>
          <p className="text-sm text-muted-foreground">
            {phase === "identity"
              ? "Name this appliance to bring its secure address online."
              : "Distribute the certificate authority root, then continue."}
          </p>
        </div>

        <div className="rounded-lg border border-border bg-card p-6 shadow-md">
          {phase === "identity" ? (
            <IdentityPhase onDone={() => setPhase("trust")} />
          ) : (
            <TrustPhase onContinue={() => void refreshMe()} />
          )}
        </div>
      </div>
    </div>
  )
}

/** Box name + IP form. Saving provisions the box and issues the TLS leaf. */
function IdentityPhase({ onDone }: { onDone: () => void }) {
  const [name, setName] = useState("")
  const [ip, setIp] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const submit = async (e: SyntheticEvent) => {
    e.preventDefault()
    if (ip.trim() === "" || busy) return
    setError(null)
    setBusy(true)
    try {
      await setItIdentity(name.trim(), ip.trim())
      onDone()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not save the identity")
      setBusy(false)
    }
  }

  return (
    <form onSubmit={(e) => void submit(e)} className="flex flex-col gap-4">
      <Field
        label="DNS name"
        hint="optional"
        value={name}
        onChange={setName}
        placeholder="pilot.acme.corp"
        autoFocus
      />
      <Field label="LAN IP address" value={ip} onChange={setIp} placeholder="192.168.1.116" />
      <p className="-mt-1 text-[11px] text-muted-foreground">
        Saving issues the TLS certificate for this name/IP and brings the secure (https) site up.
        Use a static lease so the address doesn't change.
      </p>
      {error && (
        <div className="rounded-md bg-danger/10 px-3 py-2 text-xs text-danger">{error}</div>
      )}
      <button
        type="submit"
        disabled={ip.trim() === "" || busy}
        className="w-full rounded-md bg-signal px-4 py-2 text-sm font-semibold text-background
                   transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {busy ? "Provisioning…" : "Save & bring the secure site up"}
      </button>
    </form>
  )
}

/** CA-root download + fingerprint, then continue into the cockpit. */
function TrustPhase({ onContinue }: { onContinue: () => void }) {
  const [fingerprint, setFingerprint] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let live = true
    fetchItCaFingerprint()
      .then((r) => {
        if (live) setFingerprint(r.fingerprint)
      })
      .catch(() => {
        if (live) setError("The CA root isn't ready yet — reload in a moment.")
      })
    return () => {
      live = false
    }
  }, [])

  const download = async () => {
    setError(null)
    setBusy(true)
    try {
      await downloadItCaCert()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Download failed")
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-muted-foreground">
        Install this certificate-authority root as trusted on every client (push it via Group Policy
        or your MDM). Verify the fingerprint below out-of-band before trusting it.
      </p>

      <div className="rounded-md border border-border bg-background p-3">
        <div className="mb-1 text-xs font-medium text-foreground/90">SHA-256 fingerprint</div>
        <div className="font-mono text-xs break-all text-foreground">
          {fingerprint ?? "loading…"}
        </div>
      </div>

      {error && (
        <div className="rounded-md bg-danger/10 px-3 py-2 text-xs text-danger">{error}</div>
      )}

      <button
        type="button"
        onClick={() => void download()}
        disabled={busy}
        className="w-full rounded-md border border-border px-4 py-2 text-sm font-medium text-foreground/80
                   transition-colors hover:bg-muted/60 disabled:opacity-50"
      >
        {busy ? "Downloading…" : "Download CA root (root.crt)"}
      </button>

      <button
        type="button"
        onClick={onContinue}
        className="w-full rounded-md bg-signal px-4 py-2 text-sm font-semibold text-background
                   transition-opacity hover:opacity-90"
      >
        I've distributed the root — continue
      </button>
    </div>
  )
}

function Field({
  label,
  value,
  onChange,
  hint,
  placeholder,
  autoFocus,
}: {
  label: string
  value: string
  onChange: (v: string) => void
  hint?: string
  placeholder?: string
  autoFocus?: boolean
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-foreground/90">
        {label}
        {hint && <span className="ml-2 text-muted-foreground/60">{hint}</span>}
      </span>
      <input
        type="text"
        value={value}
        autoFocus={autoFocus}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="rounded-md border border-border bg-background px-3 py-2 font-mono text-sm text-foreground
                   placeholder:text-muted-foreground/50 focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
      />
    </label>
  )
}
