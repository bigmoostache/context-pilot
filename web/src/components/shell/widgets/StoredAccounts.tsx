import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { Loader2, XCircle } from "lucide-react"
import {
  fetchClaudeAccounts,
  storeClaudeAccount,
  switchClaudeAccount,
  deleteClaudeAccount,
} from "@/lib/api"
import type { ClaudeAccountSummary } from "@/lib/api/generated/types.gen"
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
 *  Extracted from UsageButton for the P8 line budget. */
export function StoredAccounts({ isValid, onSwitch }: { isValid: boolean; onSwitch: () => void }) {
  const queryClient = useQueryClient()

  const accountsQuery = useQuery({
    queryKey: ["claude-accounts"],
    queryFn: fetchClaudeAccounts,
    staleTime: 10_000,
    retry: 1,
  })

  const storeMutation = useMutation({
    mutationFn: storeClaudeAccount,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["claude-accounts"] })
    },
  })

  const switchMutation = useMutation({
    mutationFn: switchClaudeAccount,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["claude-token-status"] })
      void queryClient.invalidateQueries({ queryKey: ["claude-usage"] })
      void queryClient.invalidateQueries({ queryKey: ["claude-accounts"] })
      onSwitch()
    },
  })

  const deleteMutation = useMutation({
    mutationFn: deleteClaudeAccount,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["claude-accounts"] })
    },
  })

  const accounts: ClaudeAccountSummary[] = accountsQuery.data?.accounts ?? []

  return (
    <div className="space-y-2">
      {isValid && (
        <button
          onClick={() => storeMutation.mutate()}
          disabled={storeMutation.isPending}
          className="flex w-full items-center justify-center gap-2 rounded-md bg-muted px-3 py-1.5 text-[12px] font-medium text-foreground transition-colors hover:bg-muted/80 disabled:opacity-50"
        >
          {storeMutation.isPending ? (
            <>
              <Loader2 className="size-3.5 animate-spin" /> Storing…
            </>
          ) : (
            "Store current account"
          )}
        </button>
      )}
      {storeMutation.isError && (
        <p className="text-[11px] text-red-500">
          {storeMutation.error instanceof Error ? storeMutation.error.message : "Store failed"}
        </p>
      )}
      {accounts.length > 0 && (
        <div className="space-y-1.5">
          <p className="text-[11px] font-medium text-muted-foreground">Stored accounts</p>
          {accounts.map((account) => (
            <StoredAccountRow
              key={account.email}
              account={account}
              switching={switchMutation.isPending}
              onSwitch={() => switchMutation.mutate(account.email)}
              onDelete={() => deleteMutation.mutate(account.email)}
            />
          ))}
        </div>
      )}
      {switchMutation.isError && (
        <p className="text-[11px] text-red-500">
          {switchMutation.error instanceof Error ? switchMutation.error.message : "Switch failed"}
        </p>
      )}
    </div>
  )
}
