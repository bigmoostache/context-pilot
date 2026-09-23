// ── Thread command payload helpers (pure) ────────────────────────────
//
// The create/branch payload shape and the pure builders shared by the create
// and branch flows. Split out of ./index for the 500-line file budget.

import { buildUploadMessage, type UploadedFile } from "@/lib/live/threadUpload"

/** Rich thread-creation payload collected by the New Thread dialog (T674):
 *  a title plus an optional first message (auto-sent), file attachments, and a
 *  "create paused" flag that queues the seeded message without waking the agent. */
export interface CreateThreadOpts {
  title: string
  /** first user message, auto-sent to the new thread once its id is known (empty = none) */
  firstMessage: string
  /** files ALREADY uploaded to `.uploads/` (the dialog uploads on attach so the
   *  draft — paths included — survives a close/reopen via localStorage); folded
   *  into the first message as `file-upload` blocks at send time */
  files: UploadedFile[]
  /** create the thread already paused (seeded message queued, no MY_TURN nudge) */
  paused: boolean
}

/**
 * Build a combined message body from user text and pending file attachments,
 * reusing the exact same ` ```file-upload ` block composer the thread composer
 * uses ({@link buildUploadMessage}). Either part can be absent — a send with
 * only files produces just the file blocks; one with only text produces just
 * text.
 *
 * `filesFirst` controls ordering. The thread composer sends text first then the
 * file blocks (default, `false`). The new-thread create flow prepends the file
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
