import "./lib/api/client/setup"
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { QueryClientProvider } from "@tanstack/react-query"
import { queryClient } from "./lib/query/queryClient"
import { initTelemetry } from "./lib/support/telemetry"
import "./index.css"
import App from "./App.tsx"

// Arm the client-side performance telemetry (web-vitals + Long Animation Frames)
// once, before first paint, so INP/LoAF sampling covers the whole session. The
// dev-mode HUD reads the live snapshot; production non-profiling builds still
// collect vitals/frames but the React <Profiler> is inert.
initTelemetry()

const rootEl = document.querySelector("#root")
if (!rootEl) throw new Error("Fatal: #root mount point missing from index.html")

createRoot(rootEl).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
)
