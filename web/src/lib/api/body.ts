// ── Content-addressed body hydrate ────────────────────────────────────
//
// A `MessageCreated` oplog delta carries its body **inline** only when small;
// a large message spills to the content-addressed store and the delta carries
// just its `head` hash (I13). This module fetches those spilled bytes so the
// SSE→cache bridge can fold a big message into the live thread log without
// waiting for the slow tier-② disk poll (T357). Split from ./index for the
// 500-line file budget; re-exported there so `@/lib/api` stays the single
// import surface.

import { request } from "./client"

/** Raw body payload from `GET /api/agent/{id}/body/{hash}` — the bytes of a
 *  content-addressed message body, serialized as a JSON number array. */
interface BodyPayload {
  bytes: number[]
}

/**
 * Hydrate a spilled (large) message body by its content hash and return it as a
 * UTF-8 string — the same JSON payload a small message rides inline.
 *
 * The body is immutable + stored before the referencing delta is emitted (the
 * I13 body-before-reference barrier), so this hydrate is race-free.
 */
export function fetchMessageBody(agentId: string, hash: string): Promise<string> {
  return request<BodyPayload>(`/api/agent/${agentId}/body/${hash}`).then((p) =>
    new TextDecoder().decode(new Uint8Array(p.bytes)),
  )
}
