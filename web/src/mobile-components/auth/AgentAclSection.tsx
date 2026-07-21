// ── Per-agent ACL management section (Phase 10) — mobile twin ───────
//
// Touch twin of the desktop AgentAclSection, embedded inside the mobile
// AgentModal (manage mode). Logic is byte-identical (list / grant / revoke /
// toggle-role via the same TanStack Query hooks); the presentation is
// mobile-tuned: the hover-revealed row actions become always-visible (there is
// no hover on touch), rows and controls grow to ≥44px touch targets, and the
// grant-form selects use a 16px control so focusing never triggers iOS
// focus-zoom.

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
        isAdmin ? "bg-(--signal)/15 text-(--signal)" : "bg-muted text-muted-foreground",
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
      updateAgentRole(agentId, e.user_id, e.role === "agent-admin" ? "agent-user" : "agent-admin"),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  })

  const revoke = useMutation({
    mutationFn: (userId: string) => revokeAccess(agentId, userId),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  })

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
          Access control
        </span>
        <button
          onClick={() => setShowGrant((s) => !s)}
          className="flex items-center gap-1 rounded-md px-2 py-1.5 text-[11.5px] font-medium text-(--signal) transition-colors active:bg-(--signal)/10"
        >
          <Plus className="size-3.5" />
          Grant
        </button>
      </div>

      {showGrant && (
        <GrantForm
          agentId={agentId}
          existingUserIds={entries.map((e) => e.user_id)}
          onGranted={() => {
            void qc.invalidateQueries({ queryKey: key })
            setShowGrant(false)
          }}
          onCancel={() => setShowGrant(false)}
        />
      )}

      {isLoading && (
        <p className="animate-pulse py-3 text-center text-[11px] text-muted-foreground">Loading…</p>
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
            className="flex items-center gap-2 rounded-md px-2 py-2.5 transition-colors active:bg-muted/40"
          >
            <span className="flex size-7 shrink-0 items-center justify-center rounded-full bg-muted text-[9px] font-semibold text-muted-foreground">
              {e.user_name
                .split(" ")
                .map((w) => w[0])
                .join("")
                .slice(0, 2)
                .toUpperCase()}
            </span>
            <div className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-[13px] font-medium text-foreground">
                {e.user_name}
              </span>
              <span className="truncate text-[11px] text-muted-foreground">{e.user_email}</span>
            </div>
            {/* role toggle + revoke — always visible on touch (no hover). */}
            <button
              title="Toggle role"
              onClick={() => toggleRole.mutate(e)}
              disabled={toggleRole.isPending}
              className="transition-opacity active:opacity-80"
            >
              <AgentRoleBadge role={e.role} />
            </button>
            <button
              title="Revoke access"
              onClick={() => revoke.mutate(e.user_id)}
              disabled={revoke.isPending}
              className="flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors active:bg-(--danger)/10 active:text-(--danger)"
            >
              <Trash2 className="size-3.5" />
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
    <div className="rounded-lg border border-border bg-muted/30 p-3">
      {available.length === 0 ? (
        <p className="text-[11px] text-muted-foreground">All users already have access.</p>
      ) : (
        // Single column on a phone; 16px selects kill iOS focus-zoom.
        <div className="flex flex-col gap-2">
          <select
            value={selected}
            onChange={(e) => setSelected(e.target.value)}
            className="w-full rounded-md border border-border bg-background p-3 text-base text-foreground focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
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
            className="w-full rounded-md border border-border bg-background p-3 text-base text-foreground focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
          >
            <option value="agent-user">agent-user</option>
            <option value="agent-admin">agent-admin</option>
          </select>
          <div className="flex items-center gap-2">
            <button
              onClick={() => {
                setError("")
                grant.mutate()
              }}
              disabled={!selected || grant.isPending}
              className="flex-1 rounded-md bg-signal p-3 text-[13px] font-medium text-background transition-opacity active:opacity-90 disabled:opacity-50"
            >
              Grant
            </button>
            <button
              onClick={onCancel}
              className="flex size-11 items-center justify-center rounded-md text-muted-foreground active:bg-muted"
            >
              <X className="size-4" />
            </button>
          </div>
        </div>
      )}
      {error && <p className="mt-1 text-[10px] text-(--danger)">{error}</p>}
    </div>
  )
}
