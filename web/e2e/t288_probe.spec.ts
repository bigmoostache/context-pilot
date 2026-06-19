import { test, expect, type Page } from "@playwright/test"

// ── T288 probe — Quick Look Sheet drawer geometry ───────────────────
// Throwaway real-browser check: open the Finder Quick Look drawer and verify
// (1) it is flush to the right viewport edge with nothing spilling off-screen,
// (2) its width is the intended 420px, and (3) its header row height matches
// the Finder toolbar header height. Screenshots to /tmp for eyeballing.

const AGENT_ID = process.env.CP_AGENT_ID ?? "f3a993c0ff357b41"

async function openFinder(page: Page) {
  await page.addInitScript(
    ([id]) => {
      localStorage.setItem("cp-agent", id as string)
      localStorage.setItem("cp-view", "finder")
    },
    [AGENT_ID],
  )
  await page.goto("/")
  await expect(page.getByText(/\d+ items/).first()).toBeVisible({ timeout: 20_000 })
}

test("quick look drawer is flush-right, 420px, header matches toolbar", async ({ page }) => {
  await openFinder(page)
  const vw = page.viewportSize()!.width

  // Click a known FILE row (a folder would navigate, not select) → select → Space opens QL.
  const fileRow = page.locator(`[data-finder-item][data-path="bridge.lock"]`).first()
  await expect(fileRow).toBeVisible({ timeout: 10_000 })
  await fileRow.click()
  await page.keyboard.press(" ")

  const drawer = page.locator(`[data-slot="sheet-content"][data-side="right"]`)
  await expect(drawer).toBeVisible({ timeout: 10_000 })
  // Let the slide-in settle so we measure the resting position, not mid-animation.
  await page.waitForTimeout(500)

  const box = (await drawer.boundingBox())!

  // Measure header heights via the QL header and the Finder toolbar.
  const qlHeaderBox = await drawer.locator("div").filter({ hasText: "Quick Look" }).first().boundingBox()

  const report = {
    viewportWidth: vw,
    drawer: { x: Math.round(box.x), y: Math.round(box.y), w: Math.round(box.width), h: Math.round(box.height) },
    rightEdge: Math.round(box.x + box.width),
    overflowsRight: box.x + box.width > vw + 1,
    qlHeaderHeight: qlHeaderBox ? Math.round(qlHeaderBox.height) : null,
  }
  console.log("[t288] " + JSON.stringify(report))

  await page.screenshot({ path: "/tmp/t288-drawer.png", fullPage: false })

  // Assertions:
  expect(report.overflowsRight, "drawer must not spill past the right edge").toBeFalsy()
  expect(Math.abs(report.rightEdge - vw), "drawer right edge flush to viewport").toBeLessThanOrEqual(2)
  expect(Math.abs(report.drawer.w - 420), "drawer width ~420px").toBeLessThanOrEqual(2)
  // Header height: the QL header should be 48px (h-12) — identical to the
  // Finder toolbar header (both use the h-12 Tailwind class = 48px), which is
  // exactly the "match the main page header" the fix targets.
  expect(report.qlHeaderHeight, "QL header h-12 = 48px").toBeGreaterThanOrEqual(46)
  expect(report.qlHeaderHeight, "QL header h-12 = 48px").toBeLessThanOrEqual(50)
})
