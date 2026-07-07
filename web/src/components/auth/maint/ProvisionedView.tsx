// ── Post-provisioning maintenance view (Obj 5.5) ─────────────────────
//
// Once the box is live the maintenance console stays reachable on :9090. From
// here the operator can reach the cockpit, re-download the CA root, re-issue the
// certificate for a new name/IP, or sign out.

import { useEffect, useState } from "react"
import { downloadCaCert, fetchCaFingerprint, fetchIdentity, type Identity } from "@/lib/api/maint"
import { ErrorNote, GhostButton } from "./parts"

export function ProvisionedView({
  onReconfigure,
  onLogout,
}: {
  onReconfigure: () => void
  onLogout: () => void
}) {
  const [identity, setIdentity] = useState<Identity | null>(null)
  const [fingerprint, setFingerprint] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let live = true
    fetchIdentity()
      .then((r) => live && setIdentity(r.identity))
      .catch(() => {
        /* identity is best-effort display — ignore a fetch failure */
      })
    fetchCaFingerprint()
      .then((r) => live && setFingerprint(r.fingerprint))
      .catch(() => {
        /* fingerprint is best-effort display — ignore a fetch failure */
      })
    return () => {
      live = false
    }
  }, [])

  const cockpitUrl = identity ? `https://${identity.name || identity.ip}` : null

  const download = async () => {
    setError(null)
    try {
      await downloadCaCert()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Download failed")
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-foreground">
        ✓ This appliance is provisioned and the cockpit is live.
      </p>
      {identity && (
        <div className="rounded-md border border-border bg-background p-3 text-sm">
          <div className="text-foreground">
            <span className="text-muted-foreground">Name:</span> {identity.name || "—"}
          </div>
          <div className="text-foreground">
            <span className="text-muted-foreground">IP:</span> {identity.ip}
          </div>
          {fingerprint && (
            <div className="mt-1 break-all font-mono text-[11px] text-muted-foreground">
              CA: {fingerprint}
            </div>
          )}
        </div>
      )}
      <ErrorNote error={error} />
      {cockpitUrl && (
        <a
          className="w-full rounded-md bg-signal px-4 py-2 text-center text-sm font-semibold text-background hover:opacity-90"
          href={cockpitUrl}
        >
          Open the cockpit
        </a>
      )}
      <GhostButton onClick={() => void download()}>Re-download CA root</GhostButton>
      <GhostButton onClick={onReconfigure}>Change name / IP &amp; re-issue certificate</GhostButton>
      <GhostButton onClick={onLogout}>Sign out</GhostButton>
    </div>
  )
}
