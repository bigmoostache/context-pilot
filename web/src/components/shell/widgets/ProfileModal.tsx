import { useState } from "react"
import {
  Building2,
  Camera,
  CreditCard,
  KeyRound,
  Lock,
  ShieldCheck,
  Trash2,
} from "lucide-react"
import { Dialog, DialogContent, DialogClose } from "@/components/ui/dialog"
import { useAccount } from "@/lib/support/account"
import { AvatarMark } from "./UserMenu"
import { cn } from "@/lib/utils"

/**
 * Profile modal (T30) — opened from the account avatar menu, sibling to the
 * Settings dialog and built on the same portaled Base-UI {@link Dialog}.
 *
 * It surfaces the human's personal info (photo, name, email) and — crucially —
 * an **account-management** section that adapts to whether the account is
 * provisioned by an organization. When `managedByCompany` is set, identity &
 * security live with the org (fields lock, an org callout explains it); when
 * personal, the user gets self-service actions (password, billing, delete).
 *
 * The account section adapts to {@link User.managedByCompany}: an org callout
 * when managed, self-service actions when personal. Everything here is
 * decorative (no backend).
 */
export function ProfileModal({
  open,
  onClose,
}: {
  open: boolean
  onClose: () => void
}) {
  const { user: u, managed, setManaged } = useAccount()
  const [name, setName] = useState(u.name)

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[88vh] w-[540px] max-w-[calc(100vw-2rem)] flex-col">
        {/* header */}
        <header className="flex items-start gap-3 border-b border-border/70 px-6 pb-4 pt-5">
          <div className="flex flex-1 flex-col gap-0.5">
            <h2 className="text-[17px] font-semibold tracking-tight text-foreground">Profile</h2>
            <p className="text-[12px] text-muted-foreground">Your account and personal information.</p>
          </div>
          <DialogClose
            aria-label="Close"
            className="-mr-1 -mt-1 flex size-7 items-center justify-center rounded-md text-muted-foreground/55 transition-colors hover:bg-muted/70 hover:text-foreground"
          >
            ✕
          </DialogClose>
        </header>

        {/* body */}
        <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-6 py-5">
          {/* identity / photo */}
          <div className="flex items-center gap-4">
            <div className="relative">
              <AvatarMark user={u} className="size-[68px] text-[24px]" />
              <button
                type="button"
                title="Change photo"
                className="absolute -bottom-1 -right-1 flex size-7 items-center justify-center rounded-full border-2 border-popover bg-[var(--interactive)] text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
              >
                <Camera className="size-3.5" />
              </button>
            </div>
            <div className="flex min-w-0 flex-col gap-1">
              <span className="truncate text-[15px] font-semibold text-foreground">{name || u.name}</span>
              <span className="flex flex-wrap items-center gap-1.5 text-[12px] text-muted-foreground">
                {u.role && (
                  <span className="rounded-md bg-muted/70 px-1.5 py-0.5 text-[11px] font-medium text-foreground/75">
                    {u.role}
                  </span>
                )}
                {managed && u.company && (
                  <span className="inline-flex items-center gap-1 rounded-md bg-[var(--interactive)]/12 px-1.5 py-0.5 text-[11px] font-medium text-[var(--interactive)]">
                    <Building2 className="size-3" />
                    {u.company}
                  </span>
                )}
              </span>
            </div>
          </div>

          {/* editable fields */}
          <div className="flex flex-col gap-4">
            <Field label="Display name">
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="w-full rounded-lg border border-border bg-card px-3 py-2 text-[13px] text-foreground outline-none transition-colors focus:border-[var(--interactive)]/70 focus:ring-2 focus:ring-[var(--interactive)]/15"
              />
            </Field>

            <Field
              label="Email"
              hint={
                managed ? (
                  <span className="inline-flex items-center gap-1 text-muted-foreground/70">
                    <Lock className="size-3" /> Managed by {u.company}
                  </span>
                ) : undefined
              }
            >
              <div className="relative">
                <input
                  defaultValue={u.email}
                  readOnly={managed}
                  className={cn(
                    "w-full rounded-lg border border-border px-3 py-2 text-[13px] outline-none transition-colors",
                    managed
                      ? "cursor-not-allowed bg-muted/50 text-muted-foreground"
                      : "bg-card text-foreground focus:border-[var(--interactive)]/70 focus:ring-2 focus:ring-[var(--interactive)]/15",
                  )}
                />
                {managed && (
                  <Lock className="absolute right-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground/50" />
                )}
              </div>
            </Field>
          </div>

          {/* account management */}
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between gap-2">
              <span className="text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
                Account
              </span>
              <AccountTypeSwitch managed={managed} onChange={setManaged} />
            </div>
            {managed ? <ManagedAccount company={u.company ?? "your organization"} /> : <PersonalAccount />}
          </div>
        </div>

        {/* footer */}
        <footer className="flex items-center justify-end gap-2 border-t border-border/70 bg-muted/25 px-6 py-4">
          <DialogClose className="rounded-lg px-3.5 py-2 text-[12.5px] font-medium text-muted-foreground transition-colors hover:bg-muted/70 hover:text-foreground">
            Cancel
          </DialogClose>
          <DialogClose className="rounded-lg bg-[var(--interactive)] px-4 py-2 text-[13px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105">
            Save changes
          </DialogClose>
        </footer>
      </DialogContent>
    </Dialog>
  )
}

