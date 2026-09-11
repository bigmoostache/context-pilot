# Environment variables

<!-- GENERATED from crates/cp-env/src/specs/ by `cargo test -p cp-env --test generate -- --ignored`. Do not edit by hand. -->

Every variable either binary reads is declared once, in `crates/cp-env/src/specs/`. Validation, the typed configuration and this page are derived from that table.

## Rules

- **Precedence**: the process environment, then the project `.env`, then `~/.context-pilot/.env` (each file overrides what came before it - the global file is where the cockpit writes provider keys).
- **Strict**: at boot, both binaries validate everything at once and refuse to start on any problem, listing every offending variable, the value received and what was expected. `cp-orchestrator --check-env` and `cpilot --check-env` run the same validation and exit.
- **Unknown names**: a `CP_*` variable absent from this table is an error.
- **Empty means unset**: a variable set to the empty string counts as unset (blank a line to disable it).
- **Booleans**: exactly `0`, `1`, `true` or `false`.
- **Paths**: an explicitly set path must satisfy its precondition (existing file, directory, executable, parent). A *defaulted* path is never checked; a missing default is logged as a warning.
- **Scope**: each binary parses the variables of its own scope plus the shared ones; the other binary's names are known (never "unknown") but ignored. The orchestrator passes its whole environment to every agent it spawns.

## Invariants

Combinations rejected at boot:

- `CP_CADDY_BIN` requires `CP_CADDYFILE`.
- `CP_LLM_GATEWAY_KEY` requires `CP_LLM_GATEWAY`.
- `CP_FEATURE_IT_PANE=1` requires `CP_CADDYFILE`.
- `CP_FEATURE_DAY0_SETUP=1` requires `CP_FEATURE_IT_PANE=1`.
- `CP_FEATURE_UPDATER=1` requires `CP_WEB_ROOT`.
- `CP_SEED_<ROLE>_EMAIL` requires `CP_SEED_<ROLE>_PASSWORD` or `CP_SEED_<ROLE>_PASSWORD_FILE`.
- Any `CP_SEED_*` variable requires `CP_AUTH_ENABLED=1`.

## Core

Read by both binaries.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `HOME` | path (existing directory) | - | both | yes | Home of the process. `~/.context-pilot` (global `.env`, registry, `auth.db`, releases, Meilisearch), the Claude credentials file and the default agent code root all live under it. |
| `XDG_CONFIG_HOME` | path | `$HOME/.config` | both |  | Parent of the central `context-pilot/config.json` store (Linux only). |
| `CP_AGENTS_DIR` | path | `$HOME/.context-pilot/agents` | both |  | Registry directory shared by the orchestrator and its agents (created on demand). The orchestrator passes its own value to every agent it spawns. |

## Orchestrator

Where the orchestrator listens and where it finds its agents and front.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `CP_ORCH_PORT` | port (1-65535) | `7878` | orchestrator |  | TCP port of the REST + SSE API. |
| `CP_ORCH_BIND` | text | `127.0.0.1` | orchestrator |  | Listen address. Loopback by default: the backend speaks cleartext and its auth model assumes an encrypted transport, so only the reverse proxy faces the LAN. Containers set `0.0.0.0` and publish the port on loopback instead. |
| `CP_SCAN_INTERVAL_MS` | integer | `2000` | orchestrator |  | Registry scan period, in milliseconds. |
| `CP_AGENTS_ROOT` | path (existing directory) | `$HOME/code` | orchestrator |  | Where the project directories of newly created agents are made. |
| `CP_AGENT_BINARY` | path (executable) | `<cwd>/target/release/tui` | orchestrator |  | The agent binary the supervisor spawns. A persisted active release overrides it after an OTA update. |
| `CP_WEB_ROOT` | path (existing directory) | - | orchestrator |  | Directory of the built cockpit (SPA) served by the orchestrator. Unset, only the API is served. The updater repoints this path after a release swap. |
| `CP_PROVISION_FLAG` | path (parent directory must exist) | `<CP_AGENTS_DIR>/.provisioned` | orchestrator |  | Durable flag file written once the box identity is set (day-0 setup). |
| `CP_RELEASES_BREAK_GLASS` | bool (0/1/true/false) | `0` | orchestrator |  | Re-enable manual version selection in the releases API. The auto-updater owns version choice otherwise. |

