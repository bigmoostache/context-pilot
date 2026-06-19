import { test, expect, type Page, type APIRequestContext } from "@playwright/test"

// ── Phase 2 — message send over the live push plane ──────────────────
//
// Drives a real message through the thread composer and proves the full K7
// path end-to-end: the web composer → POST /command {send_message} → agent
// bridge injects the user ThreadMessage + flips MY_TURN → emit_messages appends
// a MessageCreated oplog delta → SSE → applyThreadDelta appends to the log.
//
// We assert the *user's* message specifically (the bubble appears in the
// conversation AND the backend roster's msg_count for the thread increments).
// We deliberately do NOT assert the agent's reply: that is a live, non-
// deterministic LLM stream. Proving the user message lands live is the wiring
// contract under test.
//
// Hygiene: the throwaway `e2e-msg-` thread is archived in afterAll.

const AGENT_ID = "f3a993c0ff357b41"
const API = process.env.CP_API_URL ?? "http://localhost:7878"

async function command(req: APIRequestContext, kind: Record<string, unknown>): Promise<void> {
  const token = `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`
  const res = await req.post(`${API}/api/agent/${AGENT_ID}/command`, {
    data: { schema_version: 1, id: token, seq: 0, dedup_token: token, kind },
  })
  expect(res.ok(), `command ${JSON.stringify(kind)} accepted`).toBeTruthy()
}

interface RawThread {
  id: string
  name: string
  archived?: boolean
  log?: unknown[]
}

async function rawThreads(req: APIRequestContext): Promise<RawThread[]> {
  const res = await req.get(`${API}/api/agent/${AGENT_ID}/threads`)
  expect(res.ok()).toBeTruthy()
  const raw = await res.json()
  return Array.isArray(raw) ? raw : raw.threads ?? []
}

/** Wait until `name` exists in the roster (non-archived) and return its id. */
async function awaitThreadId(req: APIRequestContext, name: string): Promise<string> {
  let id = ""
  await expect
    .poll(
      async () => {
        const hit = (await rawThreads(req)).find((t) => t.name === name && !t.archived)
        id = hit?.id ?? ""
        return !!hit
      },
      { timeout: 15_000 },
    )
    .toBe(true)
  return id
}

/** The message-log length the backend reports for a thread. */
async function logLen(req: APIRequestContext, id: string): Promise<number> {
  const t = (await rawThreads(req)).find((x) => x.id === id)
  return t?.log?.length ?? 0
}

async function openThreads(page: Page) {
  await page.goto("/")
  await page.getByRole("button", { name: /Open/i }).first().click()
  await expect(page.getByRole("button", { name: /New Thread/i })).toBeVisible()
}

test.describe("messages / send over the push plane", () => {
  test("composer send → user bubble appears live + roster log grows", async ({ page }) => {
    // Precondition: a fresh thread to send into.
    const NAME = `e2e-msg-${Date.now()}`
    await command(page.request, { kind: "create_thread", name: NAME })
    const id = await awaitThreadId(page.request, NAME)
    const before = await logLen(page.request, id)

    await openThreads(page)

    // Select the thread.
    await page.getByPlaceholder(/Search threads/i).fill(NAME)
    await page.getByText(NAME, { exact: true }).click()

    // The composer is the thread reply box.
    const composer = page.getByPlaceholder(/Reply to this thread/i)
    await expect(composer).toBeVisible()

    const MSG = `hello from e2e ${Date.now()}`
    await composer.fill(MSG)
    await composer.press("Enter")

    // Composer clears after send (matches the TUI clearing its input).
    await expect(composer).toHaveValue("", { timeout: 10_000 })

    // The user's message bubble appears in the conversation (push plane).
    // Generous timeout for the same reason the roster-log poll below is: the
    // suite drives a single LIVE agent that may be backlogged by earlier specs'
    // command preconditions, so the user-message apply (and its SSE delta) can
    // trail the action.
    await expect(page.getByText(MSG, { exact: false }).first()).toBeVisible({ timeout: 25_000 })

    // Ground truth: the backend roster's message log grew by at least one.
    // Generous timeout: the suite drives a single LIVE agent, so by this test
    // its bridge command queue may carry backlog from earlier specs — the
    // user-message apply (and thus the log bump) can trail the DOM bubble.
    await expect
      .poll(async () => logLen(page.request, id), { timeout: 25_000 })
      .toBeGreaterThan(before)
  })

  // NOTE — the T123 "no flicker" regression (a just-sent message must SURVIVE a
  // poll cycle, never appear→disappear→reappear) is proven DETERMINISTICALLY by
  // `web/repro_t123_flicker.py`, which drives the exact data layer the hook
  // consumes and asserts the MERGE model (the shipped `mergeThreadLogs`
  // reconciler) never drops a delta-applied message — while showing the old
  // REPLACE model reproducing the full flicker. A browser-level survival test
  // was deliberately NOT added here: it would have to send a fresh message and
  // then hold across a 5 s poll, racing the SINGLE shared live agent this suite
  // drives (which is also serving the human's own session), making it flaky for
  // a property already deterministically nailed by the repro.

  test.afterAll(async ({ request }) => {
    for (const t of await rawThreads(request)) {
      if (t.name.startsWith("e2e-") && !t.archived) {
        await command(request, { kind: "archive_thread", thread_id: t.id })
      }
    }
  })
})
