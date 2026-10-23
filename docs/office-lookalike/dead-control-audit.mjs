// Dead-control audit: clicks every ribbon control on every tab and reports
// controls whose click changes neither the document DOM nor any UI state.
// Usage: node audit.mjs <url>   (default Word harness)
import { chromium } from "/Users/guillaume/context-pilot/web/node_modules/@playwright/test/index.mjs"
const url = process.argv[2] || "http://127.0.0.1:5197/?f=demo.docx&word"
const b = await chromium.launch({ channel: "chrome", headless: true })
const p = await b.newPage({ viewport: { width: 1440, height: 900 } })
const errs = []
p.on("pageerror", (e) => errs.push(e.message))
p.on("dialog", (d) => d.dismiss())
await p.goto(url)
await p.getByRole("tab", { name: "Home" }).waitFor({ timeout: 30000 })
await p.waitForTimeout(3000)
const snap = () =>
  p.evaluate(() => {
    const body = document.querySelector(".oox-root")
    const ui = [...document.querySelectorAll("[aria-pressed],[aria-expanded],[aria-checked],[role=dialog],[role=menu],[role=listbox]")]
      .map((e) => e.tagName + (e.getAttribute("aria-label") || "") + e.getAttribute("aria-pressed") + e.getAttribute("aria-expanded"))
      .join("|")
    const s = (body?.innerHTML || "") + "§" + ui + "§" + document.querySelectorAll("body *").length
    let h = 0
    for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0
    return h
  })
const select = async () => {
  await p.mouse.dblclick(700, 420)
  await p.waitForTimeout(200)
}
// Close whatever the click opened (menus, engine dialogs, backstage).
const dismiss = async () => {
  for (let k = 0; k < 3; k++) {
    await p.keyboard.press("Escape")
    await p.waitForTimeout(100)
    const close = p.getByRole("button", { name: /^(Cancel|Close|Annuler|Fermer|Back)$/i })
    if (await close.count()) await close.first().click({ timeout: 1000 }).catch(() => {})
  }
}
const tabs = await p.getByRole("tab").allInnerTexts()
const rows = []
for (const tab of tabs.filter((t) => t !== "File")) {
  await p.getByRole("tab", { name: tab }).click()
  await p.waitForTimeout(300)
  const n = await p.locator("[role=toolbar] button, [role=toolbar] [role=combobox]").count()
  for (let i = 0; i < n; i++) {
    await p.getByRole("tab", { name: tab }).click({ timeout: 3000 }).catch(async () => {
      rows.push([tab, "(tab blocked by overlay)", "STUCK"])
      console.log(rows.at(-1).join(" | "))
      await dismiss()
    })
    await select()
    const el = p.locator("[role=toolbar] button, [role=toolbar] [role=combobox]").nth(i)
    const label = (await el.getAttribute("aria-label")) || (await el.innerText()).trim()
    if (await el.isDisabled()) { rows.push([tab, label, "DISABLED"]); console.log(rows.at(-1).join(" | ")); continue }
    const before = await snap()
    const isCombo = (await el.getAttribute("role")) === "combobox"
    try {
      if (isCombo) { await el.click(); await p.keyboard.press("ArrowDown") } else await el.click({ timeout: 3000 })
    } catch (e) { rows.push([tab, label, "CLICK_FAIL"]); continue }
    await p.waitForTimeout(400)
    const after = await snap()
    rows.push([tab, label, before === after ? "DEAD" : "ok"])
    console.log(rows.at(-1).join(" | "))
    await dismiss()
  }
}
console.log("SUMMARY", JSON.stringify(rows.reduce((a, r) => ((a[r[2]] = (a[r[2]] || 0) + 1), a), {})))
console.log("ERRS", JSON.stringify(errs.slice(0, 5)))
await b.close()
console.log("AUDIT_END")
