# Provisionnement d'une box Context Pilot — procédure complète

> De la box (Armbian/Debian 13, systemd) au cockpit en prod. Cible matérielle : Photonicat 2 (**RK3576**) sur l'**image officielle Armbian Debian 13** (systemd, aarch64). Flashage : **zéro-touch via `photonicat/emmc-install/`** (SD installatrice → `dd` Armbian sur l'eMMC + clé root injectée), ou manuel `photonicat/docs/debian2-flash-protocol.md`. Détails playbook : `ansible/README.md`. Décisions & caveats : sections en bas.

## Deux plans d'accès (à garder en tête)

- **Vendeur (nous) → le tailnet uniquement** : SSH/admin à distance, outbound-only, par MagicDNS.
- **Client → son réseau local** : cockpit `:443` + maintenance IT `:9090` à l'IP LAN de la box.

## Allocation des ports sur la box (après déploiement)

| Port | Service | Accès |
| --- | --- | --- |
| `:80`/`:443` | **Caddy → cockpit Context Pilot** (`:80` redirige vers `:443`) | client (LAN) |
| `:9090` | **Caddy → maintenance IT** (orchestrateur loopback `:9191`) | client (LAN) + nous (tailnet) |
| `:22` | SSH | nous (tailnet, Tailscale SSH) |

> Sur l'image Armbian standard, rien ne se dispute `:80/:443` (pas de LuCI, pas de `pcat-manager-web`) : Caddy prend les deux ports directement, aucun *free-port* à jouer. Le daemon hardware `pcat-manager` (cellulaire/batterie), s'il est présent sur l'image, reste actif — il n'expose pas de web sur `:80`.

---

## Phase 0 — Control plane Tailscale (une fois, console web)

1. Créer le tailnet (login.tailscale.com), y connecter ta machine ops. Activer **MagicDNS**.
2. **Access Controls** → policy file, puis **Save** (sinon les tags sont refusés) :
   - `groups.group:ops` = tes identités (ex. `Anima879@github`)
   - `tagOwners."tag:cp-<client>"` = `["group:ops"]`
   - `acls` : `group:ops` → `tag:cp-<client>` sur `:22/:9090/:7878`
   - `ssh` : `group:ops` → `tag:cp-<client>`, user `root` (Tailscale SSH, pas de clé à distribuer)
3. **Settings → Keys** : générer une auth-key **taguée** `tag:cp-<client>`**, reusable, non-ephemeral, pre-approved**. C'est un secret → Vault / fichier local, jamais commité.

## Phase 0 bis — Control node (machine qui lance Ansible)

- Sur le tailnet. Venv + Ansible : `python3 -m venv .venv && ./.venv/bin/pip install ansible` (`.venv` gitignoré).

## Phase 1 — Day-0 sur la box (Armbian Debian)

