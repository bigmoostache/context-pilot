# Nestor — Contrat Pi ↔ navigateur (V1)

Le contrat à deux faces du doc de cadrage (§5) : face entrante (commandes web → `Action`),
face sortante (`WebState`, view-model dédié). Les deux faces ont été définies ensemble, en
listant d'abord les commandes entrantes.

Transport : **WebSocket JSON** (snapshot à la connexion + deltas), **HTTP** pour l'auth et la SPA.

---

## 1. Face entrante — commandes web

Enveloppe client → serveur :

```jsonc
{ "t": "cmd",   ...WebCommand }                 // commande (fire-and-forget)
{ "t": "query", "req_id": "q1", ...WebQuery }   // requête (réponse corrélée par req_id)
```

### 1.1 Commandes (`WebCommand`, champ `cmd` discriminant)

| `cmd` | Payload | Traitement côté cœur |
|---|---|---|
| `submit` | `{ text }` | `state.input = text` puis `Action::InputSubmit` |
| `stop` | — | `Action::StopStreaming` |
| `select_panel` | `{ id }` | `Action::SelectContextById(id)` |
| `clear_conversation` | — | `Action::ClearConversation` |
| `new_context` | — | `Action::NewContext` |
| `reset_costs` | — | `Action::ResetSessionCosts` |
| `reload` | — | `flags.lifecycle.reload_pending = true` (le binaire se relance via `exec()`) |
| `set_provider` | `{ scope: "primary"\|"secondary", provider }` | `Action::ConfigSelect[Secondary]Provider` |
| `set_model` | `{ scope, model }` | `Action::ConfigSelect*Model` selon le provider actif du scope |
| `set_theme` | `{ theme }` | `Action::ConfigSetTheme(theme)` *(nouveau variant, set direct)* |
| `toggle_auto_continue` | — | `Action::ConfigToggleAutoContinue` |
| `toggle_reverie` | — | `Action::ConfigToggleReverie` |
| `set_context_budget` | `{ tokens: number\|null }` | `Action::ConfigSetContextBudget` *(nouveau, clampé 10 %–100 % de la fenêtre)* |
| `set_cleaning_threshold` | `{ value }` | `Action::ConfigSetCleaningThreshold` *(clampé 0.30–0.95)* |
| `set_cleaning_target` | `{ value }` | `Action::ConfigSetCleaningTarget` *(clampé 0.30–0.95)* |
| `set_max_cost` | `{ value: number\|null }` | `Action::ConfigSetMaxCost` (garde-fou spine, `null` = désactivé) |
| `set_think_threshold` | `{ value }` | `Action::ConfigSetThinkThreshold` (cap à −1) |
| `answer_question` | `{ tool_use_id, answers: [{ selected: [idx], other_text? }] }` | mutation directe de `PendingForm` + `submit()` — le check de la boucle produit le `ToolResult` |
| `dismiss_question` | `{ tool_use_id }` | `PendingForm::dismiss()` |

Les interactions qui « mutent l'état directement » dans la TUI (formulaire de question) gardent
le même chemin : la commande web reproduit la mutation, la boucle existante fait le reste.

**Édition d'input possédée par le navigateur** (décision n°8) : aucune commande de frappe.
Le front n'envoie que le texte soumis. Pas de `InputChar`/cursor par le réseau.

### 1.2 Requêtes (`WebQuery`, champ `q` discriminant)

| `q` | Payload | Réponse (`{ "t": "result", req_id, data }`) |
|---|---|---|
| `list_dir` | `{ dir, prefix }` | `{ entries: [{ name, is_dir }] }` — autocomplete `@` côté web (réutilise `cp_mod_tree::tools::list_dir_entries` + filtre du TreeState) |
| `panel_content` | `{ id }` | `{ id, kind, name, content, metadata }` — contenu d'un panneau non sélectionné |
| `prompt_history` | `{ limit? }` | `{ entries: [string] }` — historique des prompts (jsonl) |
| `index_status` | — | `{ text }` — contenu de l'overlay de statut d'index (Ctrl+I) |

### 1.3 Nouveaux variants `Action` (set directs, web-natifs)

Le web manipule des `Slider`/`Select`, pas des hotkeys incrémentales. Ajout dans
`cp_base::state::actions::Action` (réutilisables par la TUI plus tard) :

```rust
ConfigSetTheme(String),
ConfigSetContextBudget(Option<usize>),
ConfigSetCleaningThreshold(f32),
ConfigSetCleaningTarget(f32),
ConfigSetMaxCost(Option<f64>),
ConfigSetThinkThreshold(i64),
```

Le clamping réutilise la logique de `src/app/actions/config.rs`.

---

## 2. Face sortante — `WebState`

View-model **dédié et stable**, assemblé par `build_web_state(&State)` (`src/web/build.rs`),
miroir du pattern `build_frame()`. Le web ne consomme **pas** le `Frame` IR (décision verrouillée).

