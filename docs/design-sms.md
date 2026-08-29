# Accès SMS — plan d'implémentation et de test

> Branche `feat/sms`, partant de `master` (537a0e3d). Indépendante de
> `dockerisation`.
>
> **État : livré.** L'étape 0 a tranché pour le chemin A, mesuré sur la box
> (Armbian, `mmcli 1.24.0`, RM520N-GL en MBIM) : `--messaging-status` répond
> `supported storages: mt` et un aller-retour create → list → read → delete rend
> accents, emoji et retour ligne intacts. Les écarts entre ce plan et le code
> livré sont notés en ligne ci-dessous ; **le code fait foi**.

Feature : lire, envoyer et archiver les SMS de la SIM depuis le cockpit.
**Disponible uniquement sur les Photonicat équipés du module 5G.**

---

## 1. Périmètre

**Dedans (v1)**

- Ingestion de fond des SMS entrants, avec purge du stockage modem.
- Historique persistant (SQLite), survivant aux reboots et aux resets modem.
- Envoi d'un SMS depuis le cockpit, tracé.
- Un panneau cockpit, desktop + mobile, sous IT Settings.
- Rétention configurable.

**Dehors (v1)**

- USSD, appels voix, MMS (le `pysms_tool` du vendeur les fait ; pas notre besoin).
- Notification temps réel (pas de bus d'événements générique : le cockpit poll,
  comme le fait déjà `ItNetworkPane` à 5 s).
- Règles/hooks sur SMS entrant (leur `process_hook_shell`) — v2 éventuelle.

---

## 2. Décision d'architecture

Le gate matériel **existe déjà** : `modem_present()`
(`it/network/apply/mod.rs:127`) sonde sysfs (`/sys/class/usbmisc/cdc-wdm*`,
`/sys/class/net/ww*`), est surchargeable par `CP_WWAN_PRESENT`, est déjà remontée
par `GET /api/it/network` et déjà consommée par le cockpit. **Aucun nouveau
mécanisme de détection n'est à écrire** : la feature SMS se branche sur la même
réponse, au même instant, pour la même raison.

Deux chemins possibles vers le modem, tranchés par l'**étape 0** :

| | **A — `mmcli --messaging-*`** | **B — AT/PDU sur `ttyUSB2`** |
|---|---|---|
| Décodage GSM-7 / UCS2 | fourni | à faire (crate `sms-pdu` ou `tpdu`) |
| Réassemblage multipart | fourni | à faire (UDH) |
| Contention de port | aucune (MM est le propriétaire légitime) | MM possède `ttyUSB2` → règle udev `ID_MM_PORT_IGNORE="1"` |
| Précédent | `photonicat2_mini_display/linuxFallback.go` — le repli « Linux générique » écrit par le vendeur, c.-à-d. exactement notre cas Armbian | `pcat-manager-web/app/pysms_tool.py` — ce qui tournait sur votre OpenWrt |
| Coût | référence | +1,5 j |

**A est le chemin par défaut.** B est un repli documenté, pas une branche morte :
son coût est borné et son fonctionnement est prouvé sur ce matériel précis.

Le reste du plan (lots 1, 2, 4, 5, 6, 7) est **identique dans les deux cas** :
seul le lot 3 change d'implémentation, derrière un trait unique.

---

## 3. Étape 0 — mesure sur la box (bloque le lot 3, rien d'autre)

La box est injoignable au moment de la rédaction. Trois commandes :

```sh
mmcli -m 0 --messaging-status      # A viable ? (stockages supportés)
mmcli -m 0 --messaging-list-sms    # A: retourne une liste, même vide
ls /dev/ttyUSB*                    # B: ttyUSB2 survit-il en composition MBIM ?
mmcli --version                    # ≥1.10 pour --messaging-create-sms-with-text
```

Puis un aller-retour réel :

