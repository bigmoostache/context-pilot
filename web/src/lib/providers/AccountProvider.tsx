import { useMemo, useState, type ReactNode } from "react"
import type { AuthUser } from "../api/generated/types.gen"
import { currentUser } from "../mock"
import type { User } from "../types"
import { AccountContext } from "./account"
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

/** Supplies the shared {@link AccountContext} (managed-flag toggle + live user). */
export function AccountProvider({ children }: { children: ReactNode }) {
  const { authEnabled, user: authUser } = useAuth()
  const [managed, setManaged] = useState(currentUser.managedByCompany)

  const baseUser = authEnabled && authUser ? userFromAuth(authUser) : currentUser

  const user = useMemo<User>(
    () => ({ ...baseUser, managedByCompany: managed }),
    [baseUser, managed],
  )
  return <AccountContext value={{ user, managed, setManaged }}>{children}</AccountContext>
}
