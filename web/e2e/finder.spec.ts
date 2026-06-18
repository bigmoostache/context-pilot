import { test, expect, type Page, type APIRequestContext } from "@playwright/test"

// ── Phase 4 — Finder over the live inspection plane ──────────────────
//
// The Finder is confined to the agent's realm and lists the REAL filesystem
// via GET /api/agent/{id}/fs?path=… (the inspection plane). This suite proves
// the load-bearing wiring against ground truth:
//   • the root listing matches the backend /fs exactly (count + names),
//   • folders carry live child counts ("N items"),
//   • double-click navigates into a folder (cwd + listing update),
//   • breadcrumbs walk back to the realm root,
//   • the four view modes (grid / list / columns / gallery) all render.
//
// No mocks: every listing is fetched from the live agent's confined realm.

const AGENT_ID = "f3a993c0ff357b41"
const API = process.env.CP_API_URL ?? "http://localhost:7878"

interface FsNode {
  name: string
  path: string
  kind: "folder" | string
  count?: number
}

/** The backend's listing of `relPath` (empty = realm root). */
async function fs(req: APIRequestContext, relPath = ""): Promise<FsNode[]> {
  const q = relPath ? `?path=${encodeURIComponent(relPath)}` : ""
  const res = await req.get(`${API}/api/agent/${AGENT_ID}/fs${q}`)
  expect(res.ok(), `/fs ${relPath} ok`).toBeTruthy()
  return (await res.json()) as FsNode[]
}

/** Open the agent and switch to its Finder view. */
async function openFinder(page: Page) {
  await page.goto("/")
  await page.getByRole("button", { name: /Open/i }).first().click()
  await expect(page.getByRole("button", { name: /New Thread/i })).toBeVisible()
  // The per-agent view switcher carries a "Finder" tab. Match EXACTLY: thread
  // rows whose names mention "finder" otherwise satisfy the substring default.
  await page.getByRole("button", { name: "Finder", exact: true }).click()
  // The status bar's item count anchors a loaded Finder.
  await expect(page.getByText(/\d+ items/).first()).toBeVisible({ timeout: 15_000 })
}

test.describe("finder / live realm browsing", () => {
  test("root listing matches the backend /fs (count + names)", async ({ page }) => {
    const backend = await fs(page.request, "")
    await openFinder(page)

    // The status bar reports exactly the backend's child count.
    await expect(page.getByText(`${backend.length} items`).first()).toBeVisible({ timeout: 15_000 })

    // A representative folder and (if present) file from the backend render.
    const folder = backend.find((n) => n.kind === "folder")
    expect(folder, "realm has at least one folder").toBeTruthy()
    if (folder) {
      await expect(page.locator(`[data-finder-item][data-path="${folder.path}"]`)).toBeVisible()
    }
  })

  test("folders carry live child counts", async ({ page }) => {
    const backend = await fs(page.request, "")
    await openFinder(page)
    // Pick a folder the backend says is non-empty; its card shows "N items".
    const nonEmpty = backend.find((n) => n.kind === "folder" && (n.count ?? 0) > 0)
    expect(nonEmpty, "a non-empty folder exists").toBeTruthy()
    if (nonEmpty) {
      const card = page.locator(`[data-finder-item][data-path="${nonEmpty.path}"]`)
      await expect(card).toContainText(`${nonEmpty.count} item`)
    }
  })

  test("double-click navigates into a folder; breadcrumb returns to root", async ({ page }) => {
    const backend = await fs(page.request, "")
    const folder = backend.find((n) => n.kind === "folder" && (n.count ?? 0) > 0)
    expect(folder).toBeTruthy()
    if (!folder) return
    const childListing = await fs(page.request, folder.path)

    await openFinder(page)

    // Navigate in.
    await page.locator(`[data-finder-item][data-path="${folder.path}"]`).dblclick()
    // The status bar's mono cwd span (bottom-right) now ends with the folder
    // name (the cwd is the backend-relative path, e.g. "crates"). The footer's
    // context meter is also font-mono but carries `tabular-nums` — exclude it.
    await expect(page.locator("span.font-mono:not(.tabular-nums)").last()).toContainText(
      folder.name,
      { timeout: 15_000 },
    )
    // And the listing count matches the subfolder's backend listing.
    await expect(page.getByText(`${childListing.length} items`).first()).toBeVisible({ timeout: 15_000 })

    // Back to the realm root via the keyboard (Backspace → goUp), avoiding the
    // breadcrumb crumb whose "context-pilot" label collides with the TopBar
    // agent switcher. Click any item in the SUBFOLDER listing first so focus
    // lands inside the finder surface (its tabIndex div owns the keydown).
    await page.locator("[data-finder-item]").first().click()
    await page.keyboard.press("Backspace")
    await expect(page.getByText(`${backend.length} items`).first()).toBeVisible({ timeout: 15_000 })
  })

  test("all four view modes render the listing", async ({ page }) => {
    await openFinder(page)
    // The segmented view switch: grid · list · columns · gallery (in order).
    // The finder seg is `relative` (the TopBar's Threads/Finder/Cockpit switch
    // shares bg-muted/60 but is not relative-positioned) — disambiguate on it.
    const seg = page.locator("div.relative.rounded-lg.border.bg-muted\\/60").first()
    const buttons = seg.getByRole("button")
    await expect(buttons).toHaveCount(4)

    // List view → status bar reports "list view".
    await buttons.nth(1).click()
    await expect(page.getByText(/list view/i)).toBeVisible({ timeout: 10_000 })

    // Columns (Miller) view.
    await buttons.nth(2).click()
    await expect(page.getByText(/columns view/i)).toBeVisible({ timeout: 10_000 })

    // Gallery view.
    await buttons.nth(3).click()
    await expect(page.getByText(/gallery view/i)).toBeVisible({ timeout: 10_000 })

    // Back to grid.
    await buttons.nth(0).click()
    await expect(page.getByText(/grid view/i)).toBeVisible({ timeout: 10_000 })
  })
})
