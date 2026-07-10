import { test, expect, type Page } from "@playwright/test"

// ── Update pane (O5.2, update-policy §5.9) ───────────────────────────
//
// Drives the LIVE stack like every other spec here (web :5175 →
// orchestrator :7878). Assumes an admin-capable session: either access
// control is off (god-mode) or the signed-in account is admin+ — the same
// assumption the IT-pane surface makes. V5.2a's negative half (a non-admin
// account never sees the pane) is enforced by `CATEGORIES`' `adminOnly` gate
// in ConfigPanel and asserted server-side by `update_routes_rbac` (user →
// 403 on every /api/update/* route).

const API = process.env.CP_API_URL ?? "http://localhost:7878"

/** Open Settings (avatar menu → Settings) and select the Update category. */
async function openUpdatePane(page: Page) {
  await page.goto("/")
  await page.getByLabel("Account menu").click()
  await page.getByRole("menuitem", { name: "Settings" }).click()
  await page.getByRole("button", { name: "Update" }).click()
  await expect(page.getByTestId("update-current")).toBeVisible()
}

test.describe("update pane", () => {
  test("admin sees the pane with live status (V5.2a positive half)", async ({
    page,
    request,
  }) => {
    // The backend status is the source of truth the pane must reflect.
    const status = await (await request.get(`${API}/api/update/status`)).json()
    await openUpdatePane(page)
    await expect(page.getByTestId("update-current")).toContainText(status.current)
    await expect(page.getByTestId("update-availability")).toContainText(
      status.available ? `Update available: ${status.available}` : "Up to date",
    )
    // The three mode options render, with the server's mode selected.
    for (const mode of ["auto", "manual", "paused"]) {
      await expect(page.getByTestId(`update-mode-${mode}`)).toBeVisible()
    }
    await expect(page.getByTestId(`update-mode-${status.mode}`)).toHaveAttribute(
      "aria-pressed",
      "true",
    )
  })

  test("manual toggle persists across reload — server state, not localStorage (V5.2b)", async ({
    page,
    request,
  }) => {
    const before = await (await request.get(`${API}/api/update/status`)).json()
    await openUpdatePane(page)

    await page.getByTestId("update-mode-manual").click()
    await expect(page.getByTestId("update-mode-manual")).toHaveAttribute("aria-pressed", "true")

    // Wipe localStorage before reloading: persistence must come from the box.
    await page.evaluate(() => localStorage.clear())
    await page.reload()
    await openUpdatePane(page)
    await expect(page.getByTestId("update-mode-manual")).toHaveAttribute("aria-pressed", "true")

    // The server agrees — then restore the original mode.
    const after = await (await request.get(`${API}/api/update/status`)).json()
    expect(after.mode).toBe("manual")
    const restore = await request.put(`${API}/api/update/mode`, {
      data: { mode: before.mode },
    })
    expect(restore.ok()).toBeTruthy()
  })

  test("Check now fires /api/update/check and refreshes the status (V5.2c)", async ({
    page,
  }) => {
    await openUpdatePane(page)
    const checkRequest = page.waitForRequest(
      (req) => req.url().includes("/api/update/check") && req.method() === "POST",
    )
    await page.getByTestId("update-check-now").click()
    await checkRequest
    // The pane reflects the refreshed check instant ("never" only before any
    // check; after clicking it must show a real timestamp or a check error).
    await expect(page.getByText(/Last check: (?!never)/)).toBeVisible()
  })
})
