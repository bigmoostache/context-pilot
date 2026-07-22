import { Check, Loader2, Plus, Trash2 } from "lucide-react"
import type { ClaudeAccountSummary } from "@/lib/api/generated/types.gen"
import { useClaudeAccounts } from "@/lib/live/useClaudeUsage"
import { cn } from "@/lib/utils"

// ── Stored Claude accounts — mobile big-touch twin ───────────────────
//
// Divergent (marker-less) twin of `components/shell/widgets/StoredAccounts`.
// The desktop widget is a dense 10-12px list with size-4 controls; on the flat
// touch-first mobile usage page that reads as leftover scraps. This forks the
// PRESENTATION only — full-bleed rows, a status presence dot, a relative-expiry
// caption, and big (≥40px) Switch / Delete tap targets — while the store /
// switch / delete mutation wiring stays shared in `useClaudeAccounts` (M141:
// logic in @/lib, only chrome forks). Boxless: the owning page wraps it in a
// flat `Section`, so this renders bare rows separated by hairlines.

/** Relative expiry string (mirrors the desktop helper). */
function formatExpiry(epochMs: number): string {
  const diff = epochMs - Date.now()
  if (diff < 0) return "Expired"
  const mins = Math.floor(diff / 60_000)
  if (mins < 60) return `${mins}m left`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ${mins % 60}m left`
  return `${Math.floor(hrs / 24)}d left`
}

/** One stored-account row — presence dot + email over a relative-expiry
 *  caption, then a big Switch pill and a Delete icon button. Full-bleed, tall
 *  enough for a comfortable tap; separated from its siblings by a top hairline. */
function AccountRow({
  account,
  switching,
  onSwitch,
  onDelete,
}: {
  account: ClaudeAccountSummary
  switching: boolean
  onSwitch: () => void
  onDelete: () => void
}) {
  return (
    <div className="flex items-center gap-3 border-t border-border/40 py-3 first:border-t-0">
      <span
        className={cn(
          "size-2.5 shrink-0 rounded-full",
          account.valid ? "bg-(--ok)" : "bg-(--danger)",
        )}
      />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[15px] font-medium text-foreground/90">{account.email}</span>
        {account.expires_at != null && (
          <span className="text-[12.5px] text-muted-foreground/60 tabular-nums">
            {formatExpiry(account.expires_at)}
          </span>
        )}
      </span>
      <button
        onClick={onSwitch}
        disabled={switching}
        className="shrink-0 rounded-xl bg-muted px-4 py-2 text-[14px] font-semibold text-(--signal) transition-colors active:bg-muted/70 disabled:opacity-50"
      >
        Switch
      </button>
      <button
        onClick={onDelete}
        aria-label={`Delete ${account.email}`}
        className="flex size-10 shrink-0 items-center justify-center rounded-xl text-muted-foreground/50 transition-colors active:bg-muted active:text-(--danger)"
      >
        <Trash2 className="size-5" />
      </button>
    </div>
  )
}

/**
 * Stored accounts list — mobile big-touch. Store the current credential, list
 * stored accounts, switch/delete. All wiring lives in the shared
 * {@link useClaudeAccounts} hook; `onSwitch` (the page's token-cache
 * invalidator) fires after a successful switch so the app re-reads the now-
 * current credential.
 */
export function StoredAccounts({ isValid, onSwitch }: { isValid: boolean; onSwitch: () => void }) {
  const { accounts, storeCurrent, storing, storeError, switchTo, switching, switchError, remove } =
    useClaudeAccounts(onSwitch)

  return (
    <div className="flex flex-col gap-3">
      {isValid && (
        <button
          onClick={storeCurrent}
          disabled={storing}
          className="flex h-13 w-full items-center justify-center gap-2.5 rounded-2xl bg-muted text-[16px] font-semibold text-foreground/90 transition-colors active:bg-muted/70 disabled:opacity-50"
        >
          {storing ? <Loader2 className="size-5 animate-spin" /> : <Plus className="size-5" />}
          {storing ? "Storing…" : "Store current account"}
        </button>
      )}
      {storeError != null && (
        <span className="text-[13px] text-(--danger)">
          {storeError instanceof Error ? storeError.message : "Store failed"}
        </span>
      )}

      {accounts.length > 0 ? (
        <div className="flex flex-col">
          {accounts.map((account) => (
            <AccountRow
              key={account.email}
              account={account}
              switching={switching}
              onSwitch={() => switchTo(account.email)}
              onDelete={() => remove(account.email)}
            />
          ))}
        </div>
      ) : (
        <span className="flex items-center gap-2 text-[14px] text-muted-foreground/60">
          <Check className="size-4 text-(--ok)" /> No other stored accounts.
        </span>
      )}

      {switchError != null && (
        <span className="text-[13px] text-(--danger)">
          {switchError instanceof Error ? switchError.message : "Switch failed"}
        </span>
      )}
    </div>
  )
}
