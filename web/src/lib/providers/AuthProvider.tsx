// ── AuthProvider — session lifecycle driver (Phase 9) ──
//
// Sits near the root of the React tree. It:
//   1. Probes `GET /api/auth/status` to know whether the backend has auth
//      enabled at all (NFR-09: zero overhead when disabled).
//   2. If enabled, validates any persisted localStorage token via `/me`.
//   3. Exposes login / register / logout + the current user to children via
//      the shared {@link AuthContext} (defined in `./auth`).

import { useCallback, useEffect, useState, type ReactNode } from "react"
import {
  fetchAuthStatus,
  authLogin,
  authRegister,
  authLogout,
  authMe,
  getToken,
  setToken,
  type AuthMe,
} from "@/lib/api"
import { AuthContext } from "./auth"

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<AuthMe | null>(null)
  const [token, setTokenState] = useState<string | null>(getToken)
  const [authEnabled, setAuthEnabled] = useState<boolean | null>(null)
  const [bootstrapped, setBootstrapped] = useState(true)
  const [loading, setLoading] = useState(true)

  // Probe backend on mount: is auth enabled? If yes, validate stored token.
  useEffect(() => {
    let cancelled = false
    // Read `cancelled` through a getter: its only mutation is in the deferred
    // cleanup return, so a direct `!cancelled` inside this async body is narrowed
    // by control-flow analysis to the literal `false` (always-true guard). A
    // function call is opaque to CFA — the honest way to express "may flip".
    const isCancelled = () => cancelled
    void (async () => {
      try {
        const status = await fetchAuthStatus()
        if (isCancelled()) return
        setAuthEnabled(status.enabled)
        setBootstrapped(status.bootstrapped ?? true)

        if (!status.enabled) {
          setLoading(false)
          return
        }

        // Auth is enabled — try to restore the session from localStorage.
        const stored = getToken()
        if (stored) {
          try {
            const me = await authMe()
            if (!isCancelled()) {
              setUser(me)
              setTokenState(stored)
            }
          } catch {
            // Token invalid or expired — clear it silently.
            setToken(null)
            if (!isCancelled()) setTokenState(null)
          }
        }
      } catch {
        // Backend unreachable — assume auth disabled so the app doesn't
        // permanently lock the user out when the server is down.
        if (!isCancelled()) setAuthEnabled(false)
      } finally {
        if (!isCancelled()) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  // Listen for 401 "session expired" events from the fetch wrapper.
  useEffect(() => {
    const onExpired = () => {
      setUser(null)
      setTokenState(null)
    }
    window.addEventListener("cp-auth-expired", onExpired)
    return () => window.removeEventListener("cp-auth-expired", onExpired)
  }, [])

  const login = useCallback(async (email: string, password: string) => {
    const res = await authLogin(email, password)
    setToken(res.token)
    setTokenState(res.token)
    // Pull /me for the canonical profile + backend-driven next_action (the
    // login response carries the user but not the post-login step).
    setUser(await authMe())
  }, [])

  const register = useCallback(
    async (email: string, name: string, password: string) => {
      await authRegister(email, name, password)
      // Auto-login after bootstrap registration.
      await login(email, password)
      setBootstrapped(true)
    },
    [login],
  )

  const refreshMe = useCallback(async () => {
    try {
      const me = await authMe()
      setUser(me)
    } catch {
      // Leave the cached user as-is on a transient failure.
    }
  }, [])

  const logout = useCallback(async () => {
    try {
      await authLogout()
    } catch {
      // Server may be unreachable — clear locally regardless.
    }
    setToken(null)
    setTokenState(null)
    setUser(null)
  }, [])

  return (
    <AuthContext.Provider
      value={{
        user,
        token,
        authEnabled,
        bootstrapped,
        loading,
        login,
        register,
        logout,
        refreshMe,
      }}
    >
      {children}
    </AuthContext.Provider>
  )
}