```sh
mmcli -m 0 --messaging-create-sms="text='cp-test',number='+33XXXXXXXXX'"
mmcli -s 0 --send
# et se faire envoyer un SMS depuis un autre téléphone, puis re-lister
```

**Décision** : `--messaging-status` répond avec des stockages *et* l'aller-retour
passe → A. Sinon → B, et `ls /dev/ttyUSB*` doit montrer `ttyUSB2` (si les deux
échouent, la feature n'est pas livrable et il faut remonter au firmware modem).

**Capture des fixtures au passage** — c'est la sortie de cette étape qui rend
tout le reste testable hors box :

```sh
mmcli -J -m 0 --messaging-list-sms  > tests/fixtures/sms/list.json
mmcli -J -s 0                       > tests/fixtures/sms/deliver.json
mmcli -J -s 1                       > tests/fixtures/sms/submit.json
mmcli -J -m 0 --messaging-status    > tests/fixtures/sms/status.json
```

---

## 4. Lot 1 — libérer un slot de dossier (prérequis CI)

`.github/checks/check-structure.sh` impose **≤ 500 lignes par fichier** et
**≤ 8 entrées par dossier**, sur l'arbre Rust *et* `web/src`. État actuel :

| Dossier | Entrées | Marge |
|---|---|---|
| `transport/it/` | **8** | **pleine** |
| `transport/it/network/` | **8** | **pleine** |
| `transport/it/network/apply/` | 2 | ok |
| `runtime/` | 4 | ok |
| `deploy/ansible/` | **8** | **pleine** |
| `deploy/ansible/tasks/` | **8** | **pleine** |
| `deploy/ansible/tasks/net/` | 2 | ok |
| `web/src/lib/api/it/` | 3 | ok |
| `web/src/components/shell/config/it/` | 4 | ok |

C'est exactement le mur du commit `00abfb2a` (« the gateway had nowhere to
live »). Deux conséquences, à traiter **avant** d'écrire la feature :

1. **`it/network/routes.rs` → `it/network/apply/routes.rs`.** Justifié
   sémantiquement, pas seulement par le compteur : `routes.rs` *est* une étape
   d'applier (« Mode → routing table, and the two sysctl/drop-in side effects »),
   il n'importe que depuis `apply`, et il n'est consommé que par les deux
   `step(...)` de `apply/mod.rs:236-237`. Un seul `mod routes;` à déplacer, plus
   trois liens de doc à corriger (`profiles.rs:136`, `uplink.rs:117`,
   `state.rs:182`). Zéro changement de comportement.
   → `network/` passe à 7, `apply/` à 3.
2. **Le nouveau task Ansible va dans `tasks/net/sms.yml`**, jamais à la racine de
   `tasks/` ni de `ansible/`. Cohérent : c'est du modem.

**Commit isolé**, vérifié par `.github/checks/check-structure.sh --ci` et
`cargo test -p cp-orchestrator`. Ce lot est indépendant de l'étape 0 et peut
démarrer immédiatement.

---

## 5. Arborescence cible

```
crates/cp-orchestrator/src/transport/it/network/sms/
  mod.rs      handlers HTTP, validation, projection de lecture
  store.rs    SQLite : schéma, insert idempotent, requêtes, rétention
  modem.rs    trait SmsPort + impl mmcli (A) | impl AT/PDU (B)
  poll.rs     boucle d'ingestion de fond
  tests.rs    unitaires + gate + fixtures
```

`network/` : 7 + `sms/` = 8. Tenu, sans marge — à retenir pour la v2.

---

## 6. Lot 2 — le store (`store.rs`)

