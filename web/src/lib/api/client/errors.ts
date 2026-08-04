// ── What a rejected API call actually threw ──────────────────────────
//
// One helper, because the generated client does NOT throw an `Error`.

/**
 * The human-readable message behind a rejected SDK call, or `fallback`.
 *
 * The generated client throws the **parsed JSON body** on a non-2xx
 * (`generated/client/client.gen.ts` — `throw jsonError ?? textError`) and the
 * backend's error envelope is `{"error": "…"}` (`rest/mod.rs::error`). So the
 * widespread idiom
 *
 * ```ts
 * error instanceof Error ? error.message : "Save failed"
 * ```
 *
 * takes its **else** branch for every server-sent 4xx/5xx: the thrown value is
 * a plain object, never an `Error`. Everything the backend took the trouble to
 * say — "pin must be 4–8 digits", "channel is not valid for the selected band",
 * the 502 rollback line — is replaced by the fallback, and the operator is told
 * only that something failed (review finding R5).
 *
 * This handles the three shapes that actually reach a `catch` here:
 *
 *  - the `{ error }` envelope — every REST route's failure reply;
 *  - a real `Error` — a transport failure (the box is unreachable, the request
 *    was aborted) and this layer's own hand-written `fetch` wrappers;
 *  - anything else — a bare string body, `null`, a number: fall back.
 *
 * `fallback` is required rather than defaulted so each call site names the
 * operation that failed; a generic "Request failed" is rarely what a pane wants.
 */
export function apiErrorMessage(failure: unknown, fallback: string): string {
  if (typeof failure === "string" && failure.trim() !== "") return failure
  if (failure instanceof Error && failure.message !== "") return failure.message
  if (typeof failure !== "object" || failure === null) return fallback
  const { error } = failure as { error?: unknown }
  if (typeof error === "string" && error.trim() !== "") return error
  // Defensive: a nested `{ error: { message } }` envelope, should one ever
  // reach here. Cheap to accept, and the alternative is the useless fallback.
  if (typeof error === "object" && error !== null) {
    const { message } = error as { message?: unknown }
    if (typeof message === "string" && message.trim() !== "") return message
  }
  return fallback
}
