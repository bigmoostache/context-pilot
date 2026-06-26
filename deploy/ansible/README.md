# Appliance provisioning (Ansible) — admin seed

Generates a **unique per-unit** admin password, installs it as the box's
`seed.env` (chmod 600, git-ignored), and prints a delivery sheet for the IT
operator. This is the Milestone 6 "paper password" flow of the
`local-tls-onboarding` design.

## Run

```sh
cp deploy/ansible/inventory.example.ini deploy/ansible/inventory.ini
# edit inventory.ini with each box's LAN IP

ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/provision-seed.yml
# optional: override the default admin email (admin@admin.fr) for all hosts
#   -e cp_admin_email=ops@acme.corp
```

After a run, the printable sheets land in `deploy/ansible/out/<unit>-admin.txt`
(git-ignored). Hand the matching sheet to whoever installs each unit; it carries
the one-time email + password and the first-time setup steps.

## Guarantees (M6)

- **6.1.1 — unique per unit**: each host gets a fresh 20-char random password
  (`lookup('password', …)`), set once per run via `set_fact`.
- **6.1.2 — printable sheet**: `out/<unit>-admin.txt` per unit, rendered on the
  control node.
- **6.1.3 — secret hygiene**: `seed.env` is written `0600` on the box and is
  git-ignored (along with `out/`); the password is `no_log` throughout, so it
  never appears in the Ansible output.
- **6.2.1 — default email**: `CP_SEED_ADMIN_EMAIL` defaults to `admin@admin.fr`;
  the operator changes it during the first-login wizard.

## Idempotence

A unit that already has `seed.env` is skipped — re-running never rotates a live
unit's password or prints a stale sheet. To re-provision a unit, remove its
`seed.env` on the box first.

> The orchestrator only seeds the admin while the user table is empty, and forces
> a password change on first login — so the paper password's exposure window is
> bounded by LAN-only access (M1) + that forced change (M5).
