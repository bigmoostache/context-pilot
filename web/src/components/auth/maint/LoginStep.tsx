// ── Wizard step 1a: maintenance login (Obj 5.1) ──────────────────────

import { useState, type FormEvent } from "react"
import { maintLogin, type MaintUser } from "@/lib/api/maint"
import { Field, ErrorNote, PrimaryButton } from "./parts"

export function LoginStep({ onSuccess }: { onSuccess: (user: MaintUser) => void }) {
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (!email || !password || busy) return
    setError(null)
    setBusy(true)
    try {
      onSuccess(await maintLogin(email, password))
    } catch (err) {
      setError(err instanceof Error ? err.message : "Login failed")
      setBusy(false)
    }
  }

  return (
    <form onSubmit={submit}>
      <Field
        label="Email"
        type="email"
        value={email}
        onChange={setEmail}
        autoFocus
        autoComplete="username"
      />
      <Field
        label="Password"
        type="password"
        value={password}
        onChange={setPassword}
        hint="from the delivery sheet"
        autoComplete="current-password"
      />
      <ErrorNote error={error} />
      <PrimaryButton type="submit" disabled={!email || !password} busy={busy}>
        Sign in
      </PrimaryButton>
    </form>
  )
}
