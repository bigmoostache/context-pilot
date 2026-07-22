// ── Live mutations — imperative agent + finder writes ────────────────
//
// Split out of live/index for the file-size limit; re-exported there so
// `@/lib/live` stays the single import surface. Each is a one-shot POST, not a
// delta-covered resource, so they ride `useMutation` rather than the SSE push
// plane and invalidate the affected query family on success.

import { useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query"
import { qk } from "../query/sync"
import * as api from "../api"
import type { Agent } from "../types"
import { useLive, type LiveQueryResult } from "./core"

/** Invalidate the active-fleet query — refetches the dashboard roster. Hoisted
 *  to module scope so the timeout-retry closures don't re-create it per call. */
function invalidateFleet(client: QueryClient) {
  void client.invalidateQueries({ queryKey: qk.fleet() })
}

/** Invalidate BOTH the active and retired fleet queries (an agent moving
 *  between the two grids changes both listings). */
function invalidateBothFleets(client: QueryClient) {
  void client.invalidateQueries({ queryKey: qk.fleet() })
  void client.invalidateQueries({ queryKey: qk.retiredFleet() })
}

// ── Finder upload (TanStack mutation) ─────────────────────────────────
//
// Uploading files is a set of one-shot POSTs (one per file — the backend takes
// raw bytes + a filename, sidestepping multipart). It is not a delta-covered
// resource, so it rides a `useMutation`. On success the current directory's
// listing query is invalidated so the new files appear immediately.

/** Max upload size per file (32 MiB) — matches the backend transport's
 *  `MAX_BODY`; a larger body would be silently truncated server-side, so we
 *  reject it client-side with a clear message instead. */
export const MAX_UPLOAD_BYTES = 32 * 1024 * 1024

/**
 * Mutation to upload one or more files into a realm directory. Files are sent
 * concurrently (one POST each); any over {@link MAX_UPLOAD_BYTES} are rejected
 * before sending.
 *
 * Uploads ride `Promise.allSettled`, not `Promise.all`, so one failure no longer
 * hides the files that DID land — the listing is invalidated in `onSettled`
 * (success OR partial failure) so every succeeded upload surfaces immediately
 * instead of waiting on the 15s backstop, and a partial failure names exactly
 * which files were rejected (M7).
 */
export function useUploadFiles(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: async ({ dir, files }: { dir: string; files: File[] }) => {
      const tooBig = files.filter((f) => f.size > MAX_UPLOAD_BYTES)
      if (tooBig.length > 0) {
        throw new Error(`${tooBig.map((f) => f.name).join(", ")} exceeds the 32 MB upload limit`)
      }
      const results = await Promise.allSettled(files.map((f) => api.uploadFile(agentId, dir, f)))
      const failed = results
        .map((r, i) => (r.status === "rejected" ? (files[i]?.name ?? null) : null))
        .filter((n): n is string => n !== null)
      if (failed.length > 0) {
        throw new Error(
          `Failed to upload ${failed.join(", ")} (${files.length - failed.length} of ${files.length} succeeded)`,
        )
      }
      return { count: files.length, dir }
    },
    // Invalidate on settle — succeeded uploads must appear even when a sibling
    // failed, without waiting on the backstop poll.
    onSettled: (_data, _err, { dir }) => {
      void client.invalidateQueries({ queryKey: qk.fs(agentId, dir) })
    },
  })
}

/**
 * Mutation to overwrite an existing realm file (the Finder's in-place editor —
 * e.g. saving the WYSIWYG markdown editor back to its `.md`). Not a delta-covered
 * resource → a `useMutation`. On success the file's `useFsPreview` cache is
 * invalidated so a re-open shows the saved content, and the containing
 * directory's `useFs` listing is invalidated so its size/mtime refresh.
 */
export function useWriteFile(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ path, content }: { path: string; content: string }) =>
      api.writeFile(agentId, path, content),
    onSuccess: (_res, { path }) => {
      void client.invalidateQueries({ queryKey: qk.fsPreview(agentId, path) })
      void client.invalidateQueries({ queryKey: ["fs", agentId] })
    },
  })
}

