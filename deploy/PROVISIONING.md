# Provisionnement d'une box Context Pilot — procédure complète

> De la box (Armbian/Debian 13, systemd) au cockpit en prod. Cible matérielle : Photonicat 2 (**RK3576**) sur l'**image officielle Armbian Debian 13** (systemd, aarch64). Flashage : **zéro-touch via `photonicat/emmc-install/`** (SD installatrice → `dd` Armbian sur l'eMMC + clé root injectée + **ULA IPv6 dérivée du serial**), ou manuel `photonicat/docs/debian2-flash-protocol.md`. Détails playbook : `ansible/README.md`. Décisions & caveats : sections en bas.

## Trois plans d'accès (à garder en tête)

- **Vendeur (nous) → le tailnet uniquement** : SSH/admin à distance, outbound-only, par MagicDNS. Seul plan qui traverse les NAT ⇒ seul plan « flotte entière ».
- **Vendeur, sur site → l'ULA IPv6** : adresse **déterministe dérivée du serial**, posée par l'installeur eMMC, joignable sans DHCP ni routeur ni DNS. Plan **day-0 / break-glass**, valable uniquement depuis le **même segment L2** (le poste qui se connecte doit porter une adresse du même `/64`). SSH **et** cockpit : l'ULA est sujet du certificat, donc `https://[<ula>]/` sert la SPA.
- **Client → son réseau local** : cockpit `:443` à l'IP LAN de la box (ou à son nom, une fois le DNS posé).

## Allocation des ports sur la box (après déploiement)

Relevé sur matériel (RK3576, box de test, 2026-07-26) depuis un poste du LAN :

| Port | Service | Accès | Mesuré |
| --- | --- | --- | --- |
| `:443` | **Caddy → cockpit Context Pilot** (cert `tls internal`) | client (IP/nom) + nous (ULA) | ouvert, `200`, SPA servie sur l'IPv4 **et** sur l'ULA |
| `:80` | **Caddy** → `308` vers `https://<hôte>` une fois provisionnée ; **cockpit en clair sur n'importe quel hôte** avant | day-0 : quiconque atteint la box | ouvert |
| `:7878` | **orchestrateur** — l'upstream de Caddy, **bindé sur le loopback** | personne depuis le LAN | fermé de l'extérieur (`127.0.0.1` seulement) |
| `:22` | SSH | nous (ULA sur site, tailnet à distance) | ouvert |

