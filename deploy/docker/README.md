# Context Pilot in Docker

The container counterpart of `deploy/ansible` (the Photonicat appliance): the
same release bundle, the same binaries, packaged instead of provisioned. One
container runs the whole product — orchestrator, agent, console server and
Meilisearch — and serves the cockpit on `:7878`.

No scripts: the image, the compose file, your `.env`, and this page — plus
`litellm.yaml` and `providers.env` if you turn the optional gateway on.

## First run

```sh
cd deploy/docker
cp .env.example .env && chmod 600 .env
openssl rand -hex 16                      # paste as CP_SEED_SUPERADMIN_PASSWORD
$EDITOR .env                              # + at least one model provider key
docker compose up -d --build
```

Open <http://127.0.0.1:7878> and log in as the superadmin. The first login forces
a password change. `docker compose logs -f` shows the seed and the boot.

## Configuration

`.env` is the whole surface. Compose passes it into the container's environment,
which is where the binaries already look — the seed reads its accounts from
`CP_SEED_*` (`runtime/seed.rs`) and the vault reads provider keys from the
environment (`cp-vault/src/local.rs:45`). Any `CP_*` variable the product
understands can be added the same way.

| | Required | |
|---|---|---|
| `CP_SEED_SUPERADMIN_EMAIL` + `_PASSWORD` | **yes** | Vendor account: provider secrets, IT settings. The only role that can create another superadmin. |
| `CP_SEED_ADMIN_EMAIL` + `_PASSWORD` | no | The customer's top account. A superadmin can create it later from the cockpit. |
| `ANTHROPIC_API_KEY`, `BRAVE_API_KEY`, … | no | Without a model provider key the cockpit works but no agent can answer. |

Accounts are seeded **only while the user table is empty**, exactly as on the
appliance; after that the values are ignored, so leaving them in `.env` is safe.
Omitting the superadmin on the first boot is **not reversible** — only a
superadmin can create a superadmin — so `docker compose up` refuses to start
without one rather than let that happen silently.

Keys are read at every start and never written to the volume: rotating one is
editing `.env` and `docker compose up -d`. Keys set from the cockpit UI live in
the volume and are not touched by this file.

Anything in `.env` is visible in `docker inspect` to whoever can reach the
daemon, and to the agents running inside the container — that is the cost of the
single-file form. Keep `.env` at `0600`, and if that trade-off is not acceptable,
use `CP_SEED_*_PASSWORD_FILE` with a mounted file instead (the binary reads
either, `runtime/seed.rs`).

## Optional: an LLM gateway

`CP_LLM_GATEWAY` points the API-key providers at a LiteLLM proxy instead of their
own domains. Empty or absent — the default — every provider is called directly
with the keys in `.env`, and none of this code runs.

```sh
cp providers.env.example providers.env && chmod 600 providers.env
cp litellm.yaml.example litellm.yaml
$EDITOR providers.env      # the model keys move here
$EDITOR litellm.yaml       # keep only the providers whose keys you just set
$EDITOR .env               # CP_LLM_GATEWAY + CP_LLM_GATEWAY_KEY
docker compose --profile gateway up -d
```

Both halves are required: the variable without the profile leaves the agent
posting to a host that does not exist. In exchange, the model keys are read only
by the gateway container — the agents and `docker inspect` on the product no
longer see them.

Two routes are used, and the difference matters:

- Anthropic-format requests go to `/anthropic/v1/messages`, LiteLLM's
  **pass-through**, which forwards the body unmodified. Nothing is normalized, so
  provider-specific fields survive.
- OpenAI-compatible requests (Grok, Groq, DeepSeek) go to `/v1/chat/completions`,
  where LiteLLM routes on the body's `model`. **Every model string the product
  sends must be declared in `litellm.yaml`** — a missing one is a 400 on the
  first message. The two Groq `openai/gpt-oss-*` entries are the trap: LiteLLM
  reads their slash as a provider prefix, so without their explicit entries the
  requests go to OpenAI instead of Groq.

Never enable `drop_params` in `litellm.yaml`. It removes parameters silently,
which is how tool definitions and cache fields vanish without an error.

### `litellm.yaml` drives the model picker

Under a gateway the orchestrator has no provider keys of its own, so it cannot
decide availability from a key check. It asks the gateway instead — `GET
/v1/models`, cached five minutes, no tokens — and offers only the intersection
with its catalogue. **Comment out the providers whose keys you do not have**: a
model declared there but keyless is offered and then fails with a provider 401.

