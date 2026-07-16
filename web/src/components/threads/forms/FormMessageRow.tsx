// ── FormMessageRow — the un-memoized row for a form-bearing message ──────
//
// Split out of ThreadConversation (500-line budget). A message carrying a
// ` ```form ` or ` ```form-answer ` block renders through here rather than the
// memoized plain MessageRow, so it re-renders when a matching `form-answer`
// lands and flips the widget to its locked state (docs/forms.md §4/§5).

import { Message } from "@/components/conversation/Message"
import type { ThreadMsg } from "@/lib/types"
import type { UploadedFile } from "../fileUpload/helpers"
import { toChatMessage } from "@/lib/support/threadMessages"
import { FormWidget } from "./FormWidget"
import {
  parseFormAnswerBlock,
  parseFormBlocks,
  stripFormBlocks,
  type FormAnswer,
  type FormAnswerEntry,
} from "./helpers"

/**
 * One message row that carries form content — NOT memoized, so it re-renders
 * when a matching `form-answer` lands and flips the widget to its locked state.
 * Form-bearing messages are rare (a handful per thread), so re-rendering them on
 * every delta is cheap — the bulk plain-chat rows keep their memo boundary
 * intact (the T510 perf fix).
 *
 * A `form-answer` message renders ONLY a compact "answered" summary line (never
 * the raw YAML, never a widget — docs/forms.md §5). A `form` message renders its
 * prose (form blocks stripped out) followed by one {@link FormWidget} per block,
 * each resolving its locked state from `answersByForm` by `form-id`.
 */
export function FormMessageRow({
  msg,
  agentId,
  threadId,
  answersByForm,
  onFormSubmit,
  onOpenFile,
  onShowInFinder,
  onDelete,
}: {
  msg: ThreadMsg
  agentId: string
  threadId: string
  answersByForm: Map<string, FormAnswer>
  onFormSubmit: (formId: string, entries: FormAnswerEntry[]) => void
  onOpenFile: (file: UploadedFile) => void
  onShowInFinder: ((path: string) => void) | undefined
  onDelete: (msg: ThreadMsg) => void
}) {
  const text = msg.text ?? ""
  const answerBlock = parseFormAnswerBlock(text)

  // A form-answer message: render only the summary line, never the YAML.
  if (answerBlock) {
    return (
      <div className="rise flex flex-col items-end py-1.5">
        <span className="inline-flex items-center gap-1.5 rounded-full border border-(--signal)/40 bg-(--signal)/10 px-3 py-1 text-[11.5px] text-(--signal)">
          ✓ You answered form <span className="font-mono">{answerBlock.formId}</span>
        </span>
      </div>
    )
  }

  // A form message: prose (form blocks stripped) + one widget per block.
  const forms = parseFormBlocks(text)
  const stripped = stripFormBlocks(text)
  const chatMsg = { ...toChatMessage(msg), text: stripped }
  return (
    <div className="[contain-intrinsic-size:auto_5rem]">
      {stripped.length > 0 && (
        <Message
          msg={chatMsg}
          agentId={agentId}
          onOpenFile={onOpenFile}
          onShowInFinder={onShowInFinder}
          onDelete={() => onDelete(msg)}
        />
      )}
      <div className="pl-7">
        {forms.map((spec) => (
          <FormWidget
            key={spec.formId}
            spec={spec}
            agentId={agentId}
            answer={answersByForm.get(spec.formId) ?? null}
            draftKey={`cp-form-${agentId}-${threadId}-${spec.formId}`}
            onSubmit={onFormSubmit}
          />
        ))}
      </div>
    </div>
  )
}
