// Unit tests for the pure SSE delta folds (./reducers, ./taskReducer,
// ./notesReducer). Each fold has three verdicts the sync bridge relies on:
// a NEW value when the delta mutates the cache, the SAME reference when it is
// irrelevant (so `setQueryData` is skipped), and `null` when it can't be folded
// (unknown agent → the bridge invalidates and re-fetches).

import { describe, expect, it } from "vitest"
import type { AgentTask } from "../api/generated/types.gen"
import type { AgentDetail, Fleet } from "../types"
import {
  applyAgentDelta,
  applyFleetDelta,
  mergeAgentLogs,
  type OpEntry,
  type OpEntryKind,
} from "./reducers"

function agent(id: string, extra: Partial<AgentDetail> = {}): AgentDetail {
  return { id, fleetId: "f1", name: id, status: "THEIR_TURN", log: [], ...extra }
}

function delta(kind: OpEntryKind, rev = 1): OpEntry {
  return { kind, rev }
}

const FLEET: Fleet = {
  accent: "interactive",
  agents: 1,
  costUsd: 0,
  folder: "/tmp",
  id: "f1",
  lastActivity: "now",
  name: "fleet",
  status: "idle",
}

describe("applyAgentDelta", () => {
  it("returns null before the agents cache is loaded", () => {
    expect(applyAgentDelta(undefined, delta({ kind: "agent_paused", agent_id: "a" }))).toBeNull()
  })

  it("returns null for a flag on an unknown agent", () => {
    expect(
      applyAgentDelta([agent("a")], delta({ kind: "agent_archived", agent_id: "zz" })),
    ).toBeNull()
  })

  it("sets the flag on the target only and keeps other refs", () => {
    const a = agent("a")
    const b = agent("b")
    const next = applyAgentDelta([a, b], delta({ kind: "agent_paused", agent_id: "a" }))
    expect(next?.[0]?.paused).toBe(true)
    expect(next?.[1]).toBe(b)
  })

  it("returns the same reference for a fleet-only delta", () => {
    const prev = [agent("a")]
    expect(applyAgentDelta(prev, delta({ kind: "phase_transition", phase: "idle" }))).toBe(prev)
  })

  it("drops a deleted agent and ignores an absent one", () => {
    const prev = [agent("a"), agent("b")]
    expect(
      applyAgentDelta(prev, delta({ kind: "agent_deleted", agent_id: "a" }))?.map((t) => t.id),
    ).toEqual(["b"])
    expect(applyAgentDelta(prev, delta({ kind: "agent_deleted", agent_id: "zz" }))).toBe(prev)
  })

  it("removes a message by its epoch-ms timestamp", () => {
    const log: AgentDetail["log"] = [
      { id: "m1", author: "user", text: "hi", ts: 100 },
      { id: "m2", author: "assistant", text: "yo", ts: 200 },
    ]
    const prev = [agent("a", { log })]
    const next = applyAgentDelta(
      prev,
      delta({ kind: "message_deleted", agent_id: "a", message_ts: 100 }),
    )
    expect(next?.[0]?.log.map((m) => m.id)).toEqual(["m2"])
    expect(
      applyAgentDelta(prev, delta({ kind: "message_deleted", agent_id: "a", message_ts: 999 })),
    ).toBe(prev)
  })

  it("moves focus and returns the same reference when nothing changes", () => {
    const prev = [agent("a", { focused: true }), agent("b", { focused: false })]
    expect(applyAgentDelta(prev, delta({ kind: "agent_focus_changed", agent_id: "a" }))).toBe(prev)
    const next = applyAgentDelta(prev, delta({ kind: "agent_focus_changed", agent_id: "b" }))
    expect(next?.map((t) => t.focused)).toEqual([false, true])
  })

  it("replaces tasks and folds wire parent_id onto parentId", () => {
    const child: AgentTask & { parent_id: string } = {
      id: "t2",
      name: "child",
      status: "planned",
      parent_id: "t1",
    }
    const prev = [agent("a")]
    const next = applyAgentDelta(
      prev,
      delta({ kind: "task_list_changed", agent_id: "a", tasks: [child] }),
    )
    expect(next?.[0]?.tasks).toEqual([
      { id: "t2", name: "child", description: "", status: "planned", parentId: "t1" },
    ])
    expect(
      applyAgentDelta(prev, delta({ kind: "task_list_changed", agent_id: "zz", tasks: [] })),
    ).toBeNull()
  })

  it("replaces notes wholesale", () => {
    const notes = [{ id: "n1", title: "t", content: "c" }]
    const next = applyAgentDelta(
      [agent("a")],
      delta({ kind: "notes_changed", agent_id: "a", notes }),
    )
    expect(next?.[0]?.notes).toEqual(notes)
  })
})

describe("applyFleetDelta", () => {
  it("returns null before the fleet cache is loaded", () => {
    expect(applyFleetDelta(undefined, delta({ kind: "cost_aggregate", cost_usd: 1 }))).toBeNull()
  })

  it("marks the fleet working while streaming and idle afterwards", () => {
    const working = applyFleetDelta(FLEET, delta({ kind: "phase_transition", phase: "streaming" }))
    expect(working?.status).toBe("working")
    const idle =
      working && applyFleetDelta(working, delta({ kind: "phase_transition", phase: "idle" }))
    expect(idle?.status).toBe("idle")
  })

  it("ignores unknown phases and unchanged costs", () => {
    expect(applyFleetDelta(FLEET, delta({ kind: "phase_transition", phase: "bogus" }))).toBe(FLEET)
    expect(applyFleetDelta(FLEET, delta({ kind: "cost_aggregate", cost_usd: 0 }))).toBe(FLEET)
  })

  it("folds cost and token counters", () => {
    const next = applyFleetDelta(
      FLEET,
      delta({ kind: "cost_aggregate", cost_usd: 2, input_tokens: 10 }),
    )
    expect(next).toMatchObject({ costUsd: 2, inputTokens: 10 })
  })
})

describe("mergeAgentLogs", () => {
  it("returns the poll as-is on a cold load", () => {
    const next = [agent("a")]
    expect(mergeAgentLogs(undefined, next)).toBe(next)
  })

  it("keeps a delta-only message the poll has not flushed yet", () => {
    const local = { id: "m9", author: "user" as const, text: "new", ts: 5 }
    const merged = mergeAgentLogs([agent("a", { log: [local] })], [agent("a")])
    expect(merged[0]?.log).toEqual([local])
  })

  it("does not duplicate a message whose id drifted between planes", () => {
    const msg = { author: "user" as const, text: "same", ts: 5 }
    const merged = mergeAgentLogs(
      [agent("a", { log: [{ id: "msg_1", ...msg }] })],
      [agent("a", { log: [{ id: "msg_2", ...msg }] })],
    )
    expect(merged[0]?.log.map((m) => m.id)).toEqual(["msg_2"])
  })

  it("follows the poll's membership", () => {
    const merged = mergeAgentLogs([agent("a"), agent("gone")], [agent("a")])
    expect(merged.map((t) => t.id)).toEqual(["a"])
  })
})
