// ── Auth context — session lifecycle for the web frontend (Phase 9) ──
//
// AuthProvider sits near the root of the React tree. It:
//   1. Probes `GET /api/auth/status` to know whether the backend has auth
//      enabled at all (NFR-09: zero overhead when disabled).
//   2. If enabled, validates any persisted localStorage token via `/me`.
//   3. Exposes login / register / logout + the current user to children.
//
// The companion AuthGuard component (components/auth/) renders the login
// page when auth is required but no valid session exists.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react"
import {
  fetchAuthStatus,
  authLogin,
  authRegister,
  authLogout,
  authMe,
  getToken,
  setToken,
  type AuthUser,
} from "@/lib/api"

// ── Context shape ────────────────────────────────────────────────────

interface AuthContextValue {
  /** The authenticated user, or null when logged out / auth disabled. */
  user: AuthUser | null
  /** Raw session token (mostly for debugging; Bearer injection is automatic). */
  token: string | null
  /** `true` = backend requires auth; `false` = auth disabled; `null` = still probing. */
  authEnabled: boolean | null
  /** True when ≥1 user exists (login mode); false = bootstrap-register mode. */
  bootstrapped: boolean
  /** True while the initial status + token validation is in flight. */
  loading: boolean
  /** Authenticate with email + password. Throws on failure. */
  login: (email: string, password: string) => Promise<void>
  /** Bootstrap-register the first user (admin). Throws on failure.
   *  After success, auto-logs in and sets `bootstrapped` to true. */
  register: (email: string, name: string, password: string) => Promise<void>
  /** End the current session and clear the stored token. */
  logout: () => Promise<void>
  /** Re-fetch `/me` and refresh the cached user (after a profile edit). */
  refreshMe: () => Promise<void>
}

const AuthContext = createContext<AuthContextValue | null>(null)

// ── Provider ─────────────────────────────────────────────────────────

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<AuthUser | null>(null)
  const [token, setTokenState] = useState<string | null>(getToken)
  const [authEnabled, setAuthEnabled] = useState<boolean | null>(null)
  const [bootstrapped, setBootstrapped] = useState(true)
  const [loading, setLoading] = useState(true)

  // Probe backend on mount: is auth enabled? If yes, validate stored token.
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const status = await fetchAuthStatus()
        if (cancelled) return
        setAuthEnabled(status.enabled)
        setBootstrapped(status.bootstrapped)

        if (!status.enabled) {
          setLoading(false)
          return
        }

        // Auth is enabled — try to restore the session from localStorage.
        const stored = getToken()
        if (stored) {
          try {
            const me = await authMe()
            if (!cancelled) {
              setUser(me)
              setTokenState(stored)
            }
          } catch {
            // Token invalid or expired — clear it silently.
            setToken(null)
            if (!cancelled) setTokenState(null)
          }
        }
      } catch {
        // Backend unreachable — assume auth disabled so the app doesn't
        // permanently lock the user out when the server is down.
        if (!cancelled) setAuthEnabled(false)
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => { cancelled = true }
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
    setUser(res.user)
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

// ── Hook ─────────────────────────────────────────────────────────────

/** Read the auth context. Must be called inside an AuthProvider. */
export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error("useAuth must be used within AuthProvider")
  return ctx
}
