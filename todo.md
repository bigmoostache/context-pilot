# TODO — Mono-box local TLS + IT onboarding (Photonicat appliance)

> Scénario « tout local, rien sur le web » : la box possède sa propre CA, le nom/IP
> est choisi par l'IT, le cockpit reste éteint tant que le provisioning n'est pas terminé.
> Référence d'architecture : mémoire projet `local-tls-onboarding`.
> Branche cible : `auth-rbac`.
>
> Convention : chaque **Objectif** porte une assertion observable (`✓ Assert:`) — la
> condition mesurable qui prouve qu'il est atteint. Les **Tâches**/**Sous-tâches** sont
> le découpage d'implémentation.

> **⚠️ Tests = exécution LOCALE sur la machine de dev.** Toutes les assertions
> (`✓ Assert:`) se vérifient en lançant l'orchestrateur localement sur le poste de dev,
> PAS sur la box. Remplacer `<box>` par `localhost`/`127.0.0.1` dans les commandes
> (`curl -k https://localhost:9090/...`, etc.). On ne déploie sur le Photonicat qu'après
> que tout est vert en local. Caddy + `tls internal` tournent aussi en local pour les
> tests TLS ; les vérifs spécifiques box (procd, reboot, `:8088` vendor) sont marquées
> comme telles et restent les seules à exiger le matériel réel.

---

## État des lieux (déjà en place — ne pas réimplémenter)

- [x] Seed admin idempotent — `runtime.rs:181 seed_admin_if_empty` (count==0), pose `must_change_password=true`
- [x] Changement de mdp forcé — flag `must_change_password` en base + `POST /api/auth/password` (`transport/auth/mod.rs:259`) qui le clear
- [x] Rôle Admin + gating inline — `UserRole::Admin` (`services/auth/types.rs:31`)
- [x] `seed.env` (mdp admin + clés provider) câblé dans `deploy/photonicat/context-pilot.init`
- [x] Serveur HTTP `tiny_http` mono-listener `0.0.0.0:7878` — `transport/mod.rs:77` / `runtime.rs:286`
- [~] Notion de flag d'état — `onboarding_completed` (`settings.rs:25`) existe mais c'est un flag *produit UI*, PAS un gate de boot → à ne pas confondre avec `provisioned`

---

## Milestone 1 — Plan de maintenance sur `:9090`

> Un second listener, isolé réseau (LAN-only) et restreint au rôle Admin.
> Dépend de : rien. Débloque : M2, M4, M5.

### Objectif 1.1 — Un second listener écoute sur `:9090`, indépendant du `:7878`
`✓ Assert:` `curl -k https://<box>:9090/api/health` répond 200 ; `:7878` continue de répondre en parallèle ; tuer l'un ne tue pas l'autre.
- [ ] Tâche 1.1.1 — Ajouter un 2ᵉ accept-loop `tiny_http` dans `Runtime::serve` (thread dédié)
  - [ ] Extraire la construction du serveur de `transport/mod.rs:77` en helper réutilisable `serve_bound(addr, router_kind)`
  - [ ] Lire `CP_MAINT_PORT` (défaut `9090`) dans `runtime.rs` (à côté de `CP_ORCH_PORT`)
  - [ ] Démarrer le 2ᵉ listener depuis `Runtime::serve` sans bloquer le 1ᵉ
- [ ] Tâche 1.1.2 — Bind LAN-only
  - [ ] Paramétrer l'adresse de bind (`CP_MAINT_BIND`, défaut = IP LAN ou `0.0.0.0` + filtrage)
  - [ ] Documenter le choix (bind interface vs firewall) dans le `.init`

### Objectif 1.2 — Le routeur `:9090` n'expose QUE des routes maintenance, toutes gated Admin
`✓ Assert:` une requête `:9090` sans token → 401 ; avec token rôle `User` → 403 ; avec token Admin → 200. Aucune route produit (agents, chat) n'est routée sur `:9090`.
- [ ] Tâche 1.2.1 — Routeur maintenance dédié
  - [ ] Créer `transport/maint/mod.rs` (router séparé du `route_rest` produit)
  - [ ] Y router uniquement : auth/login, password, name/ip, ca, finalize, status
- [ ] Tâche 1.2.2 — Middleware « Admin requis » sur tout le plan
  - [ ] Réutiliser `auth::authenticate` puis exiger `caller.role == UserRole::Admin` en entrée de routeur
  - [ ] Whitelist minimale non-authentifiée : login + status seulement

