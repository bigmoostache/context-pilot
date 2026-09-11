import { test, expect, type Page } from "@playwright/test"

// ── Feature flags (CP_FEATURE_*, docs/ENV.md) ───────────────────────
//
// Drives the LIVE stack like every other spec here (web :5175 → orchestrator
// :7878). The backend's `GET /api/features` is the source of truth: whatever
// the orchestrator was started with, the cockpit must mirror it AND the
// backend must enforce it — a switched-off surface answers 404, and a
// read-only key store refuses the PUT. Run the suite twice with different
// flags to exercise both halves (e.g. `CP_FEATURE_UPDATER=0` then `=1`).
//
// Assumes an admin-capable session (access control off, or an admin+ account),
// the same assumption the IT/Update pane specs make.

const API = process.env.CP_API_URL ?? "http://localhost:7878"

/** The flags the running orchestrator reports (public, no token needed). */
async function features(request: Parameters<Parameters<typeof test>[2]>[0]["request"]) {
  const res = await request.get(`${API}/api/features`)
  expect(res.status()).toBe(200)
  return (await res.json()) as Record<string, boolean>
}

/** Open Settings (avatar menu → Settings). */
async function openSettings(page: Page) {
  await page.goto("/")
  await page.getByLabel("Account menu").click()
  await page.getByRole("menuitem", { name: "Settings" }).click()
}

test.describe("feature flags", () => {
  test("the settings categories mirror the deployment's flags", async ({ page, request }) => {
    const flags = await features(request)
    await openSettings(page)
    // A pane whose flag is off is offered to no one; on, it is there.
    for (const [name, key] of [
      ["IT", "it_pane"],
      ["Update", "updater"],
    ] as const) {
      const button = page.getByRole("button", { name, exact: true })
      if (flags[key]) await expect(button).toBeVisible()
      else await expect(button).toHaveCount(0)
    }
  })

  test("a switched-off surface answers 404 from the backend", async ({ request }) => {
    const flags = await features(request)
    const probes = [
      ["updater", `${API}/api/update/status`],
      ["it_pane", `${API}/api/it/provisioned`],
      ["claude_oauth", `${API}/api/claude-login/status`],
    ] as const
    for (const [key, url] of probes) {
      const status = (await request.get(url)).status()
      if (flags[key]) expect(status, `${url} on`).not.toBe(404)
      else expect(status, `${url} off`).toBe(404)
    }
  })

  test("provider keys are read-only when the deployment says so", async ({ request }) => {
    const flags = await features(request)
    // A no-op value keeps the probe harmless; the gate answers before any write.
    const res = await request.put(`${API}/api/env-keys/BRAVE_API_KEY`, {
      data: { value: "" },
    })
    if (flags["keys_editable"]) expect(res.status()).not.toBe(403)
    else expect(res.status()).toBe(403)
  })
})
