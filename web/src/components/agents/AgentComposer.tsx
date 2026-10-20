import { useMemo } from "react"
import type { AgentStatus } from "@/lib/types"
import { ComposerBubbles } from "./fileUpload"
import type { UploadedFile, CommandSuggestion } from "./fileUpload/helpers"
import { ComposerInputRow, ComposerBanner } from "./fileUpload/composerInput"
import { resolveComposerBanner } from "@/lib/support/agentMessages"
import { useComposer } from "@/lib/live/agentView/composer"

// CommandSuggestion now lives beside the file-chip abstraction in ./fileUpload
// (both composer pill families share ONE module + ONE rendered row). Re-exported
// here for the existing `import { type CommandSuggestion } from "./AgentComposer"`
// consumers (AgentConversation).
export type { CommandSuggestion } from "./fileUpload/helpers"

/**
 * Agent composer — always active, regardless of turn status. Turn-status
 * banner reflects agent activity on this agent (T39/T371). Structure (P8):
 * draft logic in {@link useComposer}, banner in {@link ComposerBanner},
 * input row in {@link ComposerInputRow}. T556: prefix-filtered `/command`
 * bubbles with Tab autocomplete and Space expansion.
 */
export function AgentComposer({
  status,
  focused = false,
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
  /** true when this is the single agent the agent is currently focused on */
  focused?: boolean | undefined
  /** true when this agent has been paused by the user (T371) */
  paused?: boolean | undefined
  onSend?: ((text: string) => Promise<boolean>) | undefined
  /** Upload picked files into this agent (paperclip). May be async (T471). */
  onAttach?: ((files: File[]) => void | Promise<void>) | undefined
  /** Files uploaded but not yet sent — rendered as removable chips (T331). */
  pendingFiles?: UploadedFile[] | undefined
  /** Remove a staged file by its index in pendingFiles. */
  onRemoveFile?: ((index: number) => void) | undefined
  /** `/command` suggestions (T348). Non-empty renders clickable bubbles; click prefills. */
  suggestions?: CommandSuggestion[] | undefined
  /** True when agent has no messages yet — scopes auto-show bubbles to first message (T350). */
  firstMessage?: boolean
  /** Opens the "create command" dialog (T350). Omit to hide the pill. */
  onCreateCommand?: (() => void) | undefined
  /** Opens the command editor prefilled (T654). Omit to hide the per-pill edit button. */
  onEditCommand?: ((s: CommandSuggestion) => void) | undefined
  /** localStorage key for persisting the unsent draft + caret per agent (T304).
   *  Stored as `{text,selStart,selEnd}` JSON; legacy bare-string also read. */
  draftKey?: string | undefined
  /** localStorage key for the durably-attached `/command` prefix per agent
   *  (T654). Omit to disable command attachment persistence. */
  commandKey?: string | undefined
}) {
  const composer = useComposer({ draftKey, commandKey, onSend, device: "desktop" })

  const userTurn = status === "THEIR_TURN"
  const streaming = status === "ACTIVE"
  // The agent owes a response on this agent (its turn, or actively streaming).
  const fleetBusy = !userTurn
  const banner = resolveComposerBanner(paused, fleetBusy, streaming, focused)

  const sendable = composer.canSend(pendingFiles.length)

  // Whether the /command bubbles should be offered right now: mid-draft on a
  // slash-prefixed line (any agent), OR on a brand-new agent with an empty
  // composer (the first-message palette). File chips show independently of this.
  const commandsActive = composer.slashPrefix !== null || (firstMessage && !composer.text.trim())

  // Filter suggestions by the typed prefix (T556). `/bo` shows only commands
  // starting with `/bo`. A bare `/` (prefix "") shows all commands.
  const filteredSuggestions = useMemo(() => {
    const prefix = composer.slashPrefix
    if (prefix === null || prefix === "") return suggestions
    const lower = prefix.toLowerCase()
    return suggestions.filter((s) => s.command.slice(1).toLowerCase().startsWith(lower))
  }, [suggestions, composer.slashPrefix])

  // Wrap the composer's keydown handler to intercept Tab (autocomplete first
  // filtered suggestion) and Space (expand an exact command match) before the
  // base handler runs (T556).
  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Tab autocomplete: if slash bubbles are showing and there are matches,
    // attach the first one instead of indenting (T654).
    if (e.key === "Tab" && !e.shiftKey && composer.slashPrefix !== null) {
      const first = filteredSuggestions[0]
      if (first) {
        e.preventDefault()
        composer.attachCmd(first)
        return
      }
    }

    // Space expansion: if the current line is exactly a known command (e.g.
    // `/boss-hunt`), pressing Space attaches it (T654).
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
    // No padding — the composer runs flush to its column. `shrink-0` stays: the
    // conversation above is the flex child that scrolls, so without it the
    // composer would compress as the log grows instead of holding its height.
    <div className="shrink-0">
      {/* Unified bubble row (T350) — file-upload chips + /command suggestions +
          the create-command pill, all in ONE transparent, normal-flow container
          between the conversation and the textarea. */}
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
    </div>
  )
}