## Authentication

Login enforcement and session lifetime.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `CP_AUTH_ENABLED` | bool (0/1/true/false) | `0` | orchestrator |  | Require login and enforce role-based access. Every production profile sets 1; off, every caller is treated as a superadmin. |
| `CP_SESSION_TTL_SECS` | integer | `2592000` | orchestrator |  | Session lifetime in seconds (default 30 days). |
| `CP_AUTH_DB` | path (parent directory must exist) | `$HOME/.context-pilot/orchestrator/auth.db` | orchestrator |  | The auth SQLite database (users, sessions, agent ACL). Orchestrator-level, never inside the agents directory. |

## Account seeding

Accounts created once, at the first boot with an empty user table. Every seeded account must change its password on first login. Requires `CP_AUTH_ENABLED=1`; an email without a password is an error.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `CP_SEED_SUPERADMIN_EMAIL` | text | - | orchestrator |  | Email of the vendor account (superadmin: provider secrets, IT settings, the only role that can create another superadmin). Setting it enables the seed. |
| `CP_SEED_SUPERADMIN_NAME` | text | `superadmin` | orchestrator |  | Display name of the vendor account. |
| `CP_SEED_SUPERADMIN_PASSWORD` | text | - | orchestrator |  | Initial password of the vendor account (changed at first login). |
| `CP_SEED_SUPERADMIN_PASSWORD_FILE` | path (existing file) | - | orchestrator |  | File holding the initial password of the vendor account (preferred: keeps the secret out of the process environment). Wins over the inline variable. |
| `CP_SEED_ADMIN_EMAIL` | text | - | orchestrator |  | Email of the client's top account (admin: everything but provider secrets). Optional; a superadmin can create it from the cockpit. |
| `CP_SEED_ADMIN_NAME` | text | `admin` | orchestrator |  | Display name of the client's top account. |
| `CP_SEED_ADMIN_PASSWORD` | text | - | orchestrator |  | Initial password of the client's top account (changed at first login). |
| `CP_SEED_ADMIN_PASSWORD_FILE` | path (existing file) | - | orchestrator |  | File holding the initial password of the client's top account. Wins over the inline variable. |

## LLM gateway

Optional LiteLLM-style proxy. When set, Anthropic, Grok, Groq and DeepSeek traffic goes through it and needs no local provider key. Claude Code OAuth and MiniMax always talk to their own API.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `CP_LLM_GATEWAY` | http(s) URL | - | both |  | Base URL of the gateway. Unset or empty means no gateway: each provider is called directly with its own key. |
| `CP_LLM_GATEWAY_KEY` | text | - | both |  | Key presented to the gateway on every call. Requires `CP_LLM_GATEWAY`. |

## Agent bridge

How a spawned agent finds its orchestrator. The orchestrator sets these on every agent it starts.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `CP_BRIDGE` | bool (0/1/true/false) | `0` | agent |  | Activate the orchestration bridge module and the bridge-backed vault. The orchestrator sets 1 on every agent it spawns; `cpilot --bridge` is the CLI equivalent. |
| `CP_BRIDGE_URL` | http(s) URL | `http://127.0.0.1:7878` | agent |  | The orchestrator API as seen by the agent. |

## Appliance gates

