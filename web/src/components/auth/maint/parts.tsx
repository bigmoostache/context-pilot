// ── Shared building blocks for the IT maintenance wizard ─────────────
//
// Small presentational helpers matching the app's auth-screen idiom (plain
// inputs/buttons styled with the design tokens), so the wizard looks native
// without pulling in extra component surface.

import type { ReactNode } from "react"

/** Full-screen centered shell with the Context Pilot header + a step label. */
export function Shell({
  title,
  subtitle,
  step,
  children,
}: {
  title: string
  subtitle?: string
  step?: { current: number; total: number }
  children: ReactNode
}) {
  return (
    <div className="flex min-h-screen w-screen items-center justify-center overflow-auto bg-background p-4">
      <div className="w-full max-w-md py-8">
        <div className="mb-6 text-center">
          <div className="mb-2 font-mono text-2xl font-bold tracking-tight text-foreground">
            <span className="text-signal">▌</span> Context Pilot
          </div>
          <div className="text-xs uppercase tracking-wider text-muted-foreground">IT maintenance console</div>
        </div>
        {step && (
          <div className="mb-3 flex items-center justify-center gap-1.5">
            {Array.from({ length: step.total }, (_, i) => (
              <span
                key={i}
                className={`h-1.5 w-8 rounded-full ${i <= step.current ? "bg-signal" : "bg-border"}`}
              />
            ))}
          </div>
        )}
        <div className="rounded-lg border border-border bg-card p-6 shadow-md">
          <h2 className="mb-1 text-base font-semibold text-foreground">{title}</h2>
          {subtitle && <p className="mb-4 text-sm text-muted-foreground">{subtitle}</p>}
          {children}
        </div>
      </div>
    </div>
  )
}

/** Labelled text/password input. */
export function Field({
  label,
  value,
  onChange,
  type = "text",
  hint,
  placeholder,
  autoFocus,
  autoComplete,
}: {
  label: string
  value: string
  onChange: (v: string) => void
  type?: string
  hint?: string
  placeholder?: string
  autoFocus?: boolean
  autoComplete?: string
}) {
  return (
    <label className="mb-3 flex flex-col gap-1.5">
      <span className="text-xs font-medium text-foreground/90">
        {label}
        {hint && <span className="ml-2 text-muted-foreground/60">{hint}</span>}
      </span>
      <input
        type={type}
        value={value}
        placeholder={placeholder}
        autoFocus={autoFocus}
        autoComplete={autoComplete}
        onChange={(e) => onChange(e.target.value)}
        className="rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground
                   focus:border-signal focus:outline-none focus:ring-1 focus:ring-signal"
      />
    </label>
  )
}

/** Primary action button (signal-coloured). */
export function PrimaryButton({
  children,
  disabled,
  busy,
  onClick,
  type = "button",
}: {
  children: ReactNode
  disabled?: boolean
  busy?: boolean
  onClick?: () => void
  type?: "button" | "submit"
}) {
  return (
    <button
      type={type}
      disabled={disabled || busy}
      onClick={onClick}
      className="w-full rounded-md bg-signal px-4 py-2 text-sm font-semibold text-background
                 transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
    >
      {busy ? "Working…" : children}
    </button>
  )
}

/** Secondary / ghost button. */
export function GhostButton({ children, onClick }: { children: ReactNode; onClick?: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="w-full rounded-md border border-border bg-background px-4 py-2 text-sm font-medium
                 text-foreground transition-colors hover:bg-muted"
    >
      {children}
    </button>
  )
}

/** Inline error banner. */
export function ErrorNote({ error }: { error: string | null }) {
  if (!error) return null
  return <div className="mb-3 rounded-md bg-danger/10 px-3 py-2 text-xs text-danger">{error}</div>
}
