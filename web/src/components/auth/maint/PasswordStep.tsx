// ── Wizard step 1b: forced password + email change (Obj 5.1) ─────────
//
// The seeded admin logs in with the paper password and MUST rotate it (and may
// set a real email) before anything else. The finalize pre-req (backend) blocks
// provisioning until this is done.

import { useState, type FormEvent } from "react"
import { maintChangePassword, maintUpdateProfile, type MaintUser } from "@/lib/api/maint"
import { Field, ErrorNote, PrimaryButton } from "./parts"

const MIN_PASSWORD_LEN = 8

export function PasswordStep({ user, onDone }: { user: MaintUser; onDone: () => void }) {
  const [current, setCurrent] = useState("")
  const [next, setNext] = useState("")
  const [confirm, setConfirm] = useState("")
  const [email, setEmail] = useState(user.email)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const passwordsOk = next.length >= MIN_PASSWORD_LEN && next === confirm
  const canSubmit = current !== "" && passwordsOk && email.trim() !== "" && !busy

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    setError(null)
    setBusy(true)
    try {
      // Update the email first (if changed); a failure here shouldn't leave the
      // password rotated-but-email-stale, so order email → password.
      if (email.trim() !== user.email) {
        await maintUpdateProfile(user.name, email.trim())
      }
      await maintChangePassword(current, next)
      onDone()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not update credentials")
      setBusy(false)
    }
  }

  return (
    <form onSubmit={submit}>
      <Field
        label="Admin email"
        type="email"
        value={email}
        onChange={setEmail}
        autoComplete="email"
      />
      <Field
        label="Current (paper) password"
        type="password"
        value={current}
        onChange={setCurrent}
        autoFocus
        autoComplete="current-password"
      />
      <Field
        label="New password"
        type="password"
        value={next}
        onChange={setNext}
        hint={`min ${MIN_PASSWORD_LEN} chars`}
        autoComplete="new-password"
      />
      <Field
        label="Confirm new password"
        type="password"
        value={confirm}
        onChange={setConfirm}
        autoComplete="new-password"
      />
      {confirm !== "" && next !== confirm && (
        <p className="-mt-1 mb-3 text-[11px] text-danger">Passwords don't match.</p>
      )}
      <ErrorNote error={error} />
      <PrimaryButton type="submit" disabled={!canSubmit} busy={busy}>
        Save and continue
      </PrimaryButton>
    </form>
  )
}
