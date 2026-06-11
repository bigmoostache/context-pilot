# `cp-mod-browser` — Browser Navigation Module Design

Status: **DESIGN ONLY — pending captain validation.** No implementation yet.
Date: 2026-06-11

---

## 1. Goal

A first-class Context Pilot module that lets the agent **drive a real browser**
(Chrome / Chromium / Brave — one is enough, Chrome is the target) in **both
headless and headed** modes, to: navigate + interact, extract page content,
maintain authenticated sessions, and capture screenshots.

Hard requirements (from the captain, 2026-06-11):

1. Control a real browser, headless **and** headed. Chrome alone suffices.
2. Easy for an AI to use, and battle-tested.
3. Rust = bonus, granted only if #1 and #2 are green.
4. **Fully embedded > managed service.** No cloud dependency by default.

Scope (all four selected): **navigate + interact**, **extract / read pages**,
**authenticated sessions**, **screenshots / visual**.

---

## 2. Architecture decision: embedded native-Rust CDP

### 2.1 The three layers, and where each lives

| Layer | What it is | Where it runs |
|---|---|---|
| **Control plane** | Rust code that speaks the protocol, defines tools, formats panels | **In-process**, compiled into the `tui` binary (the `cp-mod-browser` crate) |
| **Browser engine** | Chrome itself | **Local child process** spawned by CP (never in-binary — true of *every* automation tool) |
| **(optional) Remote** | A cloud/remote Chrome | Off by default; reachable via a `connect_url` config knob |

The control plane is **100% embedded** — same `Module` trait, same in-process
dispatch as `cp-mod-entities` / `cp-mod-files`. No Node, no MCP server, no
managed service, no API key, no network hop, no recurring cost.

The **browser engine is always a separate OS process** — you cannot link a
Chromium engine into a Rust binary, and *no* tool does (Playwright, Puppeteer,
Selenium all spawn the browser as a subprocess and drive it over a socket). CP
launches `chrome --remote-debugging-port=N` locally and speaks **CDP** (Chrome
DevTools Protocol) over the resulting WebSocket. This is a *local child*, not a
managed service.

### 2.2 Why CDP / native Rust over the alternatives

| Option | Embedded? | Browser(s) | Battle-tested | AI ergonomics | Verdict |
|---|---|---|---|---|---|
| **Native Rust CDP** (`chromiumoxide` / `headless_chrome`) | ✅ fully (Rust crate in-binary) | Chrome/Chromium/Brave/Edge | CDP is what Puppeteer/Playwright use under the hood; crates are actively maintained | We design the tool surface → token-minimal by construction | **CHOSEN** |
| fantoccini / thirtyfour (WebDriver) | ⚠️ client embedded, but needs a **separate driver daemon** (chromedriver) | cross-browser | most battle-tested *Rust* crate | lower-level, weaker auto-wait, no a11y snapshot | alt (only if cross-browser later) |
| playwright-rs | ⚠️ Rust API, but bundles + spawns the **Node driver** | cross-browser | engine battle-tested; binding pre-1.0 | best | alt (accept Node) |
| Playwright CLI as Skill | ⚠️ shells out to **Node** CLI | cross-browser | yes | very good, disk-state | alt (accept Node) |
| playwright-mcp | ❌ external **MCP server** | cross-browser | yes | token-hostile (~114k tok/task) | avoid |
| Browserbase / cloud | ❌ **managed service** | cross-browser | yes | good | fallback only |

Native Rust CDP is the *only* option that is **both** pure-Rust **and** maximally
embedded — it satisfies requirements 2, 3 and 4 simultaneously, and Chrome-only
(requirement 1) removes CDP's sole limitation (no Firefox).

### 2.3 Crate choice: `chromiumoxide` vs `headless_chrome`

| | `chromiumoxide` | `headless_chrome` |
|---|---|---|
| Model | async (tokio) | **synchronous** (plain threads) |
| CDP surface | full, generated (~60K LOC) | pragmatic subset ("Rust Puppeteer") |
| Compile cost | heavy (slow; matters — CB1/CB3 fire on every `.rs` edit) | light |
| Network interception / advanced CDP | strong | basic |
| Fit with CP tool dispatch (`execute_tool` is **sync**) | needs an event-loop handler task | **natural fit** |
| Deps | more | fewer |

**Recommendation: `headless_chrome`.** Its synchronous model maps cleanly onto
CP's synchronous `execute_tool` dispatch, it compiles fast (keeping the
edit→callback loop snappy), has fewer deps, and covers all four scope items
(navigate, click/type, `evaluate` for extraction, `capture_screenshot`,
`user_data_dir` for auth). Escalate to `chromiumoxide` later *only* if we need
heavy network interception or the full CDP surface.

> Open sub-decision for the captain: accept `headless_chrome`, or prefer
> `chromiumoxide` for maximum CDP capability up front?

