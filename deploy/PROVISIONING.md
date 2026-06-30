# Provisionnement d'une box Context Pilot — procédure complète

> De la box d'usine au cockpit en prod. Validé end-to-end sur `cp-test-01` (2026-06-27).
> Détails : [`photonicat/runbook-day0.md`](./photonicat/runbook-day0.md) (Phase 1 pas à pas),
> [`ansible/README.md`](./ansible/README.md) (playbook). Décisions & caveats : sections en bas.

## Deux plans d'accès (à garder en tête)
- **Vendeur (nous) → le tailnet uniquement** : SSH/admin à distance, outbound-only, par MagicDNS.
- **Client → son réseau local** : cockpit `:443` + maintenance IT `:9090` à l'IP LAN de la box.

## Allocation des ports sur la box (après déploiement)
| Port | Service | Accès |
|------|---------|-------|
| `:80`/`:443` | **Caddy → cockpit Context Pilot** (`:80` redirige vers `:443`) | client (LAN) |
| `:9090` | **Caddy → maintenance IT** (orchestrateur loopback `:9191`) | client (LAN) + nous (tailnet) |
| `:8088` | **uhttpd / LuCI** (config hardware) | IT client |
| `:22` | SSH | nous (tailnet, Tailscale SSH) |

> Le board Photonicat embarque deux UIs web qui se disputent `:80/:443` ; le déploiement
> (`free-port-80.sh`) les écarte : **LuCI → `:8088`** (conservé pour l'IT), **`pcat-manager-web`
> (dashboard board, port `:80` codé en dur) → DÉSACTIVÉ** (décision produit : le client voit Context
> Pilot, l'IT utilise LuCI). Le daemon hardware `pcat-manager` (cellulaire/batterie) reste actif —
> seul le dashboard *web* est retiré.

---

## Phase 0 — Control plane Tailscale (une fois, console web)
1. Créer le tailnet (login.tailscale.com), y connecter ta machine ops. Activer **MagicDNS**.
2. **Access Controls** → policy file, puis **Save** (sinon les tags sont refusés) :
   - `groups.group:ops` = tes identités (ex. `Anima879@github`)
   - `tagOwners."tag:cp-<client>"` = `["group:ops"]`
   - `acls` : `group:ops` → `tag:cp-<client>` sur `:22/:9090/:7878`
   - `ssh` : `group:ops` → `tag:cp-<client>`, user `root` (Tailscale SSH, pas de clé à distribuer)
3. **Settings → Keys** : générer une auth-key **taguée `tag:cp-<client>`, reusable, non-ephemeral,
   pre-approved**. C'est un secret → Vault / fichier local, jamais commité.

## Phase 0 bis — Control node (machine qui lance Ansible)
- Sur le tailnet. Venv + Ansible : `python3 -m venv .venv && ./.venv/bin/pip install ansible` (`.venv` gitignoré).

## Phase 1 — Day-0 sur la box (manuel, via l'AP WiFi `172.16.0.1`)
> Runbook détaillé : [`photonicat/runbook-day0.md`](./photonicat/runbook-day0.md).
1. `ssh root@172.16.0.1` (creds d'usine).
2. **Installer Tailscale — binaire statique officiel** (le feed opkg fige 1.76.1 = CVE) :
   ```sh
   opkg list-installed | grep -q ca-bundle || { opkg update && opkg install ca-bundle kmod-tun; }
   cd /tmp && V=1.98.4
   wget "https://pkgs.tailscale.com/stable/tailscale_${V}_arm64.tgz" && tar xzf tailscale_${V}_arm64.tgz
   install -m0755 tailscale_${V}_arm64/tailscale{,d} /usr/sbin/
   ```
3. Poser le service procd : `scp deploy/photonicat/init.d/tailscale.init root@172.16.0.1:/etc/init.d/tailscale`
   puis `chmod +x … && /etc/init.d/tailscale enable && start`.
4. Enrôler : `tailscale up --authkey=<key> --advertise-tags=tag:cp-<client> --hostname=<unit> --ssh --accept-routes=false`
5. La box est joignable en `<unit>.<tailnet>.ts.net`. Vérifier (console) : tag OK + **Key expiry disabled**.

## Phase 2 — Construire l'artefact (control node)
```sh
deploy/photonicat/build.sh        # cross-compile aarch64 + SPA → deploy/ansible/.artifacts/ (release=local)
```
(ou un tag GitHub Release : `-e release=v0.x.y`).

## Phase 3 — Déployer via Ansible (par le tailnet)
1. **Inventaire** : `cp examples/inventory.example.ini inventory.ini`, `ansible_host=<unit>.<tailnet>.ts.net`,
   un groupe par client. (`inventory.ini` gitignoré.)
2. **Secrets au lancement** (jamais commités) :
   ```sh
   cp deploy/ansible/examples/secrets.example.yml deploy/ansible/<client>.local.yml
   $EDITOR deploy/ansible/<client>.local.yml      # cp_provider_keys + cp_admin_email/password
   chmod 600 deploy/ansible/<client>.local.yml
   ```
3. **Lancer** :
   ```sh
   ./.venv/bin/ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml \
     --limit <client> -e @deploy/ansible/<client>.local.yml -e release=local
   ```
   Le playbook : **fetch** (artefact) → **deploy** (binaires/SPA/init/Caddyfile, free `:80`,
   **ouvre `:80/:443/:9090` côté client** via `open-client-firewall.sh`) → **keys** (`providers.env`)
   → **seed** (admin write-once + fiche `out/<unit>-admin.txt`) → **start** (+ sondes santé).

## Phase 3 bis — (optionnel) Claude Code OAuth par abonnement
> Cas particulier, **hors `site.yml`** : par défaut les providers sont en clé API
> (`cp_provider_keys` → `providers.env`). À n'utiliser que si le client paie en
> **abonnement Claude Pro/Max** plutôt qu'en clé API console. La box ne touche à
> rien : elle **lit** seulement le fichier déposé (pas de flow OAuth, pas de
> refresh — le backend **rejette un token expiré**, d'où le token longue durée).
1. **Générer un token longue durée** (sur ta machine, abonnement Pro/Max requis) :
   ```sh
   claude setup-token        # ~1 an. PAS `/login` (access token = quelques heures)
   ```
2. **Fabriquer le credentials file** à la forme attendue (`setup-token` affiche le
   token mais n'écrit pas forcément ce JSON) :
   ```sh
   TOKEN='sk-ant-oat01-…'                         # collé depuis setup-token
   EXP=$(( $(date -d '+1 year' +%s) * 1000 ))     # expiresAt (ms) = vie réelle du token
   mkdir -p ~/.claude
   printf '{"claudeAiOauth":{"accessToken":"%s","expiresAt":%s}}\n' "$TOKEN" "$EXP" \
     > ~/.claude/.credentials.json && chmod 600 ~/.claude/.credentials.json
   ```
3. **Déposer sur la box** (token jamais commité, reste sur ta machine) :
   ```sh
   ./.venv/bin/ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/claude-oauth.yml \
     --limit <client> -e oauth_creds_file=$HOME/.claude/.credentials.json
   ```
   Écrit `~/.claude/.credentials.json` (0600) sous le `HOME` de l'orchestrateur ;
   refuse un token mal formé/expiré ; affiche la date d'expiration.
> **Rotation** : pas de refresh côté box → relancer ce playbook avant l'expiration
> (modèle identique à la rotation des clés). `expiresAt` n'est qu'une garde locale ;
> le cale sur la vraie durée du token (le mettre au-delà = tentatives 401 inutiles).
> **Côté code** (déjà en place) : le cockpit surface « Claude Code (OAuth) » dès que
> ce fichier est présent et non expiré — cf. `inspect/providers/oauth_creds.rs`
> (`claude_oauth_available`), en lecture seule.

## Phase 4 — Onboarding (IT, depuis le LAN client, à l'IP de la box)
1. Navigateur → `https://<box-LAN-IP>:9090` (avertissement TLS — CA privée, attendu).
2. Se connecter (identifiants de la fiche de livraison), changer le mdp, fixer l'email admin réel.
3. **Nommer la box**, télécharger la **CA root** (à pousser aux postes via GPO/MDM), **Finalize**.
4. → le cockpit monte sur `:443` (`https://<box>`), CA installée = plus d'avertissement.

---

## Exploitation courante
- **Admin distant** : `tailscale ssh root@<unit>` (sans clé) ; re-run Ansible par le tailnet.
- **Rotation des clés** : éditer `<client>.local.yml`, re-run `--limit <client>` (seed admin intact).
- **MàJ app** : re-run `site.yml` (push). [Pull-agent à manifeste signé = cible long terme, cf §3 archi.]
- **MàJ Tailscale** : binaire statique ; ⚠️ restart détaché (pas par la session tailnet, cf runbook).

## Contexte & décisions (figées le 2026-06-27)
- **Accès distant = Tailscale.** SaaS d'abord, **Headscale en migration** (client identique → bascule
  = un flag `--login-server`). Nœuds **tagués par client**, **Tailscale SSH** (pas de clé distribuée),
  auth-key taguée **reusable → single-use** à l'industrialisation. C'est aussi une **hypothèse de
  sécurité du design auth** (le transport chiffré est supposé par le modèle bearer-token/CORS).
- **Tailscale via binaire statique officiel**, pas le feed opkg (qui fige une version **CVE** : 1.76.1).
- **Day-0 manuel d'abord** (runbook) → `bootstrap.sh` → gravure image (`/etc/uci-defaults`).
- **MàJ app = push Ansible** maintenant ; **agent pull à manifeste signé** = cible long terme (scale +
  tolérance offline). **Signer les artefacts** (cosign/minisign) dans les deux cas.
- **Board dashboard `pcat-manager-web` désactivé** (Caddy possède `:80/:443`) ; daemon hardware
  `pcat-manager` + LuCI (`:8088`) conservés pour l'IT.
- **Secrets au lancement** (`-e @file` gitignoré), jamais commités — pas de vault.
- **Control node** : laptop maintenant → **VPS bastion tagué** quand la flotte grossit (concentre root
  sur toutes les box + le déchiffrement des secrets → cible à durcir).

## Caveats / landmines opérationnelles
- **Upgrade Tailscale = self-cut.** `ssh root@<unit>.<tailnet>` passe par Tailscale SSH (session
  **fille de `tailscaled`**) → redémarrer `tailscaled` **tue ta propre session**, et busybox n'a pas
  `setsid`. Upgrader en **restart détaché** (`start-stop-daemon -b`) ou via le LAN break-glass (cf. runbook).
- **Key-expiry.** Vérifier **« Key expiry disabled »** sur chaque nœud tagué (sinon il tombe du tailnet
  à ~180 j). C'est automatique pour les nœuds tagués mais à confirmer en console.
- **Firewall.** `eth0` (l'IP que le client utilise) est en zone OpenWrt **`wan` (REJECT)** →
  `open-client-firewall.sh` ouvre `:80/:443/:9090`. `:9090` reste gardé par l'auth admin ; le périmètre
  réseau est la responsabilité de l'IT client.
- **Cert maintenance / nom.** Le cert est lié à l'identité (IP LAN **+** nom). Le **nom** (`pilot.acme.fr`)
  est **inerte tant que le DNS client ne pointe pas dessus** — accès par IP en attendant. Nommer la box
  ≠ créer le DNS : prévoir l'enregistrement A côté client.
- **`tailscale.init`.** Le `stop_service` ne fait **pas** `tailscale down` (sinon `wantRunning=false`
  persiste → nœud *down* après reboot/restart, casse l'auto-rejoin).
- **`/mnt/data`.** Sur une box reset ce n'est pas une partition séparée (retombe sur l'overlay f2fs,
  persistant) ; sans impact Tailscale, mais le « big data partition » du déploiement devra être monté à part.

## Reste à industrialiser (non bloquant)
- [ ] Scripter la Phase 1 en `bootstrap.sh`, puis graver dans `/etc/uci-defaults` (reset usine → re-join auto).
- [ ] Révoquer les auth-keys de provisioning après usage.
- [ ] Signer les artefacts (cosign/minisign) + vérif on-device.
- [ ] Control node : passer du laptop à un VPS bastion tagué quand la flotte grossit.
