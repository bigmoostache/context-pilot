# Nestor — V1

> **Nestor** : la version *headless + web* de Context Pilot.
> La logique tourne en continu sur une Raspberry Pi du réseau local ; on la pilote
> depuis le navigateur du PC via une interface web élégante — la TUI reste accessible
> en SSH comme roue de secours.

Document de cadrage V1. Synthèse du brainstorm. À lire avant de commencer.

---

## 1. Le projet

Context Pilot est une TUI Rust (~62 000 lignes, 23 crates) : un assistant de code/écriture/recherche
piloté par un agent IA. L'IA, l'OCR, la recherche web passent par des **API réseau** — il n'y a
**aucune inférence locale**. Au runtime, c'est un **client réseau léger** (ratatui + reqwest blocking
+ SQLite + tree-sitter).

**Nestor** déplace ce client sur une Raspberry Pi toujours allumée et lui ajoute une **interface web**.
L'idée de fond : un assistant **asynchrone** — on lui donne une tâche, on ferme le laptop, il travaille
en fond sur la Pi, il prévient quand c'est prêt. Le moteur d'autonomie existe déjà (`cp-mod-spine` :
auto-continuation + notifications + garde-fous).

### Cible matérielle (POC)
Raspberry Pi 3 Model A+ — BCM2837B0 Cortex-A53 ARMv8 64-bit @ 1.4 GHz, **512 Mo LPDDR2**, WiFi.

- **Compilation : OBLIGATOIREMENT en cross-compile** depuis le PC vers `aarch64-unknown-linux-gnu`
  (512 Mo ne peuvent pas linker le workspace). On copie le binaire sur la Pi.
