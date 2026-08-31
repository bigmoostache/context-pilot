# Daharness — positionnement, UVP & matrice de valeurs

> **Statut :** v2 — 2026-08-06 (remplace la v1 du 2026-08-05)
> **Objet :** fonder le discours de la landing sur ce que le produit contient
> réellement, et sur ce que le marché contient réellement.
> **Méthode :** chaque revendication produit est adossée à un fichier du dépôt ;
> chaque revendication marché à une source datée (§14). Les revendications non
> adossées sont marquées ⚠️.
>
> **La page n'est plus dans ce dépôt.** Depuis le 2026-08-08 la landing a le
> sien — `github.com/Daharness/web`, répertoire `landing-page/`, publié sur
> daharness.com à chaque merge sur `master`. Ce document, lui, reste ici, parce
> qu'ici se trouve ce qui rend une promesse vraie : chaque ligne de §4, §5 et §7
> est adossée à un fichier de ce dépôt, et une phrase ne devient publiable que
> quand le code d'ici la rend exacte. Les deux évoluent séparément : §11 et §12
> sont donc des consignes **à porter dans l'autre dépôt**, et à revérifier
> contre la page telle qu'elle est en ligne — les lire ici ne dit pas ce que la
> page affiche aujourd'hui.
>
> **Ce qui change depuis la v1 :** la catégorie est tranchée (§2), la cartographie
> concurrentielle est faite (§3) et elle corrige deux erreurs d'appréciation de la
> v1 — V6 n'est pas incopiable, V1 est déjà copiée. En contrepartie, V5 était
> sous-évaluée. Et P1/P2 changent de nature : ce ne sont plus des optimisations
> mais des **conditions de véracité** de la phrase d'accroche.

---

## 1. Le problème de départ

« Harness » est un descripteur commoditisé : des dizaines de projets s'en
réclament, et il range Daharness dans le tas des surcouches d'un CLI de labo.
Deux issues possibles :

1. **Créer une catégorie** — cher, long, et il n'y a pas de budget pour ça.
2. **Revendiquer une catégorie existante et gagner la comparaison à l'intérieur.**

C'est l'option 2 qui est retenue. Elle est possible parce qu'il existe une
catégorie déjà financée par d'autres, et que Daharness s'y conforme mieux que
ceux qui la financent.

---

## 2. La catégorie revendiquée : **aOS, au sens systèmes**

**Daharness est un système d'exploitation agentique (aOS).**

Ce n'est pas un nom de produit — la marque reste **Daharness**. C'est la case
que l'on revendique, et elle a l'avantage rare d'être **déjà expliquée au marché
par d'autres** : Amdocs, Fiserv, Infobip, PwC et Legora dépensent en ce moment
des millions à convaincre les directions que cette catégorie est nécessaire. On
entre dans un emplacement mental déjà creusé.

### Le piège : deux définitions coexistent

| | **Définition A — éditeur / conseil** | **Définition B — systèmes / académique** |
|---|---|---|
| Portée | couche de coordination **par-dessus la stack existante** | **plan de contrôle avec noyau** |
| Composants | connecteurs, workflows, supervision métier | ordonnanceur · gestion contexte/mémoire · registre d'outils · politique & confiance · observabilité & audit |
| Critères d'évaluation | ROI, time-to-value | **enforcement déterministe · auditabilité · compréhensibilité opérateur** |
| Porteurs | Amdocs (sur BSS/OSS), Fiserv (core banking), Infobip, PwC, Make, Lyzr | arXiv 2606.01508, AIOS (arXiv 2403.16971) |
| Ce que l'acheteur en attend | un catalogue de connecteurs SaaS | de l'exécution contrôlable et vérifiable |

**Revendiquer « aOS » sans préciser, c'est hériter de la définition A** — celle
que les gros ont installée — donc promettre des intégrations Salesforce /
ServiceNow / SAP qu'on n'a pas et qu'on ne veut pas avoir.

### La revendication doit donc être définitionnelle

> « Un système d'exploitation agentique, au sens propre : un ordonnanceur, une
> gestion de contexte, un registre d'outils, une politique d'accès, un journal
> d'audit. Pas une couche de workflow au-dessus de votre SaaS. »

On pose la définition B — **celle de la littérature, pas la nôtre** — puis on est
la seule implémentation qui la satisfait (§5). Un cabinet de conseil ne peut pas
contester une définition systèmes : c'est le terrain où son « OS » se révèle
être un formulaire.