### Objectif 1.3 — Tests d'isolation
`✓ Assert:` la suite de tests passe.
- [ ] Tâche 1.3.1 — Test : route produit absente de `:9090`
- [ ] Tâche 1.3.2 — Test : 401/403/200 selon rôle
- [ ] Tâche 1.3.3 — Test : bind refuse une origine hors LAN (si filtrage applicatif)

---

## Milestone 2 — Machine à états `provisioned` (cockpit éteint par défaut)

> Le cockpit `:80/:443` ne sert qu'une fois le provisioning finalisé. Gate persistant,
> robuste au reboot. Reco : le gate effectif vit dans **Caddy** (ne pas servir 443 tant
> que `provisioned=false`), piloté par un reload depuis l'orchestrateur.
> Dépend de : M1 (finalize vit sur `:9090`). Couplé à : M3 (reload Caddy).

### Objectif 2.1 — Un flag durable `provisioned` lu au boot
`✓ Assert:` à froid (flag absent) → `provisioned=false` ; après finalize → le fichier/colonne persiste `true` ; un reboot conserve l'état.
- [ ] Tâche 2.1.1 — Définir le stockage du flag (fichier sur `/mnt/data` ou colonne dédiée, distinct de `onboarding_completed`)
- [ ] Tâche 2.1.2 — Lecture au boot dans `Runtime::new`/`serve`
- [ ] Tâche 2.1.3 — Écriture atomique sur finalize

### Objectif 2.2 — `:80/:443` ne servent pas le cockpit tant que `!provisioned`
`✓ Assert:` box neuve → `curl -k https://<box>/` (443) renvoie la page placeholder « non configurée → :9090 » (ou refuse), PAS le SPA ; après finalize → `https://<box>/` sert le SPA.
- [ ] Tâche 2.2.1 — Mode « non provisionné » de Caddy
  - [ ] Bloc Caddy 443/80 = placeholder statique (ou absent) quand `!provisioned`
  - [ ] Bloc 9090 toujours actif
- [ ] Tâche 2.2.2 — Bascule sur finalize
  - [ ] `POST /api/maint/finalize` (Admin) : valide pré-requis (mdp changé, nom/IP set, TLS prêt) → écrit le flag → régénère la conf Caddy → `caddy reload`
  - [ ] Vérifier l'idempotence (re-finalize ne casse rien)

### Objectif 2.3 — Comportement reboot
`✓ Assert:` reboot **avant** finalize → seul `:9090` remonte ; reboot **après** finalize → `:80/:443` remontent automatiquement.
- [ ] Tâche 2.3.1 — Logique de boot procd/orchestrateur conditionnée au flag
- [ ] Tâche 2.3.2 — Test manuel documenté dans `deploy/photonicat/` (procédure de vérif sur box)

---

## Milestone 3 — TLS privé via Caddy `tls internal`

> Remplace DNS-01 OVH par une CA locale. Self-signed dès le boot, leaf réémis pour le
> nom/IP saisi par l'IT. Dépend de : M1 (saisie nom/IP). Couplé à : M2 (reload).

### Objectif 3.1 — HTTPS self-signed dès le premier boot (sans nom)
`✓ Assert:` box neuve, aucune config → `:9090` et `:443` répondent en HTTPS avec un cert `tls internal` couvrant l'IP (avertissement navigateur attendu, mais TLS chiffré, pas de HTTP clair).
- [ ] Tâche 3.1.1 — Réécrire `deploy/photonicat/Caddyfile` : retirer le bloc DNS-01/OVH, passer en `tls internal`
- [ ] Tâche 3.1.2 — Émettre un leaf avec SAN sur l'IP de la box au boot
- [ ] Tâche 3.1.3 — Retirer les creds OVH du `caddy.init` (plus nécessaires)

### Objectif 3.2 — Le leaf est réémis pour le nom/IP choisi par l'IT
`✓ Assert:` après saisie `nom=pilot.acme.corp` sur `:9090`, le cert présenté par `:443` contient `CN/SAN=pilot.acme.corp` (+ IP). Vérifiable via `openssl s_client -connect <box>:443 | openssl x509 -noout -text`.
- [ ] Tâche 3.2.1 — Endpoint `POST /api/maint/identity` (Admin) : reçoit nom + IP
  - [ ] Persister nom/IP
  - [ ] Régénérer la conf Caddy (template) avec ces valeurs
  - [ ] `caddy reload` et vérifier le succès
