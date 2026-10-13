// ── Content-addressed body hydrate (SDK) ─────────────────────────────
//
// A `MessageCreated` oplog delta carries its body **inline** only when small;
// a large message spills to the content-addressed store and the delta carries
// just its `head` hash (I13). This module fetches those spilled bytes so the
// SSE→cache bridge can fold a big message into the live thread log without
// waiting for the slow tier-② disk poll (T357). Split from ./index for the
// 500-line file budget; re-exported there so `@/lib/api` stays the single
// import surface.

import type { BodyPayload } from "./generated/types.gen"
import { getApiFleetByIdBodyByHash } from "./generated"
import { sdk } from "./client"

export type { BodyPayload } from "./generated/types.gen"

/**
 * Hydrate a spilled (large) message body by its content hash and return it as a
 * UTF-8 string — the same JSON payload a small message rides inline.
 *
 * The body is immutable + stored before the referencing delta is emitted (the
 * I13 body-before-reference barrier), so this hydrate is race-free.
 */
export function fetchMessageBody(fleetId: string, hash: string): Promise<string> {
  return sdk<BodyPayload>(getApiFleetByIdBodyByHash({ path: { id: fleetId, hash } })).then((p) =>
    new TextDecoder().decode(new Uint8Array(p.bytes)),
  )
}