`rusqlite` est déjà une dépendance (`services/auth/db.rs` fournit le patron
d'ouverture + migration). La base vit à côté de `.network.json`, dans
`agents_dir`, sous `CP_SMS_DB` (défaut `<agents_dir>/sms.db`).

```sql
CREATE TABLE IF NOT EXISTS sms(
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  digest       TEXT    NOT NULL UNIQUE,   -- idempotence de l'ingestion
  direction    INTEGER NOT NULL,          -- 0 reçu, 1 envoyé
  peer         TEXT    NOT NULL,          -- E.164
  body         TEXT    NOT NULL,          -- UTF-8
  sent_at      INTEGER,                   -- horodatage réseau (epoch s)
  ingested_at  INTEGER NOT NULL,
  state        TEXT    NOT NULL,          -- received|sending|sent|failed
  read_at      INTEGER,
  sent_by      TEXT,                      -- user id, pour l'envoi (audit)
  error        TEXT
);
CREATE INDEX IF NOT EXISTS sms_by_time ON sms(ingested_at DESC);
```

**Le digest est le cœur du lot**, et c'est repris du vendeur
(`pc_sms_client.py`, `hash_digest VARCHAR(256) UNIQUE`). Sans lui, un
`--messaging-delete` raté après une insertion réussie **duplique le message à
chaque tour de poll**. `digest = sha256(peer ‖ sent_at ‖ body ‖ direction)` :
volontairement indépendant de l'index modem, qui est réattribué.

**Correction apportée à l'implémentation** : un `UNIQUE(digest)` simple, comme
chez le vendeur, échoue dans l'autre sens et plus gravement. Un numéro court
d'opérateur qui envoie deux fois la même alerte, sans horodatage réseau — cas
réel — hache pareil : la seconde copie est avalée en silence *et* supprimée du
modem. L'unicité est donc **partielle**, `WHERE modem_handle IS NOT NULL` : elle
ne lie que tant que le modem détient encore une copie. Une fois celle-ci
effacée, un message identique est un nouveau message.

Rétention : purge par âge (`CP_SMS_RETENTION_DAYS`, défaut 90) **et** par volume
(plafond dur, défaut 5000) à chaque tick. Les SMS sont des données personnelles
stockées sur une box client — la rétention est dans la v1, pas après.

---

## 7. Lot 3 — la couche modem (`modem.rs`)

Un seul trait, pour que le choix A/B ne fuie nulle part :

```rust
trait SmsPort {
    fn list(&self) -> Result<Vec<Incoming>, String>;
    fn delete(&self, handle: &str) -> Result<(), String>;
    fn send(&self, to: &str, body: &str) -> Result<(), String>;
    fn available(&self) -> bool;
}
```

**Impl A (mmcli).** Réutilise `run(bin, args)` (`apply/mod.rs:339`) et le gate
`CP_MMCLI_BIN` déjà templaté (`context-pilot.service.j2:47`). Commandes,
reprises telles quelles de `linuxFallback.go` :

```
mmcli -J -m <path> --messaging-list-sms      → modem.messaging.sms[]
mmcli -J -s <sms>                            → number, text, pdu-type, timestamp
                                               ne garder que pdu-type == "deliver"
mmcli -m <path> --messaging-delete-sms=<sms>
```

Le chemin D-Bus du modem est **découvert** via `mmcli -J -L`, jamais l'index `0`
codé en dur — la leçon déjà tirée dans `signal_dbm()`
(`status/mod.rs:314`) : MM incrémente l'index à chaque ré-énumération.

Envoi — **ne jamais interpoler le texte dans la chaîne d'option** (apostrophes,
virgules, retours ligne, accents la cassent) :

```
mmcli -m <path> --messaging-create-sms="number='+33…'" \
                --messaging-create-sms-with-text=<fichier temporaire>
mmcli -s <path retourné> --send
```

**Impl B (AT/PDU), si l'étape 0 l'impose.** `AT+CPMS="SM"` puis `"ME"` →
`AT+CMGF=0` → `AT+CMGL=4` → décodage PDU → `AT+CMGD=<index>` ; envoi par
`AT+CMGS=<len>` + PDU + `^Z`. Crate `sms-pdu` ou `tpdu` plutôt qu'un portage
maison des ~700 lignes de `pysms_tool.py`. Plus la règle udev
`ENV{ID_MM_PORT_IGNORE}="1"` sur `ttyUSB2` dans `tasks/net/sms.yml` : MM garde le
modem en MBIM pour la data, nous possédons un canal AT dédié. **Ne jamais
`systemctl stop ModemManager`** — ce serait couper l'uplink 5G en production.