/**
 * Mutation to create a new folder inside a realm directory (the Finder's
 * "New Folder" action). Not a delta-covered resource → a `useMutation`. On
 * success the destination directory's `useFs` listing is invalidated so the
 * new folder appears at once.
 */
export function useCreateFolder(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ dir, name }: { dir: string; name: string }) =>
      api.createFolder(agentId, dir, name),
    onSuccess: (_res, { dir }) => {
      void client.invalidateQueries({ queryKey: qk.fs(agentId, dir) })
    },
  })
}

/**
 * Mutation to move one or more entries into a realm directory (the Finder's
 * internal drag-and-drop). Not a delta-covered resource → a `useMutation`. On
 * success the WHOLE `fs` query family for the agent is invalidated (both the
 * source and destination listings changed) so the move is reflected at once.
 */
export function useMoveItems(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ items, dest }: { items: string[]; dest: string }) =>
      api.moveItems(agentId, items, dest),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["fs", agentId] })
    },
  })
}

/**
 * Mutation to rename one entry in place (the Finder's inline rename). Not a
 * delta-covered resource → a `useMutation`. On success the WHOLE `fs` query
 * family for the agent is invalidated so the renamed entry surfaces under its
 * new name at once (the containing directory's listing changed).
 */
export function useRenameItem(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ path, name }: { path: string; name: string }) =>
      api.renameItem(agentId, path, name),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["fs", agentId] })
    },
  })
}

/**
 * Mutation to move one or more entries to the realm trash (the Finder's
 * right-click "Move to Trash"). Not a delta-covered resource → a `useMutation`.
 * On success the WHOLE `fs` query family for the agent is invalidated so the
 * trashed entries vanish from the current listing at once (they move into a
 * hidden `.cp-trash/` the listing never shows).
 */
export function useTrashItems(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ items }: { items: string[] }) => api.trashItems(agentId, items),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["fs", agentId] })
    },
  })
}

// ── Agent lifecycle (create / restart / retire / rename) ──────────────
//
// Creating/restarting/retiring an agent is a one-shot POST; the spawn or kill
// is async, so each invalidates the fleet query immediately AND on a short
// delay so the change surfaces well before the slow (15s) fleet backstop poll.

/**
 * Mutation to set or clear an agent's custom display name (T328). The override
 * is persisted orchestrator-side in `agent-names.json`, independent of the
 * agent process. On success both the fleet and the per-agent meta queries are
 * invalidated so the new name surfaces everywhere at once.
 */
export function useRenameAgent() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ agentId, name }: { agentId: string; name: string }) =>
      api.renameAgent(agentId, name),
    onSuccess: (_res, { agentId }) => {
      void client.invalidateQueries({ queryKey: qk.fleet() })
      void client.invalidateQueries({ queryKey: qk.retiredFleet() })
      void client.invalidateQueries({ queryKey: qk.agent(agentId) })
    },
  })
}

/**
 * Mutation to create a new agent. On success it nudges the fleet query to
 * refetch immediately and again shortly after, so the freshly-spawned agent
 * surfaces on the dashboard the moment it self-registers (the spawn is async —
 * the receipt only confirms the launch).
 */
export function useCreateAgent() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (body: { name: string; folder?: string; model?: string }) => api.createAgent(body),
    onSuccess: () => {
      invalidateFleet(client)
      // The agent self-registers in ~1-2s after the pty spawn; re-poll a couple
      // of times to catch it well before the 15s backstop.
      window.setTimeout(() => invalidateFleet(client), 1500)
      window.setTimeout(() => invalidateFleet(client), 3500)
    },
  })
}

/**
 * Mutation to restart an agent (kill its stale process + respawn from the
 * current binary). Like {@link useCreateAgent}, the respawn is async: the agent
 * re-registers under the same id within ~2-3s, so we nudge the fleet query to
 * refetch immediately and again shortly after, surfacing the back-to-life agent
 * well before the slow backstop poll.
 */
export function useRestartAgent() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (agentId: string) => api.restartAgent(agentId),
    onSuccess: () => {
      invalidateFleet(client)
      window.setTimeout(() => invalidateFleet(client), 2000)
      window.setTimeout(() => invalidateFleet(client), 4000)
    },
  })
}