```jsonc
{
  "status": {
    "stream_phase": "idle" | "receiving" | "executing_tools",
    "streaming_tool": { "name", "input_so_far" } | null,
    "guard_rail_blocked": string | null,
    "last_stop_reason": string | null,
    "api_check_in_progress": bool,
    "api_check": { "ok": bool, "message": string } | null,
    "provider": "anthropic" | ..., "model": string,
    "secondary_provider": string, "secondary_model": string,
    "theme": string,
    "auto_continue": bool, "reverie_enabled": bool,
    "think_threshold": int, "max_cost": number | null,
    "cleaning_threshold": number, "cleaning_target": number,
    "context_used_tokens": int, "context_budget": int | null, "context_window": int,
    "session_tokens": { "cache_hit", "cache_miss", "output", "uncached_input" },
    "tick_tokens":    { "cache_hit", "cache_miss", "output", "uncached_input" },
    "alive_breakpoints": int, "bp_positions_permille": [int],
    "spine_notifications": int            // non traitées
  },
  "panels": [ {
    "id": "P1", "uid": string | null, "kind": "todo" | "file" | ...,
    "name": string, "is_fixed": bool, "selected": bool,
    "token_count": int, "full_token_count": int,
    "page": int, "total_pages": int, "last_refresh_ms": int
  } ],
  "active_panel": {                        // contenu du panneau sélectionné uniquement
    "id": string, "kind": string, "name": string,
    "content": string,                     // texte LLM-facing (cached_content)
    "metadata": object                     // ex. file_path → coloration Shiki côté client
  } | null,
  "conversation": [ {
    "id": "U1" | "A1" | "T1" | "R1", "uid": string | null,
    "role": "user" | "assistant", "kind": "text" | "tool_call" | "tool_result",
    "content": string, "status": "full" | "deleted" | "detached",
    "tool_uses":   [{ "id", "name", "input" }],
    "tool_results":[{ "tool_use_id", "content", "tldr", "is_error", "tool_name" }],
    "timestamp_ms": int
  } ],
  "question_form": {
    "tool_use_id": string,
    "questions": [{ "text", "header", "multi_select", "options": [{ "label", "description" }] }]
  } | null,
  "input_draft": string,                   // informatif (draft TUI) — le web possède son input
  "meta": {                                // snapshot uniquement (statique par session)
    "themes": [string],
    "providers": [{ "id": string, "label": string, "models": [{ "id", "label" }] }],
    "tools": [{ "id", "name", "short_desc", "enabled" }],
    "workspace": string, "version": string
  }
}
```

### 2.1 Synchronisation : snapshot + deltas (décision n°1)

Serveur → client :

```jsonc
{ "t": "snapshot", "state": WebState }       // à la connexion (et sur demande)
{ "t": "delta",                              // sections changées uniquement
  "status"?, "panels"?, "active_panel"?,
  "conversation_upsert"?: [WebMessage],      // messages nouveaux/modifiés (granularité message)
  "conversation_remove"?: [id],
  "question_form"?: WebQuestionForm | null,
  "input_draft"?: string }
{ "t": "append", "id": "A3", "text": "..." } // fast-path streaming : suffixe du dernier message
{ "t": "result", "req_id", "data" }
{ "t": "error", "message" }
{ "t": "bye", "reason" }                     // arrêt / perte d'ownership de session
```

Mécanique côté cœur (`WebSink`, `OutputSink`) :
- déclenchée par le `ui.dirty` existant, throttlée (~20 Hz max) ;
- hash FxHash par section (`status`, `panels`, `active_panel`, `question_form`) → n'émet que ce qui change ;
- conversation : hash par message (uid) → upsert ciblé ;
- si seul le contenu du dernier message assistant a grandi (préfixe inchangé) → `append`
  (le typewriter existant se mappe naturellement sur ce flux).

---

## 3. Auth & réseau (décision n°4)

- `POST /api/login { password, device_name }` → `{ token, device_id }`.
  Mot de passe vérifié contre un hash **argon2id** stocké dans `.context-pilot/web-auth.json`
  (initialisé depuis `CP_WEB_PASSWORD` au premier lancement). Throttle anti-brute-force (1 essai/s).
- Token de session **par appareil, révocable** : aléatoire 256 bits, stocké hashé (SHA-256),
  persistant. `GET /api/devices` (liste), `POST /api/devices/revoke { device_id }`.
- WS : `GET /ws?token=…` — refusé sans token valide.
- **Bind explicite** : `--web-bind <ip:port>` obligatoire en mode headless ;
  `0.0.0.0` refusé sans `--web-bind-any` explicite. Jamais de défaut ouvert.
- HTTP simple en V1 (LAN) — TLS prévu post-V1.
- SPA servie depuis `--web-dist <dir>`.

## 4. Threading

```
thread principal (sync)             thread tokio (cp-web-server)
┌──────────────────────────┐        ┌─────────────────────────────┐
│ boucle App::run          │        │ axum : /api/*, /ws, SPA     │
│  WebSource (InputSource) │◄─mpsc──│ conn WS → WebEvent          │
│  WebSink   (OutputSink)  │──bcast►│ broadcast frames → conns WS │
└──────────────────────────┘        └─────────────────────────────┘
```

- `std::sync::mpsc<WebEvent>` web → cœur (commandes, requêtes, connexions).
- `tokio::sync::broadcast<WireFrame>` cœur → web ; frames pré-sérialisées,
  adressables à une connexion (`to: Some(conn_id)`) pour snapshot/result.
- La TUI et le web sont deux impls des mêmes traits `InputSource`/`OutputSink` —
  le cœur reste agnostique (décision §4 du cadrage).
