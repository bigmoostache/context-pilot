# Appliance provisioning (Ansible)

Freshly-installed Armbian/Debian 13 box (systemd) → fully deployed and ready, in
one pass.

## Full deploy — `site.yml`

Prereqs: the box is enrolled on the tailnet (`tag:cp-<client>`, see
`deploy/PROVISIONING.md`) and **this control node is on the tailnet too**.

```sh
cp deploy/ansible/examples/inventory.example.ini deploy/ansible/inventory.ini
# edit inventory.ini with each box's Tailscale MagicDNS name
# (<hostname>.<tailnet>.ts.net) — not a LAN IP. Auth is Tailscale SSH (no key).

ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml
```

What it does, end to end:

1. **Fetch** (control node — needs internet): downloads the **pre-built appliance
   bundle** (`cpilot-appliance-aarch64.tar.gz` = `bin/cp-orchestrator`, `bin/tui`,
   `web/<spa>`) from GitHub Releases — produced by
   `.github/workflows/release.yml` — plus a stock `caddy` arm64 binary. No local
   build/toolchain needed. Pin a tag with `-e release=v0.1.0-abc1234` (default:
   `latest`). The systemd units + bootstrap Caddyfile come from this repo.
2. **Deploy** (each box): ships the binaries + SPA + the `context-pilot` and
   `caddy` systemd units + bootstrap Caddyfile under `/opt/context-pilot`. A stock
   Armbian box has nothing on `:80`/`:443`, so no port juggling is needed.
3. **Seed** (Obj 6): writes a unique per-unit admin `seed.env` (chmod 600) and a
   printable delivery sheet on the control node (`out/<unit>-admin.txt`).
4. **Start**: enables + starts the orchestrator (which seeds the admin and writes
   the real Caddyfile) then Caddy, and waits until both answer.

> Requires a published release that contains `cpilot-appliance-aarch64.tar.gz`
> (cut by pushing a tag → `release.yml`). The control node fetches it; the
> offline LAN box never needs internet.

### No release yet? Build the bundle locally

When you can't cut a GitHub release, build the same bundle on the dev box and
deploy it with `-e release=local`:

```sh
deploy/photonicat/build.sh                 # → deploy/ansible/.artifacts/{cpilot-appliance-aarch64.tar.gz, caddy}
ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml -e release=local
```

`build.sh` cross-builds the orchestrator + agent TUI (aarch64-musl), builds the
SPA, packages the exact same tarball `release.yml` would, and downloads Caddy.
With `-e release=local` the playbook skips the GitHub download and uses it.

Result: the box boots **unprovisioned** with the IT maintenance console live at
`https://<box-ip>:9090` (private-CA TLS). The operator finishes setup there
(change password → name the box → distribute the CA → finalize), which brings the
cockpit up on `:443`.

### Options

```sh
-e release=v0.1.0-abc1234       # pin a release tag (default: latest)
-e cp_admin_email=ops@acme.corp # override the default admin email (admin@admin.fr)
-e cp_admin_password=…          # force a fixed admin password (default: random per unit)
```

## Per-client provider API keys

Keys are written to `providers.env` on the box (chmod 600, sourced at boot); the
orchestrator picks them up so agents can use the models. They come from the var
`cp_provider_keys` — which you supply **at launch, never committed**.

**Default — runtime injection (nothing secret in the repo):** keep the keys in a
gitignored local file and pass it with `-e @file`. One file per client; run one
client at a time with `--limit`.

```sh
# 1. group the boxes by client in inventory.ini:  [acme] … / [globex] …
# 2. copy the template to a GITIGNORED local file and fill in acme's keys:
cp deploy/ansible/examples/secrets.example.yml deploy/ansible/acme.local.yml
$EDITOR deploy/ansible/acme.local.yml
# 3. deploy that client (its keys only):
ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml \
  --limit acme -e @deploy/ansible/acme.local.yml -e release=local
```

- Pass secrets as **`-e @file`**, not `-e KEY=value` — the latter leaks in
  `ps aux` and shell history; `@file` exposes only the filename.
- `*.local.yml` under `deploy/ansible/` is gitignored. Keep it `chmod 600`; it
  lives only on the control node.
- Only the keys you list are written — choose providers per client. `seed.env`
  (per-unit admin, write-once) is untouched; `providers.env` is rewritten every run.

**Alternative — ansible-vault** (encrypted secrets versioned in git, if you want
recovery/audit): put the same `cp_provider_keys` in `group_vars/<client>/vault.yml`,
`ansible-vault encrypt` it, and run with `--ask-vault-pass` (no `-e @file` needed).
- The rendered `providers.env` is box-only and git-ignored; commit **only the
  encrypted** `vault.yml`.

## Seed only — `provision-seed.yml`

Re-seed (or seed) the admin without redeploying binaries:

```sh
ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/provision-seed.yml
```

## Layout

- `site.yml` — full deploy (fetch → deploy → keys → seed → start).
- `provision-seed.yml` — seed only.
- `tasks/{fetch,deploy,keys,seed,start}.yml` — the steps (seed is shared).
- `group_vars/<client>/vault.yml` — per-client provider keys (ansible-vault).
- `group_vars/example-client.yml` — copy this to make a real client's vault.
- `templates/{context-pilot,caddy}.service.j2` — the systemd units.
- `templates/{seed.env.j2,providers.env.j2,admin-sheet.txt.j2}` — the seed/keys
  env files + delivery sheet.
- `../photonicat/Caddyfile` — bootstrap Caddyfile (the orchestrator regenerates it).
- `examples/inventory.example.ini` — copy to `inventory.ini`.

## Guarantees & hygiene

- **Unique per unit** (Obj 6.1.1): each host gets a fresh 20-char random password
  (forced fixed with `-e cp_admin_password=…`), generated once via `set_fact`.
- **Printable sheet** (6.1.2): `out/<unit>-admin.txt`, rendered on the control node.
- **Secret hygiene** (6.1.3): `seed.env` is `0600`; the password is `no_log`
  throughout; `seed.env`, `out/`, and `.artifacts/` are all git-ignored.
- **Default email** (6.2.1): `admin@admin.fr`, changed during the wizard.
- **Armbian/Debian**: we log in as root and a minimal image may lack sudo →
  `become: false`. The box needs `python3` (present on the Armbian image) for the
  Ansible modules.

## Idempotence

Re-running is safe: the appliance bundle is re-shipped and re-extracted, and a
unit that already has `seed.env` keeps its password (never rotated, no stale
sheet). To re-provision a unit from scratch, wipe `/opt/context-pilot` on the box
first.
