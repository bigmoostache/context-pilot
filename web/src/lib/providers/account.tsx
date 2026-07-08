import { createContext, use } from "react"
import type { User } from "../types"

/**
 * Account state for the avatar menu / Profile modal / Settings (T30).
 *
 * The single mutable thing about the account is whether it's
 * **company-managed** — that flag flips identity editability in the Profile
 * modal AND the API-key lock in Settings. It lives in one shared context
 * (supplied by `AccountProvider` in `./AccountProvider`) so both surfaces stay
 * in sync. It also backs the account-type switch in the Profile modal.
 */
export interface AccountCtx {
  /** the live user, with {@link User.managedByCompany} reflecting the toggle */
  user: User
  /** convenience mirror of `user.managedByCompany` */
  managed: boolean
  /** flip the account between company-managed and personal */
  setManaged: (managed: boolean) => void
}

/** Account context object. Supplied by `AccountProvider`, read by {@link useAccount}. */
export const AccountContext = createContext<AccountCtx | null>(null)

export function useAccount(): AccountCtx {
  const ctx = use(AccountContext)
  if (!ctx) throw new Error("useAccount must be used within an AccountProvider")
  return ctx
}