> **Tout le trafic applicatif passe par Caddy.** L'orchestrateur écoute sur `127.0.0.1:7878` (défaut du binaire, réaffirmé par `CP_ORCH_BIND=127.0.0.1` dans l'unité systemd) : le backend n'a **aucune** socket face au LAN, donc le modèle d'auth (bearer-token/CORS) tient sa prémisse de transport chiffré, et le `308` de `:80` vers `:443` n'est plus contournable. Élargir le bind (`CP_ORCH_BIND=0.0.0.0`) est réservé au dev — jamais sur une box livrée. Le relevé du 26/07 ci-dessus est antérieur au correctif : `:7878` y répondait `200` en clair sur le LAN.
>
> **Corollaire pour nous.** Atteindre le backend sans Caddy (debug, `curl` d'API) passe désormais par un tunnel SSH — `ssh -L 8878:127.0.0.1:7878 root@<box>` puis `http://127.0.0.1:8878/…` — y compris depuis le tailnet, où `:7878` n'est plus ouvert. Le cockpit lui-même reste en `https://[<ula>]/` ou `https://<ip-lan>/`.
>
> **Caddy filtre par `Host`, et un hôte inconnu reçoit un `200` au corps vide.** Le code HTTP seul ne prouve donc rien : toujours vérifier la **taille du corps** (la SPA fait ~5,3 ko). Les hôtes servis sont l'IP et le nom saisis à l'onboarding **plus les ULA de la box**, ajoutées automatiquement ; avant provisionnement le day-0 répond sur n'importe quel hôte. `127.0.0.1` n'est jamais servi — tester par une adresse réelle.
>
> Sur l'image Armbian standard, rien ne se dispute `:80/:443` (pas de LuCI, pas de `pcat-manager-web`) : Caddy prend les deux ports directement, aucun *free-port* à jouer. Le daemon hardware `pcat-manager` **est présent** sur l'image Armbian (module DKMS `photonicat-pm`) : il gère PMU, batterie, RTC, ventilateur et bouton power — il n'expose pas de web.

---

## Phase 0 — Control plane Tailscale (une fois, console web)

1. Créer le tailnet (login.tailscale.com), y connecter ta machine ops. Activer **MagicDNS**.
2. **Access Controls** → policy file, puis **Save** (sinon les tags sont refusés) :
   - `groups.group:ops` = tes identités (ex. `Anima879@github`)
   - `tagOwners."tag:cp-<client>"` = `["group:ops"]`
   - `acls` : `group:ops` → `tag:cp-<client>` sur `:22/:443` (pas `:7878` : le backend est sur le loopback, on l'atteint par Caddy)
   - `ssh` : `group:ops` → `tag:cp-<client>`, user `root` (Tailscale SSH, pas de clé à distribuer)
3. **Settings → Keys** : générer une auth-key **taguée** `tag:cp-<client>`**, reusable, non-ephemeral, pre-approved**. C'est un secret → Vault / fichier local, jamais commité.

## Phase 0 bis — Control node (machine qui lance Ansible)

- Sur le tailnet. Venv + Ansible : `python3 -m venv .venv && ./.venv/bin/pip install ansible` (`.venv` gitignoré).

## Phase 1 — Day-0 sur la box (Armbian Debian)

**Voie recommandée — installeur eMMC zéro-touch** (`photonicat/emmc-install/`) : une SD installatrice flashe l'image Armbian pristine sur l'eMMC, **injecte la clé root**, **impose l'ULA IPv6 de flotte**, vérifie (sha256) et affiche `DONE` + l'adresse sur le LCD — **la carte reste allumée** pour qu'on puisse lire l'adresse (le signal de fin est l'écran, plus l'extinction). Noter l'adresse, **appuyer sur le bouton power**, attendre l'extinction, retirer la SD (tant que la carte tourne, c'est son rootfs), rallumer → la box boote Armbian sur l'eMMC (identité régénérée + rootfs agrandi par l'Armbian first-run), joignable en `ssh root@<ula>` **ou** `ssh root@<ip-lan>`. Détails : `photonicat/emmc-install/README.md`. Sur un reflash, l'empreinte d'hôte change → `ssh-keygen -R <adresse>` d'abord.

**Adresse de la box = son serial.** Le préfixe `/48` est une constante produit, le subnet-id est le port Ethernet, et l'identifiant d'interface est le serial device-tree tel quel :

```
serial 7681f2a227e0f10d   →   fd59:ec78:2da4:1:7681:f2a2:27e0:f10d   (port 1 ; port 2 = :2:)
```

Donc l'inventaire peut être écrit **avant** que la box ne boote — le suffixe affiché sur le LCD à la fin du flash n'est qu'un confort de vérification. Une fois par control node, sur l'interface qui fait face aux box :

```sh
sudo ip -6 addr add fd59:ec78:2da4:1::1/64 dev <iface>   # sans ça, aucune adresse source pour joindre l'ULA
ssh root@fd59:ec78:2da4:1:7681:f2a2:27e0:f10d
photonicat/tools/pcat-discover.sh --inventory            # « qu'est-ce qui est branché sur ce switch ? »
```

Le préfixe est dupliqué à **deux** endroits qui doivent rester synchrones : `ULA_PREFIX` (`photonicat/emmc-install/build/build-init-sd.sh`) et `box_ula_prefix` (`ansible/site.yml`). Ne jamais le regénérer : les box et le control node doivent partager le préfixe, et tout inventaire écrit contre l'ancien tombe.

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

> **État réel (2026-07-26)** : la chaîne **SD → eMMC → Ansible est validée bout-en-bout sur RK3576** (flash + clé root + `bringup` hostname `dh-<serial>` / user `dh` + deploy stable *et* nightly, services actifs, API 200 ; box de test `192.168.1.38` / `dh-7681f2a227e0f10d`). **Tailscale reste manuel** : l'overlay a été validé le 2026-06-27 mais **sur l'ancienne box OpenWrt** — le chemin apt/systemd Debian n'a pas encore été rejoué. Le break-glass LAN (clé) reste le fallback.
>
> **L'ULA IPv6 est validée sur matériel** (2026-07-26, RK3576, serial `7681f2a227e0f10d` → `fd59:ec78:2da4:1:7681:f2a2:27e0:f10d`). Vérifié : adresse présente **au premier boot** sur les deux ports (`:1:` sur `end0`, `:2:` sur `end1`, y compris lien down), identique à celle prédite avant tout démarrage ; deux runs `site.yml` **intégralement par l'IPv6** (canal `stable` v0.2.12 puis `nightly` v0.1.0-7dcc567, `failed=0`) ; survie à un reboot et à `networkctl reconfigure`. Les fichiers du kit ULA déposés par Ansible sortent en `ok` et non `changed` — donc identiques à l'octet près à ceux de l'installeur, ce qui vérifie l'invariant « une seule source de vérité ».
>
> Pile réseau réelle : **netplan → systemd-networkd** (NetworkManager n'est pas installé). Le hook dispatcher NM reste livré comme filet mais ne sert pas ici.
>
> **Cockpit sur l'ULA : validé** (2026-07-26, build local `-e release=local`). Le Caddyfile rendu porte quatre sujets — `192.168.1.38`, `pilot.acme.corp`, et les deux ULA entre crochets — `https://[fd59:…:f10d]/` renvoie `200` avec la SPA (5266 o), et le certificat réémis porte bien `IP Address:FD59:EC78:2DA4:1:7681:F2A2:27E0:F10D` (émetteur `Caddy Local Authority`). L'API `GET /api/it/identity` rend `detected.ipv4` + `detected.ulas`, consommés par le formulaire d'onboarding. **Non encore publié dans un canal** : un `-e channel=…` ramène l'ancien code.

## Phase 2 — Choisir la release (control node)

**Voie recommandée — canal signé** : le playbook résout `channels/stable.json` (défaut) ou `nightly.json` sur la branche `channels`, épingle l'URL de l'artefact et **vérifie le sha256** au téléchargement. Rien à builder — passer `-e channel=stable` (défaut) ou `-e channel=nightly`.

Alternatives : un tag précis `-e release=v0.x.y`, ou un build local `-e release=local` (via `deploy/photonicat/build.sh` → `.artifacts/` : cross-compile les trois binaires en musl aarch64 puis bâtit la SPA). Indispensable pour tester du code produit non encore publié — un `-e channel=...` ultérieur **écrase** ce build.

> Dépendances web installées par **pnpm** (`pnpm-lock.yaml`) : `npm ci` échoue, faute de `package-lock.json`. `build.sh` appelle encore `npm run build`, ce qui fonctionne (npm exécute le script du `package.json`) une fois `pnpm install` passé.

## Phase 3 — Déployer via Ansible (par le tailnet, ou LAN break-glass)

1. **Inventaire** : `cp examples/inventory.example.ini inventory.ini`, `ansible_host=<unit>.<tailnet>.ts.net`, un groupe par client. (`inventory.ini` gitignoré.) Pour le break-glass, `ansible_host=<ula>` (déterministe, cf Phase 1) ou `<ip-lan>` + clé.
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

   Le playbook (`site.yml`, systemd) : **bringup** (hostname `dh-<serial>` + user `dh` avec clé root + NOPASSWD sudo) → **fetch** (canal signé, sha256, control node) → **deploy** (binaires/SPA/units systemd/Caddyfile sous `/opt/context-pilot`) → **keys** (`providers.env`) → **seed** (admin write-once + fiche `out/<unit>-admin.txt`) → **start** (units `enable`+`start`, sondes santé) → **display** (driver LCD GC9307) → **modem** (outillage 5G). Aucune manipulation de firewall : l'image Armbian n'a pas de règles, mais le déploiement **ouvre `:80` et `:443`** — les deux ports de Caddy, et eux seuls : l'orchestrateur reste sur le loopback (cf. tableau des ports).

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

## Phase 4 — Onboarding (dans le navigateur, sur le LAN de la box)

Une box fraîchement déployée est **non provisionnée** : pas de certificat, donc le cockpit est servi **en clair sur `:80`, pour n'importe quel hôte**. C'est voulu — au day-0 personne ne connaît encore l'IPv4 du DHCP, on entre donc par l'adresse qui est connue d'avance.

1. Navigateur → `http://[<ula>]/` (nous, sur site — crochets obligatoires pour une IPv6) **ou** `http://<ip-lan>/` si l'IT connaît déjà l'adresse de son DHCP.
2. Se connecter (identifiants de la fiche de livraison), **changer le mot de passe**, fixer l'email admin réel.
3. **Identité** : le formulaire arrive **prérempli avec l'IPv4 que la box observe elle-même**, modifiable, et affiche à côté ses ULA de maintenance. Ajouter le nom DNS si le client en veut un, puis valider → la box se provisionne, Caddy réémet le certificat pour `IP + nom + ULA` et monte `:443` ; `:80` devient une redirection `308`.
4. **CA root** : télécharger, vérifier l'empreinte hors bande, la **pousser sur les postes** (GPO/MDM), puis continuer.
5. → cockpit sur `https://<ip-lan>` (ou `https://<nom>`), sans avertissement une fois la CA installée.

> Trois conditions pour que ce soit vraiment fini, et aucune n'est faite par la box : **l'enregistrement DNS** (nommer ≠ créer le A record), **la distribution de la CA**, et une **réservation de bail DHCP** — l'IPv4 devient sujet du certificat, un bail qui change la périme (l'ULA reste alors le chemin de secours).
>
> L'état de provisionnement vit dans `/opt/context-pilot/home/.context-pilot/agents/` : `.provisioned` et `.identity.json` (points initiaux — un `find -name identity.json` les manque).

---

## Phase 5 — Réseau : uplink (WAN / 5G) et point d'accès Wi-Fi

Optionnelle et **désactivée par défaut** côté Wi-Fi. Conception complète :
`docs/design-network-uplink.md`. Ce qui est installé par `site.yml`
(`tasks/net/network.yml`, sauté avec `-e cp_net_enabled=false`) :
NetworkManager (avec l'ethernet **non géré**, cf. caveats), `cp-regdom.service`
(domaine réglementaire) et `cp-uplink.service` (bascule WAN ⇄ 5G).

**Ansible ne configure rien — il *sème*.** `.network.json` est écrit **une seule
fois** (`0600`, à côté de `.identity.json`) ; ensuite l'admin IT est seul maître à
bord depuis le cockpit, et un `site.yml` rejoué ne défait pas ses choix. Re-semer
volontairement : `-e cp_net_force=true`.

### Depuis le cockpit (IT → uplink / Wi-Fi / 5G)

- **Uplink** — trois modes. `Ethernet` (câble seul), `Ethernet + 5G` (le 5G prend
  le relais quand le câble cesse de porter du trafic), `5G seul` (la route par
  défaut du câble est **supprimée**, câble branché ou non).
- **Point d'accès** — SSID, phrase secrète, bande, canal, **pays** (obligatoire),
  SSID masqué, et l'interrupteur « partager l'internet ». Sans partage le réseau
  reste utilisable : les clients obtiennent une adresse et joignent le cockpit,
  mais **rien n'est routé** vers l'extérieur.
- **5G** — APN, identifiants, PIN, itinérance, veille `hot`/`cold`.

Les secrets (phrase Wi-Fi, mot de passe porteur, PIN SIM) sont **en écriture
seule** : aucune lecture ne les renvoie. Laisser le champ vide conserve la valeur
enregistrée.

### Variables Ansible (valeurs par défaut)

`cp_net_mode=wan`, `cp_ap_enabled=false`, `cp_ap_country=FR`, `cp_ap_band=a`,
`cp_ap_share=true`, `cp_wwan_standby=hot`. APN, identifiants et phrase secrète
vont dans `<client>.local.yml` (secrets, non commités).

### Matrice validée sur `dh-7681f2a227e0f10d` (2026-07-29)

3 modes × {AP éteint, AP + partage, AP sans partage} = 9 combinaisons. Dans
**toutes**, `end0` conserve son bail DHCP **et** l'ULA de flotte, et le cockpit
répond `200`/5266 o sur l'IPv4 LAN **et** sur l'ULA.

| mode | AP | route par défaut | `ip_forward` | tables `nft` | cockpit LAN / ULA / `10.42.0.1` |
|---|---|---|---|---|---|
| `wan` | éteint | `end0` (100) | 0 | 0 | 200 / 200 / — |
| `wan` | + partage | `end0` (100) | 1 | 1 | 200 / 200 / 200 |
| `wan` | sans partage | `end0` (100) | 0 | 0 | 200 / 200 / 200 |
| `wan_5g` | éteint | `end0` (100) | 0 | 0 | 200 / 200 / — |
| `wan_5g` | + partage | `end0` (100) | 1 | 1 | 200 / 200 / 200 |
| `wan_5g` | sans partage | `end0` (100) | 0 | 0 | 200 / 200 / 200 |
| `5g` | éteint | **aucune** | 0 | 0 | 200 / 200 / — |
| `5g` | + partage | **aucune** | 1 | 1 | 200 / 200 / 200 |
| `5g` | sans partage | **aucune** | 0 | 0 | 200 / 200 / 200 |

Redémarrage en `5g` + AP allumé : tout revient seul (mode, AP sur le canal 36,
domaine `FR`, `ip_forward` conforme), boot total **13,7 s**, cockpit `200` sur les
trois adresses. Cas adverses rejoués : modem absent ⇒ `wwan: null` et le reste
reste honnête ; pays vide ⇒ `400` explicite ; `SIGKILL` en plein `apply` ⇒
fichier d'état intact et box joignable ; `pcat-ula` rejoué avec le drop-in strict
en place ⇒ le drop-in survit.

**Non validé, et pourquoi.** Le porteur 5G ne transporte pas de données sur cette
box : le réseau nominal (Bouygues, `20820`) n'est vu qu'à **RSRP ≈ −113 dBm**, le
modem s'attache par intermittence puis retombe en service limité. C'est un
problème **d'antenne/couverture**, pas de logiciel — le SIM ne demande pas de PIN,
la chaîne AT répond, et le refus d'Orange est la réponse *attendue* pour une SIM
domestique sans itinérance. À vérifier sur site : antennes principale et diversité
5G bien vissées sur les bons connecteurs u.FL/SMA (et pas interverties avec les
queues de cochon Wi-Fi), puis re-mesurer `AT+QENG="servingcell"` — viser mieux que
−100 dBm. La bascule de `cp-uplink` a donc été validée par **simulation de panne
amont** (cible de sonde bloquée par `nft`, câble et bail intacts), qui est le cas
que les métriques de route ne savent pas voir.

---

## Exploitation courante

- **Admin distant** : `tailscale ssh root@<unit>` (sans clé) ; re-run Ansible par le tailnet.
- **Rotation des clés** : éditer `<client>.local.yml`, re-run `--limit <client>` (seed admin intact).
- **MàJ app** : re-run `site.yml` (push). \[Pull-agent à manifeste signé = cible long terme, cf `docs/update-policy.md`.\]
- **MàJ Tailscale** : `apt update && apt install tailscale` puis `systemctl restart tailscaled`(systemd détache le restart de ta session — pas d'auto-coupure, cf caveat).
- **Éteindre une box** : `ssh root@<box> poweroff`. Par le bouton : **appui bref**, on relâche (cf caveat — un appui maintenu la rallume).

## Contexte & décisions (figées le 2026-06-27, révisées Debian le 2026-07-05)

- **OS = Armbian Debian 13 / systemd.** La box est reflashée sur l'image officielle Armbian ; Context Pilot tourne sous deux units systemd (`context-pilot`, `caddy`), racine `/opt/context-pilot`. (L'ancien chemin d'usine OpenWrt/procd a été retiré.)
- **Accès distant = Tailscale.** SaaS d'abord, **Headscale en migration** (client identique → bascule = un flag `--login-server`). Nœuds **tagués par client**, **Tailscale SSH** (pas de clé distribuée), auth-key taguée **reusable → single-use** à l'industrialisation. C'est aussi une **hypothèse de sécurité du design auth** (le transport chiffré est supposé par le modèle bearer-token/CORS).
- **Tailscale via le dépôt apt officiel Debian** (`install.sh`), service systemd `tailscaled`.
- **Day-0 = installeur eMMC zéro-touch** (`photonicat/emmc-install/`, RK3576 validé 2026-07-25) : SD installatrice → `dd` Armbian pristine + clé root injectée + **ULA IPv6 imposée**, puis Ansible (`bringup` hostname/user, deploy). Remplace le plan « scripter `bootstrap.sh` → image bakée ». **Tailscale reste à intégrer dans `bringup`** (encore manuel).
- **Identification day-0 = ULA IPv6 dérivée du serial** (RFC 4193, préfixe tiré une fois). Choix : l'adresse doit être connue **avant** le premier contact, sans dépendre du DHCP client, du mDNS (absent/désactivable) ni d'un scan (un `/64` ne s'énumère pas). Posée par l'installeur (source de vérité, avant tout accès) **et** ré-affirmée par Ansible `bringup` (rattrape les box déjà déployées, garde le déclaratif comme référence). Un `/64` par port Ethernet, sinon conflit DAD si les deux ports arrivent sur le même switch. Deux moitiés complémentaires : `ip addr replace` pour que l'adresse soit vivante tout de suite, et un `.network` par port pour que **networkd la possède** — sans quoi elle est purgée à la première reconfiguration (cf. caveats).
- **MàJ app = push Ansible** pour le bootstrap ; **pull à manifeste signé = en place côté produit** (canaux `channels/*.json` + **minisign** vérifié par l'orchestrateur OTA, clé `UPDATE_PUBKEY`). Ansible vérifie le **sha256** du canal ; la vérif minisign côté Ansible reste un TODO.
- **Secrets au lancement** (`-e @file` gitignoré), jamais commités — pas de vault (option ansible-vault dispo).
- **Control node** : laptop maintenant → **VPS bastion tagué** quand la flotte grossit (concentre root sur toutes les box + le déchiffrement des secrets → cible à durcir).

## Caveats / landmines opérationnelles

- **Releases = canal signé** : `-e channel=stable|nightly` (sha256 vérifié) est la voie normale ; `-e release=<tag>|local` restent dispo.
- **L'ULA doit être DÉCLARÉE à networkd, pas injectée.** Mesuré (systemd 257) : `networkctl reconfigure <if>` **supprime** une adresse que networkd n'a pas configurée lui-même — un renouvellement DHCP ou une coupure de porteuse suffit donc à perdre l'ULA. `ManageForeignAddresses` **n'existe pas** dans cette version (clé ignorée en silence). Comme un seul `.network` s'applique par lien (le premier qui matche) et que celui de netplan matche `e*` d'un coup, un drop-in ne peut pas porter une adresse *par port* : `pcat-ula` génère donc **un `.network` par interface** (`/etc/systemd/network/05-pcat-ula-<if>.network`), trié avant celui de la distro, héritant de son contenu **verbatim** (DHCP, RA, métriques) et y ajoutant `Address=`. Le `ip addr replace` impératif reste, pour que l'adresse soit vivante immédiatement — y compris avant le premier Ansible. Attention : `networkctl reload` **reconfigure** les liens dont le fichier a changé (vérifié sans perte de bail ni de session).
- **Bouton power : appui bref, on relâche.** Le bouton fonctionne (input `rk805 pwrkey`, `logind`, extinction en ~1 s), mais le PMU n'a qu'une source de rallumage — ce même bouton. Un appui **maintenu** éteint puis rallume aussitôt : neuf boots de test l'attestent, tous rapportés `power-on event: power button`. Sur une box sans DC branché (donc sur batterie), c'est la seule cause possible de redémarrage. Alternative propre : `ssh root@<box> poweroff` (l'arrêt logiciel passe par PSCI et coupe bien).
- **On ne peut pas savoir depuis une box quelle release elle exécute.** Le binaire répond `cp-orchestrator v0.1.0` quel que soit le canal déployé (chaîne figée côté Cargo) et rien sur disque ne marque la version. Seul le journal Ansible le dit. Corollaire : les numéros de canal sont trompeurs — `nightly` = `v0.1.0-7dcc567` (publié le 26/07) est **plus récent** que `stable` = `v0.2.12` (publié le 16/07).
- **ULA = portée L2 uniquement.** Ce n'est pas un accès distant : joignable seulement depuis le même segment que la box, et le control node doit porter une adresse du même `/64` (sinon aucune adresse source ⇒ échec, pas de repli). La découverte par multicast (`pcat-discover.sh`) peut aussi être cassée par de l'isolation client sur un AP Wi-Fi ou du MLD snooping agressif — se brancher en filaire dans ce cas. L'adressage reste bon même quand la découverte échoue : le serial suffit à calculer l'adresse.
- **ULA stable ⇒ empreinte SSH à purger plus souvent.** Avec une IP DHCP changeante, un reflash passait souvent inaperçu ; l'adresse étant maintenant fixe, chaque reflash produit un conflit d'empreinte à la même adresse → `ssh-keygen -R <ula>` fait partie de la procédure de reflash.
- **Cert lié à l'identité.** Le cert `tls internal` couvre `IP LAN + nom + ULA de chaque port`. Tester via une **adresse réelle** de la box, pas `127.0.0.1` (SNI/identité ≠ loopback → `tlsv1 alert internal error`). Le **nom** est **inerte tant que le DNS client ne pointe pas dessus** — accès par IP en attendant. Si l'IPv4 épinglée change (bail DHCP), son bloc de site et son SAN se périment : le cockpit reste joignable **par l'ULA**, et un re-provisionnement remet l'IPv4 à jour.
- **Copie SPA lente.** Le déploiement ship+untar **un** tarball de \~19 Mo (`unarchive`) plutôt qu'une copie récursive par fichier (centaines de fonts KaTeX → round-trips SFTP + checksum, timeout 2 min). Lancer le playbook **en tâche de fond** (le foreground a un cap 2 min).
- **Upgrade Tailscale.** `tailscale ssh` passe par une session gérée par `tailscaled` → redémarrer le daemon coupe la session SSH. Sous systemd, `systemctl restart tailscaled` **détache** le restart de ta session (il survit) ; via `tailscale up` interactif, préférer le LAN break-glass.
- **Key-expiry.** Vérifier **« Key expiry disabled »** sur chaque nœud tagué (sinon il tombe du tailnet à \~180 j). C'est automatique pour les nœuds tagués mais à confirmer en console.
- **NetworkManager ne doit JAMAIS toucher à l'ethernet.** NM purge les adresses qu'il n'a pas posées lui-même, exactement comme networkd : s'il attrape `end0`, il emporte l'ULA de flotte, c'est-à-dire le seul chemin de secours de toute la flotte. Le fichier `/etc/NetworkManager/conf.d/10-cp-unmanaged.conf` (`interface-name:end*;lan*;wan*`) est posé **avant** le paquet, et c'est un pré-requis dur de son installation. Le hook dispatcher `50-pcat-ula` déjà en place est le filet.
- **`NetworkManager-wait-online.service` doit rester masqué.** Installer `network-manager` l'active ; il est `WantedBy=network-online.target`, `context-pilot.service` attend cette cible, et son `ExecStart` est `nm-online -s` avec `TimeoutStartUSec=infinity`. Dès que `cp-wwan` passe en `autoconnect yes` (modes `wan_5g`/`5g`), « NM startup complete » attend un modem qui, sans SIM ou sans couverture, peut ne jamais se connecter : délai **non borné** devant le cockpit. `systemd-networkd-wait-online` couvre déjà l'uplink réel.
- **Pays réglementaire = pré-requis fonctionnel du Wi-Fi.** Sous le domaine mondial `00`, `iw phy phy0 info` marque **89** canaux `no IR` (toute la bande 5 GHz, plus 2.4 GHz ch. 12/13) et un AP ne peut pas démarrer dessus. `iw reg set FR` ramène ce compte à **0**. Les deux phys sont `(self-managed)`, ce qui laisse croire que l'indication sera ignorée : mesuré, `phy0` (ath11k) l'honore, `phy1` (aic8800) reste à `00` pour toujours — ne jamais lire le premier bloc `country` venu, lire celui de `global`.
- **Le sous-réseau de l'AP a besoin de son propre site Caddy.** Caddy écoute sur `*:443` mais le Caddyfile généré **énumère des adresses de site explicites** : sans `10.42.0.1` dans la liste, un client Wi-Fi reçoit un `internal error` TLS alors que le HTTP nu répond `200`. L'applicateur régénère donc le Caddyfile **avant** d'allumer l'AP.
- **NM remet `ip_forward` à 1 et ne le rebaisse jamais.** `ipv4.method shared` l'active ; repasser en mode cul-de-sac retire bien la table `nft` et arrête `dnsmasq`, mais le sysctl reste à `1` — mesuré. C'est l'applicateur qui le restaure, sinon la box reste un routeur sur le LAN client à l'insu de tout le monde.
- **Un seul applicateur, et il sérialise.** Deux appels cockpit concurrents se sont entrelacés en test : un `apply` bloqué 90 s dans `nmcli connection up` (modem sans couverture) a réécrit le drop-in strict *après* qu'un autre l'ait retiré → état persisté `wan`, box sans route par défaut. D'où le verrou côté backend et le `--wait` sur chaque appel `nmcli`. Corollaire opérationnel inchangé : un humain qui lance `nmcli` sur la box sera défait au prochain apply ou au prochain boot.
- **Firewall = responsabilité IT client.** Le périmètre réseau (qui atteint la box sur le LAN) est du ressort de l'IT. L'image Armbian nue n'ouvre que `:22` ; **après déploiement la box expose `:22`, `:80` et `:443`** — le backend, lui, est sur le loopback, donc rien n'écoute sur le LAN qui ne soit ni SSH ni Caddy.

## Reste à industrialiser (non bloquant)

- [x] Day-0 zéro-touch → **fait** via `photonicat/emmc-install/` (SD → eMMC + clé root, validé RK3576 2026-07-25). Reste à **archiver/épingler** l'image Armbian de base dans notre stockage.

- [ ] **Intégrer l'enrôlement Tailscale dans Ansible `bringup`** (clé taguée réutilisable, `--ssh`) — encore manuel ; à rejouer/valider **sur Debian/systemd** (validé jusqu'ici sur l'ancienne box OpenWrt).

- [x] Release via **canal signé** (`channels/*.json`, sha256) → **fait** (`-e channel=stable|nightly`). Sortie de `release=local` obtenue.

- [ ] Révoquer les auth-keys de provisioning après usage.

- [x] Signer les artefacts + vérif on-device → **fait côté produit** (minisign, orchestrateur OTA). Reste : vérif **minisign côté Ansible** (aujourd'hui sha256 seul).

- [ ] Control node : passer du laptop à un VPS bastion tagué quand la flotte grossit.

- [x] **Identification day-0 par ULA IPv6** → **fait et validé sur RK3576** (installeur + Ansible, moitiés impérative *et* déclarative, deux runs `site.yml` par l'IPv6, cockpit + certificat sur l'ULA).

- [ ] **Publier une release** portant le cockpit-sur-ULA et l'onboarding qui annonce l'IPv4 : validé aujourd'hui en `-e release=local` seulement.

- [ ] **Simplifier la fiche de livraison** : son URL day-0 reprend `ansible_host` (donc l'ULA quand on pilote en IPv6). Maintenant que la page d'onboarding annonce elle-même l'IPv4, la fiche n'a plus besoin d'URL client du tout — elle peut ne porter que l'ULA, imprimable dès la sortie d'usine.

- [x] **Binder l'orchestrateur sur le loopback** → **fait** : bind par défaut `127.0.0.1` (`CP_ORCH_BIND` pour élargir en dev), posé aussi explicitement dans l'unité systemd, et **vérifié à chaque déploiement** — `start.yml` lit `ss` sur la box et échoue si le backend écoute ailleurs que sur le loopback. Reste à rejouer un scan depuis le LAN sur la box de test après la prochaine release.

- [ ] **Marquer la version déployée sur la box** (fichier ou `--version` câblé sur le tag) : aujourd'hui impossible de savoir quelle release tourne.