- [ ] Tâche 3.2.2 — Génération dynamique du Caddyfile depuis l'orchestrateur
  - [ ] Template de conf paramétré (nom, IP, mode TLS)
  - [ ] Helper `caddy_reload()` + gestion d'erreur (rollback si reload échoue)

### Objectif 3.3 — Libérer `:80` du vendor + redirection
`✓ Assert:` `:80` est servi par Caddy (308 → 443 une fois provisionné) ; l'admin vendeur reste joignable sur `:8088`.
- [ ] Tâche 3.3.1 — Script de déplacement `pcat-manager-web` → `:8088` intégré au provisioning d'image
- [ ] Tâche 3.3.2 — Bloc redirect `:80→:443` dans le Caddyfile (mode provisionné)

---

## Milestone 4 — Distribution de la racine (chemin A)

> `/ca.crt` téléchargeable + empreinte SHA-256 affichée pour vérif hors-bande.
> Dépend de : M1, M3 (la racine `tls internal` existe).

### Objectif 4.1 — La racine CA est téléchargeable
`✓ Assert:` `curl -k https://<box>:9090/api/maint/ca.crt -o root.crt` renvoie un PEM valide ; `openssl x509 -in root.crt -noout -subject` montre la CA `tls internal`.
- [ ] Tâche 4.1.1 — Localiser la racine dans le data-dir Caddy (`pki/authorities/local/root.crt`)
- [ ] Tâche 4.1.2 — Endpoint `GET /api/maint/ca.crt` (Admin) qui la sert (content-type `application/x-pem-file`)
  - [ ] Gérer le cas « pas encore générée » (404 explicite)

### Objectif 4.2 — L'empreinte SHA-256 est exposée et affichée
`✓ Assert:` `GET /api/maint/ca/fingerprint` renvoie le SHA-256 ; il correspond à `openssl x509 -in root.crt -noout -fingerprint -sha256`. L'UI l'affiche à côté du bouton de download.
- [ ] Tâche 4.2.1 — Calculer le fingerprint côté orchestrateur
- [ ] Tâche 4.2.2 — Endpoint + affichage UI (couplé M5)

---

## Milestone 5 — UI de maintenance (front, servie sur `:9090`)

> Le wizard IT. Ordre imposé. Dépend de : M1–M4.

### Objectif 5.1 — Login maintenance + changement mdp/email forcé en première étape
`✓ Assert:` connexion avec l'admin papier → l'UI force le changement de mot de passe ET permet de changer l'email AVANT toute autre action ; impossible d'atteindre les étapes suivantes tant que `must_change_password`.
- [ ] Tâche 5.1.1 — Écran login (réutiliser le flow auth existant)
- [ ] Tâche 5.1.2 — Étape forcée mdp + email
  - [ ] Étendre `POST /api/auth/password` ou ajouter un endpoint pour changer l'email
  - [ ] Garde-fou front : router bloqué tant que flag actif

### Objectif 5.2 — Formulaire nom/IP
`✓ Assert:` saisir nom/IP → appelle `identity` → le cert se réémet (vérif via Objectif 3.2) ; l'UI confirme le succès du reload.
- [ ] Tâche 5.2.1 — Form + validation (nom DNS valide, IP valide)
- [ ] Tâche 5.2.2 — Feedback succès/échec du reload Caddy

### Objectif 5.3 — Étape confiance TLS (chemin A)
`✓ Assert:` bouton « Télécharger la racine » → fichier `.crt` ; empreinte affichée ; instructions GPO/MDM visibles.
- [ ] Tâche 5.3.1 — Bouton download `ca.crt` + affichage empreinte
- [ ] Tâche 5.3.2 — Texte d'aide (pousser via GPO/MDM, vérif empreinte hors-bande)

### Objectif 5.4 — Finalisation
`✓ Assert:` bouton « Finaliser » désactivé tant que (mdp changé + nom/IP set + racine dispo) ; à l'activation → `provisioned=true`, cockpit démarre, l'UI redirige vers `https://<nom>`.
- [ ] Tâche 5.4.1 — Bouton finalize conditionné aux pré-requis (état renvoyé par `GET /api/maint/status`)
- [ ] Tâche 5.4.2 — Redirection post-finalize

