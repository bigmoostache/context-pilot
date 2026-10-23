export type SaveState = "idle" | "saving" | "saved" | "conflict" | "error"

/** One-line save status; a conflict offers to reload the agent's version. */
export function SaveBar({ state, onReload }: { state: SaveState; onReload: () => void }) {
  if (state === "idle") return null
  const label: Record<Exclude<SaveState, "idle">, string> = {
    saving: "Saving…",
    saved: "Saved",
    conflict: "The file changed on disk since you opened it.",
    error: "Save failed.",
  }
  return (
    <div className="flex h-7 shrink-0 items-center gap-2 px-3 text-[11px] text-muted-foreground">
      <span
        className={state === "saved" ? "text-(--ok)" : state === "saving" ? "" : "text-(--danger)"}
      >
        {label[state]}
      </span>
      {state === "conflict" && (
        <button
          type="button"
          onClick={onReload}
          className="rounded-md bg-muted px-2 py-0.5 font-medium hover:bg-muted/70"
        >
          Reload (discard my edits)
        </button>
      )}
    </div>
  )
}
