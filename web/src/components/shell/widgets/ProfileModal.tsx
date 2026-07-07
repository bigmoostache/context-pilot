import { useEffect, useState, type SyntheticEvent } from "react"
import { KeyRound, Loader2, Monitor, Trash2 } from "lucide-react"
import { Dialog, DialogContent, DialogClose } from "@/components/ui/dialog"
import { useAuth } from "@/lib/providers/auth"
import {
  changePassword,
  fetchSessions,
  revokeSession,
  updateProfile,
  type DeviceSession,
} from "@/lib/api"
import { cn } from "@/lib/utils"

/** Shared text-input styling for the profile fields. */
const INPUT_CLS =
  "w-full rounded-lg border border-border bg-card px-3 py-2 text-[13px] text-foreground outline-none transition-colors placeholder:text-muted-foreground/40 focus:border-signal focus:ring-1 focus:ring-signal"

/**
 * Profile modal — opened from the account avatar menu. Functional self-serve
 * account management for the signed-in user (the demaquetting profile, wired
 * to the RBAC backend):
 *   • edit display name + email (`PATCH /api/auth/me`),
 *   • change password (`POST /api/auth/password`),
 *   • review and revoke active device sessions (`/api/auth/sessions`).
 *
 * When auth is disabled there is no account to manage, so the modal shows a
 * short note instead.
 */
export function ProfileModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { user, authEnabled, refreshMe } = useAuth()

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[88vh] w-[540px] max-w-[calc(100vw-2rem)] flex-col">
        <header className="flex items-start gap-3 border-b border-border/70 px-6 pb-4 pt-5">
          <div className="flex flex-1 flex-col gap-0.5">
            <h2 className="text-[17px] font-semibold tracking-tight text-foreground">Profile</h2>
            <p className="text-[12px] text-muted-foreground">
              Your account and personal information.
            </p>
          </div>
          <DialogClose
            aria-label="Close"
            className="-mr-1 -mt-1 flex size-7 items-center justify-center rounded-md text-muted-foreground/55 transition-colors hover:bg-muted/70 hover:text-foreground"
          >
            ✕
          </DialogClose>
        </header>

        <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-6 py-5">
          {!authEnabled || !user ? (
            <p className="text-[13px] text-muted-foreground">
              Profile management is available when authentication is enabled.
            </p>
          ) : (
            <>
              <IdentitySection
                name={user.name}
                email={user.email}
                role={user.role}
                onSaved={refreshMe}
              />
              <PasswordSection />
              <SessionsSection open={open} />
            </>
          )}
        </div>

        <footer className="flex items-center justify-end gap-2 border-t border-border/70 bg-muted/25 px-6 py-4">
          <DialogClose className="rounded-lg px-3.5 py-2 text-[12.5px] font-medium text-muted-foreground transition-colors hover:bg-muted/70 hover:text-foreground">
            Close
          </DialogClose>
        </footer>
      </DialogContent>
    </Dialog>
  )
}

/** Initials disc + editable name/email, saved via PATCH /api/auth/me. */
function IdentitySection({
  name: initialName,
  email: initialEmail,
  role,
  onSaved,
}: {
  name: string
  email: string
  role: string
  onSaved: () => Promise<void>
}) {
  const [name, setName] = useState(initialName)
  const [email, setEmail] = useState(initialEmail)
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null)

  // Re-sync the editable fields when the underlying user changes (e.g. after a
  // profile refresh). React's canonical "adjust state when a prop changes"
  // pattern — a render-phase compare against the previously-seen values — rather
  // than an effect that would trip @eslint-react/set-state-in-effect (and cost
  // an extra commit). Setting state during render re-renders immediately, before
  // the browser paints, so there is no flash.
  const [seen, setSeen] = useState({ name: initialName, email: initialEmail })
  if (seen.name !== initialName || seen.email !== initialEmail) {
    setSeen({ name: initialName, email: initialEmail })
    setName(initialName)
    setEmail(initialEmail)
  }

  const dirty = name.trim() !== initialName || email.trim() !== initialEmail
  const initials = initialName
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? "")
    .join("")

  const save = async (e: SyntheticEvent) => {
    e.preventDefault()
    if (!dirty || busy) return
    setBusy(true)
    setMsg(null)
    try {
      await updateProfile(name.trim(), email.trim())
      await onSaved()
      setMsg({ ok: true, text: "Profile updated." })
    } catch (err) {
      setMsg({ ok: false, text: err instanceof Error ? err.message : "Update failed" })
    } finally {
      setBusy(false)
    }
  }

  return (
    <form onSubmit={(e) => void save(e)} className="flex flex-col gap-4">
      <div className="flex items-center gap-4">
        <span className="flex size-[60px] shrink-0 items-center justify-center rounded-full bg-signal/15 text-[20px] font-semibold text-signal">
          {initials || "?"}
        </span>
        <div className="flex flex-col gap-1">
          <span className="text-[15px] font-semibold text-foreground">{initialName}</span>
          <span className="w-fit rounded-md bg-muted/70 px-1.5 py-0.5 text-[11px] font-medium text-foreground/75">
            {role}
          </span>
        </div>
      </div>

      <Field label="Display name">
        <input value={name} onChange={(e) => setName(e.target.value)} className={INPUT_CLS} />
      </Field>
      <Field label="Email">
        <input
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className={INPUT_CLS}
        />
      </Field>

      {msg && (
        <p className={cn("text-[12px]", msg.ok ? "text-signal" : "text-danger")}>{msg.text}</p>
      )}

      <div className="flex justify-end">
        <button
          type="submit"
          disabled={!dirty || busy}
          className="inline-flex items-center gap-1.5 rounded-lg bg-signal px-4 py-2 text-[13px] font-medium text-background transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busy && <Loader2 className="size-3.5 animate-spin" />}
          Save changes
        </button>
      </div>
    </form>
  )
}