Gates that turn the orchestrator from "persist the document" into "reconfigure this machine". Leave every one of them UNSET outside the appliance (containers, developer machines): absent, the Caddy integration and the network applier are inert.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `CP_CADDYFILE` | path (parent directory must exist) | - | orchestrator |  | Caddyfile the orchestrator regenerates (box name and IP, provisioned gate: cleartext :80 at day-0, private-CA :443 once provisioned). Unset, Caddy is never touched. |
| `CP_CADDY_BIN` | path (executable) | - | orchestrator |  | Caddy binary used to reload the regenerated Caddyfile. Requires `CP_CADDYFILE`. |
| `CP_CA_ROOT` | path | - | orchestrator |  | Root certificate of the private CA, served for download at `/api/it/ca.crt`. Caddy creates it on first TLS use, so it need not exist at boot. |
| `CP_NMCLI_BIN` | path (executable) | - | orchestrator |  | `nmcli` - the master gate of the network applier. Unset, the applier is inert (documents are persisted, never applied) and the modem is assumed present. |
| `CP_MMCLI_BIN` | path (executable) | - | orchestrator |  | `mmcli` - modem status for `GET /api/it/network`. |
| `CP_IW_BIN` | path (executable) | - | orchestrator |  | `iw` - Wi-Fi radio capabilities and regulatory domain. |
| `CP_IP_BIN` | path (executable) | - | orchestrator |  | `ip` - interface addresses for the network status. |
| `CP_NETWORKCTL_BIN` | path (executable) | - | orchestrator |  | `networkctl` - reload of systemd-networkd after writing its units. |
| `CP_SYSTEMCTL_BIN` | path (executable) | - | orchestrator |  | `systemctl` - restart of the uplink supervisor after its configuration changes. |
| `CP_NFT_BIN` | path (executable) | - | orchestrator |  | `nft` - NAT rules for the access-point clients. |
| `CP_REGDOM_BIN` | path (executable) | - | orchestrator |  | The `cp-regdom` script that puts the Wi-Fi country in force (shared with the boot oneshot). |
| `CP_NETWORKD_DIR` | path (existing directory) | - | orchestrator |  | Directory of the systemd-networkd units the applier writes (`/etc/systemd/network`). |
| `CP_UPLINK_ENV` | path (parent directory must exist) | - | orchestrator |  | Environment file of the uplink supervisor (`/etc/default/cp-uplink`), regenerated by the applier and read by `cp-uplink-watch`. |
| `CP_UPLINK_STATE` | path | `/run/cp-uplink/state` | orchestrator |  | State file published by `cp-uplink-watch` and projected by `GET /api/it/network`. Both sides read the same name with the same default. |
| `CP_NETWORK_APPLIED` | path | `/run/cp-network-applied` | orchestrator |  | Marker recording the hash of the last applied network document, so a hand edit is reverted at the next apply or boot. |
| `CP_WAN_IFACE` | text | `end0` | orchestrator |  | The Ethernet uplink port. |
| `CP_AP_IFACE` | text | `wlp1s0` | orchestrator |  | The Wi-Fi radio used for the access point. |
| `CP_WWAN_DEV` | text | `cdc-wdm0` | orchestrator |  | The modem's NetworkManager device (its control port). |
| `CP_WWAN_PRESENT` | bool (0/1/true/false) | - | orchestrator |  | Force whether a 5G modem is present (1) or absent (0) instead of probing the hardware. Field debugging only. |

## Feature flags

Behaviour switches, enforced server-side and mirrored by the cockpit through `GET /api/features`. Set every flag explicitly in each deployment profile.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `CP_FEATURE_CLAUDE_OAUTH` | bool (0/1/true/false) | `1` | orchestrator |  | Offer the Claude Code subscription (OAuth) as a provider. Off: the login routes answer 404, the provider is dropped from the catalogue, the token sweeper is not started and the cockpit hides the subscription UI. |
| `CP_FEATURE_DAY0_SETUP` | bool (0/1/true/false) | `0` | orchestrator |  | Run the day-0 identity/TLS setup for the first IT-capable login. Off (cloud, containers): the box counts as provisioned and no login ever lands on that step. Requires `CP_FEATURE_IT_PANE=1`. |
| `CP_FEATURE_IT_PANE` | bool (0/1/true/false) | `0` | orchestrator |  | Expose the IT pane (identity, TLS trust, network) and every `/api/it/*` route. Off: the routes answer 404 and the pane is hidden. Requires `CP_CADDYFILE`. |
| `CP_FEATURE_UPDATER` | bool (0/1/true/false) | `0` | orchestrator |  | Expose the OTA updater: the Update pane, `/api/releases/*` and `/api/update/*`, and the nightly scheduler. Requires `CP_WEB_ROOT`. |
| `CP_FEATURE_KEYS_EDITABLE` | bool (0/1/true/false) | `1` | orchestrator |  | Let superadmins reveal and edit provider keys from the cockpit (written to `~/.context-pilot/.env`). Off: keys come from the environment only and the pane is read-only. |
| `CP_FEATURE_ONBOARDING` | bool (0/1/true/false) | `1` | orchestrator |  | Run the first-run product onboarding tour for the first manager-level login. |

