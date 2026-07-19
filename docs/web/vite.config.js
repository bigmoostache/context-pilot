import { defineConfig } from 'vite'
import { resolve } from 'node:path'

const r = (p) => resolve(import.meta.dirname, p)

// Static multi-page site: the Daharness landing plus the Trust Center pages.
// Each HTML file is its own Rollup entry so Vite hashes their assets and keeps
// the on-disk layout (index.html at root, trust-center/* in a subfolder).
export default defineConfig({
  base: '/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: r('index.html'),
        trust: r('trust-center/index.html'),
        trustSecurity: r('trust-center/security.html'),
        trustPrivacy: r('trust-center/privacy.html'),
        trustCompliance: r('trust-center/compliance.html'),
        trustSubprocessors: r('trust-center/subprocessors.html'),
      },
    },
  },
})
