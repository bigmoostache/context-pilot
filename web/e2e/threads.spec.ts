import { test, expect, type Page, type APIRequestContext } from "@playwright/test"

// ── Phase 1 — threads CRUD over the live push plane ──────────────────
//
// Drives the real create → archive → restore lifecycle through the UI and
// asserts each step BOTH in the DOM (the SSE push plane / cold fetch reflect
// the mutation) AND against the orchestrator's roster REST (ground truth).
// No mocks: every mutation is a real `command` POST to the live agent's
// bridge, journaled to its oplog, folded into the MaterializedView, and
// served back over /threads.
//
// Each test is INDEPENDENT: it builds its own precondition state via the API
// (create / archive) so tests never depend on each other's ordering or on a
// shared in-session push store. The archive + restore *actions under test* are
// driven through the real UI; everything else is API setup. Hygiene: every
// `e2e-`-prefixed thread is archived in afterAll so the live roster stays clean.

const AGENT_ID = "f3a993c0ff357b41"
const API = process.env.CP_API_URL ?? "http://localhost:7878"

/** Post a command envelope to the live agent's bridge. */
async function command(req: APIRequestContext, kind: Record<string, unknown>): Promise<void> {
  const token = `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`
  const res = await req.post(`${API}/api/agent/${AGENT_ID}/command`, {
    data: { schema_version: 1, id: token, seq: 0, dedup_token: token, kind },
  })
  expect(res.ok(), `command ${JSON.stringify(kind)} accepted`).toBeTruthy()
}

/** The live roster as the backend sees it (ground-truth cross-check). */
async function roster(
  req: APIRequestContext,
): Promise<Array<{ id: string; name: string; archived: boolean }>> {
  const res = await req.get(`${API}/api/agent/${AGENT_ID}/threads`)
  expect(res.ok()).toBeTruthy()
  const raw = await res.json()
  const list = Array.isArray(raw) ? raw : (raw.threads ?? [])
  return list.map((t: { id: string; name: string; archived?: boolean }) => ({
    id: t.id,
    name: t.name,
    archived: !!t.archived,
  }))
}

/** Wait until the roster contains `name` with the expected archived state, return its id. */
async function awaitRoster(
  req: APIRequestContext,
  name: string,
  archived: boolean,
): Promise<string> {
  let id = ""
  await expect
    .poll(
      async () => {
        const hit = (await roster(req)).find((t) => t.name === name && t.archived === archived)
        id = hit?.id ?? ""
        return !!hit
      },
      { timeout: 15_000 },
    )
    .toBe(true)
  return id
}

/** Open the agent and land in its threads view. */
async function openThreads(page: Page) {
  await page.goto("/")
  await page.getByRole("button", { name: /Open/i }).first().click()
  await expect(page.getByRole("button", { name: /New Thread/i })).toBeVisible()
}

test.describe("threads / CRUD over the push plane", () => {
  test("create → row appears live + roster confirms", async ({ page }) => {
    const NAME = `e2e-create-${Date.now()}`
    await openThreads(page)

    // Create through the New Thread dialog.
    await page.getByRole("button", { name: /New Thread/i }).click()
    const input = page.getByPlaceholder(/Refactor the cache engine/i)
    await expect(input).toBeVisible()
    await input.fill(NAME)
    await page.getByRole("button", { name: /Create thread/i }).click()

    // Push plane: the thread_created delta prepends the row with no refetch.
    // Narrow via search so the assertion is deterministic against the roster.
    await page.getByPlaceholder(/Search threads/i).fill(NAME)
    await expect(page.getByText(NAME, { exact: true })).toBeVisible({ timeout: 15_000 })

    // Ground truth: the backend roster carries it, not archived.
    await awaitRoster(page.request, NAME, false)
  })

  test("archive → leaves live list, lands in Archived", async ({ page }) => {
    // Precondition via API: a fresh, non-archived thread exists.
    const NAME = `e2e-archive-${Date.now()}`
    await command(page.request, { kind: "create_thread", name: NAME })
    await awaitRoster(page.request, NAME, false)

    await openThreads(page)
    await page.getByPlaceholder(/Search threads/i).fill(NAME)
    const row = page.getByText(NAME, { exact: true })
    await expect(row).toBeVisible({ timeout: 15_000 })

    // Archive through the UI: hover the row, click its archive action.
    await row.hover()
    await page.getByTitle("Archive thread").first().click()

    // It leaves the live (non-archived) list, and the backend agrees.
    await expect(page.getByText(NAME, { exact: true })).toBeHidden({ timeout: 15_000 })
    await awaitRoster(page.request, NAME, true)

    // The Archived view shows it (its search placeholder is "Search archived…").
    await page
      .getByRole("button", { name: /^Archived/i })
      .first()
      .click()
    await page.getByPlaceholder(/Search archived/i).fill(NAME)
    await expect(page.getByText(NAME, { exact: true })).toBeVisible({ timeout: 15_000 })
  })

  test("restore → returns to the live list", async ({ page }) => {
    // Precondition via API: a thread that already exists AND is archived.
    const NAME = `e2e-restore-${Date.now()}`
    await command(page.request, { kind: "create_thread", name: NAME })
    const id = await awaitRoster(page.request, NAME, false)
    await command(page.request, { kind: "archive_thread", thread_id: id })
    await awaitRoster(page.request, NAME, true)

    await openThreads(page)
    // Enter the Archived view and find the thread.
    await page
      .getByRole("button", { name: /^Archived/i })
      .first()
      .click()
    await page.getByPlaceholder(/Search archived/i).fill(NAME)
    const row = page.getByText(NAME, { exact: true })
    await expect(row).toBeVisible({ timeout: 15_000 })

    // Restore through the UI.
    await row.hover()
    await page.getByTitle("Restore thread").first().click()

    // Backend roster un-archives it.
    await awaitRoster(page.request, NAME, false)
  })

  // Hygiene: archive every e2e- thread we created so the live roster stays
  // clean for the human. Best-effort — failures here don't fail the suite.
  test.afterAll(async ({ request }) => {
    for (const t of await roster(request)) {
      if (t.name.startsWith("e2e-") && !t.archived) {
        await command(request, { kind: "archive_thread", thread_id: t.id })
      }
    }
  })
})
