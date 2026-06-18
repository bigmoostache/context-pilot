import { defineConfig, devices } from "@playwright/test"

// ── Playwright e2e config — drives the LIVE stack ────────────────────
//
// Targets the running web dev server (:5175 — the live web/ app; note :5173
// and :5174 serve the FROZEN ui/ maquette) which talks to the live
// orchestrator (:7878) and this agent's bridge. No `webServer` block: the
// dev server + orchestrator are expected to already be up (the harness
// asserts against the real, running system — never a mock).
//
// No screenshots anywhere (per the directive): assertions are DOM/role/
// devtools-level via expect() + page.evaluate(). Headless Chromium.

const WEB_URL = process.env.CP_WEB_URL ?? "http://localhost:5175"

export default defineConfig({
  testDir: "./e2e",
  // Serial: tests mutate the single live agent's real thread roster, so a
  // deterministic order keeps create→archive→restore flows from racing.
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [["line"]],
  timeout: 30_000,
  expect: { timeout: 10_000 },
  use: {
    baseURL: WEB_URL,
    headless: true,
    screenshot: "off",
    video: "off",
    trace: "off",
    actionTimeout: 10_000,
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
  ],
})