### Ce qu'on ne fait pas

- ❌ Nommer le produit « aOS » ou « Daharness aOS ». Legora appose le ™ sur
  « Legora aOS » ; Amdocs et Fiserv ont leurs variantes. Un sigle de trois
  lettres contesté par trois acteurs dont deux cotés est le pire actif possible
  sans budget contentieux. Par ailleurs AOS = Animate-On-Scroll et AOSP.
- ❌ Chercher à ranker sur « aOS ». Perdu d'avance. On rank sur
  *« agentic operating system on-premise »*, *« self-hosted agentic OS »*.

---

## 3. Cartographie concurrentielle

Quatre anneaux. Aucun ne contient Daharness ; c'est le point.

### Anneau 1 — les aOS verticaux (définition A)

| Acteur | Nom | Date | Domaine |
|---|---|---|---|
| Legora | **Legora aOS™** | mai 2026 | juridique |
| Amdocs | **aOS** | févr. 2026 | télécom |
| Fiserv | **agentOS** | mai 2026 | banque |
| Infobip | **AgentOS** | févr. 2026 | CX |
| PwC | **agent OS** | 2025-26 | entreprise |

SaaS verticaux, posés sur la stack existante du client. **Aucun noyau.** Cinq
lancements en six mois : la catégorie se verrouille vite, mais par le haut et
par métier — le créneau générique et souverain reste ouvert.

### Anneau 2 — les souverains / on-premise

| Acteur | Forme | Point fort | Point faible |
|---|---|---|---|
| **Aradia** | appliance NVIDIA DGX pré-installée | **modèles locaux**, « Own Your Intelligence » | rack de DSI, prix entreprise, propriétaire, runtime tiers ⚠️ |
| **SanctumOS** | aOS auto-hébergé, OSS | revendique la définition B | pas de matériel, maturité inconnue |
| **SUSE** | « Agentic OS » | légitimité OS | couche d'abstraction, pas une flotte d'agents |

**Correction majeure vs la v1 :** j'avais noté V6 (l'appliance) 🟢 incopiable.
**C'est faux** — Aradia vend déjà une appliance agentique souveraine, et elle
fait tourner des **modèles locaux**, ce que Daharness ne sait pas faire. Ce qui
reste différencié n'est pas le *concept* d'appliance mais son **format et son
scénario** : Aradia livre un rack à une DSI ; Daharness livre une boîte basse
consommation qui apporte sa 5G et sa borne Wi-Fi sur un chantier ou dans un
cabinet de cinq personnes. Segments et prix disjoints — l'argument doit être
formulé en *format*, jamais en *nouveauté*.

### Anneau 3 — les orchestrateurs local-first

**AgentOS** (Product Hunt, ~106 votes, 2026) : *local-first, self-hosted, open
source, « run agents like a company »*, cible « builders, solo founders,
one-person companies », suivi de coût par agent, kill switches, détection de
boucle. Plus Conductor, Crystal, Vibe Kanban.

**C'est V1 mot pour mot, déjà livrée par quelqu'un d'autre.** La notation v1
« V1 = ⚠️ faible, copiable en un trimestre » est confirmée empiriquement : c'est
déjà fait.

### Anneau 4 — les CLI des labos

Claude Code, Codex CLI, Gemini CLI, Cursor, Aider, OpenHands. Cloud-liés,
code-centrés, abonnement au siège, pas de flotte, pas de plan de durabilité.

### L'intersection vide — où est Daharness

> **Un runtime d'agent qui lui appartient**, doté d'un **journal durable et
> rejouable**, d'un **RBAC multi-utilisateur**, livrable sur une **boîte basse
> consommation qui apporte son propre réseau**.

Personne n'occupe cette intersection. Et un détail de l'anneau 2/3 mérite
attention : ⚠️ d'après leurs pages, **Aradia et AgentOS s'appuient tous deux sur
un runtime tiers (OpenClaw)** — à re-vérifier, mais si c'est exact, ils héritent
de sa gestion de contexte et ne peuvent pas revendiquer les fonctions noyau.
**Daharness possède son runtime.** C'est précisément ce qui autorise la
revendication de définition B, et ce qui le sépare de l'anneau 3.

---

## 4. Inventaire des actifs — vérifié

