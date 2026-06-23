// ── Per-agent ACL management section (Phase 10) ─────────────────────
//
// Embedded inside AgentModal (manage mode) when auth is enabled.
// Shows who has access, lets admins/agent-admins grant, revoke, toggle roles.

import { useState } from "react"
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { Plus, Shield, ShieldCheck, Trash2, X } from "lucide-react"
import {
  fetchAgentAcl,
  fetchUsers,
  grantAccess,
  updateAgentRole,
  revokeAccess,
  type AclEntry,
  type AuthUser,
} from "@/lib/api"
import { cn } from "@/lib/utils"

/** Per-agent role badge — distinct from the system-level RoleBadge. */
function AgentRoleBadge({ role }: { role: string }) {
  const isAdmin = role === "agent-admin"
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-1.5 py-px text-[9.5px] font-semibold",
        isAdmin
          ? "bg-[var(--signal)]/15 text-[var(--signal)]"
          : "bg-muted text-muted-foreground",
      )}
    >
      {isAdmin ? <ShieldCheck className="size-2.5" /> : <Shield className="size-2.5" />}
      {isAdmin ? "admin" : "user"}
    </span>
  )
}

export function AgentAclSection({ agentId }: { agentId: string }) {
  const qc = useQueryClient()
  const key = ["agent-acl", agentId]
  const { data: entries = [], isLoading } = useQuery({
    queryKey: key,
    queryFn: () => fetchAgentAcl(agentId),
  })

  const [showGrant, setShowGrant] = useState(false)

  const toggleRole = useMutation({
    mutationFn: (e: AclEntry) =>
      updateAgentRole(
        agentId,
        e.user_id,
        e.role === "agent-admin" ? "agent-user" : "agent-admin",
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  })

  const revoke = useMutation({
    mutationFn: (userId: string) => revokeAccess(agentId, userId),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  })

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <span className="text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
          Access control
        </span>
        <button
          onClick={() => setShowGrant((s) => !s)}
          className="flex items-center gap-1 rounded-md px-2 py-1 text-[10.5px] font-medium text-[var(--signal)] transition-colors hover:bg-[var(--signal)]/10"
        >
          <Plus className="size-3" />
          Grant
        </button>
      </div>

      {showGrant && (
        <GrantForm
          agentId={agentId}
          existingUserIds={entries.map((e) => e.user_id)}
          onGranted={() => {
            qc.invalidateQueries({ queryKey: key })
            setShowGrant(false)
          }}
          onCancel={() => setShowGrant(false)}
        />
      )}

      {isLoading && (
        <p className="py-3 text-center text-[11px] text-muted-foreground animate-pulse">
          Loading…
        </p>
      )}

      {!isLoading && entries.length === 0 && (
        <p className="py-3 text-center text-[11px] text-muted-foreground">
          No access entries — only system admins can see this agent.
        </p>
      )}

      <div className="flex flex-col gap-0.5">
        {entries.map((e) => (
          <div
            key={e.user_id}
            className="group flex items-center gap-2 rounded-md px-2 py-1.5 transition-colors hover:bg-muted/40"
          >
            <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-muted text-[9px] font-semibold text-muted-foreground">
              {e.user_name
                .split(" ")
                .map((w) => w[0])
                .join("")
                .slice(0, 2)
                .toUpperCase()}
            </span>
            <div className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-[12px] font-medium text-foreground">
                {e.user_name}
              </span>
              <span className="truncate text-[10px] text-muted-foreground">
                {e.user_email}
              </span>
            </div>
            <button
              title="Toggle role"
              onClick={() => toggleRole.mutate(e)}
              disabled={toggleRole.isPending}
              className="transition-opacity hover:opacity-80"
            >
              <AgentRoleBadge role={e.role} />
            </button>
            <button
              title="Revoke access"
              onClick={() => revoke.mutate(e.user_id)}
              disabled={revoke.isPending}
              className="flex size-6 items-center justify-center rounded-md text-muted-foreground opacity-0 transition-all hover:bg-[var(--danger)]/10 hover:text-[var(--danger)] group-hover:opacity-100"
            >
              <Trash2 className="size-3" />
            </button>
          </div>
        ))}
      </div>
    </div>
  )
}

// ── Grant access form ────────────────────────────────────────────────

function GrantForm({
  agentId,
  existingUserIds,
  onGranted,
  onCancel,
}: {
  agentId: string
  existingUserIds: string[]
  onGranted: () => void
  onCancel: () => void
}) {
  const { data: allUsers = [] } = useQuery({
    queryKey: ["auth-users"],
    queryFn: fetchUsers,
  })
  const available = allUsers.filter((u: AuthUser) => !existingUserIds.includes(u.id))

  const [selected, setSelected] = useState("")
  const [role, setRole] = useState<"agent-admin" | "agent-user">("agent-user")
  const [error, setError] = useState("")

  const grant = useMutation({
    mutationFn: () => grantAccess(agentId, selected, role),
    onSuccess: onGranted,
    onError: (e) => setError(e instanceof Error ? e.message : "Grant failed"),
  })

  return (
    <div className="rounded-lg border border-border bg-muted/30 px-3 py-2.5">
      {available.length === 0 ? (
        <p className="text-[11px] text-muted-foreground">
          All users already have access.
        </p>
      ) : (
        <div className="flex items-center gap-2">
          <select
            value={selected}
            onChange={(e) => setSelected(e.target.value)}
            className="flex-1 rounded-md border border-border bg-background px-2 py-1.5 text-[11.5px] text-foreground focus:border-signal focus:outline-none focus:ring-1 focus:ring-signal"
          >
            <option value="">Select user…</option>
            {available.map((u: AuthUser) => (
              <option key={u.id} value={u.id}>
                {u.name} ({u.email})
              </option>
            ))}
          </select>
          <select
            value={role}
            onChange={(e) => setRole(e.target.value as "agent-admin" | "agent-user")}
            className="rounded-md border border-border bg-background px-2 py-1.5 text-[11.5px] text-foreground focus:border-signal focus:outline-none focus:ring-1 focus:ring-signal"
          >
            <option value="agent-user">agent-user</option>
            <option value="agent-admin">agent-admin</option>
          </select>
          <button
            onClick={() => { setError(""); grant.mutate() }}
            disabled={!selected || grant.isPending}
            className="rounded-md bg-signal px-2.5 py-1.5 text-[11px] font-medium text-background transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            Grant
          </button>
          <button
            onClick={onCancel}
            className="flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-muted"
          >
            <X className="size-3.5" />
          </button>
        </div>
      )}
      {error && <p className="mt-1 text-[10px] text-[var(--danger)]">{error}</p>}
    </div>
  )
}
