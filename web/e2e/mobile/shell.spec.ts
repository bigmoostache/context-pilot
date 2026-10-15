import { test, expect } from "@playwright/test"

// ── Mobile tree smoke (phone project only) ───────────────────────────
//
// Proves the device switch picks the MOBILE component tree on a phone
// viewport and that the shell fits the screen. Live stack, no mocks (same
// harness contract as smoke.spec.ts).

test.describe("mobile / shell", () => {
  test("phone viewport loads the mobile tree", async ({ page }) => {
    await page.goto("/")
    // mobile-components/Root tags <html class="mobile"> on mount.
    await expect(page.locator("html")).toHaveClass(/\bmobile\b/)
  })

  test("shell never scrolls horizontally", async ({ page }) => {
    await page.goto("/")
    await expect(page.locator("html")).toHaveClass(/\bmobile\b/)
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - window.innerWidth,
    )
    expect(overflow, "page wider than the phone screen").toBeLessThanOrEqual(0)
  })

  test("text inputs are >=16px so iOS does not zoom on focus", async ({ page }) => {
    await page.goto("/")
    await expect(page.locator("html")).toHaveClass(/\bmobile\b/)
    const small = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>("textarea, input[type=text], input:not([type])")]
        .filter((el) => el.offsetParent !== null)
        .filter((el) => parseFloat(getComputedStyle(el).fontSize) < 16)
        .map((el) => el.outerHTML.slice(0, 80)),
    )
    expect(small).toEqual([])
  })
})
