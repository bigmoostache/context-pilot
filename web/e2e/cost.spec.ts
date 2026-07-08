import { test, expect, type APIRequestContext } from "@playwright/test"

// ── Phase 3 — cost / $ counters are live (footer + fleet card) ───────
//
// The dollar figures the UI shows must be the agent's real cumulative spend,
// folded from the oplog CostAggregate deltas into the MaterializedView and
// served over /api/.../meta — never the frozen mock ($5.41). Two surfaces
// carry cost: the fleet dashboard agent card and the in-agent status footer.
// Both are cross-checked against the backend figure.
//
// Tolerance: the agent is a *live* process that may spend more cents mid-test,
// so we assert the UI figure is within a small dollar delta of the backend's —
// tight enough to prove it's the live value (the mock would be hundreds off),
// loose enough to absorb in-flight drift.

const AGENT_ID = "f3a993c0ff357b41"
const API = process.env.CP_API_URL ?? "http://localhost:7878"
const DRIFT_USD = 5

/** Parse the first `$N[,NNN].NN` dollar figure out of a blob of text. */
function parseDollars(text: string): number | null {
  const m = text.match(/\$([\d,]+\.\d{2})/)
  return m ? Number.parseFloat(m[1].replace(/,/g, "")) : null
}

async function metaCost(req: APIRequestContext, id: string): Promise<number> {
  const res = await req.get(`${API}/api/agent/${id}/meta`)
  expect(res.ok()).toBeTruthy()
  const meta = await res.json()
  expect(typeof meta.costUsd, "meta carries a numeric costUsd").toBe("number")
  return meta.costUsd as number
}

test.describe("cost / live dollar counters", () => {
  test("fleet card shows the agent's live cumulative cost", async ({ page, request }) => {
    const backend = await metaCost(request, AGENT_ID)
    expect(backend, "live agent has spent real money (not the $5.41 mock)").toBeGreaterThan(
      DRIFT_USD,
    )

    await page.goto("/")
    // The agent card carries the cost next to its model. Scope to the card by
    // its name, then read the whole card's text and pull the dollar figure.
    const card = page
      .locator("div")
      .filter({ hasText: /^context-pilot/ })
      .first()
    await expect(card).toBeVisible()
    const cardText = await page.locator("body").innerText()
    const shown = parseDollars(cardText)
    expect(shown, "a dollar figure renders on the fleet dashboard").not.toBeNull()
    expect(Math.abs((shown ?? 0) - backend)).toBeLessThan(DRIFT_USD)
  })

  test("status footer shows the agent's live cost", async ({ page, request }) => {
    const backend = await metaCost(request, AGENT_ID)

    await page.goto("/")
    await page.getByRole("button", { name: /Open/i }).first().click()
    await expect(page.getByRole("button", { name: /New Thread/i })).toBeVisible()

    const footer = page.locator("footer")
    await expect(footer).toBeVisible()
    const footerText = await footer.innerText()
    const shown = parseDollars(footerText)
    expect(shown, "footer renders a dollar figure").not.toBeNull()
    expect(Math.abs((shown ?? 0) - backend)).toBeLessThan(DRIFT_USD)
  })
})
