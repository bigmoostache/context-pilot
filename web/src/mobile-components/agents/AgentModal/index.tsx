import { X } from "lucide-react"
import { usePickerProviders } from "@/lib/support/models"
import { useAuth } from "@/lib/providers/auth"
import { useState } from "react"
import { slugify, useSelectionState, type AgentModalMode } from "./controller"
import { useAgentModalActions } from "./actions"
import { AgentModalHeader, AgentModalBody, AgentModalFooter, type Controller } from "./parts"

export type { AgentModalMode } from "./controller"

/**
 * Agent create / manage dialog — mobile twin of `components/agents/AgentModal`.
 *
 * Same single-source-of-truth for both flows (create from fleet, manage from a
 * card), same selection + mutation wiring (all shared via ./controller +
 * ./actions). The fork is the surface: the desktop centered `460/1120px`
 * floating card becomes a **full-screen sheet** (`inset-0`, no rounded gutters,
 * no max-width) — a phone has no room for a floating modal, so the modal owns
 * the whole viewport and its body scrolls. The manage layout's desktop
 * two-column grid (form | vitals+ACL) is stacked to a single column in
 * {@link AgentModalBody}.
 */
export function AgentModal({
  modal,
  onClose,
  onFlash,
}: {
  modal: AgentModalMode
  onClose: () => void
  onFlash?: (m: string) => void
}) {
  const isManage = modal.mode === "manage"
  const agent = isManage ? modal.agent : undefined
  const [name, setName] = useState(agent?.name ?? "")

  // Server-computed picker list: only providers with a configured key, org
  // allowlist already applied (empty ⇒ all).
  const { data: providers = [] } = usePickerProviders()
  const sel = useSelectionState(isManage, agent, providers)
  const actions = useAgentModalActions({
    isManage,
    agent,
    name,
    sel,
    providers,
    onClose,
    onFlash,
  })
  const { authEnabled } = useAuth()

  const realm = isManage ? (agent?.folder ?? "") : `~/code/${slugify(name)}`
  const canSubmit = (isManage || name.trim().length > 0) && !actions.pending

  const c: Controller = {
    isManage,
    agent,
    name,
    setName,
    providers,
    provId: sel.provId,
    modelId: sel.modelId,
    setSel: sel.setSel,
    realm,
    canSubmit,
    authEnabled: authEnabled ?? false,
    ...actions,
  }

  return (
    <div className="fixed inset-0 z-40 flex flex-col">
      {/* Click-to-dismiss backdrop as a focusable sibling button behind the
          sheet, so the sheet need not stopPropagation. */}
      <button
        type="button"
        aria-label="Close"
        onClick={onClose}
        className="backdrop-fade absolute inset-0 z-[-1] cursor-default bg-black/40 backdrop-blur-[3px]"
      />
      {/* full-screen sheet — the mobile replacement for the desktop floating
          card. Owns the whole viewport; body scrolls. */}
      <div className="modal-pop relative flex size-full flex-col overflow-hidden border-0 bg-popover">
        <AgentModalHeader
          isManage={c.isManage}
          agent={c.agent}
          avatarBust={c.avatarBust}
          onAvatarChange={c.onAvatarChange}
          onRandomizeAvatar={c.onRandomizeAvatar}
          onClose={onClose}
        />
        <AgentModalBody c={c} />
        {c.error && (
          <div
            role="alert"
            className="mx-4 mb-1 flex items-start gap-2 rounded-lg border border-(--danger)/30 bg-(--danger)/10 px-3 py-2 text-[12px] leading-snug text-(--danger)"
          >
            <X className="mt-px size-3.5 shrink-0" />
            <span>{c.error}</span>
          </div>
        )}
        <AgentModalFooter c={c} />
      </div>
    </div>
  )
}
