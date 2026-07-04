// ── Wizard step 2: box name + IP (Obj 5.2) ───────────────────────────
//
// Setting the identity re-issues the private-CA leaf for the chosen name/IP
// (backend regenerates the Caddyfile + reloads Caddy).

import { useState, type FormEvent } from "react"
import { setIdentity, type Identity } from "@/lib/api/maint"
import { Field, ErrorNote, PrimaryButton } from "./parts"

export function IdentityStep({ initial, onDone }: { initial: Identity | null; onDone: () => void }) {
  const [name, setName] = useState(initial?.name ?? "")
  const [ip, setIp] = useState(initial?.ip ?? "")
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (ip.trim() === "" || busy) return
    setError(null)
    setBusy(true)
    try {
      await setIdentity(name.trim(), ip.trim())
      onDone()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not save the identity")
      setBusy(false)
    }
  }

  return (
    <form onSubmit={submit}>
      <Field
        label="DNS name"
        value={name}
        onChange={setName}
        placeholder="pilot.acme.corp"
        hint="optional"
        autoFocus
      />
      <Field label="LAN IP address" value={ip} onChange={setIp} placeholder="192.168.1.116" />
      <ErrorNote error={error} />
      <p className="mb-3 text-xs text-muted-foreground">
        Saving re-issues the TLS certificate for this name/IP. Use a static lease so the address doesn't change.
      </p>
      <PrimaryButton type="submit" disabled={ip.trim() === ""} busy={busy}>
        Save name &amp; re-issue certificate
      </PrimaryButton>
    </form>
  )
}
