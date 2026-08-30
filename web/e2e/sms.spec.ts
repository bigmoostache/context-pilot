import { test, expect, type Page } from "@playwright/test"

// ── SMS panel (design-sms.md §13.6) ──────────────────────────────────
//
// Drives the LIVE stack like every other spec here (web :5175 → orchestrator
// :7878), with ONE thing stubbed at the network boundary: `status.sms` on
// `GET /api/it/network`. That field is the whole "SMS only on 5G Photonicats"
// promise, and no dev box can produce both of its values — so it is the one
// place a `page.route` earns its keep. Everything else in the response is the
// box's real answer, patched in flight rather than invented.
//
// NOTE ON ENFORCEMENT: nothing runs this file on a push. The TS-TESTS CI family
// is a documented no-op (`.github/checks/check-ts-tests.sh`) because this suite
// needs the live stack up. It is run by hand — `cd web && npx playwright test
// e2e/sms.spec.ts` — and the comments in `ItSmsPane` say exactly that.
//
// Assumes an admin-capable session (access control off, or the signed-in
// account is admin+), the same assumption `update.spec.ts` and the rest of the
// IT surface make. The third test below stubs its OWN session on purpose.

/** How long the modem-less probe is held open. Long enough that a version
 *  rendering a loading frame would be caught mid-frame by the polling assertion
 *  below — a route that resolves instantly cannot tell the two apart, which is
 *  the whole reason this constant is not zero. */
const HOLD_MS = 1_000

/** Two archived messages, newest first, in the shape `ItSmsList` promises. The
 *  first is unread — the row that must carry an accessible "Unread", not just a
 *  coloured dot. */
const MESSAGES = [
  {
    id: 42,
    peer: "+33612345678",
    body: "Colis livre au point relais, code 8891.",
    direction: "received",
    delivery: "received",
    read: false,
    sent_at: null,
    ingested_at: Math.floor(Date.now() / 1000) - 120,
    error: null,
    sent_by: null,
  },
  {
    id: 41,
    peer: "Bouygues",
    body: "Votre forfait a ete renouvele.",
    direction: "received",
    delivery: "received",
    read: true,
    sent_at: Math.floor(Date.now() / 1000) - 4000,
    ingested_at: Math.floor(Date.now() / 1000) - 3600,
    error: null,
    sent_by: null,
  },
]

/**
 * Answer `GET /api/it/network` with the box's REAL response, its `status.sms`
 * replaced by `sms`. `hold` delays only the first answer, so the poll that
 * follows stays fast.
 */
async function stubSmsStatus(page: Page, sms: unknown, hold = 0): Promise<void> {
  let first = true
  await page.route("**/api/it/network", async (route) => {
    const response = await route.fetch()
    const body = await response.json()
    body.status.sms = sms
    if (first && hold > 0) {
      first = false
      await new Promise((resolve) => setTimeout(resolve, hold))
    }
    await route.fulfill({ response, json: body })
  })
}

/** Open Settings (avatar menu → Settings) and select the IT category. */
async function openItPane(page: Page): Promise<void> {
  await page.goto("/")
  await page.getByLabel("Account menu").click()
  await page.getByRole("menuitem", { name: "Settings" }).click()
  await page.getByRole("button", { name: "IT", exact: true }).click()
}