Trois statuts, à ne jamais confondre :
🟩 **produit** (livré, générique) · 🟨 **méthode** (pratiqué sur ce dépôt, non
livré — utilisable comme *preuve*, jamais comme *promesse*) · 🟥 **absent**.

| # | Actif | Statut | Preuve |
|---|---|---|---|
| A1 | Curation de contexte architecturale — panels content-hashés, freeze, pagination, `reverie`, Context Radar | 🟩 | `cp-base/src/panels.rs`, `src/app/run/reverie.rs`, `src/app/prompt_builder.rs` |
| A2 | Boucles de feedback bloquantes configurables — bash quelconque, glob, `blocking`, timeout | 🟩 | `crates/cp-mod-callback/` ; ex. `.context-pilot/shared/callbacks.yaml` |
| A3 | Autonomie encadrée — auto-continuation par notifications, garde-fous (durée/messages/tokens/retries), backoff, timers `coucou` | 🟩 | `cp-mod-spine/src/{engine,guard_rail,coucou}.rs` |
| A4 | Durabilité 3 niveaux — oplog WAL append-only rev-numéroté (CRC32C, group-commit, checkpoints, compaction) | 🟩 | `crates/cp-oplog/`, `docs/design-orchestration-backend.md` |
| A5 | Plan de flotte — registry, vue matérialisée, SSE delta, superviseur (spawn pty / stop / restart / adopt) | 🟩 | `crates/cp-orchestrator/` |
| A6 | Double surface — le même agent en TUI et en cockpit web | 🟩 | `src/` (Ratatui) + `web/` (React 19 / TanStack Query / SSE) |
| A7 | RBAC + ACL par agent — 4 rôles, ACL par agent, plan IT (CA privée) dans le cockpit | 🟩 | `cp-orchestrator/src/services/auth/types.rs`, `transport/it/ca.rs`, `docs/design-auth.md` §13 |
| A8 | Coffre à secrets — en mode bridge l'agent ne détient pas les clés, il les demande à l'orchestrateur | 🟩 | `crates/cp-vault/` (`local::Backend` vs `bridge::Backend`) |
| A9 | Surface non-code — recherche web, scraping, OCR, SQLite d'entités, full-text Meilisearch | 🟩 | `cp-mod-{brave,firecrawl,ocr,entities,search}/` |
| A10 | Appliance — Photonicat 2 (RK3576), Armbian Debian 13, installeur eMMC zéro-touch, ULA IPv6 dérivée du serial, Caddy + CA privée, 5G (MBIM), borne Wi-Fi | 🟩 | `deploy/PROVISIONING.md`, `deploy/photonicat/`, `deploy/ansible/` |
| A11 | Souveraineté des données — zéro télémétrie, zéro compte, état dans les dossiers du client, MIT | 🟩 | `docs/trust-center/` |
| A12 | Barre non-négociable — chaîne SHA-256 sur 30+ fichiers, mot de passe humain pour la régénérer | 🟨 **méthode** | `.github/checks/{protected-files.yaml,check-lint-config.sh}`, `chain.sh` — **0 référence dans le code produit** |
| A13 | Inférence locale / air-gap complet | 🟥 **absent** | `cp-base/src/config/llm/models.rs` : 7 backends, tous distants |
| A14 | Le produit s'est construit lui-même | 🟩 fait, ⚠️ à qualifier | 1 955 commits, 2026-01-30 → 2026-08-03, ~114k lignes Rust + ~50k TS, 29 crates. La part exacte écrite par l'agent reste à documenter avant tout chiffrage public |

---

## 5. Matrice de conformité — l'argument central

C'est le tableau qui gagne la comparaison à l'intérieur de la catégorie. Il ne
compare pas des opinions : il confronte chacun à la définition B, que personne
n'a écrite pour nous.

| Critère (définition B) | **Daharness** | Anneau 1 (aOS verticaux) | Anneau 2 (souverains) | Anneau 3 (orchestrateurs) |
|---|---|---|---|---|
| Ordonnanceur | ✅ spine engine, boucle adaptative, superviseur pty (A3, A5) | ❌ | ~ | ~ (délégué au runtime tiers) |
| Gestion contexte & mémoire | ✅ **cœur du produit** (A1) | ~ | ~ | ❌ (héritée du runtime tiers) |
| Registre d'outils & capacités | ✅ système de modules, allow-list de binaires (A9) | ✅ | ~ | ~ |
| Politique & confiance | 🟠 **RBAC oui (A7), scellement non** → P1 | ✅ | ~ | ~ |
| Observabilité & audit | ✅ oplog rejouable, watchdog, flame-graph (A4) | ~ dashboards | ❌ | ~ |
| *Enforcement déterministe* | 🟠 → **P1** | ❌ | ~ | ❌ |
| *Auditabilité* | ✅ WAL rev-numéroté | ~ | ❌ | ❌ |
| *Compréhensibilité opérateur* | ✅ TUI + cockpit (A6) | ~ | ~ | ✅ |

