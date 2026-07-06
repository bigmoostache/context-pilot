import { useEffect, useRef } from "react"

/**
 * Window-level "Drop to upload" drag lifecycle. Calls `setDragging(true)` while
 * an OS-file drag hovers the window and `setDragging(false)` on EVERY way a drag
 * can end — a real drop, leaving the window, OR a silent cancel (Esc / release
 * outside) that fires no drop and (in some browsers) no element-level dragleave.
 * On a real drop it hands the dropped files to `onFiles`.
 *
 * The whole lifecycle is tracked at the WINDOW level: dragover sets the overlay
 * and refreshes a heartbeat timestamp; an explicit window drop / dragleave-to-
 * outside clears it immediately; and a heartbeat watchdog is the catch-all —
 * dragover fires continuously while a drag is live, so once it stops (any cancel
 * path) the overlay self-clears within a couple of frames. A ref holds the
 * latest `onFiles` closure so the listeners bind once and never go stale.
 */
export function useExternalDragUpload(
  setDragging: (on: boolean) => void,
  onFiles: (files: File[]) => void,
) {
  const onFilesRef = useRef(onFiles)
  onFilesRef.current = onFiles
  const lastDragOverRef = useRef(0)
  useEffect(() => {
    const isFileDrag = (e: DragEvent) => !!e.dataTransfer?.types.includes("Files")
    const onOver = (e: DragEvent) => {
      if (!isFileDrag(e)) return
      e.preventDefault() // allow the drop
      lastDragOverRef.current = Date.now()
      setDragging(true)
    }
    const onDrop = (e: DragEvent) => {
      if (!isFileDrag(e)) return
      e.preventDefault()
      setDragging(false)
      const files = Array.from(e.dataTransfer?.files ?? [])
      if (files.length) onFilesRef.current(files)
    }
    // Fired when the cursor leaves the document for the outside (relatedTarget
    // null) — clear at once rather than waiting on the watchdog.
    const onLeave = (e: DragEvent) => {
      if (e.relatedTarget === null) setDragging(false)
    }
    // Watchdog: a live drag emits dragover continuously; if none has arrived for
    // a short grace window the drag has ended SOMEHOW (drop elsewhere, left the
    // window, or Esc-cancel) → drop the overlay. Generous enough not to flicker
    // while the pointer holds still over the window.
    const watchdog = window.setInterval(() => {
      if (lastDragOverRef.current && Date.now() - lastDragOverRef.current > 250) {
        lastDragOverRef.current = 0
        setDragging(false)
      }
    }, 100)
    const onEnd = () => setDragging(false)
    window.addEventListener("dragover", onOver)
    window.addEventListener("drop", onDrop)
    window.addEventListener("dragleave", onLeave)
    window.addEventListener("dragend", onEnd)
    return () => {
      window.clearInterval(watchdog)
      window.removeEventListener("dragover", onOver)
      window.removeEventListener("drop", onDrop)
      window.removeEventListener("dragleave", onLeave)
      window.removeEventListener("dragend", onEnd)
    }
  }, [setDragging])
}
