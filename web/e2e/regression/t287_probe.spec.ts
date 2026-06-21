import { test, expect, type Page } from "@playwright/test"

// ── T287 probe — synthetic Miller-columns drag-and-drop ──────────────
// Throwaway: drives the columns view, dispatches a real HTML5 drag sequence
// (one shared DataTransfer across dragstart→dragover→drop) from a file row onto
// a sibling folder row in a parent column, and checks the backend listing moved.

const AGENT_ID = process.env.CP_AGENT_ID ?? "834eb459c7b064db"
const API = process.env.CP_API_URL ?? "http://localhost:7878"

async function openFinder(page: Page) {
  // Boot straight into THIS agent's Finder (deterministic — the fleet has
  // several agents and the first "Open" card isn't guaranteed to be ours).
  await page.addInitScript(
    ([id]) => {
      localStorage.setItem("cp-agent", id as string)
      localStorage.setItem("cp-view", "finder")
    },
    [AGENT_ID],
  )
  await page.goto("/")
  await expect(page.getByText(/\d+ items/).first()).toBeVisible({ timeout: 15_000 })
}

test("miller columns DnD moves a file into a sibling folder", async ({ page }) => {
  await openFinder(page)

  // Navigate into t287, then into srcdir (deepest column = srcdir children).
  await page.locator(`[data-finder-item][data-path="t287"]`).dblclick()
  await expect(page.getByText(/\d+ items/).first()).toBeVisible()
  // Switch to columns view (3rd seg button).
  const seg = page.locator("div.relative.rounded-lg.border.bg-muted\\/60").first()
  await seg.getByRole("button").nth(2).click()
  await expect(page.getByText(/columns view/i)).toBeVisible({ timeout: 10_000 })

  // In columns, click srcdir to navigate (adds its column). The row lives in a
  // MillerColumn (no data-finder-item there — match by text).
  await page.getByText("srcdir", { exact: true }).first().click()
  // Now a column should show dragme.txt; a parent column shows destdir.
  await expect(page.getByText("dragme.txt").first()).toBeVisible({ timeout: 10_000 })

  // Dispatch a synthetic drag: dragstart on the file, dragover+drop on destdir.
  const result = await page.evaluate(() => {
    const rows = Array.from(document.querySelectorAll("button"))
    const file = rows.find((b) => b.textContent?.includes("dragme.txt"))
    const dest = rows.find(
      (b) => b.textContent?.includes("destdir") && !b.textContent?.includes("dragme"),
    )
    if (!file || !dest) return { ok: false, reason: `file=${!!file} dest=${!!dest}` }
    const dt = new DataTransfer()
    const fire = (el: Element, type: string) => {
      const ev = new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer: dt })
      el.dispatchEvent(ev)
    }
    fire(file, "dragstart")
    fire(dest, "dragenter")
    fire(dest, "dragover")
    fire(dest, "drop")
    fire(file, "dragend")
    return { ok: true, types: Array.from(dt.types) }
  })
  console.log("[probe] dispatch:", JSON.stringify(result))

  // Give the move mutation + invalidation a moment, then check the backend.
  await page.waitForTimeout(1500)
  const destListing = await page.request
    .get(`${API}/api/agent/${AGENT_ID}/fs?path=${encodeURIComponent("t287/destdir")}`)
    .then((r) => r.json())
  console.log("[probe] destdir now:", JSON.stringify(destListing))
  const moved = (destListing as Array<{ name: string }>).some((n) => n.name === "dragme.txt")
  expect(moved, "dragme.txt landed in destdir").toBeTruthy()
})