**Score : 6 ✅ / 2 🟠 sur 8 — contre 2 à 3 pour tous les autres.** Les deux 🟠
sont le même chantier (P1).

> **Conséquence non négociable.** Le jour où « Daharness est un aOS » est écrit
> sur la page, le trou du scellement cesse d'être une optimisation : il devient
> un **défaut de conformité à notre propre revendication**, vérifiable par
> n'importe quel ingénieur qui lit `cp-mod-callback/src/lib.rs:185`. C'est le
> meilleur argument pour prioriser P1 — non pas « ce serait un bon
> différenciateur » mais « sans lui, l'accroche est fausse ».

---

## 6. Matrice de valeurs

La conformité (§5) est l'argument ; les valeurs sont ce que l'acheteur ressent.

| # | Valeur | Preuve | Segment | Défendabilité | Évolution v1 → v2 |
|---|---|---|---|---|---|
| **V1** | **Je ne suis plus le goulot** — mes projets avancent sans moi | A3, A5, A6 | Solo | 🔴 **Nulle** — déjà livrée par AgentOS | ⚠️ → 🔴 **Reléguée en accroche.** Jamais présentée comme différenciateur |
| **V2** | **Il tient le fil** — pas de dérive à la 3ᵉ heure | A1, A14 | Tous | 🟡 avance réelle, invérifiable de l'extérieur | = mais **rôle changé** : c'est la fonction noyau que l'anneau 3 ne peut pas avoir (runtime tiers) |
| **V3a** | **Il ne repart pas tant que ce n'est pas vert** | A2, A3 | Équipes exigeantes | 🟡 mécanisme copiable, culture non | = |
| **V3b** | **Il ne peut pas desserrer ses propres règles** | A12 🟨 → P1 | Régulé, équipes | 🟢 **forte si livrée** | = mais **devient critère de conformité**, pas bonus |
| **V4** | **Ça ne répond qu'à moi** | A11, A8 | Régulé, sensible | 🟡 partagée avec Aradia + SanctumOS, entamée par A13 | 🟢 → 🟡 |
| **V5** | **Je peux prouver ce qui s'est passé** | A4, A5, A7 | Équipes, conformité | 🟢 **forte** — seul WAL rejouable du marché | 🟡 → 🟢 **sous-évaluée en v1** |
| **V6** | **Ça arrive dans un carton et ça marche, même sans réseau** | A10 | Terrain, PME sans IT | 🟡 concept copié (Aradia) ; **format et scénario** différenciés | 🟢 → 🟡 |
| **V7** | **Pas d'abonnement au siège** | A11, A10 | PME | 🟡 Aradia dit la même chose | = |

### Le déplacement du centre de gravité

La v1 construisait sur V1 + V4 + V6 — c'est-à-dire, on le sait maintenant, une
valeur déjà copiée et deux valeurs contestées. **Les deux cartes qui restent
sont V5 et V3b** : *prouvable* et *incorruptible*. Elles ne se vendent pas
séparément — ensemble elles forment une seule valeur :

> **Une autonomie qui se vérifie au lieu de se promettre.**

Et le contexte de marché la rend précieuse au bon moment : Gartner place
l'agentic AI au pic des attentes exagérées, nomme l'*agent-washing* comme
problème explicite et estime à moins de 2 % les fournisseurs réels. **Dans le
creux de désillusion, les acheteurs cessent de croire les promesses et
commencent à exiger des preuves.** Daharness est structurellement le seul de la
cartographie à pouvoir en produire : un journal rejouable, un dépôt public, une
chaîne de lints, et une boîte qu'on peut débrancher.

---

## 7. Les trois écarts

### Écart 1 — le scellement n'existe pas hors de ce dépôt *(→ P1)*

Dans le dépôt context-pilot, la boucle se ferme :

