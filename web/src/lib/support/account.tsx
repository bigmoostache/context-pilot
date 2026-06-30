import { createContext, useContext, useMemo, useState, type ReactNode } from "react"
import type { AuthUser } from "../api/generated/types.gen"
import { currentUser } from "../mock"
import type { User } from "../types"
import { useAuth } from "./auth"

/**
 * Derive a UI {@link User} from the backend {@link AuthUser}.
 *
 * Fills in the display-layer fields (`initials`, `accent`) that only exist on
 * the frontend. Called when auth is enabled so the avatar menu / Profile modal
 * show the real authenticated identity instead of the hardcoded mock.
 */
function userFromAuth(au: AuthUser): User {
  const parts = au.name.split(" ").filter(Boolean)
  const initials = parts
    .map((w) => w[0])
    .join("")
    .slice(0, 2)
    .toUpperCase()
  return {
    name: au.name,
    email: au.email,
    initials: initials || "?",
    accent: "interactive",
    managedByCompany: false,
    role: au.role.charAt(0).toUpperCase() + au.role.slice(1),
  }
}

/**
 * Account state for the avatar menu / Profile modal / Settings (T30).
 *
 * The single thing that's actually *mutable* about the (mock) account is
 * whether it's **company-managed** — that flag flips identity editability in
 * the Profile modal AND the API-key lock in Settings. To keep those two
 * surfaces in sync, the flag lives here in one shared context rather than
 * being read straight off the static {@link currentUser} mock in each place.
 *
 * It also backs the small **account-type switch** in the Profile modal, which
 * lets you flip between the managed and personal layouts to compare them.
 */
interface AccountCtx {
  /** the live user, with {@link User.managedByCompany} reflecting the toggle */
  user: User
  /** convenience mirror of `user.managedByCompany` */
  managed: boolean
  /** flip the account between company-managed and personal */
  setManaged: (managed: boolean) => void
}

const Ctx = createContext<AccountCtx | null>(null)

export function AccountProvider({ children }: { children: ReactNode }) {
  const { authEnabled, user: authUser } = useAuth()
  const [managed, setManaged] = useState(currentUser.managedByCompany)

  const baseUser = authEnabled && authUser ? userFromAuth(authUser) : currentUser

  const user = useMemo<User>(
    () => ({ ...baseUser, managedByCompany: managed }),
    [baseUser, managed],
  )
  return <Ctx.Provider value={{ user, managed, setManaged }}>{children}</Ctx.Provider>
}

export function useAccount(): AccountCtx {
  const ctx = useContext(Ctx)
  if (!ctx) throw new Error("useAccount must be used within an AccountProvider")
  return ctx
}