Two deliberate gaps in that filter:

- **Anthropic is never filtered.** It takes the pass-through route, which does not
  consult `model_list`, so its models are absent from `/v1/models` by
  construction — filtering on that list would remove a provider that works.
- **An unreachable gateway filters nothing.** The full catalogue is offered rather
  than an empty picker, because a proxy that is briefly down should not look like
  a product that lost its models.

For the exact truth — which declared models have a *working* key — ask the proxy
itself. It issues one real request per model, so run it by hand, never on a timer:

```sh
KEY=$(grep '^CP_LLM_GATEWAY_KEY=' .env | cut -d= -f2)
docker compose exec context-pilot curl -sS http://litellm:4000/health \
  -H "Authorization: Bearer $KEY" | jq -r '.healthy_endpoints[].model'
```

Reading the failures, all four measured against this compose file:

| What you see | What it means |
|---|---|
| `401 invalid x-api-key` with an Anthropic `request_id` | the route works; the key **inside the gateway** is wrong |
| `400 XaiException - Incorrect API key provided` | same, for an OpenAI-compatible provider |
| `400 Invalid model name passed in model=…` | that model has no `model_list` entry |
| `400 {"message":"No connected db."}` | `CP_LLM_GATEWAY_KEY` does not match the gateway's master key — it reads like a broken proxy and is really a bad key (LiteLLM has no database here to look keys up in) |

Unaffected on purpose: the two Claude Code providers (a subscription OAuth token
cannot survive a proxy that substitutes its own key) and MiniMax (no route
forwards an Anthropic-shaped body to a third party). They keep calling their own
API whatever `CP_LLM_GATEWAY` says — see `src/llms/gateway/mod.rs`.

## Exposure

The compose file publishes on `127.0.0.1` on purpose. Serving the cockpit over
plain `http://` on any address other than localhost **breaks it**: the SPA builds
every agent command with `crypto.randomUUID`
(`web/src/lib/api/client/index.ts:47`), which browsers only expose in a secure
context. The page loads, the login succeeds, and then every command throws — it
reads as a broken product, not as a missing certificate.

To reach it from another machine, terminate TLS in front (your own reverse proxy
and certificate) and point it at the container. Do not widen the published port.

Inside the container the orchestrator binds `0.0.0.0`, overriding the binary's
loopback default. That default exists for the appliance, where Caddy is the only
LAN-facing surface; in a container the isolation boundary is the network
namespace and the published ports. Bound to loopback, the orchestrator would be
unreachable through `-p` while the in-container healthcheck still passed.

## State

Everything that must survive a re-create is in the `cp-data` volume mounted at
`/data` (`$HOME`): `auth.db`, agent realms under `/data/code`, the Meilisearch
index. Back it up, and expect a `docker compose down -v` to be a factory reset.

The image ships that directory tree pre-built, including the Meilisearch binary,
because Docker seeds an empty named volume from the image's content at the mount
point. That is what removes the need for a boot-time copy — and it only works for
a named volume: bind-mount a host directory over `/data` instead and Meilisearch
will download its own binary from github.com at the first search
(`cp-mod-search/src/meili/server/download.rs`).

## Building against an unreleased commit

The default build downloads a pinned, checksum-verified release asset. To run a
local build instead, produce the bundle in the same flat layout
(`cpilot`, `cp-console-server`, `cp-orchestrator`, `meilisearch`, `web/`) and:

```sh
mkdir -p deploy/docker/.artifacts
cp cpilot-linux-x86_64.tar.gz deploy/docker/.artifacts/
echo BUNDLE_SOURCE=local >> deploy/docker/.env
docker compose build
```

Bumping the released version means bumping `VERSION` **and** `BUNDLE_SHA256` in
the Dockerfile; a version bump alone fails at the checksum, which is intended.

## What this image deliberately does not do

- **No Caddy, no TLS.** `CP_CADDYFILE` is unset, so the orchestrator's Caddy
  integration exits cleanly and an external proxy can front the container.
- **No network management.** The applier gates (`CP_NMCLI_BIN`, `CP_IW_BIN`, …)
  are unset and must stay unset: they switch the backend from persisting a
  network document to reconfiguring the machine. A container has no business
  rewriting its host's network.
- **No confinement of its own.** `cap_drop`, `pids_limit`, memory and CPU limits
  live in the compose file, not in the image — they are runtime flags and an
  image cannot enforce them. Anything reachable by other people needs them.
