# Appliance provisioning (Ansible)

Factory-fresh Photonicat → fully deployed and ready, in one pass.

## Full deploy — `site.yml`

```sh
cp deploy/ansible/inventory.example.ini deploy/ansible/inventory.ini
# edit inventory.ini with each box's LAN IP

ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml
```

What it does, end to end:

1. **Fetch** (control node — needs internet): downloads the **pre-built appliance
   bundle** (`cpilot-appliance-aarch64.tar.gz` = `bin/cp-orchestrator`, `bin/tui`,
   `web/<spa>`) from GitHub Releases — produced by
   `.github/workflows/release.yml` — plus a stock `caddy` arm64 binary. No local
   build/toolchain needed. Pin a tag with `-e release=v0.1.0-abc1234` (default:
   `latest`). The procd init scripts + bootstrap Caddyfile come from this repo.
2. **Deploy** (each box): ships the binaries + SPA + procd init scripts +
   bootstrap Caddyfile, and frees TCP `:80`/`:443` from the vendor admin web
   (moves it to `:8088`).
3. **Seed** (Obj 6): writes a unique per-unit admin `seed.env` (chmod 600) and a
   printable delivery sheet on the control node (`out/<unit>-admin.txt`).
4. **Start**: enables + starts the orchestrator (which seeds the admin and writes
   the real Caddyfile) then Caddy, and waits until both answer.

> Requires a published release that contains `cpilot-appliance-aarch64.tar.gz`
> (cut by pushing a tag → `release.yml`). The control node fetches it; the
> offline LAN box never needs internet.

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

## Seed only — `provision-seed.yml`

Re-seed (or seed) the admin without redeploying binaries:

```sh
ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/provision-seed.yml
```

## Layout

- `site.yml` — full deploy (fetch → deploy → seed → start).
- `provision-seed.yml` — seed only.
- `tasks/{fetch,deploy,seed,start}.yml` — the steps (seed is shared).
- `templates/{seed.env.j2,admin-sheet.txt.j2}` — the seed file + delivery sheet.
- `inventory.example.ini` — copy to `inventory.ini`.

## Guarantees & hygiene

- **Unique per unit** (Obj 6.1.1): each host gets a fresh 20-char random password
  (forced fixed with `-e cp_admin_password=…`), generated once via `set_fact`.
- **Printable sheet** (6.1.2): `out/<unit>-admin.txt`, rendered on the control node.
- **Secret hygiene** (6.1.3): `seed.env` is `0600`; the password is `no_log`
  throughout; `seed.env`, `out/`, and `.artifacts/` are all git-ignored.
- **Default email** (6.2.1): `admin@admin.fr`, changed during the wizard.
- **OpenWrt**: root login, no sudo → `become: false`. The box needs `python3`
  (present on photonicatWrt) for the Ansible modules.

## Idempotence

Re-running is safe: binaries are checksum-compared (re-copied only when changed),
`:80` freeing is a no-op once done, and a unit that already has `seed.env` keeps
its password (never rotated, no stale sheet). To re-provision a unit from scratch,
wipe `/mnt/data/context-pilot` on the box first.
