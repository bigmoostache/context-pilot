// ── Wizard step 3: TLS trust — distribute the CA root (Obj 5.3) ───────
//
// The appliance's TLS is signed by a private CA. The operator downloads the
// root, verifies its fingerprint out-of-band, and pushes it to clients via
// GPO/MDM so the cockpit is trusted.

import { useEffect, useState } from "react"
import { downloadCaCert, fetchCaFingerprint } from "@/lib/api/maint"
import { ErrorNote, GhostButton, PrimaryButton } from "./parts"

export function TrustStep({ onDone }: { onDone: () => void }) {
  const [fingerprint, setFingerprint] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let live = true
    fetchCaFingerprint()
      .then((r) => live && setFingerprint(r.fingerprint))
      .catch(() => live && setError("The CA root isn't ready yet — reload in a moment."))
    return () => {
      live = false
    }
  }, [])

  const download = async () => {
    setError(null)
    setBusy(true)
    try {
      await downloadCaCert()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Download failed")
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">
        Download the certificate authority root and install it as a trusted root on every client
        (push it via Group Policy or your MDM). Verify the fingerprint below against this screen
        out-of-band before trusting it.
      </p>

      <div className="rounded-md border border-border bg-background p-3">
        <div className="mb-1 text-xs font-medium text-foreground/90">SHA-256 fingerprint</div>
        <div className="break-all font-mono text-xs text-foreground">
          {fingerprint ?? "loading…"}
        </div>
      </div>

      <ErrorNote error={error} />

      <PrimaryButton onClick={download} busy={busy}>
        Download CA root (root.crt)
      </PrimaryButton>
      <p className="text-xs text-muted-foreground">
        Windows: import into “Trusted Root Certification Authorities”. macOS: add to the System
        keychain and mark as Always Trust. Linux: drop into{" "}
        <span className="font-mono">/usr/local/share/ca-certificates</span> and run{" "}
        <span className="font-mono">update-ca-certificates</span>.
      </p>

      <GhostButton onClick={onDone}>I've distributed the root — continue</GhostButton>
    </div>
  )
}