- **Runtime : OK** — le client headless est plus léger que la TUI (pas de rendu). Prévoir un peu de
  swap (zram) pour absorber les pics (parsing tree-sitter d'un gros repo).
- **OS : Raspberry Pi OS 64-bit obligatoire** (pas la 32-bit).
- Le `run.sh` actuel fait `cargo build` à chaque lancement → **inadapté à la Pi**, lancer le binaire directement.

---

## 2. Les grands piliers

### Pilier 1 — Headless + TUI accessible en SSH
Le projet tourne sur la Pi ; `ssh pi → lancer la TUI` fonctionne comme en local.
La TUI reste le client de secours et le **test de non-régression** de l'abstraction headless.

### Pilier 2 — Cœur headless + transport web
Exposer la boucle agent au réseau local, sans terminal, via un serveur web sur la Pi.

### Pilier 3 — Frontend web, parité fonctionnelle totale, web-natif et élégant
Reproduire **toutes** les fonctionnalités de la TUI — mais avec une UI/UX repensée pour le web
(React/TS/Tailwind/shadcn), pas un calque du terminal.

---

## 3. Les fondations (ce qui rend tout ça réaliste)

Trois coutures déjà présentes dans le code font de Nestor une **extension**, pas une réécriture :

1. **`Action` est le type de commande universel.** L'entrée clavier passe par
   `handle_event(évènement) → Action → handle_action`. La boucle agent (streaming, outils, spine)
   est *indépendante* de l'entrée. → Le front web produit des `Action`, pas des touches simulées.
   *(`src/app/actions/`, `src/app/run/lifecycle.rs`)*

2. **`State` est déjà sérialisable** (serde partout, persistance disque existante).
   → La donnée à envoyer au navigateur existe déjà. *(`crates/cp-base/src/state/`)*

3. **`cp-render` est une IR agnostique du backend, sérialisable, pensée pour le web.**
   Son doc dit littéralement « ratatui for TUI, **HTML for web** », « Web adapters map to CSS classes »,
   « shipped over the wire to a web frontend ». `src/ui/ir/` en est l'adaptateur *ratatui*.
   ⚠️ **Décision (voir §6)** : pour rester web-natif, le web **ne consomme PAS** le `Frame` IR
   (qui encode une présentation terminal). `cp-render` reste l'affaire de la TUI. Le web se branche
   un cran plus bas, sur le **domaine**.

Le `struct App` (`src/app/mod.rs`) est ~95 % logique métier ; seuls `typewriter`, `command_palette`,
`last_render_ms/spinner_ms` sont purement UI — et le typewriter se mappe naturellement sur un flux WS.

---

## 4. Architecture cible

```
Navigateur (PC) — React/TS/Tailwind/shadcn
   │  ▲
   │  │  WebSocket (deltas)  +  HTTP (snapshot, assets, auth)
   ▼  │
┌──────────────────────────────────────────────┐
│ Raspberry Pi — binaire headless               │
│                                                │
│  cp-web-server (axum + WS)                     │
│    ├─ sort: build_web_state() → WebState       │  ← miroir web de build_frame()
│    │        snapshot à la connexion + deltas   │
│    └─ entre: commande web → Action → boucle    │
│                                                │
│  Cœur (inchangé ou presque)                    │
│    ├─ boucle agent (ex-App, sans terminal)     │
│    ├─ llms/ (streaming mpsc → StreamEvent)     │
│    ├─ tool pipeline                            │
│    └─ modules (logique)                        │
│                                                │
│  cp-console-server (déjà là — possède          │
│    les process, survit aux reloads)            │
└──────────────────────────────────────────────┘
        ▲
        │ SSH → TUI (roue de secours, même session)
```

### Abstractions à introduire
| Couture | Aujourd'hui | Headless |
|---|---|---|
| **Entrée** | `event::poll/read` crossterm | trait `InputSource` → impl crossterm **ou** canal web |
| **Sortie** | `terminal.draw(ui::render)` | trait `OutputSink` → impl ratatui **ou** `build_web_state` + broadcast WS |
| **Transport** | — | crate `cp-web-server` (axum + WS), sert aussi la SPA |

Le cœur devient **générique sur entrée/sortie**. La TUI et le web sont deux impls des mêmes traits :
garder les deux garantit que le cœur reste headless-agnostique.

---

## 5. Le contrat Pi ↔ navigateur (le cœur du travail neuf)

C'est un contrat **à deux faces**. C'est *là* que se concentre le design V1 ; le reste est mécanique.

### Face sortante — `WebState` (view-model)
Un type sérialisable dédié, assemblé par `build_web_state()` qui **reflète `build_frame()`**
(même pattern, autre cible). Poussé en **hybride** : snapshot complet à la connexion, puis **deltas**
(les `StreamEvent` existent déjà ; le dirty-tracking aussi via `ui.dirty`).

- ❌ Streamer le `State` interne brut → fuite les structs internes, casse à chaque refacto.
- ❌ API REST de ressources → verbeux, perd le temps-réel facile.
- ✅ `WebState` dédié → contrat stable, web-natif, temps-réel simple.

### Face entrante — commandes web → `Action`
Définir le jeu de commandes que le front peut émettre. La plupart se mappent sur `Action`, mais
certaines interactions **mutent l'état directement** aujourd'hui et demandent un traitement explicite :
- soumission de message ;
- navigation / sélection de panneau ;
- `ask_user_question` (réponse au formulaire) ;
- autocomplete (events de frappe) ;
- config (provider/modèle, thème, toggles, barres de budget) — aujourd'hui des hotkeys qui mutent l'état.

> **Méthode recommandée** : lister d'abord les **commandes entrantes** ; elles révèlent en creux ce que
> `WebState` doit exposer pour les déclencher. Les deux faces se définissent ensemble.

---

## 6. Frontend web — stack & parité

**Stack : React + TypeScript + Tailwind + shadcn/ui.**
UI/UX **non identique** à la TUI : mêmes fonctionnalités, mais on exploite la force du web pour être
plus élégant et commode. Principe : **divulgation progressive** (détail technique replié par défaut,
accessible aux power-users).

### Surface à couvrir (parité) → composants
La TUI expose : **10 panneaux fixes + conversation**, des **panneaux dynamiques** (fichier, git, github,
search, brave, firecrawl, console, SQL entités, historique), **~50 outils agent**, et des **overlays**
(config, palette, formulaire de question, statut d'index, moniteur perf).

| Surface TUI | Composant web-natif |
|---|---|
| Conversation + streaming | Chat React ; markdown via `react-markdown`, code via **Shiki** |
| Todo / Tree | `Tree` / `Collapsible` shadcn, réordonnancement drag |
| Memory / Logs / Entities / Queue / Scratchpad | `Table` + `Card` + `Dialog` |
| Fichier ouvert | viewer **Shiki** (coloration client) |
| Git / Search / Brave / Firecrawl / OCR | `Card` / `Accordion` de résultats |
| Config (Ctrl+H) | `Sheet`/`Dialog` + `Slider` (budgets) + `Select` (provider/modèle) + `Switch` |
| Palette (Ctrl+P) | **`Command` (cmdk)** — fait pour ça |
| Formulaire de question | `Dialog` + `RadioGroup`/`Checkbox` + boutons |
| Statut index / perf | `Sheet` latéral / `HoverCard` |
| Thèmes (tokens `Semantic`) | variables CSS Tailwind → clair/sombre natif |

L'édition de texte de l'input est **possédée par le navigateur** : le front n'envoie que le texte soumis
(+ events de frappe pour l'autocomplete si besoin). Les raccourcis clavier de la TUI sont conservés
pour les power-users.

---

## 7. Sécurité (à acter dès le départ)

L'agent a le **contrôle total de la Pi** — c'est son bac à sable assumé. Donc le token web = les clés
de la Pi. Non négociable pour la V1 :
- **Authentification** : mot de passe → dérive un **token de session par-appareil, révocable**.
  (Cohérent avec la *chain of trust* du projet, déjà protégée par un mot de passe humain.)
- **Bind réseau explicite** sur le LAN — **jamais `0.0.0.0` par défaut**.
- Servir en HTTPS sur le réseau local si possible (certificat auto-signé accepté pour le POC).

---

## 8. Objectifs V1 — « Fini quand »

- [ ] **P1** — Cross-compile `aarch64` + script de déploiement ; la TUI tourne sur la Pi en SSH,
      indistinctement du local. Aucun chemin ne suppose un environnement strictement local.
- [ ] **P2** — `cp-web-server` (axum + WS) ; un client reçoit le `WebState` complet en temps réel
      (snapshot + deltas) et renvoie des commandes qui pilotent l'agent ; auth mdp→token + bind LAN.
- [ ] **P3** — Frontend React/TS/Tailwind/shadcn : **toute** fonctionnalité de la TUI est réalisable
      depuis le web, en plus élégant et fluide ; streaming live ; overlays en modales web.

**Périmètre V1 (POC)** : **une seule session** (on garde le `State` global unique d'aujourd'hui,
zéro refacto du modèle d'état). TUI et web attaquent la **même** session sur la Pi.

---

## 9. Décisions verrouillées

| # | Sujet | Décision |
|---|---|---|
| 1 | Sync d'état | **Hybride** : snapshot à la connexion + deltas (`StreamEvent`) |
| 2 | UX web | **Web-natif, repensé** (pas un calque TUI), parité fonctionnelle, divulgation progressive |
| 3 | Sessions | **Mono-session** pour le POC |
| 4 | Sandbox / sécurité | Full control de la Pi (assumé) ; auth **mdp + token révocable**, bind LAN explicite |
| 5 | TUI | **Conservée** en roue de secours via SSH ; sert de test de non-régression |
| 6 | Contrat sortant | View-model **`WebState`** via `build_web_state()` (miroir de `build_frame()`) |
| 7 | Contrat entrant | **Commandes web → `Action`** |
| 8 | Édition input | Possédée par le **navigateur** ; front envoie le texte soumis |
| 9 | Stack web | **React + TypeScript + Tailwind + shadcn/ui** |
| — | Le web ne consomme **PAS** le `Frame` IR (présentation terminal) ; il se branche sur le **domaine** |

### Reportés après la V1
- Dashboard « noob-proof » orienté tâches (la V1 vise d'abord la parité TUI).
- Multi-session.
- Dispatch des tâches lourdes vers des **VM cloud éphémères** (`cp-console-server` préfigure déjà
  l'abstraction « cible d'exécution » : remplacer le socket Unix par un agent distant).
- Notifications hors-fenêtre (Web Push / email / Matrix — intégration Matrix déjà mentionnée dans le code).

---

## 10. Par où commencer

1. **Valider le runtime sur la Pi** : cross-compile `aarch64`, déployer le binaire, lancer la TUI en SSH (Pilier 1). Dérisque le matériel tôt.
2. **Définir le contrat à deux faces** (§5) : lister les commandes entrantes → en déduire le schéma `WebState`. C'est le seul vrai design neuf.
3. **Extraire les traits `InputSource` / `OutputSink`** ; brancher la TUI dessus (non-régression) puis l'impl web.
4. **`cp-web-server`** : squelette axum + WS + auth + snapshot/deltas.
5. **Frontend** : shell shadcn + chat + streaming, puis les panneaux un par un.

> Fichiers de référence : `src/app/actions/` (Action), `src/app/run/lifecycle.rs` (boucle),
> `src/app/run/streaming.rs` (StreamEvent), `crates/cp-base/src/state/` (State),
> `crates/cp-render/` (IR — pour la TUI), `crates/cp-console-server/` (daemon process),
> `src/ui/ir/mod.rs` (`build_frame`, à refléter pour `build_web_state`),
> `yamls/tools/*.yaml` (specs des ~50 outils).
