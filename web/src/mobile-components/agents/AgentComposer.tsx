import { useMemo } from "react"
import type { AgentStatus } from "@/lib/types"
import { ComposerBubbles } from "@/mobile-components/agents/fileUpload"
import type { UploadedFile, CommandSuggestion } from "@/mobile-components/agents/fileUpload/helpers"
import {
  ComposerInputRow,
  ComposerBanner,
} from "@/mobile-components/agents/fileUpload/composerInput"
import { FrostedBottomBar } from "@/mobile-components/shell/chrome/FrostedBottomBar"
import { resolveComposerBanner } from "@/lib/support/agentMessages"
import { useComposer } from "@/lib/live/agentView/composer"

// CommandSuggestion lives beside the file-chip abstraction in ./fileUpload (both
// composer pill families share ONE module + ONE rendered row). Re-exported for
// the mobile AgentConversation consumer, matching the desktop twin's surface.
export type { CommandSuggestion } from "@/mobile-components/agents/fileUpload/helpers"

/**
 * Mobile agent composer — divergent twin of `components/agents/AgentComposer`.
 * Always active; the turn-status banner reflects agent activity (T39/T371).
 * Behaviour (draft persistence, list-aware Enter/Tab, `/command` bubbles with Tab
 * autocomplete + Space expansion) is byte-for-byte the desktop logic — only the
 * input row's touch sizing forks (16px font vs iOS focus-zoom, 36px targets) and
 * the outer padding carries `safe-area-inset-bottom` to clear the home indicator.
 */
export function AgentComposer({
  status,
  paused = false,
  onSend,
  onAttach,
  pendingFiles = [],
  onRemoveFile,
  draftKey,
  suggestions = [],
  firstMessage = false,
  onCreateCommand,
  onEditCommand,
  commandKey,
}: {
  status: AgentStatus
  /** true when this agent has been paused by the user (T371) */
  paused?: boolean | undefined
  onSend?: ((text: string) => Promise<boolean>) | undefined
  /** Upload picked files into this agent (paperclip). May be async (T471). */
  onAttach?: ((files: File[]) => void | Promise<void>) | undefined
  /** Files uploaded but not yet sent — rendered as removable chips (T331). */
  pendingFiles?: UploadedFile[] | undefined
  /** Remove a staged file by its index in pendingFiles. */
  onRemoveFile?: ((index: number) => void) | undefined
  /** `/command` suggestions (T348). Non-empty renders clickable bubbles. */
  suggestions?: CommandSuggestion[] | undefined
  /** True when agent has no messages yet — scopes auto-show bubbles (T350). */
  firstMessage?: boolean
  /** Opens the "create command" dialog (T350). Omit to hide the pill. */
  onCreateCommand?: (() => void) | undefined
  /** Opens the command editor prefilled (T654). Omit to hide the per-pill edit button. */
  onEditCommand?: ((s: CommandSuggestion) => void) | undefined
  /** localStorage key for persisting the unsent draft + caret per agent (T304). */
  draftKey?: string | undefined
  /** localStorage key for the durably-attached `/command` prefix per agent
   *  (T654). Omit to disable command attachment persistence. */
  commandKey?: string | undefined
}) {
  const composer = useComposer({ draftKey, commandKey, onSend, device: "mobile" })

  const userTurn = status === "THEIR_TURN"
  const streaming = status === "ACTIVE"
  const fleetBusy = !userTurn
  const banner = resolveComposerBanner(paused, fleetBusy, streaming)

  const sendable = composer.canSend(pendingFiles.length)

  // /command bubbles on a slash line, or on a brand-new empty agent.
  const commandsActive = composer.slashPrefix !== null || (firstMessage && !composer.text.trim())

  const filteredSuggestions = useMemo(() => {
    const prefix = composer.slashPrefix
    if (prefix === null || prefix === "") return suggestions
    const lower = prefix.toLowerCase()
    return suggestions.filter((s) => s.command.slice(1).toLowerCase().startsWith(lower))
  }, [suggestions, composer.slashPrefix])

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Tab" && !e.shiftKey && composer.slashPrefix !== null) {
      const first = filteredSuggestions[0]
      if (first) {
        e.preventDefault()
        composer.attachCmd(first)
        return
      }
    }

    if (e.key === " " && composer.slashPrefix !== null) {
      const match = suggestions.find(
        (s) => s.command.slice(1).toLowerCase() === composer.slashPrefix?.toLowerCase(),
      )
      if (match) {
        e.preventDefault()
        composer.attachCmd(match)
        return
      }
    }

    composer.handleKeyDown(e)
  }

  return (
    <FrostedBottomBar className="px-3 pt-3 pb-[max(1rem,env(safe-area-inset-bottom))]">
      {/* Unified bubble row (T350) — file chips + /command suggestions +
          create-command pill in ONE container. */}
      {(pendingFiles.length > 0 || commandsActive || composer.attachedCmd) && (
        <ComposerBubbles
          files={pendingFiles}
          onRemoveFile={onRemoveFile}
          suggestions={commandsActive ? filteredSuggestions : []}
          onPick={composer.attachCmd}
          attachedCommand={composer.attachedCmd}
          onDetachCommand={composer.detachCmd}
          onCreateCommand={commandsActive ? onCreateCommand : undefined}
          onEditCommand={onEditCommand}
        />
      )}
      {banner && <ComposerBanner banner={banner} />}
      <ComposerInputRow
        textareaRef={composer.textareaRef}
        text={composer.text}
        sendable={sendable}
        onChange={composer.onChange}
        onSelect={composer.onSelect}
        onKeyDown={handleKeyDown}
        onSubmit={composer.handleSubmit}
        onAttach={onAttach}
      />
    </FrostedBottomBar>
  )
}
