import "./lib/api/client/setup"
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { QueryClientProvider } from "@tanstack/react-query"
import { queryClient } from "./lib/query/queryClient"
import "./index.css"
import App from "./App.tsx"

const rootEl = document.querySelector("#root")
if (!rootEl) throw new Error("Fatal: #root mount point missing from index.html")

createRoot(rootEl).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
)