```
callbacks.yaml  (whole_file dans protected-files.yaml)
   └→ callbacks bloquants (rust-lints, structure, api-contract, mobile-mirror…)
        └→ check-structure.sh:114 → check-lint-config.sh   (vérifie la chaîne)
   +  .github/workflows/ci.yml (protégé) rejoue les mêmes scripts côté serveur
   +  chain.sh --update exige un mot de passe humain
```

Mais : **rien n'est livré** (0 référence dans `src/`, `crates/`, `web/`,
`yamls/`), c'est **tamper-evident et non tamper-proof** (le vrai backstop est un
serveur CI, pas le harnais), et **le produit livre les outils pour desserrer ses
propres garde-fous** :

| Outil livré | Ce que l'agent peut faire | Garde ? |
|---|---|---|
| `Callback_upsert(action="delete")` | supprimer une boucle de feedback | ❌ `pre_flight` ne vérifie que l'existence de l'id (`cp-mod-callback/src/lib.rs:185`) |
| `Callback_toggle` | désactiver une boucle | ❌ |
| `spine_configure` | mettre `max_duration_secs` / `max_messages` à `disabled` | ❌ (`cp-mod-spine/src/tools.rs:108-130`) |

### Écart 2 — la souveraineté s'arrête à l'appel modèle *(→ P2)*

Les 7 backends de `models.rs` sont tous distants, et le Trust Center l'admet
(*« LLM inference requires API connectivity »*). **Ce n'était qu'un inconfort en
v1 ; c'est désormais un retard concurrentiel** : Aradia fait tourner des modèles
locaux et en fait son accroche.

### Écart 3 — la page promet de partir sans donner de raison de partir

« Step away », « walk away », « no babysitting required » — sans mentionner ni
les garde-fous (A3), ni les boucles bloquantes (A2), ni l'oplog (A4). Autonomie
sans laisse visible = imprudence. **Premier trou de conversion**, et il se
comble sans écrire une ligne de code : les trois actifs existent.

*(Copie relevée avant la refonte du 2026-08-06. L'écart se referme dans
`Daharness/web` ; ce qui est vérifiable ici, ce sont les trois actifs.)*

---

## 8. Segments

| Segment | Achète | Rôle | Effet du cadrage aOS |
|---|---|---|---|
| **Solo multi-projets** | V1, V2 | **Canal d'adoption** — OSS, bouche-à-oreille | ⚠️ légèrement refroidi : « système d'exploitation » ne parle pas à un indépendant |
| **Petite équipe en environnement sensible** (juridique, santé, défense, finance, R&D indus) | V3b, V5, V4, V7 | **Revenu principal** | ✅ **fortement servi** — c'est leur vocabulaire, déjà installé par PwC et consorts |
| **Site déconnecté / terrain** (chantier, navire, usine, clinique) | V6 | **Niche imprenable** — sous réserve de P2 | ~ neutre |
| **Ingénieur / OSS** | le runtime lui-même | Crédibilité et distribution | ✅ la définition B leur parle directement |

**La règle des deux couches.** Le cadrage aOS sert le revenu et refroidit le
canal. On ne choisit pas : **la catégorie répond à « qu'est-ce que c'est ? », le
bénéfice répond à « pourquoi ça me concerne ? »**. Le hero reste humain et
chaleureux ; la ligne de catégorie se pose juste en dessous ; les sections de
preuve implémentent la définition B. Laisser le cadrage aOS tuer la copie
humaine serait l'erreur symétrique de celle qu'on corrige.

---

## 9. L'UVP

### Structure en trois temps

> ### **Daharness est un aOS. Le vôtre.**
> ### Un agent par projet, qui avance pendant votre absence.
> ### Sous des règles qu'il ne peut pas desserrer — et tout ce qu'il a fait reste rejouable.

| Temps | Question à laquelle il répond | Valeur |
|---|---|---|
| **Catégorie** — *« Daharness est un aOS. Le vôtre. »* | qu'est-ce que c'est ? | prend la catégorie gratuitement ; « le vôtre » porte toute la différenciation et contient le reproche implicite : *le leur ne l'est pas* |
| **Promesse** — *« un agent par projet… »* | pourquoi ça me concerne ? | V1 — l'accroche, jamais présentée comme différenciateur |
| **Permission** — *« sous des règles… rejouable »* | pourquoi puis-je le laisser faire ? | V3b + V5 — **le cœur défendable** |