## Developer

Local development only.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `CP_FLAMEGRAPH` | bool (0/1/true/false) | `0` | agent |  | Write flame-graph profiling data (`run.sh --flamegraph`). |
| `CP_RUN_SH` | bool (0/1/true/false) | `0` | agent |  | Set by `run.sh`: the supervisor script handles reloads, so the agent must not re-exec itself. |
| `SHOW_CONTEXT_PILOT_IN_TREE` | bool (0/1/true/false) | `0` | agent |  | Show the `.context-pilot/` directory in the tree tool. |

## Credentials

Resolved by the vault (process environment, then `~/.context-pilot/.env`, then keychain/credential file). Never read by the configuration layer; listed here so the table is complete.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `ANTHROPIC_API_KEY` | secret (vault) | - | both |  | Anthropic. |
| `XAI_API_KEY` | secret (vault) | - | both |  | Grok (xAI). |
| `DEEPSEEK_API_KEY` | secret (vault) | - | both |  | DeepSeek. |
| `GROQ_API_KEY` | secret (vault) | - | both |  | Groq. |
| `MINIMAX_API_KEY` | secret (vault) | - | both |  | MiniMax. |
| `BRAVE_API_KEY` | secret (vault) | - | both |  | Brave Search (web search tool). |
| `FIRECRAWL_API_KEY` | secret (vault) | - | both |  | Firecrawl (web scraping tool). |
| `DATALAB_API_KEY` | secret (vault) | - | both |  | Datalab (OCR tool). |
| `VOYAGE_API_KEY` | secret (vault) | - | both |  | Voyage AI (embeddings for search). |
| `GITHUB_TOKEN` | secret (vault) | - | both |  | GitHub (the github module and the `gh` CLI it drives). |
| `TELEGRAM_BOT_TOKEN` | secret (vault) | - | both |  | Telegram bot bridge. |
| `DISCORD_BOT_TOKEN` | secret (vault) | - | both |  | Discord bot bridge. |
| `SLACK_BOT_TOKEN` | secret (vault) | - | both |  | Slack bot bridge. |
| `GOOGLECHAT_BOT_TOKEN` | secret (vault) | - | both |  | Google Chat bot bridge. |
| `TELEGRAM_API_ID` | secret (vault) | - | both |  | Telegram API id (user-account bridge). |
| `TELEGRAM_API_HASH` | secret (vault) | - | both |  | Telegram API hash (user-account bridge). |

## External names

Names that use the `CP_` prefix but are consumed by child processes or tooling, never by the binaries. Registered so strict validation tolerates them.

| Variable | Type | Default | Scope | Required | Description |
|---|---|---|---|---|---|
| `CP_CHANGED_FILES` | text | - | child only |  | Injected into global callback scripts: newline-separated changed paths. |
| `CP_CHANGED_FILE` | text | - | child only |  | Injected into local callback scripts: the one changed path. |
| `CP_PROJECT_ROOT` | text | - | child only |  | Injected into callback scripts: the project root. |
| `CP_CALLBACK_NAME` | text | - | child only |  | Injected into callback scripts: the callback's name. |
| `CP_CRASH_CHILD_DIR` | text | - | child only |  | Test harness (`cp-oplog` crash replay): the child's oplog directory. |
| `CP_CRASH_CHILD_MODE` | text | - | child only |  | Test harness (`cp-oplog` crash replay): the child's mode. |
| `CP_PORT` | text | - | child only |  | Docker Compose only: the host port the cockpit is published on. |
| `CP_API_URL` | text | - | child only |  | Playwright only: the orchestrator the end-to-end tests target. |
| `CP_WEB_URL` | text | - | child only |  | Playwright only: the dev server the end-to-end tests target. |
| `CP_AGENT_ID` | text | - | child only |  | Playwright only: the agent the regression probes target. |
| `CP_AGENT_LOCK_FD` | text | - | child only |  | Reserved (design doc): the registry lock descriptor passed across a deadman re-exec. Not implemented. |

