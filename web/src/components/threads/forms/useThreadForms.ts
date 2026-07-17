// ── useThreadForms — the thread's form derivations (docs/forms.md §5) ────
//
// Extracted from ThreadConversation so its component body stays within the
// 150-line budget. Owns the two form concerns a conversation needs: the
// answered/locked lookup (a form is locked once a matching ` ```form-answer `
// with its `form-id` appears in the thread — the single source of truth, no
// backend form state), and the submit handler that composes + sends that
// ` ```form-answer ` message via the EXISTING send path.

import { useCallback, useEffect, useMemo } from "react"
import { sendCommand } from "@/lib/api"
import { measure } from "@/lib/support/telemetry"
import type { ThreadMsg } from "@/lib/types"
import {
  buildFormAnswerContent,
  parseFormAnswerBlock,
  type FormAnswer,
  type FormAnswerEntry,
} from "./helpers"

/** What {@link useThreadForms} hands back to the conversation: the by-form-id
 *  answer lookup driving each widget's locked state, and the submit handler. */
export interface ThreadForms {
  /** answered forms keyed by `form-id` — presence locks the matching widget. */
  answersByForm: Map<string, FormAnswer>
  /** compose + send the ` ```form-answer ` for a submitted form. */
  onFormSubmit: (formId: string, entries: FormAnswerEntry[]) => void
}

/**
 * Derive every form's answered state from the thread log and expose the submit
 * handler. `answersByForm` is rebuilt only when the log changes; `onFormSubmit`
 * is stable across renders (deps: agentId + threadId) so it never defeats a
 * child memo boundary.
 */
export function useThreadForms(log: ThreadMsg[], agentId: string, threadId: string): ThreadForms {
  const answersByForm = useMemo(() => {
    const m = new Map<string, FormAnswer>()
    for (const msg of log) {
      const a = parseFormAnswerBlock(msg.text ?? "")
      if (a) m.set(a.formId, msg.ts === undefined ? a : { ...a, submittedAt: msg.ts })
    }
    return m
  }, [log])

  const onFormSubmit = useCallback(
    (formId: string, entries: FormAnswerEntry[]) => {
      const content = buildFormAnswerContent(formId, entries)
      void sendCommand(agentId, { kind: "send_message", thread_id: threadId, content })
    },
    [agentId, threadId],
  )

  return { answersByForm, onFormSubmit }
}

/**
 * Pin the conversation to its bottom anchor on thread-open (`threadId` change)
 * or when a new NON-auto message lands (`nonAutoCount`). Extracted from
 * ThreadConversation to keep its body within budget.
 *
 * Converges rather than firing once: with `content-visibility:auto` every
 * off-screen row reports only its `contain-intrinsic-size` placeholder height
 * until scrolled into view, so on open the container's scrollHeight is an
 * ESTIMATE and a single `scrollIntoView` lands short of the true bottom (T512).
 * Re-scrolling across a bounded 6-frame loop reveals each chunk's real heights,
 * correcting the estimate until the position settles on the actual bottom.
 * `scrollIntoView` forces sync layout, so it's wrapped in `measure()` for freeze
 * attribution; the ~100ms bounded loop on open is imperceptible and off any hot
 * path.
 */
export function useScrollPin(
  bottomRef: React.RefObject<HTMLDivElement | null>,
  threadId: string,
  nonAutoCount: number,
): void {
  useEffect(() => {
    const el = bottomRef.current
    if (!el) return
    let raf = 0
    let tries = 0
    const settle = () => {
      measure("threads:scrollIntoView", () => el.scrollIntoView({ block: "end" }))
      tries += 1
      if (tries < 6) raf = requestAnimationFrame(settle)
    }
    raf = requestAnimationFrame(settle)
    return () => cancelAnimationFrame(raf)
  }, [bottomRef, threadId, nonAutoCount])
}