### Version honnête livrable aujourd'hui *(tant que P1 n'est pas fait)*

> « Sous des garde-fous qui l'arrêtent tant que ce n'est pas vert — et tout ce
> qu'il a fait reste rejouable. »

Vrai, livré, vérifiable (A2 + A3 + A4). Moins tranchant, tenable en réunion.

### Énoncé de positionnement

> **Pour** les équipes qui portent plus de projets qu'elles n'ont d'heures et ne
> peuvent pas confier leur travail au cloud d'un tiers,
> **Daharness** est un **système d'exploitation agentique auto-hébergé** —
> logiciel sur vos machines, ou boîtier autonome —
> **qui** fait avancer chaque projet en votre absence, sous des garde-fous que
> vous fixez, avec un journal rejouable de tout ce qui a été fait.
> **Contrairement aux** aOS verticaux posés sur le cloud d'un éditeur, il a un
> vrai noyau, il vous appartient, et vous pouvez le débrancher.

### L'angle d'attaque

À garder pour les sections comparatives et les prises de parole :

> *« Tout le monde annonce un agentic OS. Un OS qui tourne sur le cloud de
> quelqu'un d'autre, dont vous ne pouvez ni voir l'ordonnanceur, ni lire le
> journal, ni couper le réseau — ce n'est pas un OS, c'est un abonnement. Le
> nôtre arrive dans un carton. »*

---

## 10. Chantiers produit

Repriorisés : P1 et P2 ne sont plus des améliorations, ce sont des **conditions
de véracité** de §5 et §9.

### P1 — Sceller les garde-fous *(débloque V3b et 2 lignes de conformité)*

1. **`sealed: true` sur `CallbackDefinition`** — le `pre_flight` de
   `cp-mod-callback` (`lib.rs:185`, déjà le bon point d'accroche) refuse
   `delete` et `toggle` sur un callback scellé.
2. **Même verrou côté spine** — `spine_configure` ne peut ni relever ni
   désactiver un garde-fou scellé.
3. **Chaîne en primitive du realm** — manifeste + chaîne dans `.context-pilot/`,
   dont le `--update` est détenu par l'orchestrateur (le RBAC v3 fournit déjà le
   porteur : Admin/Superadmin) et jamais par l'agent.

### P2 — Backend d'inférence local *(referme l'écart 2, arme V6, rattrape Aradia)*

