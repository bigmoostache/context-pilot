// ── Admin user management dialog (Phase 10) — mobile twin ───────────
//
// Touch twin of the desktop UsersDialog. Logic is byte-identical (list / create
// / delete / force-logout via the same TanStack Query hooks, same role model);
// the presentation is mobile-tuned: the centered 520px dialog becomes a
// full-screen sheet, the two-column create form stacks to a single column, and
// every control uses a 16px input so focusing never triggers iOS focus-zoom.
// The shadcn Dialog primitive is imported from the mobile mirror token (leak
// guard) rather than the desktop tree.

import { useState, type SyntheticEvent } from "react"
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { LogOut, Plus, Shield, ShieldAlert, ShieldHalf, Trash2, Users } from "lucide-react"
import { fetchUsers, createUser, deleteUser, forceLogoutUser } from "@/lib/api"
import { useAuth } from "@/lib/providers/auth"
import { Dialog, DialogContent, DialogClose } from "@/mobile-components/ui/dialog"
import { assignableRoles, type Role } from "@/lib/support/roles"
import { cn } from "@/lib/utils"

// ── Role presentation ────────────────────────────────────────────────
//
// The role model itself (order/rank/assignable/manage predicate) lives in the
// component-free `@/lib/support/roles` module so it can be shared with the shell
// menu without tripping react-refresh. What stays here is purely the dialog's
// presentation of a role: its label and badge.

const ROLE_LABEL: Record<Role, string> = {
  superadmin: "Superadmin",
  admin: "Admin",
  manager: "Manager",
  user: "User",
}

// ── Role badge ───────────────────────────────────────────────────────

const ROLE_BADGE: Record<Role, { className: string; Icon?: typeof Shield }> = {
  superadmin: {
    className: "bg-(--danger)/15 text-(--danger) ring-1 ring-(--danger)/25 ring-inset",
    Icon: ShieldAlert,
  },
  admin: { className: "bg-(--signal)/15 text-(--signal)", Icon: Shield },
  manager: { className: "bg-(--warn)/15 text-(--warn)", Icon: ShieldHalf },
  user: { className: "bg-muted text-muted-foreground" },
}

export function RoleBadge({ role }: { role: string }) {
  const key: Role = Object.hasOwn(ROLE_BADGE, role) ? (role as Role) : "user"
  const style = ROLE_BADGE[key]
  const Icon = style.Icon
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-semibold tracking-wider uppercase",
        style.className,
      )}
    >
      {Icon && <Icon className="size-2.5" />}
      {role}
    </span>
  )
}

// ── Main dialog ──────────────────────────────────────────────────────

