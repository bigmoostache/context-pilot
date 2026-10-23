import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { fetchOfficeLog, postOfficeUpdate, type OfficeLog } from "@/lib/api"
import { useFsOfficeVersion } from "@/lib/live"

/**
 * The slice of the editor's CRDT replica this hook drives. The worker editor's
 * replica lives in the Web Worker: its sync encoders throw, the real one is
 * `async.encodeStateAsUpdate`.
 */
interface Replica {
  encodeStateAsUpdate: () => Uint8Array
  applyUpdate: (update: Uint8Array) => unknown
  async?: { encodeStateAsUpdate: () => Promise<Uint8Array> }
}

/** The replica's full state, through the worker when it lives there. */
const encode = async (r: Replica): Promise<Uint8Array> =>
  r.async ? r.async.encodeStateAsUpdate() : r.encodeStateAsUpdate()

/** What the editor opens: the log's base bytes plus its latest full state. */
interface Session {
  base: Uint8Array
  initialUpdate: Uint8Array | undefined
  baseVersion: string
  /** Yrs client id, fresh per open: two tabs must not share one. */
  clientId: number
  /** Bumped on every (re)open; the editor remounts only when it changes. */
  key: number
}

/** Decode base64 natively (data-URL fetch; no `atob` byte loop). */
const fromB64 = async (s: string): Promise<Uint8Array> => {
  const res = await fetch(`data:application/octet-stream;base64,${s}`)
  return new Uint8Array(await res.arrayBuffer())
}

/** Is `err` the backend's 409 (log or file moved)? */
const isConflict = (err: unknown): boolean =>
  err instanceof Error && (err.message.includes("409") || err.message.includes("moved"))

/**
 * Live xlsx session over the orchestrator's office log
 * (docs/design-office-live-updates.md), mirroring the pptx one: an
 * `office-edit` write merges into the open replica in place; any other rewrite
 * of the file reopens the workbook (only when nothing is unsaved). Saves post
 * the replica state with the file bytes, so the .xlsx on disk stays current.
 */
export function useXlsxLive(fleetId: string, path: string) {
  const [session, setSession] = useState<Session | null>(null)
  const [error, setError] = useState(false)
  const seqRef = useRef(0)
  const headRef = useRef("")
  const replicaRef = useRef<Replica | null>(null)
  const dirtyRef = useRef(false)
  const keyRef = useRef(0)

  const adopt = useCallback((log: OfficeLog) => {
    seqRef.current = log.seq
    headRef.current = log.headVersion
  }, [])

  const open = useCallback(() => {
    fetchOfficeLog(fleetId, path, true)
      .then(async (log) => {
        if (log.base == null) throw new Error("log without base")
        const base = await fromB64(log.base)
        const initialUpdate = log.state == null ? undefined : await fromB64(log.state)
        adopt(log)
        dirtyRef.current = false
        keyRef.current += 1
        setSession({
          base,
          initialUpdate,
          baseVersion: log.baseVersion,
          clientId: Math.floor(Math.random() * 2 ** 31) + 1,
          key: keyRef.current,
        })
      })
      .catch(() => {
        setError(true)
      })
  }, [fleetId, path, adopt])

  useEffect(open, [open])

  /** Pull the log: merge in place when it extends our base, else reopen. */
  const catchUp = useCallback(
    async (allowReopen: boolean): Promise<void> => {
      const log = await fetchOfficeLog(fleetId, path, false)
      const replica = replicaRef.current
      if (log.baseVersion === session?.baseVersion && replica) {
        if (log.state != null && log.seq !== seqRef.current) {
          await replica.applyUpdate(await fromB64(log.state))
        }
        adopt(log)
      } else if (allowReopen && !dirtyRef.current) {
        open()
      }
    },
    [fleetId, path, session, adopt, open],
  )

  const remote = useFsOfficeVersion(fleetId, path).data?.version
  useEffect(() => {
    if (!remote || !headRef.current || remote === headRef.current) return
    catchUp(true).catch(() => null)
  }, [remote, catchUp])

  /** Save the user's edit through the log; one replay+retry on 409. */
  const save = useCallback(
    async (bytes: Uint8Array): Promise<void> => {
      const replica = replicaRef.current
      if (!replica) throw new Error("editor not ready")
      const post = async () =>
        postOfficeUpdate(fleetId, path, {
          seq: seqRef.current,
          state: await encode(replica),
          file: bytes,
        })
      const res = await post().catch(async (e: unknown) => {
        if (!isConflict(e)) throw e
        await catchUp(false)
        return post()
      })
      seqRef.current = res.seq
      headRef.current = res.headVersion
      dirtyRef.current = false
    },
    [fleetId, path, catchUp],
  )

  const markDirty = useCallback(() => {
    dirtyRef.current = true
  }, [])

  const collaboration = useMemo(
    () =>
      session && {
        clientId: session.clientId,
        ...(session.initialUpdate && { initialUpdate: session.initialUpdate }),
        onReplica: (r: Replica | null) => {
          replicaRef.current = r
        },
      },
    [session],
  )

  return { session, error, collaboration, save, markDirty, reopen: open }
}
