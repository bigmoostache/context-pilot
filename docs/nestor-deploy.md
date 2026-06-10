# Nestor — exploitation sur la Pi

État au 11 juin 2026 : V1 + système de workspaces, déployés et vérifiés de bout en bout
sur la Pi (`nestor`, 192.168.1.145).

## Accès

- **Web** : http://192.168.1.145:8787 — mot de passe → token par appareil (révocable
  depuis Configuration → Appareils connectés). Le mot de passe initial vient de
  `CP_WEB_PASSWORD` dans `~/nestor/.env` (Pi) au premier démarrage ; ensuite il est
  hashé (argon2id) dans `~/.context-pilot/web-auth.json` (global — survit aux
  bascules de projet).
- **TUI de secours** : `ssh -t huser@192.168.1.145 nestor/bin/nestor-tui [projet]` —
  sans argument, entre dans le projet courant. Prend l'ownership de la session : le
  service headless s'arrête proprement (et ne redémarre pas tout seul,
  `Restart=on-failure`). Au Ctrl+Q, relancer : `sudo systemctl start nestor`.
- **Même session** : la TUI et le web voient la même conversation, les mêmes panneaux.

## Projets (workspaces)

Après le login, le web ouvre le **sélecteur de projets**. Chaque projet =
`~/nestor/projects/<nom>` avec sa propre session (conversation, todos, mémoire…).
Créer = dossier vierge ou `git clone` d'une URL. Ouvrir un autre projet redémarre
le cœur dedans (~3 s, reconnexion automatique). Archiver = déplacé dans
`projects/.archive/` (récupérable en SSH) ; supprimer = définitif, confirmation
par saisie du nom. Le projet actif est marqué « à bord » ; pointeur :
`~/nestor/projects/.current`.

## Paramètres généraux

Depuis le sélecteur (engrenage en haut à droite) : page machine/installation.

- **Connexion** : réseau WiFi actuel + scan + connexion (nmcli). Attention :
  changer de réseau peut rendre la Pi inaccessible depuis ton appareil.
- **Clés API** : gestion de `~/nestor/.env` (valeurs masquées, jamais renvoyées
  en clair) ; badge OAuth Claude Code. Appliquées au **redémarrage du service**
  (bouton intégré — `sudo systemctl restart nestor` relit le `.env`).
- **Défauts des nouveaux projets** : provider/modèle (`projects/.defaults.json`),
  appliqués au premier boot d'un projet créé depuis le sélecteur.
- **Système** : hostname, IP, RAM/disque/température, uptime, version + boutons
  Redémarrer Nestor / Redémarrer la Pi.
- **Sécurité** : changement du mot de passe web (vérification de l'actuel,
  argon2, révocation optionnelle des autres appareils).

API : `/api/system/{info,wifi,wifi/connect,env,restart,reboot}`,
`/api/projects/defaults`, `/api/auth/password` — Bearer token comme le reste.

## Service

```
sudo systemctl status|start|stop|restart nestor
journalctl -u nestor -f
```

Unité : `/etc/systemd/system/nestor.service` → `nestor-web` →
`cpilot --headless --web-bind 192.168.1.145:8787 --web-dist ~/nestor/web-dist`.

## Déployer une nouvelle version

```
./deploy_pi.sh          # cross-compile (cross + Docker) + binaires
./deploy_pi.sh --web    # + build SPA (web/) + rsync vers ~/nestor/web-dist
ssh huser@192.168.1.145 sudo systemctl restart nestor
```

Prérequis PC : rustup (target `aarch64-unknown-linux-gnu`), `cross`, Docker, Node.

## Arborescence sur la Pi

```
~/nestor/bin/        cpilot, cp-console-server, nestor-tui, nestor-web
~/nestor/web-dist/   SPA buildée
~/nestor/workspace/  cwd de l'agent (.context-pilot/ : état, web-auth.json, erreurs)
~/nestor/.env        CP_WEB_PASSWORD (+ clés API éventuelles)
~/.claude/.credentials.json   OAuth Claude Code (provider « claudecode »)
```

## Notes

- LLM : provider actif = `claudecode` (OAuth). Pour une clé API : l'ajouter dans
  `~/nestor/.env` (ex. `ANTHROPIC_API_KEY=...`) et choisir le provider dans la config web.
- Le binaire se relance tout seul sur « Recharger Nestor » (exec self-restart).
- Bureau Pi désactivé (boot console) pour la RAM — réversible : `sudo raspi-config`.
- Restes V1 connus : l'overlay perf (F12) n'est pas exposé au web (données internes
  non sérialisées) ; TLS non activé (HTTP sur LAN, prévu post-V1).