/** Change password — verifies the current one server-side. */
function PasswordSection() {
  const [current, setCurrent] = useState("")
  const [next, setNext] = useState("")
  const [confirm, setConfirm] = useState("")
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null)

  const match = next.length > 0 && next === confirm
  const canSubmit = current.length > 0 && next.length >= 8 && match && !busy

  const submit = async (e: SyntheticEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    setBusy(true)
    setMsg(null)
    try {
      await changePassword(current, next)
      setMsg({ ok: true, text: "Password changed." })
      setCurrent("")
      setNext("")
      setConfirm("")
    } catch (err) {
      setMsg({ ok: false, text: err instanceof Error ? err.message : "Change failed" })
    } finally {
      setBusy(false)
    }
  }

  return (
    <form
      onSubmit={(e) => void submit(e)}
      className="flex flex-col gap-3 border-t border-border/60 pt-5"
    >
      <span className="flex items-center gap-1.5 text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
        <KeyRound className="size-3.5" /> Change password
      </span>
      <Field label="Current password">
        <input
          type="password"
          autoComplete="current-password"
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
          className={INPUT_CLS}
        />
      </Field>
      <Field
        label="New password"
        hint={next.length > 0 && next.length < 8 ? "Min 8 characters" : undefined}
      >
        <input
          type="password"
          autoComplete="new-password"
          value={next}
          onChange={(e) => setNext(e.target.value)}
          className={INPUT_CLS}
        />
      </Field>
      <Field
        label="Confirm new password"
        hint={confirm.length > 0 && !match ? "Doesn't match" : undefined}
      >
        <input
          type="password"
          autoComplete="new-password"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          className={INPUT_CLS}
        />
      </Field>
      {msg && (
        <p className={cn("text-[12px]", msg.ok ? "text-signal" : "text-danger")}>{msg.text}</p>
      )}
      <div className="flex justify-end">
        <button
          type="submit"
          disabled={!canSubmit}
          className="inline-flex items-center gap-1.5 rounded-lg border border-border bg-card px-4 py-2 text-[13px] font-medium text-foreground transition-colors hover:bg-muted/60 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busy && <Loader2 className="size-3.5 animate-spin" />}
          Update password
        </button>
      </div>
    </form>
  )
}

/** Active device sessions — list + revoke (current device is not revocable). */
function SessionsSection({ open }: { open: boolean }) {
  const [sessions, setSessions] = useState<DeviceSession[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = () => {
    fetchSessions()
      .then((s) => {
        setSessions(s)
        setError(null)
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "Failed to load sessions"))
  }

  useEffect(() => {
    if (open) load()
  }, [open])

  const revoke = async (id: string) => {
    try {
      await revokeSession(id)
      load()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Revoke failed")
    }
  }

  return (
    <div className="flex flex-col gap-3 border-t border-border/60 pt-5">
      <span className="flex items-center gap-1.5 text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
        <Monitor className="size-3.5" /> Active sessions
      </span>
      {error && <p className="text-[12px] text-danger">{error}</p>}
      {sessions === null ? (
        <p className="text-[12px] text-muted-foreground">Loading…</p>
      ) : sessions.length === 0 ? (
        <p className="text-[12px] text-muted-foreground">No active sessions.</p>
      ) : (
        <div className="overflow-hidden rounded-xl border border-border">
          {sessions.map((s) => (
            <div
              key={s.id}
              className="flex items-center gap-3 border-b border-border/60 bg-card px-3.5 py-2.5 last:border-0"
            >
              <Monitor className="size-4 shrink-0 text-muted-foreground" />
              <div className="flex min-w-0 flex-1 flex-col leading-tight">
                <span className="truncate text-[12.5px] font-medium text-foreground/90">
                  {s.user_agent ?? "Unknown device"}
                  {s.current && (
                    <span className="ml-2 rounded bg-signal/15 px-1.5 py-px text-[10px] font-medium text-signal">
                      This device
                    </span>
                  )}
                </span>
                <span className="text-[11px] text-muted-foreground/70">
                  Signed in {new Date(s.created_at).toLocaleString()}
                </span>
              </div>
              {!s.current && (
                <button
                  type="button"
                  onClick={() => void revoke(s.id)}
                  title="Revoke session"
                  className="shrink-0 rounded-md p-1.5 text-muted-foreground/60 transition-colors hover:bg-danger/10 hover:text-danger"
                >
                  <Trash2 className="size-4" />
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: React.ReactNode
  children: React.ReactNode
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="flex items-center justify-between text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
        {label}
        {hint ? (
          <span className="text-[10px] font-medium normal-case tracking-normal text-muted-foreground/70">
            {hint}
          </span>
        ) : null}
      </span>
      {children}
    </label>
  )
}