---

## 3. Chrome process lifecycle — owned by `cp-console-server`

CP already runs `cp-console-server`: a daemon that spawns child processes which
**survive TUI reloads** (Unix socket, JSON-line protocol: create/send/kill/
status/list). Chrome becomes just another managed child.

### 3.1 Two channels — lifecycle vs control

The most important thing to internalize: **the console-server never sees a
browser command.** It is life-support, not a steering wheel. There are two
fully separate channels:

| | **Channel A — lifecycle** | **Channel B — control** |
|---|---|---|
| Endpoints | module → `cp-console-server` (Unix socket) | module → **Chrome directly** (CDP WebSocket) |
| Payload | `create` / `status` / `kill` | navigate, click, type, screenshot, eval |
| Frequency | rare (open, reconnect, close) | every browser action |
| Transits the daemon? | yes | **no — bypasses it entirely** |

High-frequency control (every navigate/click/screenshot) is pure Channel B and
goes straight to Chrome's `--remote-debugging-port` WebSocket. The daemon only
handles the three rare lifecycle verbs.

The diagram below makes the split explicit:

```
┌─────────────────────────────┐         CDP / WebSocket          ┌──────────────┐
│  tui  (cp-mod-browser)      │ ───────────────────────────────▶ │   Chrome     │
│  • in-process CDP client    │   ws://127.0.0.1:<debug-port>     │  (headed or  │
│  • browser tools + panels   │                                   │   headless)  │
└──────────────┬──────────────┘                                   └──────▲───────┘
               │ spawn / status / kill (Unix socket)                     │ child of
               ▼                                                          │
        ┌─────────────────────────────────────────────────────────┐     │
        │  cp-console-server  (survives TUI reloads) ──────────────┼─────┘
        └─────────────────────────────────────────────────────────┘
```

### 3.2 Why console-server owns Chrome (not the module directly)

- **Persistence across reloads.** A fixed `--user-data-dir` + stable debug port
  means cookies / logins / tabs survive a `system_reload`. On reload the module
  re-discovers the running Chrome via console-server `status` and **reconnects**
  to the same debug port — no re-login. This directly serves the
  *authenticated-sessions* scope item.
- **Consistency.** Mirrors how Meilisearch and console processes are already
  kept alive. One lifecycle model, not a bespoke one.
- **Crash isolation.** Chrome dying doesn't take down the TUI; console-server
  reports exit, module surfaces a clean error + offers relaunch.

A module-spawned `Command::spawn()` child would instead die on every reload —
losing the authenticated session — which is exactly why we route through the
daemon.

### 3.3 Launch contract, port, discovery

```
chrome \
  --remote-debugging-port=<port>     # CDP endpoint
  --user-data-dir=<cp-dir>/browser/profile   # persistent profile (auth)
  [--headless=new]                   # omitted in headed mode
  --no-first-run --no-default-browser-check
```

Port: pick a free ephemeral port, persist it in module data so reconnect works
across reloads. (Alternatively pass `--remote-debugging-port=0` and read the
actual port from Chrome's stderr line `DevTools listening on ws://…` — see §3.4,
this comes for free.) Profile dir under `.context-pilot/browser/` (gitignored,
like entities).

**Chrome discovery:** use system Chrome/Chromium/Brave if present (probe common
paths + `$BROWSER`); otherwise offer to fetch a pinned Chromium (chromiumoxide
has a fetcher; for `headless_chrome` we document the install or bundle a
download step). Never a hard cloud dependency.

### 3.4 Reusing the console client API — **zero daemon changes**

Source audit of `cp-mod-console` + `cp-console-server` (2026-06-11) confirms the
existing public client API is sufficient; `cp-mod-browser` depends on
`cp-mod-console` and calls it directly:

| Need | Existing public API (`cp-mod-console::manager`) |
|---|---|
| Ensure daemon up | `find_or_create_server() -> Result<(), String>` |
| Spawn Chrome | `SessionHandle::spawn(key, command, cwd) -> Result<SessionHandle>` |
| Reconnect after reload | `SessionHandle::reconnect(ReconnectMeta) -> SessionHandle` |
| Status / PID / kill | `handle.get_status()`, `handle.pid()`, `handle.kill()` |
| Read Chrome stderr | `handle.buffer` (`RingBuffer`) — tailed from the session log |

Two consequences:

- **The git/gh guardrail and the console *panel* live in
  `tools.rs::execute_create`, not in `SessionHandle::spawn`.** Calling `spawn`
  directly gives clean lifecycle management with **no** LLM-tool baggage and
  **no** stray console panel.
- **Dynamic-port discovery is free.** `spawn` already tails the child's stderr
  into `handle.buffer`, so the `DevTools listening on ws://…` line is readable
  via `buffer.contains_pattern(...)`.

