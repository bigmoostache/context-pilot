// ── Wizard step 4: finalize provisioning (Obj 5.4) ───────────────────
//
// Flips the box to provisioned: the cockpit starts serving on :443 and the
// wizard redirects there. The button is disabled until the pre-requisites the
// backend enforces are met (password changed + identity set), reported by
// `GET /api/maint/status`.

import { useState } from "react"
import { finalizeProvisioning, fetchMaintStatus, type MaintStatus } from "@/lib/api/maint"
import { ErrorNote, PrimaryButton } from "./parts"

export function FinalizeStep({
  status,
  cockpitName,
  passwordChanged,
}: {
  status: MaintStatus
  cockpitName: string
  passwordChanged: boolean
}) {
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState(false)

  const finalize = async () => {
    setError(null)
    setBusy(true)
    try {
      await finalizeProvisioning()
      setDone(true)
    } catch (err) {
      // The flag may have been persisted before a Caddy reload hiccup (502):
      // re-probe, and treat an already-provisioned box as success rather than
      // misreporting it as a failure.
      const provisioned = await fetchMaintStatus()
        .then((s) => s.provisioned)
        .catch(() => false)
      if (provisioned) {
        setDone(true)
      } else {
        setError(err instanceof Error ? err.message : "Finalize failed")
        setBusy(false)
      }
    }
  }

  // The provisioned cockpit URL, used by both the done confirmation and the
  // pre-finalize form below. Declared here (immediately before the `done` early
  // exit that first consumes it) so no unrelated code sits between it and its
  // use (unicorn/no-declarations-before-early-exit).
  const cockpitUrl = `https://${cockpitName}`

  if (done) {
    return (
      <div className="flex flex-col gap-3">
        <p className="text-sm text-foreground">
          ✓ Provisioned. The cockpit is now served on{" "}
          <span className="font-mono">{cockpitUrl}</span>. It may take a few seconds for TLS to come
          up.
        </p>
        <a
          className="w-full rounded-md bg-signal px-4 py-2 text-center text-sm font-semibold text-background hover:opacity-90"
          href={cockpitUrl}
        >
          Open the cockpit
        </a>
      </div>
    )
  }

  // Enable finalize only once the backend pre-requisites are met (password
  // changed + identity set + a known host). Declared after the `done` early
  // exit so that return path doesn't compute it
  // (unicorn/no-declarations-before-early-exit).
  const ready = status.identity_set && passwordChanged && cockpitName !== ""

  return (
    <div className="flex flex-col gap-3">
      <ul className="flex flex-col gap-1 text-sm">
        <Check ok={passwordChanged}>Admin password changed</Check>
        <Check ok={status.identity_set}>Box name / IP set</Check>
      </ul>
      <p className="text-xs text-muted-foreground">
        Finalizing starts the cockpit on <span className="font-mono">{cockpitUrl}</span> and turns
        off this setup flow for normal use (the maintenance console stays reachable on :9090).
      </p>
      <ErrorNote error={error} />
      <PrimaryButton onClick={() => void finalize()} disabled={!ready} busy={busy}>
        Finalize &amp; launch the cockpit
      </PrimaryButton>
    </div>
  )
}

function Check({ ok, children }: { ok: boolean; children: React.ReactNode }) {
  return (
    <li className={ok ? "text-foreground" : "text-muted-foreground"}>
      <span className={ok ? "text-signal" : "text-muted-foreground"}>{ok ? "✓" : "○"}</span>{" "}
      {children}
    </li>
  )
}