/** Org-managed account callout — identity & security owned by the company. */
function ManagedAccount({ company }: { company: string }) {
  return (
    <div className="flex items-start gap-3 rounded-xl border border-[var(--interactive)]/30 bg-[var(--interactive)]/[0.06] px-3.5 py-3">
      <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-[var(--interactive)]/14 text-[var(--interactive)]">
        <ShieldCheck className="size-[18px]" />
      </span>
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[12.5px] font-semibold text-foreground/90">Managed by {company}</span>
        <span className="text-[11.5px] leading-relaxed text-muted-foreground">
          Your name, email and security settings are provisioned by your organization. Contact your
          administrator to change account details or sign-in.
        </span>
      </div>
    </div>
  )
}

/** Personal account — self-service management actions (decorative). */
function PersonalAccount() {
  return (
    <div className="overflow-hidden rounded-xl border border-border">
      <ActionRow icon={KeyRound} label="Change password" sub="Update your sign-in credentials" />
      <ActionRow icon={CreditCard} label="Billing & plan" sub="Manage subscription and invoices" />
      <ActionRow icon={Trash2} label="Delete account" sub="Permanently remove your account" danger />
    </div>
  )
}

function ActionRow({
  icon: Icon,
  label,
  sub,
  danger,
}: {
  icon: typeof KeyRound
  label: string
  sub: string
  danger?: boolean
}) {
  return (
    <button
      type="button"
      className="flex w-full items-center gap-3 border-b border-border/60 bg-card px-3.5 py-2.5 text-left transition-colors last:border-0 hover:bg-muted/40"
    >
      <Icon className={cn("size-4 shrink-0", danger ? "text-[var(--danger)]" : "text-muted-foreground")} />
      <span className="flex flex-1 flex-col leading-tight">
        <span className={cn("text-[12.5px] font-medium", danger ? "text-[var(--danger)]" : "text-foreground/90")}>
          {label}
        </span>
        <span className="text-[11px] text-muted-foreground/70">{sub}</span>
      </span>
    </button>
  )
}

/**
 * Account-type switch — flips the account between **Company** (managed) and
 * **Personal**. Drives the shared account context, so it updates both this
 * modal's account section *and* the API-key lock in Settings, letting you
 * compare the two states.
 */
function AccountTypeSwitch({
  managed,
  onChange,
}: {
  managed: boolean
  onChange: (managed: boolean) => void
}) {
  return (
    <div className="flex items-center gap-0.5 rounded-lg border border-border bg-muted/60 p-0.5">
      {(
        [
          { id: true, label: "Company" },
          { id: false, label: "Personal" },
        ] as const
      ).map((o) => {
        const on = o.id === managed
        return (
          <button
            key={o.label}
            type="button"
            onClick={() => onChange(o.id)}
            className={cn(
              "rounded-md px-2 py-0.5 text-[11px] font-medium transition-all",
              on ? "bg-card text-foreground card-shadow" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {o.label}
          </button>
        )
      })}
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
        {hint && <span className="text-[10px] font-medium normal-case tracking-normal">{hint}</span>}
      </span>
      {children}
    </label>
  )
}
