// ── Admin user management dialog (Phase 10) ─────────────────────────
//
// Accessible from the UserMenu when the authenticated user is a system
// admin. Lists all users, allows creation and deletion, force-logout.

import { useState, type SyntheticEvent } from "react"
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { LogOut, Plus, Shield, Trash2, Users } from "lucide-react"
import { fetchUsers, createUser, deleteUser, forceLogoutUser } from "@/lib/api"
import { Dialog, DialogContent, DialogClose } from "@/components/ui/dialog"
import { cn } from "@/lib/utils"

// ── Role badge ───────────────────────────────────────────────────────

export function RoleBadge({ role }: { role: string }) {
  const isAdmin = role === "admin"
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-semibold tracking-wider uppercase",
        isAdmin ? "bg-(--signal)/15 text-(--signal)" : "bg-muted text-muted-foreground",
      )}
    >
      {isAdmin && <Shield className="size-2.5" />}
      {role}
    </span>
  )
}

// ── Main dialog ──────────────────────────────────────────────────────

export function UsersDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const qc = useQueryClient()
  const { data: users = [], isLoading } = useQuery({
    queryKey: ["auth-users"],
    queryFn: fetchUsers,
    enabled: open,
  })
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
      <DialogContent className="flex max-h-[80vh] w-[520px] max-w-[calc(100vw-2rem)] flex-col overflow-hidden p-0">
        {/* header */}
        <div className="flex items-center gap-3 border-b border-border/70 px-5 py-4">
          <span className="flex size-9 items-center justify-center rounded-xl bg-(--signal)/14 text-(--signal) ring-1 ring-(--signal)/25 ring-inset">
            <Users className="size-[18px]" />
          </span>
          <div className="flex-1">
            <h3 className="text-[16px] font-semibold tracking-tight text-foreground">
              Manage Users
            </h3>
            <p className="text-[11px] text-muted-foreground">
              {users.length} registered user{users.length === 1 ? "" : "s"}
            </p>
          </div>
          <button
            onClick={() => setShowCreate((s) => !s)}
            className="flex items-center gap-1.5 rounded-lg bg-(--signal)/12 px-3 py-1.5 text-[11.5px] font-medium text-(--signal) transition-colors hover:bg-(--signal)/20"
          >
            <Plus className="size-3.5" />
            Add user
          </button>
          <DialogClose
            aria-label="Close"
            className="flex size-7 items-center justify-center rounded-md text-muted-foreground/55 transition-colors hover:bg-muted/70 hover:text-foreground"
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
        <div className="flex-1 overflow-y-auto px-5 py-3">
          {isLoading && (
            <p className="animate-pulse py-8 text-center text-xs text-muted-foreground">
              Loading users…
            </p>
          )}
          {!isLoading && users.length === 0 && (
            <p className="py-8 text-center text-xs text-muted-foreground">
              No users registered yet.
            </p>
          )}
          <div className="flex flex-col gap-1">
            {users.map((u) => (
              <div
                key={u.id}
                className="group flex items-center gap-3 rounded-lg px-3 py-2.5 transition-colors hover:bg-muted/50"
              >
                <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-muted text-[11px] font-semibold text-muted-foreground">
                  {u.name
                    .split(" ")
                    .map((w) => w[0])
                    .join("")
                    .slice(0, 2)
                    .toUpperCase()}
                </span>
                <div className="flex min-w-0 flex-1 flex-col">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-[13px] font-medium text-foreground">
                      {u.name}
                    </span>
                    <RoleBadge role={u.role} />
                  </div>
                  <span className="truncate text-[11px] text-muted-foreground">{u.email}</span>
                </div>
                {/* actions */}
                <div className="flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
                  <button
                    title="Force logout (revoke all sessions)"
                    onClick={() => logoutMut.mutate(u.id)}
                    disabled={logoutMut.isPending}
                    className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                  >
                    <LogOut className="size-3.5" />
                  </button>
                  <button
                    title="Delete user"
                    onClick={() => setConfirm({ id: u.id, name: u.name })}
                    className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-(--danger)/10 hover:text-(--danger)"
                  >
                    <Trash2 className="size-3.5" />
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* delete confirmation */}
        {confirm && (
          <div className="border-t border-border/70 bg-(--danger)/5 px-5 py-3">
            <p className="mb-2 text-[12px] text-(--danger)">
              Delete <strong>{confirm.name}</strong>? This cascades to all their sessions and agent
              access entries. This cannot be undone.
            </p>
            <div className="flex gap-2">
              <button
                onClick={() => deleteMut.mutate(confirm.id)}
                disabled={deleteMut.isPending}
                className="rounded-lg bg-(--danger) px-3 py-1.5 text-[12px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
              >
                {deleteMut.isPending ? "Deleting…" : "Delete"}
              </button>
              <button
                onClick={() => setConfirm(null)}
                className="rounded-lg px-3 py-1.5 text-[12px] font-medium text-muted-foreground transition-colors hover:bg-muted"
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
  const [email, setEmail] = useState("")
  const [name, setName] = useState("")
  const [password, setPassword] = useState("")
  const [role, setRole] = useState<"admin" | "user">("user")
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
    <form onSubmit={submit} className="border-b border-border/70 bg-muted/30 px-5 py-4">
      <div className="mb-3 grid grid-cols-2 gap-3">
        <input
          type="text"
          required
          placeholder="Name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="rounded-md border border-border bg-background px-2.5 py-1.5 text-[12.5px] text-foreground placeholder:text-muted-foreground/50 focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
        />
        <input
          type="email"
          required
          placeholder="Email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className="rounded-md border border-border bg-background px-2.5 py-1.5 text-[12.5px] text-foreground placeholder:text-muted-foreground/50 focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
        />
        <input
          type="password"
          required
          minLength={8}
          placeholder="Password (min 8)"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="rounded-md border border-border bg-background px-2.5 py-1.5 text-[12.5px] text-foreground placeholder:text-muted-foreground/50 focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
        />
        <select
          value={role}
          onChange={(e) => setRole(e.target.value as "admin" | "user")}
          className="rounded-md border border-border bg-background px-2.5 py-1.5 text-[12.5px] text-foreground focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
        >
          <option value="user">User</option>
          <option value="admin">Admin</option>
        </select>
      </div>
      {error && <p className="mb-2 text-[11px] text-(--danger)">{error}</p>}
      <button
        type="submit"
        disabled={create.isPending}
        className="rounded-md bg-signal px-3 py-1.5 text-[12px] font-medium text-background transition-opacity hover:opacity-90 disabled:opacity-50"
      >
        {create.isPending ? "Creating…" : "Create user"}
      </button>
    </form>
  )
}
