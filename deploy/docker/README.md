# Context Pilot in Docker

The container counterpart of `deploy/ansible` (the Photonicat appliance): the
same release bundle, the same binaries, packaged instead of provisioned. One
container runs the whole product — orchestrator, agent, console server and
Meilisearch — and serves the cockpit on `:7878`.

## First run

```sh
cd deploy/docker
./gen-secrets.sh                 # writes secrets/*.pw and prints them once
$EDITOR secrets/providers.env    # add at least one model provider key
docker compose up -d --build
```

Open <http://127.0.0.1:7878> and log in as the superadmin. Both seeded accounts
must change their password at first login.

## What it needs on first boot

Accounts are seeded **only while the user table is empty**, exactly as on the
appliance. After that these values are ignored, and the entrypoint stops
checking them.

| | Required | |
|---|---|---|
| `CP_SEED_SUPERADMIN_EMAIL` + password | **yes** | Vendor account: provider secrets, IT settings. The only role that can create another superadmin. |
| `CP_SEED_ADMIN_EMAIL` + password | no | The customer's top account. A superadmin can create it later from the cockpit. |
| `CP_PROVIDERS_ENV_FILE` | no | API keys. Without a model provider key the cockpit works but no agent can answer. |

Omitting the superadmin on first boot is **not reversible**: only a superadmin
can create a superadmin, and the seed never runs again once any account exists.
The entrypoint refuses to start rather than let that happen silently.

Passwords go in via `*_PASSWORD_FILE`, not `*_PASSWORD`. `docker inspect` shows
every environment variable to anyone who can reach the daemon, and the agents
running inside the container can read their own environment. The binary reads
either (`runtime/seed.rs`), so the plain variable is there for throwaway runs.

`providers.env` is one `KEY=value` per line. It is loaded into the process
environment at every start and never written to the volume, so rotating a key is
editing the file and restarting. Keys set from the cockpit UI are stored
separately and are not touched by it.

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

## Building against an unreleased commit

The default build downloads a pinned, checksum-verified release asset. To run a
local build instead, produce the bundle in the same flat layout
(`cpilot`, `cp-console-server`, `cp-orchestrator`, `meilisearch`, `web/`) and:

```sh
mkdir -p deploy/docker/.artifacts
cp cpilot-linux-x86_64.tar.gz deploy/docker/.artifacts/
docker compose build --build-arg BUNDLE_SOURCE=local
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
