// ── @hey-api client configuration (side-effect module) ───────────────
//
// Import this file ONCE at app startup (e.g. main.tsx) to configure the
// generated OpenAPI client singleton with:
//   - base URL from VITE_API_URL (falls back to localhost:7878)
//   - Bearer token injection from localStorage
//   - 401 interceptor that clears the token and fires cp-auth-expired
//
// After this runs, every SDK function in generated/sdk.gen.ts uses the
// configured client automatically — no per-call setup needed.

import { client } from "./generated/client.gen"
import { getToken, setToken } from "./client"

const BASE = import.meta.env.VITE_API_URL ?? "http://localhost:7878"

client.setConfig({
  baseUrl: BASE,
  throwOnError: true,
  responseStyle: "data",
})

// ── Auth: inject Bearer token on every request ───────────────────────

client.interceptors.request.use((request) => {
  const token = getToken()
  if (token && !request.headers.has("Authorization")) {
    request.headers.set("Authorization", `Bearer ${token}`)
  }
  return request
})

// ── 401: clear token + notify AuthProvider ───────────────────────────

client.interceptors.error.use((error, response) => {
  if (response?.status === 401 && getToken()) {
    setToken(null)
    window.dispatchEvent(new Event("cp-auth-expired"))
  }
  return error
})