The daemon (`cp-console-server`) spawns via `sh -c "<command>"` with **no
`.env_clear()`** — Chrome inherits the full environment (daemon ← TUI ← shell),
so it is a generic subprocess to the daemon. **Nothing in the daemon needs to
change.**

### 3.5 Required modification (existing crate): ownership-scoped orphan cleanup

The audit surfaced **one** real change, and it lives in the **client**
(`cp-mod-console`), *not* the daemon. Today, console's `load_module_data` calls:

```rust
manager::kill_orphaned_processes(&known_keys);  // known_keys = console's OWN sessions
```

`kill_orphaned_processes` asks the daemon to `list` **all** sessions and
`remove`s (kills) every key **not** in `known_keys`. This silently assumes
*console owns every process under the daemon*. The moment a second tenant
(our Chrome, e.g. key `browser_main`) shares the daemon, **console's next reload
reaps Chrome as an "orphan" and kills it.**

This is a latent multi-tenancy bug *exposed* (not caused) by the browser module.
**Fix:** make orphan cleanup **ownership-aware** — scope the reap by key prefix
so each tenant only reaps its own namespace:

- console reaps `c_*` only;
- browser reaps `browser_*` only.

Concretely: give `kill_orphaned_processes` (or a thin wrapper) an owner/prefix
filter and have each module pass its own. Small, self-contained, and the
*correct* multi-module design regardless of the browser. **This is the only
existing-crate code change the browser module requires — and it is a
prerequisite.**

### 3.6 Headed mode under the daemon — the one residual risk

- **Env inheritance: confirmed OK.** `handle_create` does **not** call
  `.env_clear()`, so `DISPLAY` / `WAYLAND_DISPLAY` propagate → headless is
  trivially fine, and Linux headed should be fine.
- **macOS caveat.** `cp-console-server` calls `nix::unistd::setsid()` to detach.
  `setsid` severs the controlling **TTY** (irrelevant to GUI) but does **not**
  leave the macOS **Aqua bootstrap namespace**, so headed windows *should* still
  appear — this is the one fuzzy spot. **5-minute spike to confirm.**
