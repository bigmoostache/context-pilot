import { X } from "lucide-react"
import { usePickerProviders } from "@/lib/support/models"
import { useAuth } from "@/lib/providers/auth"
import { cn } from "@/lib/utils"
import { useState } from "react"
import { slugify, useSelectionState, type AgentModalMode } from "./controller"
import { useAgentModalActions } from "./actions"
import { AgentModalHeader, AgentModalBody, AgentModalFooter, type Controller } from "./parts"

export type { AgentModalMode } from "./controller"

/**
 * Agent create / manage dialog — the single source of truth for both flows.
 *
 * Shared by the fleet dashboard (the canonical agent-management surface) and,
 * in *manage* mode, by the TopBar's per-agent shortcut button (T26) so the user
 * can edit the focused agent in one click.
 *
 * Rendering note: the backdrop is `absolute inset-0`, so the host must provide a
 * viewport-sized positioning context. The fleet dashboard renders it inside a
 * `relative` full-height container; the TopBar renders it as a *sibling* of the
 * `.vibrancy` header (never a descendant) so it anchors to the viewport and
 * escapes the header's backdrop-filter containing block.
 *
 * Structure (P8): selection state lives in {@link useSelectionState}, all
 * mutations in {@link useAgentModalActions}, and the render in the
 * {@link AgentModalHeader}/{@link AgentModalBody}/{@link AgentModalFooter}
 * subcomponents — so every function stays within the P8 budgets and this file
 * stays under the 500-line cap.
 */
export function AgentModal({
  modal,
  onClose,
  onFlash,
}: {
  modal: AgentModalMode
  onClose: () => void
  /** Optional toast sink — the fleet dashboard supplies one; the TopBar omits it. */
  onFlash?: (m: string) => void
}) {
  const isManage = modal.mode === "manage"
  const agent = isManage ? modal.agent : undefined
  const [name, setName] = useState(agent?.name ?? "")

  // The picker list is computed server-side: only providers with a configured
  // key, with the org model allowlist already applied (empty ⇒ all).
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

  // Realm folder: derived live from the name in create mode; the agent's fixed,
  // read-only realm in manage mode.
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
    <div className="absolute inset-0 z-40 flex items-center justify-center">
      {/* Click-to-dismiss backdrop as a keyboard-focusable sibling button
          (behind the card), so the card need not be wrapped in an interactive
          element nor carry a stopPropagation onClick. */}
      <button
        type="button"
        aria-label="Close"
        onClick={onClose}
        className="backdrop-fade absolute inset-0 z-[-1] cursor-default bg-black/40 backdrop-blur-[3px]"
      />
      <div
        className={cn(
          "modal-pop pop-shadow relative flex max-h-[calc(100vh-3rem)] flex-col overflow-hidden rounded-2xl border border-border bg-popover",
          c.isManage ? "w-[1120px] max-w-[calc(100vw-3rem)]" : "w-[460px]",
        )}
      >
        <AgentModalHeader
          isManage={c.isManage}
          agent={c.agent}
          avatarBust={c.avatarBust}
          onAvatarChange={c.onAvatarChange}
          onClose={onClose}
        />
        <AgentModalBody c={c} />
        {/* create/save error — surfaced inline so a failure isn't silent */}
        {c.error && (
          <div
            role="alert"
            className="mx-6 mb-1 flex items-start gap-2 rounded-lg border border-(--danger)/30 bg-(--danger)/10 px-3 py-2 text-[11.5px] leading-snug text-(--danger)"
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