export function UsersDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const qc = useQueryClient()
  const { user } = useAuth()
  // Access control off ⇒ no authenticated user ⇒ god-mode (superadmin), design §13.10.
  const viewerRole: Role = user?.role ?? "superadmin"
  const { data: users = [], isLoading } = useQuery({
    queryKey: ["auth-users"],
    queryFn: fetchUsers,
    enabled: open,
  })
  // Defense-in-depth (FR-v3-05): the server already filters superadmin rows for
  // non-superadmin callers via `can_see`; hide them client-side too.
  const visibleUsers =
    viewerRole === "superadmin" ? users : users.filter((u) => u.role !== "superadmin")
  const [showCreate, setShowCreate] = useState(false)
  const [confirm, setConfirm] = useState<{ id: string; name: string } | null>(null)

  const deleteMut = useMutation({
    mutationFn: (id: string) => deleteUser(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["auth-users"] })
      setConfirm(null)
    },
  })
  const logoutMut = useMutation({
    mutationFn: (id: string) => forceLogoutUser(id),
  })

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      {/* Full-screen sheet on mobile (vs the desktop centered 520px card). */}
      <DialogContent className="flex h-screen w-screen max-w-none flex-col overflow-hidden rounded-none p-0">
        {/* header */}
        <div className="flex items-center gap-3 border-b border-border/70 p-4">
          <span className="flex size-9 items-center justify-center rounded-xl bg-(--signal)/14 text-(--signal) ring-1 ring-(--signal)/25 ring-inset">
            <Users className="size-[18px]" />
          </span>
          <div className="flex-1">
            <h3 className="text-[16px] font-semibold tracking-tight text-foreground">
              Manage Users
            </h3>
            <p className="text-[11px] text-muted-foreground">
              {visibleUsers.length} registered user{visibleUsers.length === 1 ? "" : "s"}
            </p>
          </div>
          <button
            onClick={() => setShowCreate((s) => !s)}
            className="flex items-center gap-1.5 rounded-lg bg-(--signal)/12 px-3 py-2 text-[12.5px] font-medium text-(--signal) transition-colors active:bg-(--signal)/20"
          >
            <Plus className="size-3.5" />
            Add user
          </button>
          <DialogClose
            aria-label="Close"
            className="flex size-9 items-center justify-center rounded-md text-muted-foreground/55 transition-colors active:bg-muted/70"
          >
            ✕
          </DialogClose>
        </div>

        {/* create form (collapsible) */}
        {showCreate && (
          <CreateUserForm
            onCreated={() => {
              void qc.invalidateQueries({ queryKey: ["auth-users"] })
              setShowCreate(false)
            }}
          />
        )}

        {/* user list */}
        <div className="flex-1 overflow-y-auto px-4 py-3">
          {isLoading && (
            <p className="animate-pulse py-8 text-center text-xs text-muted-foreground">
              Loading users…
            </p>
          )}
          {!isLoading && visibleUsers.length === 0 && (
            <p className="py-8 text-center text-xs text-muted-foreground">
              No users registered yet.
            </p>
          )}
          <div className="flex flex-col gap-1">
            {visibleUsers.map((u) => (
              <div
                key={u.id}
                className="flex items-center gap-3 rounded-lg p-3 transition-colors active:bg-muted/50"
              >
                <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-muted text-[11px] font-semibold text-muted-foreground">
                  {u.name
                    .split(" ")
                    .map((w) => w[0])
                    .join("")
                    .slice(0, 2)
                    .toUpperCase()}
                </span>
                <div className="flex min-w-0 flex-1 flex-col">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-[14px] font-medium text-foreground">
                      {u.name}
                    </span>
                    <RoleBadge role={u.role} />
                  </div>
                  <span className="truncate text-[12px] text-muted-foreground">{u.email}</span>
                </div>
                {/* actions — always visible on touch (no hover) */}
                <div className="flex items-center gap-1">
                  <button
                    title="Force logout (revoke all sessions)"
                    onClick={() => logoutMut.mutate(u.id)}
                    disabled={logoutMut.isPending}
                    className="flex size-9 items-center justify-center rounded-md text-muted-foreground transition-colors active:bg-muted"
                  >
                    <LogOut className="size-4" />
                  </button>
                  <button
                    title="Delete user"
                    onClick={() => setConfirm({ id: u.id, name: u.name })}
                    className="flex size-9 items-center justify-center rounded-md text-muted-foreground transition-colors active:bg-(--danger)/10 active:text-(--danger)"
                  >
                    <Trash2 className="size-4" />
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* delete confirmation */}
        {confirm && (
          <div className="border-t border-border/70 bg-(--danger)/5 px-4 py-3 pb-[env(safe-area-inset-bottom)]">
            <p className="mb-2 text-[12px] text-(--danger)">
              Delete <strong>{confirm.name}</strong>? This cascades to all their sessions and agent
              access entries. This cannot be undone.
            </p>
            <div className="flex gap-2">
              <button
                onClick={() => deleteMut.mutate(confirm.id)}
                disabled={deleteMut.isPending}
                className="flex-1 rounded-lg bg-(--danger) px-3 py-2.5 text-[13px] font-medium text-white transition-opacity active:opacity-90 disabled:opacity-50"
              >
                {deleteMut.isPending ? "Deleting…" : "Delete"}
              </button>
              <button
                onClick={() => setConfirm(null)}
                className="flex-1 rounded-lg px-3 py-2.5 text-[13px] font-medium text-muted-foreground transition-colors active:bg-muted"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

// ── Create user form ─────────────────────────────────────────────────

function CreateUserForm({ onCreated }: { onCreated: () => void }) {
  const { user } = useAuth()
  // Access control off ⇒ god-mode (superadmin), design §13.10.
  const viewerRole: Role = user?.role ?? "superadmin"
  const roleOptions = assignableRoles(viewerRole)
  const [email, setEmail] = useState("")
  const [name, setName] = useState("")
  const [password, setPassword] = useState("")
  const [role, setRole] = useState<Role>("user")
  const [error, setError] = useState("")

  const create = useMutation({
    mutationFn: () => createUser(email, name, password, role),
    onSuccess: () => {
      setEmail("")
      setName("")
      setPassword("")
      setError("")
      onCreated()
    },
    onError: (e) =>
      setError(
        e instanceof Error ? e.message.replace(/^\d+\s+\/api\/auth\/\w+:\s*/, "") : "Failed",
      ),
  })

  const submit = (e: SyntheticEvent) => {
    e.preventDefault()
    setError("")
    create.mutate()
  }

  return (
    <form onSubmit={submit} className="border-b border-border/70 bg-muted/30 p-4">
      {/* Single column on a phone (vs the desktop 2-up grid). 16px inputs kill
          iOS focus-zoom. */}
      <div className="mb-3 flex flex-col gap-3">
        <input
          type="text"
          required
          placeholder="Name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="rounded-md border border-border bg-background p-3 text-base text-foreground placeholder:text-muted-foreground/50 focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
        />
        <input
          type="email"
          required
          placeholder="Email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className="rounded-md border border-border bg-background p-3 text-base text-foreground placeholder:text-muted-foreground/50 focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
        />
        <input
          type="password"
          required
          minLength={8}
          placeholder="Password (min 8)"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="rounded-md border border-border bg-background p-3 text-base text-foreground placeholder:text-muted-foreground/50 focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
        />
        <select
          value={role}
          onChange={(e) => setRole(e.target.value as Role)}
          className="rounded-md border border-border bg-background p-3 text-base text-foreground focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
        >
          {roleOptions.map((r) => (
            <option key={r} value={r}>
              {ROLE_LABEL[r]}
            </option>
          ))}
        </select>
      </div>
      {error && <p className="mb-2 text-[11px] text-(--danger)">{error}</p>}
      <button
        type="submit"
        disabled={create.isPending}
        className="w-full rounded-md bg-signal p-3 text-base font-medium text-background transition-opacity active:opacity-90 disabled:opacity-50"
      >
        {create.isPending ? "Creating…" : "Create user"}
      </button>
    </form>
  )
}