/**
 * The Retired (archived) fleet — agents stopped-but-kept (T271). Served from
 * the orchestrator's retired store (`GET /api/fleet/retired`), so each card is
 * rendered from a snapshot, not a live process. No SSE bridge (a retired agent
 * emits nothing); a slow poll keeps it eventually-consistent after a retire /
 * unretire on another tab.
 */
export function useRetiredFleet(): LiveQueryResult<Agent[]> {
  return useLive(qk.retiredFleet(), () => api.fetchRetiredFleet())
}

/**
 * Mutation to retire (archive) an agent — stop its process + console server,
 * keep its folder. On success both the active fleet and the retired fleet are
 * refetched (immediately + once more shortly after, since the process kill +
 * registry-record removal settle asynchronously) so the card moves from the
 * Active grid to the Retired section without waiting on the backstop poll.
 */
export function useRetireAgent() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (agentId: string) => api.retireAgent(agentId),
    onSuccess: () => {
      invalidateBothFleets(client)
      window.setTimeout(() => invalidateBothFleets(client), 1500)
    },
  })
}

/**
 * Mutation to unretire an agent — clear its retired flag and respawn it on the
 * kept folder. The respawn self-registers under the same id within ~2-3s, so we
 * refetch both fleets immediately and again shortly after to surface the
 * back-to-life agent in the Active grid before the slow backstop poll.
 */
export function useUnretireAgent() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (agentId: string) => api.unretireAgent(agentId),
    onSuccess: () => {
      invalidateBothFleets(client)
      window.setTimeout(() => invalidateBothFleets(client), 2000)
      window.setTimeout(() => invalidateBothFleets(client), 4000)
    },
  })
}

// ── Agent avatar (T338) ───────────────────────────────────────────────

/**
 * Mutation to upload or replace an agent's profile picture. On success the
 * fleet and per-agent meta queries are invalidated so the avatar appears
 * everywhere at once.
 */
export function useUploadAvatar() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ agentId, file }: { agentId: string; file: File }) =>
      api.uploadAvatar(agentId, file),
    onSuccess: (_res, { agentId }) => {
      void client.invalidateQueries({ queryKey: qk.fleet() })
      void client.invalidateQueries({ queryKey: qk.agent(agentId) })
    },
  })
}

// ── Prompt library (T350) ──────────────────────────────────────────────

/**
 * Mutation to create a new `/command` in an agent's prompt library (the thread
 * composer's "create command" button). One-shot POST writing a markdown file
 * into the agent's `.context-pilot/commands/`; not a delta-covered resource → a
 * `useMutation`. On success the agent's `useLibrary` query is invalidated so the
 * new command surfaces as a suggestion bubble immediately (the running agent
 * also picks the file up on its own filesystem watch).
 */
export function useCreateCommand(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (cmd: { name: string; description?: string; body: string }) =>
      api.createCommand(agentId, cmd),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: qk.library(agentId) })
    },
  })
}

/**
 * Mutation to create or overwrite a behaviour agent `.md` (T581 footer editor).
 * One-shot `PUT …/library/agent/{itemId}` — a user agent, or a local override
 * of a built-in. Not a delta-covered resource → a `useMutation`. On success the
 * agent's `useLibrary` query is invalidated so the new/edited agent surfaces in
 * the selector immediately (the running agent also picks the file up on its own
 * filesystem watch, and a behaviour switch re-reads it).
 */
export function useUpsertLibraryAgent(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (agent: { itemId: string; name: string; description?: string; body: string }) =>
      api.upsertLibraryAgent(agentId, agent.itemId, {
        name: agent.name,
        description: agent.description ?? "",
        body: agent.body,
      }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: qk.library(agentId) })
    },
  })
}

/**
 * Mutation to delete a behaviour agent's on-disk `.md` (T581 footer editor).
 * If the file overrode a built-in, the compiled-in seed reappears; if it was a
 * pure user agent, it is gone. Not a delta-covered resource → a `useMutation`.
 * On success the agent's `useLibrary` query is invalidated so the row updates
 * (reverts to seed, or vanishes) at once.
 */
export function useDeleteLibraryAgent(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (itemId: string) => api.deleteLibraryAgent(agentId, itemId),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: qk.library(agentId) })
    },
  })
}
