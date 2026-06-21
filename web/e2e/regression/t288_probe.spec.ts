import { test, expect, type Page } from "@playwright/test"

// ── T288 probe — Quick Look as a real shadcn Sheet drawer ───────────
// Real-browser check that the Finder Quick Look is a STANDARD modal shadcn
// drawer (the user's ask): it slides in from the right, is wider (~540px) and
// flush to the viewport edge, brings a dimming BACKDROP (the signature trait
// that was missing while it was non-modal), and closes on BOTH Esc and a
// click on the backdrop. Screenshots to /tmp for eyeballing.

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

/** Open the Quick Look drawer: click a known file row, then Space toggles it. */
async function openQuickLook(page: Page) {
  const fileRow = page.locator(`[data-finder-item][data-path="bridge.lock"]`).first()
  await expect(fileRow).toBeVisible({ timeout: 10_000 })
  await fileRow.click()
  await page.keyboard.press(" ")
  const drawer = page.locator(`[data-slot="sheet-content"][data-side="right"]`)
  await expect(drawer).toBeVisible({ timeout: 10_000 })
  await page.waitForTimeout(450) // let the slide-in settle before measuring
  return drawer
}

test("quick look is a modal shadcn drawer: backdrop, ~2/3 width, flush-right", async ({ page }) => {
  await openFinder(page)
  const vw = page.viewportSize()!.width
  const drawer = await openQuickLook(page)

  // 1. The dimming backdrop — the signature shadcn drawer trait that proves
  //    the sheet is modal (it was absent while non-modal).
  const overlay = page.locator(`[data-slot="sheet-overlay"]`)
  await expect(overlay, "modal backdrop is rendered").toBeVisible()

  // 2. Geometry: flush to the right edge, ~2/3 of the viewport wide, nothing
  //    off-screen.
  const box = (await drawer.boundingBox())!
  const expectedW = Math.round((vw * 2) / 3)
  const report = {
    viewportWidth: vw,
    drawer: { x: Math.round(box.x), w: Math.round(box.width) },
    expectedWidth: expectedW,
    rightEdge: Math.round(box.x + box.width),
    overflowsRight: box.x + box.width > vw + 1,
  }
  console.log("[t288] " + JSON.stringify(report))
  await page.screenshot({ path: "/tmp/t288-drawer.png", fullPage: false })

  expect(report.overflowsRight, "drawer must not spill past the right edge").toBeFalsy()
  expect(Math.abs(report.rightEdge - vw), "drawer flush to viewport").toBeLessThanOrEqual(2)
  expect(Math.abs(report.drawer.w - expectedW), "drawer width ~2/3 of viewport").toBeLessThanOrEqual(4)
})

test("Esc closes the drawer", async ({ page }) => {
  await openFinder(page)
  const drawer = await openQuickLook(page)
  await page.keyboard.press("Escape")
  await expect(drawer, "Esc dismisses the drawer").toBeHidden({ timeout: 5_000 })
})

test("clicking the backdrop closes the drawer", async ({ page }) => {
  await openFinder(page)
  const drawer = await openQuickLook(page)
  // Click well away from the drawer (left edge) — on the backdrop.
  await page.mouse.click(40, 300)
  await expect(drawer, "click-outside dismisses the drawer").toBeHidden({ timeout: 5_000 })
})