### Objectif 5.5 — La maintenance reste accessible après provisioning
`✓ Assert:` une fois le cockpit up, `:9090` répond toujours (logs, re-download CA, ré-émission si le nom change).
- [ ] Tâche 5.5.1 — Vue maintenance « post-provisioning » (statut, actions ré-émission/restart)

---

## Milestone 6 — Provisioning & livraison (Ansible / image)

> Le mdp admin papier + l'unicité par unité. Hors code Rust.
> Dépend de : rien (parallélisable).

### Objectif 6.1 — Mdp admin unique par unité, livré sur papier
`✓ Assert:` deux box flashées ont des mots de passe admin différents ; le mdp imprimé permet le 1ᵉʳ login sur `:9090` ; il n'apparaît dans aucun fichier versionné.
- [ ] Tâche 6.1.1 — Génération d'un mdp aléatoire par unité (template Ansible → `seed.env`)
- [ ] Tâche 6.1.2 — Sortie imprimable (fiche papier) du couple email/mdp par unité
- [ ] Tâche 6.1.3 — Vérifier que `seed.env` reste chmod 600 + git-ignored

### Objectif 6.2 — Email admin par défaut
`✓ Assert:` box neuve → admin `admin@admin.fr` présent ; l'IT peut le changer (Objectif 5.1).
- [ ] Tâche 6.2.1 — Valeur par défaut `CP_SEED_ADMIN_EMAIL=admin@admin.fr` dans le template

---

## Milestone 7 — (V2, différé) Chemin B : CSR / PKI corporate

> Pour l'IT qui refuse d'ajouter une ancre de confiance : la box chaîne sous LEUR racine.
> Ne pas implémenter avant qu'un client le réclame. Dépend de : M3.

### Objectif 7.1 — Génération d'un CSR pour le nom/IP
`✓ Assert:` `GET /api/maint/csr` renvoie un CSR PEM valide avec SAN nom+IP (`openssl req -in csr.pem -noout -text`).
- [ ] Tâche 7.1.1 — Générer keypair + CSR (rcgen/openssl) côté box
- [ ] Tâche 7.1.2 — Conserver la clé privée en sécurité (jamais exposée)

### Objectif 7.2 — Upload du leaf signé + bascule Caddy en mode cert/key explicite
`✓ Assert:` après upload du leaf signé par la CA corporate, `:443` présente une chaîne qui valide contre la racine corporate ; plus rien à distribuer.
- [ ] Tâche 7.2.1 — Endpoint upload leaf + chaîne, validation (la clé correspond, la chaîne est cohérente)
- [ ] Tâche 7.2.2 — Bascule Caddyfile en `tls <cert> <key>` + reload
- [ ] Tâche 7.2.3 — UI : étape alternative « j'ai une PKI » dans le wizard

---

## Milestone 8 — (V2, différé) Multi-box / PKI d'entreprise

> Quand une entreprise a plusieurs box : la CA doit être celle de l'entreprise,
> jamais une self-CA par box. Piste notée, non spécifiée.

### Objectif 8.1 — Enrôlement ACME vers la CA interne du client
`✓ Assert:` (à définir) Caddy `acme_ca` pointe l'endpoint ACME interne ; N box s'auto-enrôlent ; zéro distribution (racine corporate déjà de confiance).
- [ ] Tâche 8.1.1 — Étude de faisabilité (NDES/SCEP, smallstep, EJBCA ; accès réseau côté client)
- [ ] Tâche 8.1.2 — Repli : flux CSR par box (réutilise M7)

---

## Risques & points de vigilance

- [ ] **Génération dynamique Caddyfile + reload piloté depuis Rust** (M3.2) — le plus inhabituel ; prévoir rollback si `caddy reload` échoue.
- [ ] **Cycle de vie des listeners au runtime** (M2.2) — préférer le gate côté Caddy plutôt que démarrer/arrêter un listener Rust à chaud.
- [ ] **Avertissement de cert au 1ᵉʳ login `:9090`** — inévitable ; la vérif d'empreinte (M4.2) est la mitigation.
- [ ] **Fenêtre du mdp papier** — unicité par unité (M6.1) + LAN-only (M1.1) + changement forcé (M5.1) sont les trois mitigations cumulées.