**Voie recommandée — installeur eMMC zéro-touch** (`photonicat/emmc-install/`) : une SD installatrice flashe l'image Armbian pristine sur l'eMMC, **injecte la clé root**, vérifie (sha256) et s'éteint — progression sur le LCD. Retirer la SD, rallumer → la box boote Armbian sur l'eMMC (identité régénérée + rootfs agrandi par l'Armbian first-run), joignable en `ssh root@<ip-lan>`. Détails : `photonicat/emmc-install/README.md`. Sur un reflash, l'empreinte d'hôte change → `ssh-keygen -R <ip>` d'abord.

*(Alternative manuelle : flasher via `photonicat/docs/debian2-flash-protocol.md`, booter, `ssh-copy-id`.)*

Le **hostname `dh-<serial>` et le user `dh`** ne sont PAS posés ici : c'est Ansible (`bringup`, Phase 3) qui s'en charge.

**Tailscale — encore manuel** (le câblage dans Ansible `bringup` est un TODO) :

```sh
curl -fsSL https://tailscale.com/install.sh | sh    # dépôt apt officiel + service systemd
systemctl enable --now tailscaled
tailscale up --authkey=<key> --advertise-tags=tag:cp-<client> \
             --hostname=<unit> --ssh --accept-routes=false
```

La box est joignable en `<unit>.<tailnet>.ts.net`. Vérifier (console) : tag OK + **Key expiry disabled**.

> **État réel (2026-07-25)** : la chaîne **SD → eMMC → Ansible est validée bout-en-bout sur RK3576** (flash + clé root + `bringup` hostname `dh-<serial>` / user `dh` + deploy stable *et* nightly, services actifs, API 200 ; box de test `192.168.1.38` / `dh-7681f2a227e0f10d`). **Tailscale reste manuel** : l'overlay a été validé le 2026-06-27 mais **sur l'ancienne box OpenWrt** — le chemin apt/systemd Debian n'a pas encore été rejoué. Le break-glass LAN (clé) reste le fallback.

## Phase 2 — Choisir la release (control node)

**Voie recommandée — canal signé** : le playbook résout `channels/stable.json` (défaut) ou `nightly.json` sur la branche `channels`, épingle l'URL de l'artefact et **vérifie le sha256** au téléchargement. Rien à builder — passer `-e channel=stable` (défaut) ou `-e channel=nightly`.

Alternatives : un tag précis `-e release=v0.x.y`, ou un build local `-e release=local` (via `deploy/photonicat/build.sh` → `.artifacts/`, utile en dev — bâtit la SPA par `npm run build`/Vite, ce qui contourne les erreurs `tsc -b`).

> L'ancienne consigne « `release=latest` est mort → build local » est **caduque** : le canal signé est désormais la voie normale (et pointe sur des releases qui shippent bien `cpilot-linux-aarch64-musl.tar.gz`).

## Phase 3 — Déployer via Ansible (par le tailnet, ou LAN break-glass)

1. **Inventaire** : `cp examples/inventory.example.ini inventory.ini`, `ansible_host=<unit>.<tailnet>.ts.net`, un groupe par client. (`inventory.ini` gitignoré.) Pour le break-glass, `ansible_host=<ip-lan>` + clé.
2. **Secrets au lancement** (jamais commités) :

   ```sh
   cp deploy/ansible/examples/secrets.example.yml deploy/ansible/<client>.local.yml
   $EDITOR deploy/ansible/<client>.local.yml      # cp_provider_keys + cp_admin_email/password
   chmod 600 deploy/ansible/<client>.local.yml
   ```
3. **Lancer** :

   ```sh
   ./.venv/bin/ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml \
     --limit <client> -e @deploy/ansible/<client>.local.yml -e channel=stable
   ```

   Le playbook (`site.yml`, systemd) : **bringup** (hostname `dh-<serial>` + user `dh` avec clé root + NOPASSWD sudo) → **fetch** (canal signé, sha256, control node) → **deploy** (binaires/SPA/units systemd/Caddyfile sous `/opt/context-pilot`) → **keys** (`providers.env`) → **seed** (admin write-once + fiche `out/<unit>-admin.txt`) → **start** (units `enable`+`start`, sondes santé) → **display** (driver LCD GC9307). Pas de manipulation de firewall/ports : l'image Armbian n'a que `:22` ouvert.

## Phase 3 bis — (optionnel) Claude Code OAuth par abonnement

> Cas particulier, **hors** `site.yml` : par défaut les providers sont en clé API (`cp_provider_keys` → `providers.env`). À n'utiliser que si le client paie en **abonnement Claude Pro/Max** plutôt qu'en clé API console. La box ne touche à rien : elle **lit** seulement le fichier déposé (pas de flow OAuth, pas de refresh — le backend **rejette un token expiré**, d'où le token longue durée).

1. **Générer un token longue durée** (sur ta machine, abonnement Pro/Max requis) :

   ```sh
   claude setup-token        # ~1 an. PAS `/login` (access token = quelques heures)
   ```
2. **Fabriquer le credentials file** à la forme attendue (`setup-token` affiche le token mais n'écrit pas forcément ce JSON) :

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

   Écrit `~/.claude/.credentials.json` (0600) sous le `HOME` de l'orchestrateur ; refuse un token mal formé/expiré ; affiche la date d'expiration.

> **Rotation** : pas de refresh côté box → relancer ce playbook avant l'expiration (modèle identique à la rotation des clés). `expiresAt` n'est qu'une garde locale ; le cale sur la vraie durée du token (le mettre au-delà = tentatives 401 inutiles). **Côté code** (déjà en place) : le cockpit surface « Claude Code (OAuth) » dès que ce fichier est présent et non expiré — cf. `inspect/providers/oauth_creds.rs`(`claude_oauth_available`), en lecture seule.

## Phase 4 — Onboarding (IT, depuis le LAN client, à l'IP de la box)

1. Navigateur → `https://<box-LAN-IP>:9090` (avertissement TLS — CA privée, attendu).
2. Se connecter (identifiants de la fiche de livraison), changer le mdp, fixer l'email admin réel.
3. **Nommer la box**, télécharger la **CA root** (à pousser aux postes via GPO/MDM), **Finalize**.
4. → le cockpit monte sur `:443` (`https://<box>`), CA installée = plus d'avertissement.

---

## Exploitation courante

- **Admin distant** : `tailscale ssh root@<unit>` (sans clé) ; re-run Ansible par le tailnet.
- **Rotation des clés** : éditer `<client>.local.yml`, re-run `--limit <client>` (seed admin intact).
- **MàJ app** : re-run `site.yml` (push). \[Pull-agent à manifeste signé = cible long terme, cf `docs/update-policy.md`.\]
- **MàJ Tailscale** : `apt update && apt install tailscale` puis `systemctl restart tailscaled`(systemd détache le restart de ta session — pas d'auto-coupure, cf caveat).

## Contexte & décisions (figées le 2026-06-27, révisées Debian le 2026-07-05)

- **OS = Armbian Debian 13 / systemd.** La box est reflashée sur l'image officielle Armbian ; Context Pilot tourne sous deux units systemd (`context-pilot`, `caddy`), racine `/opt/context-pilot`. (L'ancien chemin d'usine OpenWrt/procd a été retiré.)
- **Accès distant = Tailscale.** SaaS d'abord, **Headscale en migration** (client identique → bascule = un flag `--login-server`). Nœuds **tagués par client**, **Tailscale SSH** (pas de clé distribuée), auth-key taguée **reusable → single-use** à l'industrialisation. C'est aussi une **hypothèse de sécurité du design auth** (le transport chiffré est supposé par le modèle bearer-token/CORS).
- **Tailscale via le dépôt apt officiel Debian** (`install.sh`), service systemd `tailscaled`.
- **Day-0 = installeur eMMC zéro-touch** (`photonicat/emmc-install/`, RK3576 validé 2026-07-25) : SD installatrice → `dd` Armbian pristine + clé root injectée, puis Ansible (`bringup` hostname/user, deploy). Remplace le plan « scripter `bootstrap.sh` → image bakée ». **Tailscale reste à intégrer dans `bringup`** (encore manuel).
- **MàJ app = push Ansible** pour le bootstrap ; **pull à manifeste signé = en place côté produit** (canaux `channels/*.json` + **minisign** vérifié par l'orchestrateur OTA, clé `UPDATE_PUBKEY`). Ansible vérifie le **sha256** du canal ; la vérif minisign côté Ansible reste un TODO.
- **Secrets au lancement** (`-e @file` gitignoré), jamais commités — pas de vault (option ansible-vault dispo).
- **Control node** : laptop maintenant → **VPS bastion tagué** quand la flotte grossit (concentre root sur toutes les box + le déchiffrement des secrets → cible à durcir).

## Caveats / landmines opérationnelles

- **Releases = canal signé** : `-e channel=stable|nightly` (sha256 vérifié) est la voie normale ; `-e release=<tag>|local` restent dispo. L'ancien « `release=latest` est mort » n'est plus d'actualité.
- **Cert maintenance / IP.** Le cert `tls internal` de Caddy est lié à l'**identité** (IP LAN + nom). Tester via l'**IP réelle** de la box, pas `127.0.0.1` (SNI/identité ≠ loopback → `tlsv1 alert internal error`). Le **nom** (`pilot.acme.fr`) est **inerte tant que le DNS client ne pointe pas dessus** — accès par IP en attendant. Nommer la box ≠ créer le DNS : prévoir l'enregistrement A côté client.
- **Copie SPA lente.** Le déploiement ship+untar **un** tarball de \~19 Mo (`unarchive`) plutôt qu'une copie récursive par fichier (centaines de fonts KaTeX → round-trips SFTP + checksum, timeout 2 min). Lancer le playbook **en tâche de fond** (le foreground a un cap 2 min).
- **Upgrade Tailscale.** `tailscale ssh` passe par une session gérée par `tailscaled` → redémarrer le daemon coupe la session SSH. Sous systemd, `systemctl restart tailscaled` **détache** le restart de ta session (il survit) ; via `tailscale up` interactif, préférer le LAN break-glass.
- **Key-expiry.** Vérifier **« Key expiry disabled »** sur chaque nœud tagué (sinon il tombe du tailnet à \~180 j). C'est automatique pour les nœuds tagués mais à confirmer en console.
- **Firewall = responsabilité IT client.** `:9090` reste gardé par l'auth admin ; le périmètre réseau (qui atteint la box sur le LAN) est du ressort de l'IT. L'image Armbian n'ouvre que `:22` par défaut.

## Reste à industrialiser (non bloquant)

- [x] Day-0 zéro-touch → **fait** via `photonicat/emmc-install/` (SD → eMMC + clé root, validé RK3576 2026-07-25). Reste à **archiver/épingler** l'image Armbian de base dans notre stockage.

- [ ] **Intégrer l'enrôlement Tailscale dans Ansible `bringup`** (clé taguée réutilisable, `--ssh`) — encore manuel ; à rejouer/valider **sur Debian/systemd** (validé jusqu'ici sur l'ancienne box OpenWrt).

- [x] Release via **canal signé** (`channels/*.json`, sha256) → **fait** (`-e channel=stable|nightly`). Sortie de `release=local` obtenue.

- [ ] Révoquer les auth-keys de provisioning après usage.

- [x] Signer les artefacts + vérif on-device → **fait côté produit** (minisign, orchestrateur OTA). Reste : vérif **minisign côté Ansible** (aujourd'hui sha256 seul).

- [ ] Control node : passer du laptop à un VPS bastion tagué quand la flotte grossit.

- [ ] Onboarding jour-0 dans le navigateur (`:9090`) : nommer la box, pousser la CA, `Finalize` (box de test actuelle : `dh-7681f2a227e0f10d` / `192.168.1.38`, `provisioned=false`).