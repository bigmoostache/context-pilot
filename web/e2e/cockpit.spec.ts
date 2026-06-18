import { test, expect, type APIRequestContext, type Page } from "@playwright/test"

// ── Phase 10 — Cockpit panels over the live inspection plane ─────────
//
// The cockpit's LeftRail lists the agent's context panels (from
// `/api/agent/{id}/panels`); selecting one renders its bespoke view in the
// center PanelPane. This spec proves two contracts against the LIVE stack
// (web :5175 → orchestrator :7878 → this agent), no mocks, DOM + REST
// ground-truth:
//
//   1. **Working inspection panels render real backend data.** Memories and
//      the WIP/Todo panel read genuine tier-② state (shared/memories.yaml,
//      the worker's todo module) — clicking them shows the real panel, never
//      the "unavailable" notice.
//   2. **Derived-state panels degrade honestly.** Tools, Context Radar, and
//      Entities read state the read-only inspection plane structurally cannot
//      rebuild from tier-② files (compiled tool catalog / live log-ranking /
//      open SQLite connection). Their endpoints return an empty shape by
//      design (a deliberate 200, not a 404), and the panel renders an explicit
//      "Unavailable over the web inspection plane" notice instead of a blank
//      list that would read as "nothing exists".
//
// A 5th request-level test pins the backend contract directly: the three
// derived endpoints answer 200 (route exists) rather than the 404 they used
// to (no handler/route was registered before this batch).

const AGENT_ID = "f3a993c0ff357b41"
const API = process.env.CP_API_URL ?? "http://localhost:7878"

const UNAVAILABLE = /Unavailable over the web inspection plane/i

/** Open the agent and switch to its Cockpit view, waiting for the LeftRail. */
async function openCockpit(page: Page) {
  await page.goto("/")
  await page.getByRole("button", { name: /Open/i }).first().click()
  // Threads view is the landing surface — its New Thread button anchors it.
  await expect(page.getByRole("button", { name: /New Thread/i })).toBeVisible()
  // The per-agent view switcher carries a "Cockpit" tab (exact: Threads/Finder
  // also match a loose /C/).
  await page.getByRole("button", { name: "Cockpit", exact: true }).click()
  // The LeftRail panel navigator anchors a loaded cockpit (the "Panels" label).
  await expect(page.getByText("Panels", { exact: true })).toBeVisible({ timeout: 10_000 })
}

/** Click a LeftRail panel button by its backend `name` label.
 *
 * The button's accessible name concatenates the panel name with its token
 * count (e.g. "Memories 6.3k"), so we match the name as a substring and scope
 * to the LeftRail `aside` to avoid colliding with chrome elsewhere. */
async function selectPanel(page: Page, name: string) {
  await page.locator("aside").getByRole("button", { name }).first().click()
}

test.describe("cockpit / inspection panels over the live plane", () => {
  test("a working inspection panel (Memories) renders live data, no notice", async ({
    page,
    request,
  }) => {
    // Ground truth: the agent genuinely has memories on the inspection plane.
    const mem = await (await request.get(`${API}/api/agent/${AGENT_ID}/memory`)).json()
    const memCount = Array.isArray(mem) ? mem.length : Object.keys(mem).length
    expect(memCount, "agent has memories to display").toBeGreaterThan(0)

    await openCockpit(page)
    await selectPanel(page, "Memories")

    // The MemoryPanel frame header renders, and crucially the unavailable
    // notice is ABSENT — this is real data, not a degraded panel.
    await expect(page.getByText("Memories").first()).toBeVisible()
    await expect(page.getByText(UNAVAILABLE)).toHaveCount(0)
  })

  test("a working inspection panel (WIP/Todo) renders live data, no notice", async ({
    page,
    request,
  }) => {
    const todosRaw = await (await request.get(`${API}/api/agent/${AGENT_ID}/todos`)).json()
    const todos = Array.isArray(todosRaw) ? todosRaw : todosRaw.todos ?? []
    expect(todos.length, "agent has todos to display").toBeGreaterThan(0)

    await openCockpit(page)
    // The todo-kind panel's backend name is "WIP"; PanelPane renders TodoPanel
    // whose frame header is "Todo List".
    await selectPanel(page, "WIP")
    await expect(page.getByText("Todo List").first()).toBeVisible()
    await expect(page.getByText(UNAVAILABLE)).toHaveCount(0)
  })

  test("the Tools panel shows the honest unavailable notice", async ({ page }) => {
    await openCockpit(page)
    // The tools-kind panel's backend name is "Configuration".
    await selectPanel(page, "Configuration")
    const note = page.getByRole("note")
    await expect(note).toBeVisible({ timeout: 10_000 })
    await expect(note).toContainText(UNAVAILABLE)
    await expect(note).toContainText(/tool catalog/i)
  })

  test("the Context Radar panel shows the honest unavailable notice", async ({ page }) => {
    await openCockpit(page)
    await selectPanel(page, "Context Radar")
    const note = page.getByRole("note")
    await expect(note).toBeVisible({ timeout: 10_000 })
    await expect(note).toContainText(UNAVAILABLE)
    await expect(note).toContainText(/half-life ranking/i)
  })

  test("derived endpoints answer 200 with an empty shape (route exists, not 404)", async ({
    request,
  }: {
    request: APIRequestContext
  }) => {
    // tools / entities → empty array; radar → {anchors:[],results:[]}. A known
    // agent resolves (200 empty), an unknown agent still 404s.
    const tools = await request.get(`${API}/api/agent/${AGENT_ID}/tools`)
    expect(tools.status()).toBe(200)
    expect(await tools.json()).toEqual([])

    const entities = await request.get(`${API}/api/agent/${AGENT_ID}/entities`)
    expect(entities.status()).toBe(200)
    expect(await entities.json()).toEqual([])

    const radar = await request.get(`${API}/api/agent/${AGENT_ID}/radar`)
    expect(radar.status()).toBe(200)
    expect(await radar.json()).toEqual({ anchors: [], results: [] })

    const unknown = await request.get(`${API}/api/agent/nope/tools`)
    expect(unknown.status()).toBe(404)
  })
})