**Dégradation.** `CP_MMCLI_BIN` absent ⇒ `available() == false` ⇒ la feature
répond « indisponible », zéro sous-process, aucune erreur. Même contrat que
`wwan_status()` : *chaque champ adossé à un outil dégrade en `null` plutôt que
d'échouer.*

---

## 8. Lot 4 — l'ingestion de fond (`poll.rs`)

Un thread pour la vie du process, sur le patron de `runtime/update_scheduler.rs`
(`spawn` → `run_loop` → `tick`). Démarré depuis `main.rs:114-128` par un
`Runtime::start_sms_poller()` qui délègue, exactement comme
`start_oauth_sweeper()` délègue à `transport::rest::spawn_oauth_refresh()`
(**aucun nouveau fichier dans `runtime/`**).

Un tick (défaut 30 s, `CP_SMS_POLL_S` ; le vendeur poll à 30 s lui aussi, et
n'utilise pas non plus `AT+CNMI`) :

1. `available()` faux, ou `modem_present()` faux → dormir, ne rien faire.
2. `list()`.
3. Pour chaque message : insert idempotent par `digest`.
4. **Supprimer du modem seulement après un commit réussi.** Une suppression
   ratée est bénigne — le `digest` absorbe le doublon au tour suivant. L'ordre
   inverse perd des messages.
5. Purge de rétention.

**Pourquoi un thread et pas un fetch à l'ouverture du panneau** : le stockage SIM
fait ~20–50 emplacements et, une fois plein, les SMS entrants sont perdus **en
silence**. Le vendeur documente exactement ce point (`pc_sms_client.py` :
*« delete the message from modem/SIM storage to prevent overflow »*). C'est ce
qui distingue « afficher les SMS » d'un service.

**Ce que le poller ne fait jamais** : toucher au mode uplink, au profil `cp-wwan`,
ou à `ModemManager`. L'invariant du module réseau est « un seul writer », et ce
writer est `apply`. Modem non enregistré ⇒ le cockpit affiche pourquoi, il ne
« répare » pas.

---

## 9. Lot 5 — API, RBAC, contrat

| Route | Capability | Effet |
|---|---|---|
| `GET /api/it/sms` | `can_manage_it` | liste paginée (`?before=&limit=`) |
| `POST /api/it/sms` | *à trancher — §12* | envoi `{to, body}` |
| `POST /api/it/sms/{id}/read` | `can_manage_it` | marque lu |
| `DELETE /api/it/sms/{id}` | `can_manage_it` | supprime de l'historique |

Handlers minces dans `rest/config/network.rs` (mêmes gates `denied()` /
`denied_bearer()`, même forme que `it_set_network_ap`), délégant à `sms::`.
Sémantique RBAC inchangée : `auth_user == None` ⇒ god-mode, passe.

Statut greffé sur `GET /api/it/network` plutôt qu'une route de plus :

```json
"status": { "sms": { "available": true, "unread": 3 } }
```

**Sans compteurs d'occupation** : `mmcli --messaging-status` dit *quels*
stockages le modem accepte (`mt` ici), pas combien il en reste — mesuré. Un
couple `used`/`total` aurait dû être inventé, ce qui est pire qu'absent.

`null` quand pas de modem ou pas d'outil — même règle que `wwan`. Une seule
sonde par requête, une seule réponse : le cockpit ne peut pas afficher deux
vérités contradictoires.

**Garde-fous d'envoi** (l'envoi coûte de l'argent sur *votre* forfait — la
frontière que `can_manage_secrets` protège déjà sur l'APN) :

- validation E.164 stricte sur `to` ; corps ≤ **670 caractères** — dix segments
  UCS-2 à 67 caractères une fois l'en-tête de concaténation retiré, et non 1530
  comme écrit ici initialement (le brouillon comptait en GSM-7, ce que le corps
  n'est pas dès le premier accent) ; compté en **points de code** des deux côtés,
  un `.length` JavaScript compte en unités UTF-16 et se désaccorderait du
  `chars().count()` de Rust au premier emoji ;
- rate-limit par utilisateur et global (défaut 10/h, 50/j) — refus `429`, et
  qui **échoue fermé** : une erreur de lecture refuse l'envoi plutôt que de
  compter zéro, sinon le plafond s'ouvre au moment où il devrait tenir ;
- `sent_by` en base = trace d'audit non optionnelle.

**Contrat.** Toute route doit être ajoutée à `tests/openapi/paths.rs` (+ schémas
dans `schemas_net.rs`), sinon `tests/openapi/exhaustive.rs` casse. Puis :

```sh
cargo test -p cp-orchestrator --test openapi generate_openapi -- --ignored   # openapi.json
cd web && npx @hey-api/openapi-ts                                            # SDK généré
```

Le job CI `contract` régénère et fait `git diff --exit-code` : `generated/**` ne
se touche jamais à la main.

---

## 10. Lot 6 — cockpit

- `web/src/lib/api/it/sms.ts` — **toute** la logique non visuelle : clé
  React-Query, intervalle de poll, brouillon d'envoi, miroir client de la
  validation serveur, formatage. Même raison que `network.ts` : le panneau existe
  **deux fois** (desktop + `mobile-components`), et sans ce fichier chaque
  correction se fait deux fois.
- `components/shell/config/it/ItSmsPane.tsx` + son jumeau mobile, généré par
  `pnpm mirror:scaffold` puis stylé ; `pnpm mirror:check` en CI.
- Montage conditionnel sur `status.modem_present`, exactement comme
  `ItNetworkPane.tsx:163` masque déjà les modes 5G. Le panneau **n'apparaît pas**
  sur une box sans module — pas grisé, absent.
- États explicites : pas de modem / outil absent / modem non enregistré / SIM
  pleine / rate-limit atteint. Le serveur reste l'autorité (NFR-05) ; le client
  ne fait qu'éviter un aller-retour opaque.
- Contraintes : `type-coverage ≥ 99.8`, eslint, ≤ 500 lignes par fichier.

---

## 11. Lot 7 — déploiement

`deploy/ansible/tasks/net/sms.yml`, inclus depuis `net/` **sous la même sonde
matérielle que `modem.yml`** (`/sys/class/usbmisc/cdc-wdm*`, `/sys/class/net/ww*`)
— une box sans module n'installe rien et n'active rien.

- Chemin A : rien à installer (ModemManager et `mmcli` sont déjà posés par
  `modem.yml`). Uniquement les variables d'environnement dans
  `context-pilot.service.j2`, à côté de `CP_MMCLI_BIN` :
  `CP_SMS_DB`, `CP_SMS_POLL_S`, `CP_SMS_RETENTION_DAYS`, `CP_SMS_ENABLED`.
- Chemin B : + la règle udev `ID_MM_PORT_IGNORE` et son `udevadm control
  --reload-rules && udevadm trigger`, sur le patron déjà présent dans
  `modem.yml`.

`CP_SMS_ENABLED=0` désarme la feature : le thread est bien démarré — il coûte
un test par tick et reste ainsi commutable à chaud — mais chaque tick sort
immédiatement, aucun store n'est ouvert, et les routes d'écriture répondent
`503`. **Ce n'est pas une purge** : un archive déjà sur disque reste. Un
interrupteur pour une box où le client ne veut pas de SMS stockés, pas pour en
effacer.

---

## 12. Décisions à trancher

1. **Capability d'envoi.** Lecture en `can_manage_it` fait consensus. L'envoi
   coûte sur le forfait vendeur, ce qui plaide `can_manage_secrets` — mais rend
   la feature inutilisable par l'admin du site, qui est son public. *Proposition
   par défaut* : `can_manage_it` + rate-limit + audit, avec la contrepartie
   assumée. C'est un prédicat d'une ligne, réversible tant que la v1 n'est pas
   déployée.
2. **Rétention par défaut** : 90 j / 5000 messages. À confirmer côté
   engagement client (RGPD : données personnelles sur une box tierce).
3. **SMS sortants dans l'historique** : oui (`direction=1`) — utile pour l'audit,
   au prix d'une base qui contient ce que le client a écrit.

---

## 13. Plan de test

### 13.1 Unitaires (hors box, aucun gate posé — c'est le contrat existant)

| Cible | Cas |
|---|---|
| `modem.rs` (A) | parse des fixtures §3 ; `pdu-type: submit` ignoré ; liste vide ; JSON tronqué ⇒ `Err`, pas de panic ; chemin D-Bus découvert et non `0` |
| `store.rs` | insert ; ré-insert du même digest ⇒ no-op ; purge par âge ; purge par volume ; pagination `before/limit` |
| validation | E.164 rejeté/accepté ; corps vide ; corps > 1530 ; UTF-8 hors BMP (emoji) |
| rate-limit | fenêtre glissante, par user et global |

### 13.2 Le gate — le test qui protège tous les autres

Sur le patron de `status/tests.rs:160`
(`assert_null(&status, "wwan", "no mmcli gate ⇒ null bearer")`) :

- `CP_MMCLI_BIN` non posé ⇒ `status.sms == null`, `GET /api/it/sms` répond `200`
  avec une liste vide, **aucun sous-process n'est lancé**.
- `CP_WWAN_PRESENT=0` ⇒ feature indisponible même avec `mmcli` présent.
- `CP_SMS_ENABLED=0` ⇒ routes en `503`, poller non démarré.

C'est ce qui garantit que `cargo test` sur un portable ne touche jamais un modem.

### 13.3 Intégration avec un faux `mmcli`

Le gate est un chemin de binaire, donc **un script stub est un modem complet**.
`CP_MMCLI_BIN=tests/fixtures/sms/fake-mmcli` (script shell rejouant les fixtures,
avec un mode « échec » pilotable par variable d'environnement) donne, hors box :

- cycle d'ingestion complet : list → insert → delete → re-list vide ;
- **suppression modem qui échoue** ⇒ au tour suivant, toujours **un seul**
  message en base (le test qui justifie `digest UNIQUE`) ;
- envoi : fichier temporaire correctement rempli, `--send` appelé, `state`
  passant `sending` → `sent` ; échec ⇒ `failed` + `error` renseigné ;
- `mmcli` qui pend ⇒ le tick abandonne sans bloquer le thread.

### 13.4 RBAC

Matrice rôle × route, sur le patron des tests de `rest/config/network.rs`
(4 rôles × 4 routes), plus `auth_user == None` ⇒ passe (FR-v3-08).

### 13.5 Contrat & structure

```sh
.github/checks/check-structure.sh --ci     # ≤500 lignes, ≤8 entrées
cargo test -p cp-orchestrator              # dont exhaustive.rs (routeur ↔ openapi)
cd web && pnpm mirror:check && pnpm lint && pnpm type-coverage && pnpm build
```

### 13.6 e2e (Playwright, `web/e2e/sms.spec.ts`)

- Box sans modem (`CP_WWAN_PRESENT=0`) ⇒ **le panneau SMS n'existe pas** dans le
  DOM. C'est le test qui garde la promesse « uniquement les Photonicat 5G ».
- Box avec faux modem ⇒ liste rendue, marquage lu, envoi optimiste puis
  confirmation.
- Rôle sans `can_manage_it` ⇒ section absente.

### 13.7 Recette matérielle (sur la box, une fois les lots livrés)

| # | Test | Attendu |
|---|---|---|
| 1 | SMS entrant depuis un téléphone | apparaît en < 30 s ; disparaît du stockage modem (`--messaging-list-sms` vide) |
| 2 | SMS long (> 160 car., accents) | **un seul** message, texte intact — la vraie preuve du réassemblage multipart |
| 3 | Envoi depuis le cockpit | reçu sur le téléphone ; `state=sent` ; `sent_by` correct |
| 4 | 30 SMS d'affilée | aucun perdu ; stockage modem jamais saturé |
| 5 | Reboot box | historique intact |
| 6 | `mmcli --reset` du modem pendant le poll | pas de crash ; reprise ; pas de doublon (index réattribué) |
| 7 | **Non-régression uplink** — `iperf`/débit et `active_uplink` pendant 2 h de poll | aucune dégradation : le bug de framing était le motif du passage MBIM, on ne le réveille pas |
| 8 | Mode uplink `wan` + `standby=cold` | SMS reçus quand même (modem enregistré, pas de bearer) |
| 9 | Bascule de mode `wan` → `5g` → `wan` | le poller ne perturbe pas le failover, et réciproquement |
| 10 | SIM retirée | feature « indisponible », cockpit intact, aucun log en boucle |

Le test **7** est le plus important du tableau : la feature est un ajout de
confort greffé sur le chemin de connectivité de production.

---

## 14. Séquencement

```
Lot 1 (slot CI)  ──────────────┐            indépendant, démarrable tout de suite
Étape 0 (box)    ──┐           │
                   ├─ Lot 3 ───┤
Lot 2 (store) ─────┘           ├─ Lot 4 ─┬─ Lot 5 ─┬─ Lot 6 ─── recette 13.7
                               │         │         │
                               └─────────┴─ Lot 7 ─┘
```

| Lot | Charge (chemin A) |
|---|---|
| 0 — mesure box | ½ j |
| 1 — slot CI | ½ j |
| 2 — store | ½ j |
| 3 — modem | 1 j (A) · +1,5 j (B) |
| 4 — poller | ½ j |
| 5 — API/RBAC/contrat | 1 j |
| 6 — cockpit | 1–1,5 j |
| 7 — Ansible | ½ j |
| Recette matérielle | ½ j |
| **Total** | **≈ 6 j (A)** · **≈ 7,5 j (B)** |

---

## 15. Sources

Le comportement du vendeur a été lu, pas supposé :

- [`photonicat/pcat-manager-web`](https://github.com/photonicat/pcat-manager-web)
  — `app/pc_sms_client.py` (thread 30 s, SQLite, `hash_digest UNIQUE`,
  suppression après sauvegarde, `combine_sms_parts`, `serial_lock`) et
  `app/pysms_tool.py` (AT/PDU, `PDUDecoder`, `CPMS`/`CMGF=0`/`CMGL=4`/`CMGD`).
  **C'est le chemin qui tournait sur l'OpenWrt.**
- [`photonicat/photonicat2_mini_display`](https://github.com/photonicat/photonicat2_mini_display)
  — `linuxFallback.go`, `getSmsJsonFromModemManager()` : le chemin `mmcli` écrit
  par le vendeur pour les systèmes non-OpenWrt, c.-à-d. le nôtre.
- [`photonicat/rockchip_rk3568_pcat_manager`](https://github.com/photonicat/rockchip_rk3568_pcat_manager)
  — `src/modem-manager.c` : **aucun code SMS**, supervise `quectel-cm` /
  `fm350-mm` pour la data uniquement. Le démon hardware n'est pas une piste.
- [`c2h2/pysms_tool`](https://github.com/c2h2/pysms_tool) — l'amont du fichier
  vendorisé, port Python du `sms_tool` C de Cezary Jackiewicz.
