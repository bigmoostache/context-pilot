import "./lib/api/client/setup"
import { Component, StrictMode, type ErrorInfo, type ReactNode } from "react"
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

/**
 * App-wide error boundary — the safety net the app was missing (T644).
 *
 * Before this, the tree carried NO error boundary anywhere, so a single
 * uncaught render throw unmounted the whole React root: the user was left
 * staring at a blank surface with no error, no recovery, and no way to tell a
 * render throw apart from a scroll/layout glitch. That is precisely the "how
 * can it silently break" class of bug this catches.
 *
 * On a throw it renders a recoverable fallback that SHOWS the real error
 * message + component stack (so a failure is never silent again) and offers a
 * reload. It wraps the providers so a provider throw is caught too. Kept inline
 * in the entry module (rather than its own file) to stay within the src/
 * directory-entry budget — it has exactly one consumer, right here.
 */
interface BoundaryState {
  error: Error | null
  info: ErrorInfo | null
}

class RootErrorBoundary extends Component<{ children: ReactNode }, BoundaryState> {
  static getDerivedStateFromError(error: Error): Partial<BoundaryState> {
    return { error }
  }

  override state: BoundaryState = { error: null, info: null }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    // Surface the throw in the console too, so a copy survives even if the user
    // dismisses the fallback — the diagnostic four headless repros never caught.
    console.error("[RootErrorBoundary] uncaught render error:", error, info)
    this.setState({ error, info })
  }

  override render(): ReactNode {
    const { error, info } = this.state
    if (!error) return this.props.children
    return (
      <div
        role="alert"
        style={{
          padding: "24px",
          margin: "24px auto",
          maxWidth: "720px",
          fontFamily: "ui-monospace, monospace",
          fontSize: "13px",
          lineHeight: 1.5,
          color: "#e5e5e5",
          background: "#1a1a1a",
          border: "1px solid #f87171",
          borderRadius: "12px",
          overflow: "auto",
        }}
      >
        <p style={{ fontWeight: 600, color: "#f87171", marginBottom: "8px" }}>
          Something threw while rendering — the app caught it instead of going blank.
        </p>
        <pre style={{ whiteSpace: "pre-wrap", margin: "8px 0" }}>{error.message}</pre>
        {error.stack && (
          <pre style={{ whiteSpace: "pre-wrap", margin: "8px 0", opacity: 0.7 }}>{error.stack}</pre>
        )}
        {info?.componentStack && (
          <pre style={{ whiteSpace: "pre-wrap", margin: "8px 0", opacity: 0.55 }}>
            {info.componentStack}
          </pre>
        )}
        <button
          type="button"
          onClick={() => {
            location.reload()
          }}
          style={{
            marginTop: "12px",
            padding: "6px 14px",
            fontSize: "12.5px",
            fontWeight: 600,
            color: "#0a0a0a",
            background: "#34d399",
            border: "none",
            borderRadius: "8px",
            cursor: "pointer",
          }}
        >
          Reload
        </button>
      </div>
    )
  }
}

const rootEl = document.querySelector("#root")
if (!rootEl) throw new Error("Fatal: #root mount point missing from index.html")

createRoot(rootEl).render(
  <StrictMode>
    <RootErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>
    </RootErrorBoundary>
  </StrictMode>,
)