- **Clean fallback if it fails:** spawn **headed** Chrome *directly* from the
  module (no daemon, no `setsid`, inherits the TUI's session) accepting it dies
  on reload; route **headless** through the daemon for persistence. Degradation
  is graceful and isolated to the headed path.

---

## 4. Tool surface (agent-facing)

Designed for **token economy** — compact results, heavy state goes to a
**paginated, freezable panel**, not inline (this is the whole reason we avoided
playwright-mcp's inline a11y dumps). One module, a small, obvious tool set:

| Tool | Params | Returns (inline) | Panel side-effect |
|---|---|---|---|
| `browser_open` | `headless?` (bool), `url?` | confirmation + current URL/title | opens/refreshes **Browser** panel |
| `browser_goto` | `url`, `wait?` (load/domcontentloaded/selector) | URL + title + status | refreshes panel snapshot |
| `browser_snapshot` | `mode?` (a11y/text/outline) | *short* digest (title, URL, N interactive elements) | full compact accessibility/text tree → panel (paginated) |
| `browser_click` | `selector` \| `ref` (snapshot element id) | element acted-on + resulting URL/title | refresh |
| `browser_type` | `selector`/`ref`, `text`, `submit?` | confirmation | refresh |
| `browser_extract` | `selector?`, `format?` (text/html/markdown) | extracted content (capped; large → file/panel) | optional result panel |
| `browser_screenshot` | `selector?`, `full_page?` | path to PNG + dims | optional: feed to **OCR module** for text |
| `browser_eval` | `expression` (JS) | JSON-serialized result (capped) | — |
| `browser_back` / `browser_forward` / `browser_reload` | — | URL + title | refresh |
| `browser_tabs` | `action` (list/new/select/close), `index?`/`url?` | tab list | — |
| `browser_close` | — | confirmation | closes Chrome (or detaches) |

**Element addressing — the AI-ergonomics core.** `browser_snapshot` returns an
**accessibility/outline tree** where each interactive element gets a stable
short `ref` (e.g. `e12`). The agent then `browser_click {ref: "e12"}` — no
brittle CSS guessing, mirroring Playwright-MCP's proven "snapshot → act on ref"
loop, but with the verbose tree living in a **panel on disk**, not the context
window. CSS selectors remain available as an escape hatch.

**Screenshots → OCR synergy.** `browser_screenshot` writes a PNG; the existing
`cp-mod-ocr` can turn it into text. Visual + textual extraction without leaving
CP.

---

## 5. The Browser panel

A dynamic panel (`Kind::BROWSER`) showing current browser state, formatted for
token-efficiency and freezable by the cache engine:

```
Browser — https://example.com/login  [headed]  (tab 1/1)
Title: Example — Sign in
Status: loaded (200)

Interactive elements (snapshot e-refs):
  e1   link    "Home"           /
  e3   input   text  #email     "Email address"
  e4   input   pwd   #password  "Password"
  e7   button        "Sign in"
  …  (paginated)

Last action: type e3 "user@host"  →  ok
```

- Disk-backed snapshot content → paginated, freezable (same pattern as the
  file panel / entity-result panel).
- Compact by default; full a11y/DOM tree only on `browser_snapshot` and only in
  the panel, never streamed inline.

---

## 6. Crate layout

```
crates/cp-mod-browser/
  Cargo.toml          # deps: cp-base, cp-render, cp-mod-console (lifecycle),
                      #       cp-mod-ocr (optional screenshot→text), headless_chrome,
                      #       serde_json, log
  src/
    lib.rs            # Module trait impl: tools, panel factory, init/save/load,
                      #   reconnect-on-reload, dependency = ["console"]
    client.rs         # CDP client wrapper: connect, goto, click, type, eval,
                      #   screenshot, snapshot → e-ref map
    lifecycle.rs      # spawn/reconnect/kill Chrome by REUSING cp-mod-console
                      #   SessionHandle (no daemon change); port + profile mgmt;
                      #   system-Chrome discovery
    snapshot.rs       # accessibility/outline tree → compact e-ref representation
    tools.rs          # tool dispatch (≤500 lines; split if needed per CP rules)
    panel.rs          # Browser panel (paginated, freezable)
    types.rs          # BrowserState (port, profile, current tab, e-ref map, …)
```

Respects CP structure limits: ≤500 lines/file, ≤8 entries/dir (7 here, room to
grow). `#[expect]`/`#[allow]` banned — refactor instead (M27/M32). Profile dir
gitignored (M17 pattern).

---

## 7. Managed-service fallback (explicitly optional)

The same client connects to a **remote** Chrome by pointing at a different
WebSocket URL. A future `browser.connect_url` config value lets CP drive a
cloud/stealth browser (Browserbase, self-hosted remote Chrome) **without a
rewrite** — embed-by-default, cloud-by-config. Worth it only for: massive
parallelism, anti-bot stealth, or running when the user's machine is asleep.
Not built now; the architecture simply doesn't preclude it.

---

## 8. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Chrome not installed | Probe system browsers; offer pinned-Chromium fetch; clear error |
| `headless_chrome` less capable than Playwright | Scope covered today; `chromiumoxide` escalation path documented |
| Snapshot tree token bloat | Disk-backed, paginated, freezable panel; inline digest only |
| Session/profile corruption | Profile under `.context-pilot/browser/`; recreate on corruption |
| Reload races (Chrome vs TUI) | console-server owns Chrome; module reconnects by stored port |
| **Console reaps our Chrome** | **Ownership-scoped orphan cleanup (§3.5) — prerequisite client fix** |
| Headed window under daemon (macOS) | Env inherited (no `env_clear`); spike `setsid`+Aqua; fallback = direct headed spawn (§3.6) |
| `browser_eval` = arbitrary JS in page | Document as power-tool; consider a confirm/guardrail like console git/gh |

---

## 9. Decision summary for validation

| # | Decision | Recommendation |
|---|---|---|
| D1 | Embedded vs managed | **Embedded** native-Rust CDP control plane; Chrome as local child |
| D2 | Crate | **`headless_chrome`** (sync, light) — `chromiumoxide` as escalation |
| D3 | Chrome lifecycle | Owned by **`cp-console-server`**; reuse `SessionHandle::spawn/reconnect/kill`; reconnect by stored port. Two-channel: CDP bypasses the daemon (§3.1) |
| D3b | **Existing-crate changes** | Daemon: **none**. Client `cp-mod-console`: **one** prerequisite — ownership-scoped orphan cleanup (§3.5) |
| D3c | Headed mode | Env inherited (no `env_clear`); spike macOS `setsid`/Aqua; fallback = direct headed spawn (§3.6) |
| D4 | Element addressing | **Snapshot → `e-ref`** loop (Playwright-MCP-style), CSS escape hatch |
| D5 | State display | Compact inline digest; full tree only in **disk-backed Browser panel** |
| D6 | Screenshots | PNG to disk, optional **OCR** hand-off |
| D7 | Scope v1 | navigate+interact, extract, auth sessions, screenshots |
| D8 | Cloud | `connect_url` knob, off by default — fallback only |

**No implementation yet. Validated next steps, in order:**
1. **Prerequisite:** ownership-scoped orphan cleanup in `cp-mod-console` (§3.5).
2. **Spike:** `headless_chrome` launch + connect (via `SessionHandle::spawn`) +
   `browser_goto` + `browser_snapshot`.
3. **Headed spike:** confirm a visible window appears through the daemon on macOS
   (§3.6); wire the direct-spawn fallback if not.
