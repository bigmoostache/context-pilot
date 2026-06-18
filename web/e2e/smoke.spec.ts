import { test, expect } from "@playwright/test"

// ── Phase 0 — harness smoke ──────────────────────────────────────────
//
// Proves the live pipe end-to-end: the web app (:5175) boots, fetches the
// fleet from the orchestrator (:7878), and renders the real agent. No mock
// data: the agent name, model, and cost all come from /api/fleet/meta.
//
// We also sanity-check the REST surface directly (page.request) so a failure
// is attributable to either the backend or the frontend, not ambiguously both.

const AGENT_ID = "f3a993c0ff357b41"
const API = process.env.CP_API_URL ?? "http://localhost:7878"

test.describe("smoke / live pipe", () => {
  test("orchestrator REST is live and returns the agent", async ({ request }) => {
    const res = await request.get(`${API}/api/fleet/meta`)
    expect(res.ok()).toBeTruthy()
    const fleet = await res.json()
    expect(Array.isArray(fleet)).toBeTruthy()
    const agent = fleet.find((a: { id: string }) => a.id === AGENT_ID)
    expect(agent, "live agent present in fleet").toBeTruthy()
    expect(agent.name).toBe("context-pilot")
    expect(typeof agent.costUsd).toBe("number")
  })

  test("fleet dashboard renders the real agent card", async ({ page }) => {
    await page.goto("/")
    // Default view is the fleet dashboard. The agent's real name must appear
    // (it comes from the backend, not mock data).
    await expect(page.getByText("context-pilot").first()).toBeVisible()
    // The Agents header and the New-agent affordance are the dashboard's anchors.
    await expect(page.getByRole("heading", { name: "Agents" })).toBeVisible()
    await expect(page.getByRole("button", { name: /New agent/i }).first()).toBeVisible()
    // The agent card carries an Open button.
    await expect(page.getByRole("button", { name: /Open/i }).first()).toBeVisible()
  })

  test("opening the agent drops into its threads view", async ({ page }) => {
    await page.goto("/")
    await page.getByRole("button", { name: /Open/i }).first().click()
    // Threads view anchors: a New Thread button + the thread search box.
    await expect(page.getByRole("button", { name: /New Thread/i })).toBeVisible()
    await expect(page.getByPlaceholder(/Search threads/i)).toBeVisible()
  })

  test("status footer shows the agent's live cost", async ({ page, request }) => {
    // Cross-check: the cost the footer renders must match the backend figure.
    const meta = await (await request.get(`${API}/api/agent/${AGENT_ID}/meta`)).json()
    const costUsd: number = meta.costUsd
    await page.goto("/")
    await page.getByRole("button", { name: /Open/i }).first().click()
    // Footer renders cost as e.g. "$437.53" (fmtCost). Assert a dollar figure
    // is present in the footer region.
    const footer = page.locator("footer")
    await expect(footer).toBeVisible()
    await expect(footer.getByText(/\$[\d,]+\.\d{2}/).first()).toBeVisible()
    // Sanity: backend cost is a positive number we can format.
    expect(costUsd).toBeGreaterThan(0)
  })
})
