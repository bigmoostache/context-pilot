import path from "node:path"
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5174,
    // Allow the dev server to be reached through a reverse proxy that presents
    // a non-localhost Host header — chiefly `tailscale serve`, which fronts
    // this port at https://<machine>.<tailnet>.ts.net so the cockpit is
    // reachable from a phone/other device on the tailnet. Vite otherwise
    // rejects any request whose Host isn't localhost/an IP (anti-DNS-rebinding).
    // The dev server only ever listens on 127.0.0.1 and is exposed solely over
    // the private WireGuard tailnet. Scope the allow-list to the tailnet suffix
    // (`.ts.net`) plus localhost rather than disabling the check entirely, so
    // anti-DNS-rebinding protection stays on for every other Host header.
    allowedHosts: ["localhost", ".ts.net"],
    // Single-origin proxy: every backend route lives under `/api` (REST + the
    // `/api/stream` SSE endpoint). Proxying it to the orchestrator lets the
    // frontend address the backend with a RELATIVE base (VITE_API_URL="" — see
    // .env.local), so the app works identically whether loaded at
    // http://localhost:5174 or https://<host>.ts.net: the browser resolves
    // `/api/...` against the page origin and vite forwards it to :7878. No CORS,
    // no mixed-content, no hardcoded backend host. http-proxy streams the
    // response, so the long-lived `text/event-stream` SSE connection passes
    // through unbuffered.
    proxy: {
      "/api": {
        target: "http://127.0.0.1:7878",
        changeOrigin: true,
      },
    },
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  build: {
    rollupOptions: {
      output: {
        // Split the heaviest vendor groups out of the single app chunk so the
        // browser can cache them independently and the initial bundle shrinks.
        // Function form (rather than the Record form) to avoid the Vite/Rollup
        // `output` overload picking the ManualChunksFunction type and rejecting
        // the object literal under `tsc -b`.
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined
          if (/[\\/]node_modules[\\/](react|react-dom|scheduler)[\\/]/.test(id))
            return 'react'
          if (
            /[\\/]node_modules[\\/](react-markdown|remark-gfm|remark-math|rehype-katex|katex|mdast|micromark|unist|hast|vfile|property-information|space-separated-tokens|comma-separated-tokens|decode-named-character-reference|character-entities)/.test(
              id,
            )
          )
            return 'markdown'
          if (/[\\/]node_modules[\\/]highlight\.js[\\/]/.test(id))
            return 'highlight'
          return undefined
        },
      },
    },
  },
})
