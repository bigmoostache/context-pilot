import { test, expect } from "@playwright/test"

// ── §19 — a tripped breaker / degraded stream is VISIBLE on the fleet board ──
//
// X868 stood up `GET /api/agent/{id}/metrics` (durable cost-breaker state,
// stream health, view-vs-oplog rev lag). The fleet dashboard's `HealthBadge`
// polls it and surfaces the first non-nominal condition as a coloured pill, so
// an operator *sees* a tripped breaker at a glance instead of inferring it from
// a silently-failing send (T121: "breaker trip must be VISIBLE in the cockpit,
// not a silent backend latch").
//
// Separation of concerns (same contract as breaker.spec.ts):
//   • Backend metrics *production* is covered by the rust transport/lib tests.
//   • This spec covers the FRONTEND's *surfacing* by intercepting the metrics
//     GET at the network boundary with `page.route` and forcing each of the
//     three non-nominal conditions, then asserting the matching pill renders.
//     Deterministic regardless of the live agent's real (raised-to-$100M)
//     budget, which never trips on its own.

/** Force every agent's metrics poll to answer with a fixed snapshot. */
async function routeMetrics(
  page: import("@playwright/test").Page,
  body: Record<string, unknown>,
) {
  await page.route("**/api/agent/*/metrics", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ id: "x", phase: "idle", lifecycle: "running", ...body }),
    }),
  )
}

const NOMINAL = {
  breaker: { tripped: false, spendUsd: 1, budgetUsd: 100 },
  stream: { subscribers: 1, droppedFrames: 0, degraded: false },
  rev: { view: 100, oplogHead: 100, lag: 0 },
}

test.describe("§19 fleet health badge surfacing", () => {
  test("a tripped breaker shows an 'Over budget' alert on the agent card", async ({ page }) => {
    await routeMetrics(page, {
      ...NOMINAL,
      breaker: { tripped: true, spendUsd: 580.8, budgetUsd: 100 },
    })
    await page.goto("/")
    const badge = page.getByRole("status").filter({ hasText: /Over budget/i }).first()
    await expect(badge).toBeVisible({ timeout: 10_000 })
  })

  test("a degraded stream shows a 'Stream degraded' warning", async ({ page }) => {
    await routeMetrics(page, {
      ...NOMINAL,
      stream: { subscribers: 1, droppedFrames: 7, degraded: true },
    })
    await page.goto("/")
    const badge = page.getByRole("status").filter({ hasText: /Stream degraded/i }).first()
    await expect(badge).toBeVisible({ timeout: 10_000 })
  })

  test("a lagging projection shows a 'Projection lagging' warning", async ({ page }) => {
    await routeMetrics(page, {
      ...NOMINAL,
      rev: { view: 100, oplogHead: 400, lag: 300 },
    })
    await page.goto("/")
    const badge = page.getByRole("status").filter({ hasText: /Projection lagging/i }).first()
    await expect(badge).toBeVisible({ timeout: 10_000 })
  })

  test("a nominal agent shows no health badge (healthy cards stay clean)", async ({ page }) => {
    await routeMetrics(page, NOMINAL)
    await page.goto("/")
    // Wait for the fleet board to render at least one agent card, then assert
    // no health pill is present.
    await expect(page.getByRole("button", { name: /Open/i }).first()).toBeVisible({
      timeout: 10_000,
    })
    await expect(
      page.getByRole("status").filter({ hasText: /Over budget|Stream degraded|Projection lagging/i }),
    ).toHaveCount(0)
  })
})
