import { test, expect, type APIRequestContext, type Page } from "@playwright/test"

// ── Phase 9 — tripped CostBreaker is SURFACED, not swallowed (T121) ───
//
// The bug the user actually hit: an over-budget agent answers a send with
// `503 {status:"tripped"}` (design doc R2-8 / V9), and the web composer's
// `.catch(console.error)` swallowed it — the message just silently never sent,
// no feedback. This spec proves the FRONTEND now surfaces that failure.
//
// Separation of concerns:
//   • Backend 503 *production* (breaker fail-closed on send, control-plane
//     bypass) is covered deterministically by the rust transport tests
//     (cp-orchestrator transport_http.rs).
//   • This spec covers the FRONTEND's 503 *surfacing* — the untested layer —
//     by intercepting the command POST at the network boundary with
//     `page.route` and forcing the tripped-breaker response. That is the
//     honest e2e: we mock the backend boundary to drive the frontend code
//     under test, never the thing under test itself.
//
// Determinism: a `page.route` fulfilment is independent of the live agent's
// real budget (which is raised to $100M for the suite), so the test is stable
// regardless of actual spend.

const AGENT_ID = "f3a993c0ff357b41"
const API = process.env.CP_API_URL ?? "http://localhost:7878"

/** Mint a command via the test-level request context (NOT page-routed). */
async function command(req: APIRequestContext, kind: Record<string, unknown>): Promise<void> {
  const token = `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`
  const res = await req.post(`${API}/api/agent/${AGENT_ID}/command`, {
    data: { schema_version: 1, id: token, seq: 0, dedup_token: token, kind },
  })
  expect(res.ok(), `precondition command ${JSON.stringify(kind)} accepted`).toBeTruthy()
}

interface RawThread {
  id: string
  name: string
  archived?: boolean
}

async function rawThreads(req: APIRequestContext): Promise<RawThread[]> {
  const res = await req.get(`${API}/api/agent/${AGENT_ID}/threads`)
  expect(res.ok()).toBeTruthy()
  const raw = await res.json()
  return Array.isArray(raw) ? raw : raw.threads ?? []
}

/** Wait until `name` exists in the roster (non-archived) and return its id. */
async function awaitThreadId(req: APIRequestContext, name: string): Promise<string> {
  await expect
    .poll(async () => (await rawThreads(req)).some((t) => t.name === name && !t.archived), {
      timeout: 15_000,
    })
    .toBe(true)
  return (await rawThreads(req)).find((t) => t.name === name)?.id ?? ""
}

async function openThreads(page: Page) {
  await page.goto("/")
  await page.getByRole("button", { name: /Open/i }).first().click()
  await expect(page.getByRole("button", { name: /New Thread/i })).toBeVisible()
}

test.describe("breaker / tripped send is surfaced in the UI", () => {
  test("a 503 tripped-breaker send shows an actionable alert (no silent swallow)", async ({
    page,
  }) => {
    // Precondition via the un-routed request context: a real thread to send
    // into. The threads list loads over GET /threads (not intercepted), so the
    // thread appears in the UI normally.
    const NAME = `e2e-brk-${Date.now()}`
    await command(page.request, { kind: "create_thread", name: NAME })
    await awaitThreadId(page.request, NAME)

    await openThreads(page)
    await page.getByPlaceholder(/Search threads/i).fill(NAME)
    await page.getByText(NAME, { exact: true }).click()

    // Now force EVERY command POST to answer like a tripped breaker. Installed
    // after the precondition so only the UI-driven send hits it.
    await page.route("**/api/agent/*/command", (route) =>
      route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ status: "tripped" }),
      }),
    )

    const composer = page.getByPlaceholder(/Reply to this thread/i)
    await expect(composer).toBeVisible()
    await composer.fill("this send should be blocked by the breaker")
    await composer.press("Enter")

    // The surfacing contract: a visible, breaker-specific alert — the exact
    // silent-failure hole T121 named, now closed.
    const alert = page.getByRole("alert")
    await expect(alert).toBeVisible({ timeout: 10_000 })
    await expect(alert).toContainText(/budget|breaker|blocked/i)
  })

  test.afterAll(async ({ request }) => {
    for (const t of await rawThreads(request)) {
      if (t.name.startsWith("e2e-brk-") && !t.archived) {
        await command(request, { kind: "archive_thread", thread_id: t.id })
      }
    }
  })
})
