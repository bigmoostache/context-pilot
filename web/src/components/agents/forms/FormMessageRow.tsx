// ── FormMessageRow — the un-memoized row for a form-bearing message ──────
//
// Split out of AgentConversation (500-line budget). A message carrying a
// ` ```form ` or ` ```form-answer ` block renders through here rather than the
// memoized plain MessageRow, so it re-renders when a matching `form-answer`
// lands and flips the widget to its locked state (docs/forms.md §4/§5).

import { Message } from "@/components/conversation/Message"
import type { AgentMsg } from "@/lib/types"
import type { UploadedFile } from "../fileUpload/helpers"
import { toChatMessage } from "@/lib/support/agentMessages"
import { FormWidget } from "./FormWidget"
import { parseFormAnswerBlock, type FormAnswer, type FormAnswerEntry } from "./helpers"

/**
 * One message row that carries form content — NOT memoized, so it re-renders
 * when a matching `form-answer` lands and flips the widget to its locked state.
 * Form-bearing messages are rare (a handful per agent), so re-rendering them on
 * every delta is cheap — the bulk plain-chat rows keep their memo boundary
 * intact (the T510 perf fix).
 *
 * A `form-answer` message renders ONLY a compact "answered" summary line (never
 * the raw YAML, never a widget — docs/forms.md §5). A `form` message renders its
 * prose with one {@link FormWidget} inline at each block's position,
 * each resolving its locked state from `answersByForm` by `form-id`.
 */
export function FormMessageRow({
  msg,
  fleetId,
  agentId,
  answersByForm,
  onFormSubmit,
  onOpenFile,
  onShowInFinder,
  onDelete,
  onBranch,
  hideActions,
}: {
  msg: AgentMsg
  fleetId: string
  agentId: string
  answersByForm: Map<string, FormAnswer>
  onFormSubmit: (formId: string, entries: FormAnswerEntry[]) => void
  onOpenFile: (file: UploadedFile) => void
  onShowInFinder: ((path: string) => void) | undefined
  onDelete: (msg: AgentMsg) => void
  onBranch: ((msg: AgentMsg) => void) | undefined
  /** drop the timestamp + action row (narrow chat beside a file preview) */
  hideActions: boolean
}) {
  const text = msg.text ?? ""
  const answerBlock = parseFormAnswerBlock(text)

  // A form-answer message: render only the summary line, never the YAML.
  if (answerBlock) {
    return (
      <div className="flex flex-col items-end py-1.5">
        <span className="inline-flex items-center gap-1.5 rounded-full border border-(--signal)/40 bg-(--signal)/10 px-3 py-1 text-[11.5px] text-(--signal)">
          ✓ You answered form <span className="font-mono">{answerBlock.formId}</span>
        </span>
      </div>
    )
  }

  // A form message: each widget renders inline where its block sat in the text.
  return (
    <Message
      msg={toChatMessage(msg)}
      fleetId={fleetId}
      onOpenFile={onOpenFile}
      onShowInFinder={onShowInFinder}
      onDelete={() => onDelete(msg)}
      onBranch={onBranch ? () => onBranch(msg) : undefined}
      hideActions={hideActions}
      renderForm={(spec) => (
        <FormWidget
          spec={spec}
          fleetId={fleetId}
          answer={answersByForm.get(spec.formId) ?? null}
          draftKey={`cp-form-${fleetId}-${agentId}-${spec.formId}`}
          onSubmit={onFormSubmit}
        />
      )}
    />
  )
}
