// ── Branch out a thread (shared, non-forked) ─────────────────────────
//
// The Branch dialog state + the `branch_thread` command dispatch, consumed by
// both the desktop and mobile thread views through {@link useThreadActions}.
// Split out of ./index for the 500-line file budget.

import { useState, useCallback } from "react"
import { sendCommand } from "@/lib/live"
import type { ThreadMsg } from "@/lib/types"
import { buildCombinedContent, describeCommandError, type CreateThreadOpts } from "./commands"

/** The slice of the thread selection a branch drives: auto-selecting the new
 *  thread once its server-assigned id lands (same seam as a plain create). */
interface AutoSelect {
  armAutoSelect: () => void
  disarmAutoSelect: () => void
}

/** The message a Branch dialog was opened on: the parent thread and the branch
 *  point (the new thread inherits the parent's messages up to it, inclusive). */
export interface BranchTarget {
  threadId: string
  /** epoch-ms timestamp of the branch-point message (the backend's message key) */
  messageTs: number
  /** short single-line excerpt of the branch-point message, shown in the dialog */
  excerpt: string
}

/** The Branch-dialog state + handlers returned by {@link useThreadBranch}. */
export interface BranchActions {
  /** the branch point the dialog is open on (null = closed) */
  branchTarget: BranchTarget | null
  /** open the Branch dialog on `msg` of thread `threadId` (stable identity) */
  openBranch: (threadId: string, msg: ThreadMsg) => void
  closeBranch: () => void
  /** submit the Branch dialog: create the branch with its name + first message */
  handleBranch: (opts: CreateThreadOpts) => void
}

/** Epoch-ms of a log message — REST carries a number, an SSE-appended message
 *  an ISO string (see reducers `buildLogRow`). NaN when unparseable. */
function messageTsOf(msg: ThreadMsg): number {
  return typeof msg.ts === "number" ? msg.ts : new Date(msg.ts ?? "").getTime()
}

/**
 * Branch-out state + handlers: which message the Branch dialog is open on, and
 * the submit that sends ONE atomic `branch_thread` command. The agent copies the
 * parent's history from its own state (the payload only names the branch point
 * by its timestamp), then applies the optional pause + first message in the
 * same create -> pause -> send order as `create_thread` (T687). The new thread
 * is auto-selected once it appears, exactly like a plain create.
 */
export function useThreadBranch(
  activeAgentId: string,
  sel: AutoSelect,
  flash: (msg: string) => void,
): BranchActions {
  const [branchTarget, setBranchTarget] = useState<BranchTarget | null>(null)

  // Stable identity (no deps): it is handed down to memoized message rows whose
  // comparator ignores callback props, so it must never go stale.
  const openBranch = useCallback((threadId: string, msg: ThreadMsg) => {
    const messageTs = messageTsOf(msg)
    if (!Number.isFinite(messageTs)) return
    const flat = (msg.text ?? "").replaceAll(/\s+/g, " ").trim()
    const excerpt = flat.length > 140 ? `${flat.slice(0, 139)}…` : flat
    setBranchTarget({ threadId, messageTs, excerpt })
  }, [])
  const closeBranch = useCallback(() => setBranchTarget(null), [])

  const handleBranch = useCallback(
    (opts: CreateThreadOpts) => {
      if (!branchTarget) return
      setBranchTarget(null)
      const content = buildCombinedContent(opts.firstMessage, opts.files, true)
      sel.armAutoSelect()
      sendCommand(activeAgentId, {
        kind: "branch_thread",
        source_thread_id: branchTarget.threadId,
        message_ts: branchTarget.messageTs,
        name: opts.title.trim() || "Untitled branch",
        ...(content && { initial_message: content }),
        paused: opts.paused,
      }).catch((e: unknown) => {
        sel.disarmAutoSelect()
        flash(describeCommandError("branch the thread", e))
      })
    },
    [activeAgentId, branchTarget, flash, sel],
  )

  return { branchTarget, openBranch, closeBranch, handleBranch }
}