test.describe("IT · SMS panel", () => {
  test("a box that cannot do SMS shows no panel — throughout the probe, not just after it", async ({
    page,
  }) => {
    await stubSmsStatus(page, null, HOLD_MS)
    await openItPane(page)

    const panel = page.getByTestId("it-sms")

    // The load-bearing assertion: absent at EVERY instant while the read is in
    // flight. A frame rendered for the duration of the request would put an SMS
    // panel in a modem-less box's DOM, which is precisely the promise
    // ("uniquement les Photonicat 5G") this test guards.
    const until = Date.now() + HOLD_MS
    while (Date.now() < until) {
      expect(await panel.count()).toBe(0)
      await page.waitForTimeout(50)
    }

    // …and the held read really did land, so "absent" above is not vacuous:
    // "Wi-Fi access point" is rendered only by the uplink pane's LOADED state,
    // which shares this very query.
    await expect(page.getByText("Wi-Fi access point")).toBeVisible()
    await expect(panel).toHaveCount(0)
  })

  test("a box with a modem lists its archive, and marks a message read on sight, once", async ({
    page,
  }) => {
    await stubSmsStatus(page, { available: true, unread: 1 })
    // The COLLECTION url only — the regex stops at `sms` or `sms?…`, so the
    // per-message `…/sms/{id}/read` below is a separate route and the count it
    // keeps cannot be polluted by an archive refetch.
    await page.route(/\/api\/it\/sms(\?|$)/, (route) =>
      route.fulfill({ json: { messages: MESSAGES } }),
    )

    let reads = 0
    await page.route("**/api/it/sms/*/read", (route) => {
      reads += 1
      return route.fulfill({ json: { ok: true } })
    })

    await openItPane(page)

    const panel = page.getByTestId("it-sms")
    await expect(panel).toBeVisible()
    // The badge comes from `status.sms.unread` (the 5 s network poll), never
    // from the archive query — the two halves must agree.
    await expect(panel.getByText("1 unread")).toBeVisible()
    await expect(panel.getByText(MESSAGES[0].body)).toBeVisible()
    await expect(panel.getByText("Bouygues")).toBeVisible()
    // The compose half is there too.
    await expect(panel.getByPlaceholder("Type your message")).toBeVisible()

    // The unread cue is a WORD, not a dot: a 6px dot beside a monospace number
    // read as a rendering artefact, and it needed an `sr-only` twin to mean
    // anything to a screen reader. Exactly one row is unread.
    await expect(panel.getByText("New", { exact: true })).toHaveCount(1)

    // Review C4: a pristine compose form accuses nobody of anything.
    await expect(panel.getByText(/number must be digits/)).toHaveCount(0)

    // "Received" is not repeated under every inbound row — `From` already said
    // it. The delivery state is rendered for OUTBOUND messages only.
    await expect(panel.getByText("Received", { exact: true })).toHaveCount(0)

    // A message is marked read by BEING ON SCREEN, not by being clicked. The
    // click used to carry it, and that left the badge unclearable for an
    // operator who simply read the message — which is the entire normal case.
    //
    // The scroll is load-bearing, not ceremony: the SMS block sits last in the
    // IT page, roughly 2000px down, so it is BELOW THE FOLD on arrival. Nothing
    // may be marked read while it is off screen — asserting `reads` without
    // scrolling first is how this test discovered that the panel is not visible
    // when the category opens.
    expect(reads, "nothing is marked read while the panel is below the fold").toBe(0)
    await panel.scrollIntoViewIfNeeded()
    await expect.poll(() => reads).toBe(1)

    // …and exactly once. The stubbed archive never flips `read`, so the row
    // stays "unread" forever — the widest possible version of the window in
    // which a second `POST …/read` would fire, which the server answers 404.
    // Clicking still toggles the row's actions, and must not re-fire it.
    const row = panel.getByText(MESSAGES[0].body)
    await row.click()
    await expect(panel.getByRole("button", { name: /Remove this message/ })).toBeVisible()
    await row.click() // collapse
    await expect(panel.getByRole("button", { name: /Remove this message/ })).toHaveCount(0)
    await row.click() // and re-open, inside the window
    await expect(panel.getByRole("button", { name: /Remove this message/ })).toBeVisible()
    expect(reads, "mark-read is fired once per message, never twice").toBe(1)
  })

  test("a role without can_manage_it gets neither the IT category nor the panel", async ({
    page,
  }) => {
    // A stubbed session, not a real one: the box under test runs with access
    // control off, so there is no `user`-role account to log into. What is being
    // asserted is the CLIENT gate in `ConfigPanel` (`adminOnly` on the IT
    // category); the server half is asserted in Rust, where a `user` gets a 403
    // on every `/api/it/*` route regardless of what the cockpit renders.
    await page.addInitScript(() => localStorage.setItem("cp-auth-token", "e2e-stub-session"))
    await page.route("**/api/auth/status", (route) =>
      route.fulfill({ json: { enabled: true, bootstrapped: true } }),
    )
    await page.route("**/api/auth/me", (route) =>
      route.fulfill({
        json: {
          id: "e2e-plain-user",
          email: "plain@example.test",
          name: "Plain User",
          role: "user",
          must_change_password: false,
          next_action: "ready",
        },
      }),
    )

    await page.goto("/")
    await page.getByLabel("Account menu").click()
    await page.getByRole("menuitem", { name: "Settings" }).click()

    // The rail rendered (so the absence below is a gate, not a blank screen)…
    await expect(page.getByRole("button", { name: "General", exact: true })).toBeVisible()
    // …and it offers no IT category, hence no SMS panel anywhere.
    await expect(page.getByRole("button", { name: "IT", exact: true })).toHaveCount(0)
    await expect(page.getByTestId("it-sms")).toHaveCount(0)
  })
})
