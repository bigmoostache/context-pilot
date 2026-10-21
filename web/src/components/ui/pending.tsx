import { cn } from "@/lib/utils"

/**
 * The one loader for every optimistic update: a tiny muted ring that spins on
 * an element the backend has not confirmed yet. Deliberately discreet — it
 * marks "not synced", it is not a progress indicator.
 */
export function PendingSpinner({ className }: { className?: string | undefined }) {
  return (
    <span
      role="status"
      aria-label="Syncing"
      className={cn(
        "inline-block size-2.5 shrink-0 animate-spin rounded-full border border-muted-foreground/30 border-t-muted-foreground/70",
        className,
      )}
    />
  )
}
