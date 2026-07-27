import { Loader2, XCircle } from "lucide-react"
import type { ClaudeAccountSummary } from "@/lib/api/generated/types.gen"
import { useClaudeAccounts } from "@/lib/live/useClaudeUsage"
import { cn } from "@/lib/utils"

/** Format epoch ms as a short relative expiry string. */
function formatExpiry(epochMs: number): string {
  const now = Date.now()
  const diff = epochMs - now
  if (diff < 0) return "Expired"
  const mins = Math.floor(diff / 60_000)
  if (mins < 60) return `${mins}m left`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ${mins % 60}m left`
  return `${Math.floor(hrs / 24)}d left`
}

/** Single stored-account row — email, expiry, switch/delete controls. */
function StoredAccountRow({
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
    <div className="flex items-center justify-between rounded-md bg-muted/40 px-2.5 py-1.5">
      <div className="flex min-w-0 items-center gap-1.5 text-[11px]">
        <div
          className={cn(
            "size-1.5 shrink-0 rounded-full",
            account.valid ? "bg-emerald-500" : "bg-red-500",
          )}
        />
        <span className="truncate text-foreground">{account.email}</span>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        {account.expires_at != null && (
          <span className="text-[10px] text-muted-foreground/70 tabular-nums">
            {formatExpiry(account.expires_at)}
          </span>
        )}
        <button
          onClick={onSwitch}
          disabled={switching}
          className="rounded-sm px-1.5 py-0.5 text-[10px] font-medium text-(--signal) transition-colors hover:bg-muted disabled:opacity-50"
        >
          Switch
        </button>
        <button
          onClick={onDelete}
          className="flex size-4 items-center justify-center rounded-sm text-muted-foreground/50 transition-colors hover:text-red-500"
        >
          <XCircle className="size-3" />
        </button>
      </div>
    </div>
  )
}

/** Stored accounts list — store current, switch to stored, delete.
 *  Extracted from UsageButton for the P8 line budget. All the query/mutation
 *  wiring lives in the shared {@link useClaudeAccounts} hook (M141). */
export function StoredAccounts({ isValid, onSwitch }: { isValid: boolean; onSwitch: () => void }) {
  const { accounts, storeCurrent, storing, storeError, switchTo, switching, switchError, remove } =
    useClaudeAccounts(onSwitch)

  return (
    <div className="space-y-2">
      {isValid && (
        <button
          onClick={storeCurrent}
          disabled={storing}
          className="flex w-full items-center justify-center gap-2 rounded-md bg-muted px-3 py-1.5 text-[12px] font-medium text-foreground transition-colors hover:bg-muted/80 disabled:opacity-50"
        >
          {storing ? (
            <>
              <Loader2 className="size-3.5 animate-spin" /> Storing…
            </>
          ) : (
            "Store current account"
          )}
        </button>
      )}
      {storeError != null && (
        <p className="text-[11px] text-red-500">
          {storeError instanceof Error ? storeError.message : "Store failed"}
        </p>
      )}
      {accounts.length > 0 && (
        <div className="space-y-1.5">
          <p className="text-[11px] font-medium text-muted-foreground">Stored accounts</p>
          {accounts.map((account) => (
            <StoredAccountRow
              key={account.email}
              account={account}
              switching={switching}
              onSwitch={() => switchTo(account.email)}
              onDelete={() => remove(account.email)}
            />
          ))}
        </div>
      )}
      {switchError != null && (
        <p className="text-[11px] text-red-500">
          {switchError instanceof Error ? switchError.message : "Switch failed"}
        </p>
      )}
    </div>
  )
}
