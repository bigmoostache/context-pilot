import { test, expect, type APIRequestContext } from "@playwright/test"

// ── T123 REAL-BROWSER flicker probe ──────────────────────────────────
//
// Runs the ACTUAL web hook (applyThreadDelta + mergeThreadLogs + 5s poll) in a
// REAL Chromium against the live web/ app (:5175). Sends a message and samples
// the bubble's visibility every ~120ms for ~12s, printing a PRESENT/ABSENT
// timeline + a verdict. This is the faithful reproduction: if the bubble
// flickers (appear→disappear→reappear) here, the fix is insufficient and the
// timeline is the artifact to debug against.

const AGENT_ID = "f3a993c0ff357b41"
const API = process.env.CP_API_URL ?? "http://localhost:7878"

async function command(req: APIRequestContext, kind: Record<string, unknown>): Promise<void> {
  const token = `probe-${Date.now()}-${Math.random().toString(36).slice(2)}`
  const res = await req.post(`${API}/api/agent/${AGENT_ID}/command`, {
    data: { schema_version: 1, id: token, seq: 0, dedup_token: token, kind },
  })
  expect(res.ok()).toBeTruthy()
}

interface RawThread { id: string; name: string; archived?: boolean }

async function awaitThreadId(req: APIRequestContext, name: string): Promise<string> {
  let id = ""
  await expect
    .poll(async () => {
      const res = await req.get(`${API}/api/agent/${AGENT_ID}/threads`)
      const raw = await res.json()
      const list: RawThread[] = Array.isArray(raw) ? raw : raw.threads ?? []
      const hit = list.find((t) => t.name === name && !t.archived)
      id = hit?.id ?? ""
      return !!hit
    }, { timeout: 15_000 })
    .toBe(true)
  return id
}

test("T123 real-browser flicker timeline", async ({ page }) => {
  test.setTimeout(60_000)

  // surface browser console (so temporary live.ts logging shows up here),
  // prefixed with ms since send so we can locate WHEN each delta arrives.
  const clock = { sendMs: Date.now() }
  page.on("console", (m) => console.log(`[+${String(Date.now() - clock.sendMs).padStart(6)}ms][browser:${m.type()}] ${m.text()}`))
  page.on("pageerror", (e) => console.log(`[browser:pageerror] ${e.message}`))

  const NAME = `probe-${Date.now()}`
  await command(page.request, { kind: "create_thread", name: NAME })
  await awaitThreadId(page.request, NAME)

  await page.goto("/")
  await page.getByRole("button", { name: /Open/i }).first().click()
  await expect(page.getByRole("button", { name: /New Thread/i })).toBeVisible()

  await page.getByPlaceholder(/Search threads/i).fill(NAME)
  await page.getByText(NAME, { exact: true }).click()

  const composer = page.getByPlaceholder(/Reply to this thread/i)
  await expect(composer).toBeVisible()

  const MSG = `flicker-probe-${Date.now()}`
  await composer.fill(MSG)
  const sendMs = Date.now()
  clock.sendMs = sendMs
  await composer.press("Enter")

  const bubble = page.getByText(MSG, { exact: false }).first()

  // Sample visibility for ~14s (covers >2 poll cycles at 5s).
  const samples: { t: number; v: boolean }[] = []
  const deadline = Date.now() + 14_000
  while (Date.now() < deadline) {
    const v = await bubble.isVisible().catch(() => false)
    samples.push({ t: Date.now() - sendMs, v })
    await page.waitForTimeout(120)
  }

  // Print transitions only.
  console.log("\n── visibility transitions (t = ms since send) ──")
  let last: boolean | null = null
  let firstPresent: number | null = null
  for (const s of samples) {
    if (s.v !== last) {
      console.log(`  [${String(s.t).padStart(6)}ms] -> ${s.v ? "PRESENT" : "ABSENT "}`)
      last = s.v
      if (s.v && firstPresent === null) firstPresent = s.t
    }
  }

  const seq = samples.map((s) => s.v)
  const appearThenDisappear = seq.some((v, i) => i > 0 && seq[i - 1] && !v)
  const everPresent = seq.some((v) => v)
  console.log("\n══ VERDICT ══")
  console.log("ever appeared :", everPresent, firstPresent !== null ? `(first @ ${firstPresent}ms)` : "")
  console.log("flickered     :", appearThenDisappear ? "❌ YES — appeared then disappeared" : "✅ NO — stayed once shown")

  // hygiene
  await command(page.request, { kind: "archive_thread", thread_id: await awaitThreadId(page.request, NAME) })
})
