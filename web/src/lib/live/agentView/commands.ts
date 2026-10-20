// ── Agent command payload helpers (pure) ────────────────────────────
//
// The create/branch payload shape and the pure builders shared by the create
// and branch flows. Split out of ./index for the 500-line file budget.

import { buildUploadMessage, type UploadedFile } from "@/lib/live/agentUpload"

/** Rich agent-creation payload collected by the New Agent dialog (T674):
 *  a title plus an optional first message (auto-sent), file attachments, and a
 *  "create paused" flag that queues the seeded message without waking the agent. */
export interface CreateAgentOpts {
  title: string
  /** first user message, auto-sent to the new agent once its id is known (empty = none) */
  firstMessage: string
  /** files ALREADY uploaded to `.uploads/` (the dialog uploads on attach so the
   *  draft — paths included — survives a close/reopen via localStorage); folded
   *  into the first message as `file-upload` blocks at send time */
  files: UploadedFile[]
  /** create the agent already paused (seeded message queued, no MY_TURN nudge) */
  paused: boolean
}

/** Per-fleet localStorage key of the New Agent dialog's in-progress draft. */
export function newAgentDraftKey(fleetId: string): string {
  return `cp-newagent-${fleetId}`
}

/** Put a failed create's inputs back as the dialog draft, so reopening it shows
 *  them pre-filled (the draft shape is exactly {@link CreateAgentOpts}). */
export function restoreNewAgentDraft(fleetId: string, opts: CreateAgentOpts): void {
  localStorage.setItem(newAgentDraftKey(fleetId), JSON.stringify(opts))
}

/**
 * Build a combined message body from user text and pending file attachments,
 * reusing the exact same ` ```file-upload ` block composer the agent composer
 * uses ({@link buildUploadMessage}). Either part can be absent — a send with
 * only files produces just the file blocks; one with only text produces just
 * text.
 *
 * `filesFirst` controls ordering. The agent composer sends text first then the
 * file blocks (default, `false`). The new-agent create flow prepends the file
 * blocks so the attachments lead the very first message (T687).
 */
export function buildCombinedContent(
  text: string,
  files: UploadedFile[],
  filesFirst = false,
): string {
  const textPart = text.trim()
  const filePart = files.length > 0 ? buildUploadMessage(files) : ""
  const parts = filesFirst ? [filePart, textPart] : [textPart, filePart]
  return parts.filter(Boolean).join("\n\n")
}

/**
 * Turn a rejected `sendCommand` into a human sentence for the notice toast.
 *
 * Every failure is surfaced visibly so a command is never silently dropped.
 */
export function describeCommandError(verb: string, err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err)
  return `Could not ${verb}: ${msg}`
}
