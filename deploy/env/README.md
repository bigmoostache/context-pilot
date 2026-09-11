# Environment profiles

Complete, commented `.env` templates — every variable the binaries read, set or
commented — one per deployment shape. The reference (types, defaults, the
combinations refused at boot) is [`docs/ENV.md`](../../docs/ENV.md); both
binaries validate their whole environment at boot and `--check-env` prints the
report without starting anything.

| Profile | File | Login | Day-0 identity/TLS | IT pane | Network applier | Updater | Keys from cockpit |
|---|---|---|---|---|---|---|---|
| On-premise, workstation | [`on-premise-workstation.env.example`](on-premise-workstation.env.example) | off | no | no | no | no | yes |
| On-premise, server (home lab, company) | [`on-premise-server.env.example`](on-premise-server.env.example) | yes | yes | yes | optional (NetworkManager) | yes | yes |
| Embedded, Photonicat 2 | [`embedded-photonicat.env.example`](embedded-photonicat.env.example) | yes | yes | yes | yes | yes | yes |
| Cloud tenant (Daharness) | `cloud/back/tenant/tenant.env.example` in the Daharness repository | yes | no | no | no | no | no |

The Docker image's own `.env.example` lives in [`../docker/`](../docker/.env.example)
(the container profile the cloud tenant builds on). On the appliance, Ansible
renders the embedded profile into the systemd unit; the template documents it.

A `.env` file is merged in override mode — a value in it beats the process
environment — which is why no template ever sets `CP_BRIDGE` / `CP_BRIDGE_URL`:
the orchestrator sets those on every agent it spawns.
