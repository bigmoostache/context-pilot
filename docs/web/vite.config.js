import { defineConfig } from 'vite'
import { existsSync } from 'node:fs'
import { resolve } from 'node:path'

const r = (p) => resolve(import.meta.dirname, p)

// Static multi-page site: the Daharness landing, the trial gate, and the Trust
// Center pages. Each HTML file is its own Rollup entry so Vite hashes their
// assets and keeps the on-disk layout.
//
// Trust Center sources live under trust-center/. They are included only when
// present so the landing still builds on a checkout that lacks them:
//   git checkout 18e9d733 -- docs/web/trust-center/
const optional = (name, path) => (existsSync(r(path)) ? { [name]: r(path) } : {})

export default defineConfig({
  base: '/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: r('index.html'),
        start: r('start.html'),
        ...optional('trust', 'trust-center/index.html'),
        ...optional('trustSecurity', 'trust-center/security.html'),
        ...optional('trustPrivacy', 'trust-center/privacy.html'),
        ...optional('trustCompliance', 'trust-center/compliance.html'),
        ...optional('trustSubprocessors', 'trust-center/subprocessors.html'),
      },
    },
  },
})