Un 8ᵉ backend vers un endpoint OpenAI-compatible auto-hébergé. Coût faible
(l'abstraction `ModelInfo` existe), effet majeur : « le seul aOS qui tourne sans
Internet ». Le RK3576 ne fera pas tourner un modèle frontière — pointer vers un
serveur du client suffit à cocher la case.

### P3 — Exposer l'oplog comme journal lisible *(arme V5, désormais la valeur n°1)*

Le WAL existe et est rejouable, mais n'a **aucune surface humaine**. Une vue
« voici tout ce que l'agent a fait pendant votre absence, dans l'ordre » est de
l'assemblage, pas de l'architecture. **Reclassé en priorité haute** : V5 est
passée de 🟡 à 🟢 et c'est la carte la plus solide qui reste.

---

## 11. Réécriture de la page

⚠️ **Tableau écrit le 2026-08-06 contre la page d'alors ; la page a été refondue
depuis** — accroche aOS, section « où ça tourne », boîtier dans le hero, locale
française. Il n'a pas été repassé ligne à ligne sur la version en ligne. Chaque
verdict est donc une **intention à porter et à revérifier** dans
`Daharness/web` (`landing-page/src/`), pas un état des lieux.

| Section (page du 2026-08-06) | Verdict | Action |
|---|---|---|
| Hero « One agent per project » | ✅ Garder — chaleur humaine = canal d'adoption | Ajouter la **ligne de catégorie** en dessous : « Daharness est un aOS. Le vôtre. » |
| « Delegate. Walk away. » (3 étapes) | ⚠️ Incomplet | Insérer une **étape 0 : « Fixez les règles »** (écart 3) |
| Context overflow / « every token is gold » | ✅ Meilleure section | Ne pas y toucher — c'est le critère noyau n°2 |
| « months of sustained work » | ⚠️ Trop vague pour l'actif le plus fort | Chiffrer après avoir qualifié A14 |
| « Yours, and only yours » | ✅ Garder | Préciser la frontière de l'appel modèle (écart 2) |
| The box — 2 modes | ✅ Garder, **remonter** | Reformuler en **format et scénario**, pas en nouveauté (Aradia existe) |
| — | 🔴 **Manquant** | **« Il s'arrête tant que ce n'est pas vert »** (V3a) |
| — | 🔴 **Manquant** | **« Tout est rejouable »** (V5) — la section la plus importante à écrire |
| — | 🔴 **Manquant** | **Tableau de conformité §5** — l'argument comparatif |
| Formulaire de contact | ⚠️ | « submissions aren't wired to a backend yet » est affiché en clair — à câbler ou retirer avant campagne |

---

## 12. Ce qu'il ne faut PAS dire

| ❌ Ne pas écrire | Pourquoi | ✅ Écrire à la place |
|---|---|---|
| « L'agent ne peut pas contourner vos règles » | Faux hors de ce dépôt — `Callback_toggle` et `spine_configure` existent | « Vos garde-fous l'arrêtent tant que ce n'est pas vert » — jusqu'à P1 |
| « Tourne entièrement sur vos machines » | L'appel modèle sort | « Vos données restent chez vous ; seul l'appel modèle sort, vers le fournisseur que vous choisissez » |
| « Fonctionne air-gapped » | Le Trust Center dit le contraire | « Fonctionne sur votre réseau, sans compte ni télémétrie » — jusqu'à P2 |
| « La seule appliance agentique » | Aradia existe | « La seule qui tient dans une boîte et apporte son propre réseau » |
| « Notre harnais est unique » | Invérifiable et commoditisé | Revendiquer la **catégorie** et gagner sur la **conformité** (§5) |
| Se nommer « aOS » / déposer le sigle | Legora ™, Amdocs, Fiserv ; AOS = Animate-On-Scroll, AOSP | La marque est **Daharness** ; aOS est la **case** |
| « aOS » nu, sans qualification | On hérite de la définition A et de ses promesses de connecteurs | Toujours accoler la définition B ou « le vôtre » |
| « Écrit à 100 % par l'IA » | Non qualifié (A14) | Chiffres bruts + « construit en s'utilisant lui-même » |
| Un chiffre de lints incohérent | Le dépôt dit ~961, le Trust Center 1 001, les slides 999 | Une source unique, propagée partout |

**La séparation des dépôts a un coût, et c'est cette dernière ligne.** Le Trust
Center existe maintenant en deux exemplaires — `docs/trust-center/` ici, pour le
hub `docs/index.html`, et `landing-page/src/trust-center/` dans `Daharness/web`,
pour ce qui est publié. Rien ne les tient synchronisés. Toute affirmation
chiffrée ou datée corrigée d'un côté doit l'être de l'autre, sans quoi la
version en ligne dira quelque chose que ce dépôt ne dit plus.

---

## 13. Résumé en une page

- **Catégorie :** Daharness est un **aOS au sens systèmes** — noyau, ordonnanceur,
  journal — pas une couche de workflow sur du SaaS. Catégorie revendiquée, jamais
  portée comme nom.
- **Accroche (copiable, assumée comme telle) :** un agent par projet, ils avancent
  sans vous.
- **Défense (ce qui reste après la cartographie) :** **prouvable et incorruptible**
  — V5 (journal rejouable, seul du marché) + V3b (règles qu'il ne peut pas
  desserrer, *sous réserve de P1*).
- **Argument comparatif :** 6 ✅ / 2 🟠 sur les 8 critères de la définition B,
  contre 2-3 pour tous les autres (§5).
- **Timing :** la catégorie entre dans le creux de désillusion, où les acheteurs
  exigent des preuves. C'est exactement le moment où « vérifiable » bat
  « prometteur ».
- **Les deux chantiers qui rendent la phrase vraie :** P1 (sceller) puis P3
  (rendre l'oplog lisible). P2 (inférence locale) rattrape Aradia.

---

## 14. Sources marché

Amdocs aOS · Legora aOS™ · Fiserv agentOS · Infobip AgentOS · PwC agent OS ·
arXiv 2606.01508 (*Agent Operating Systems*) · arXiv 2403.16971 (*AIOS*) ·
SanctumOS · SUSE Agentic OS · Aradia · AgentOS (Product Hunt) ·
Gartner *Hype Cycle for Agentic AI 2026*. Consultées le 2026-08-06.
