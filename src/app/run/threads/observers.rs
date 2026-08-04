//! Focus + behaviour observe-on-change emitters.
//!
//! Two of the bridge's main-loop chokepoints, split out of
//! [`bridge`](super::bridge) so it stays within the workspace's 500-line
//! budget. Both follow the identical idiom used by
//! [`emit_thread_status`](super::bridge::emit_thread_status): diff the live
//! value against the snapshot held in [`BridgeState`], emit **only** on an
//! actual change, and seed (without emitting) on the first pass after boot so a
//! restarted agent never replays its current state as a spurious change.
//!
//! Both ride the **best-effort** path — focus and active behaviour are
//! disposable UI state, so a dropped delta self-heals from the agent's tier-②
//! files on the next read.

use cp_base::state::data::model_helpers::ModelPricing as _;
use cp_mod_bridge::BridgeState;
use cp_mod_threads::types::FocusState;
use cp_wire::types::oplog::OpEntryKind;

use crate::app::App;

use super::bridge::{bridge_active, emit_best_effort};

// ── Thread focus emission (focused-thread highlight — design doc I8) ──────

/// Emit a [`ThreadFocusChanged`](OpEntryKind::ThreadFocusChanged) the instant
/// the agent's focused thread changes, so the backend view (and the web UI's
/// focused-thread highlight) reflect it in milliseconds instead of waiting on
/// the debounced tier-② disk write plus the frontend's backstop poll.
///
/// Like [`emit_thread_status`] this is a main-loop **observe-on-change
/// chokepoint**: it diffs the live [`FocusState::focused_thread_id`] against the
/// snapshot held in [`BridgeState::last_focus`] and emits **only on an actual
/// change**, so it captures focus from *every* source with one uniform path —
/// the idle `MY_TURN` auto-`Read` ([`maybe_inject_auto_read`](super::maybe_inject_auto_read)),
/// a manual `Read`, or focus release on archive / a finished turn — rather than
/// an emit call scattered at each focus-mutation site.
///
/// Focus is ephemeral, disposable UI state (the same class as phase), so it
/// rides the **best-effort** path ([`emit_best_effort`]): a dropped focus delta
/// self-heals from the agent's tier-② `FocusState` on the next `/threads` read
/// and is superseded by the next focus change.
///
/// The first pass after boot **seeds** the snapshot without emitting, so a
/// (re)started agent does not replay its current focus as a spurious change
/// (the cold focus rides the frontend's initial tier-② load).
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_thread_focus(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    let focused = FocusState::get(&app.state).focused_thread_id.clone();

    // First pass: snapshot the existing focus without emitting.
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.focus());
    if !seeded {
        let bs = app.state.ext_mut::<BridgeState>();
        bs.last_focus = focused;
        bs.seeded.seed_focus();
        return;
    }

    // Emit only on an actual change.
    let changed = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.last_focus != focused);
    if changed {
        emit_best_effort(&app.state, OpEntryKind::ThreadFocusChanged { thread_id: focused.clone() });
        app.state.ext_mut::<BridgeState>().last_focus = focused;
    }
}

// ── Behaviour emission (active behaviour-agent — design doc I8, T581) ─────

/// Emit a [`BehaviourChanged`](OpEntryKind::BehaviourChanged) the instant the
/// agent's active behaviour agent (system prompt) changes, so the web footer's
/// behaviour chip reflects it in milliseconds instead of waiting on the coarse
/// `config.json` mtime backstop (~2s) plus the invalidate throttle.
///
/// A main-loop **observe-on-change chokepoint** — the exact idiom of
/// [`emit_thread_status`] / [`emit_thread_focus`]: it diffs the live
/// [`PromptState::active_agent_id`] against the snapshot held in
/// [`BridgeState::last_behaviour`] and emits **only on an actual change**, so it
/// captures a switch from *every* source with one uniform path — the local
/// `agent_load` tool **and** a web `LoadBehaviour` command — rather than an emit
/// call scattered at each mutation site.
///
/// The active behaviour is disposable UI state (the same class as focus/phase),
/// so it rides the **best-effort** path ([`emit_best_effort`]): a dropped delta
/// self-heals via the mtime backstop and is superseded by the next change. The
/// observer (the web bridge) does not fold it — it invalidates its library query
/// so the next read surfaces the fresh active agent from tier-② `config.json`.
///
/// The first pass after boot **seeds** the snapshot without emitting, so a
/// (re)started agent does not replay its current behaviour as a spurious change
/// (the cold value rides the frontend's initial library load).
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_behaviour(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    let active = cp_mod_prompt::types::PromptState::get(&app.state).active_agent_id.clone();

    // First pass: snapshot the existing active behaviour without emitting.
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.behaviour());
    if !seeded {
        let bs = app.state.ext_mut::<BridgeState>();
        bs.last_behaviour = active;
        bs.seeded.seed_behaviour();
        return;
    }

    // Emit only on an actual change.
    let changed = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.last_behaviour != active);
    if changed {
        emit_best_effort(&app.state, OpEntryKind::BehaviourChanged { agent_id: active.clone() });
        app.state.ext_mut::<BridgeState>().last_behaviour = active;
    }
}

// ── Identity emission (self-identity — design doc I8, T730) ───────────────

/// Emit an [`IdentityChanged`](OpEntryKind::IdentityChanged) the instant the
/// agent's durable self-identity (the Agora module's fixed-key value store)
/// changes, so the web agent-settings identity form reflects it in
/// milliseconds instead of waiting on the coarse `config.json` mtime backstop
/// plus the invalidate throttle.
///
/// A main-loop **observe-on-change chokepoint** — the exact idiom of
/// [`emit_behaviour`] / [`emit_thread_focus`]: it diffs a stable JSON
/// fingerprint of the live [`AgoraState`] identity against the snapshot held in
/// [`BridgeState::last_identity`] and emits **only on an actual change**, so it
/// captures an edit from *every* source with one uniform path — the local
/// `Agora_set_identity` tool **and** a web `SetIdentity` command — rather than
/// an emit call scattered at each mutation site.
///
/// The identity is disposable UI state (the same class as behaviour/focus), so
/// it rides the **best-effort** path ([`emit_best_effort`]): a dropped delta
/// self-heals via the mtime backstop and is superseded by the next change. The
/// observer (the web bridge) does not fold it — it invalidates its identity
/// query so the next read surfaces the fresh values from tier-② `config.json`.
///
/// The first pass after boot **seeds** the fingerprint without emitting, so a
/// (re)started agent does not replay its current identity as a spurious change
/// (the cold value rides the frontend's initial identity load).
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_identity(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    // Stable JSON fingerprint of the ten identity fields (serde struct order is
    // deterministic). A String memo keeps `cp-mod-bridge` free of a `cp-agora`
    // dependency — the serialisation lives here, in the agent binary.
    let fingerprint = serde_json::to_string(&cp_agora::types::AgoraState::get(&app.state).identity).unwrap_or_default();

    // First pass: snapshot the existing identity without emitting.
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.identity());
    if !seeded {
        let bs = app.state.ext_mut::<BridgeState>();
        bs.last_identity = Some(fingerprint);
        bs.seeded.seed_identity();
        return;
    }

    // Emit only on an actual change.
    let changed =
        app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.last_identity.as_deref() != Some(&fingerprint));
    if changed {
        emit_best_effort(&app.state, OpEntryKind::IdentityChanged);
        app.state.ext_mut::<BridgeState>().last_identity = Some(fingerprint);
    }
}

// ── Registry-advert projection (discovery record — §10, T739) ─────────────

/// Re-project the agent's **discovery record** (`~/.context-pilot/agents/<id>.json`)
/// from live state, rewriting it only when the projected bytes actually change.
///
/// This is the odd one out among the emitters above, and deliberately so. They
/// each diff one value and emit an oplog delta on change; this one re-derives a
/// whole record and writes a *file* — the artifact the backend's fleet scan
/// discovers agents by.
///
/// # Why a projection rather than a chokepoint
///
/// The record used to be written once at boot and never again, so every mutable
/// field it carried became a lie the moment it changed: `status` stayed
/// `Starting` for the process's whole life, and the backend's
/// `transport/inspect/meta.rs` documents `entry.model` as "unreliable" and
/// reads `config.json` instead. The obvious fix — re-publish from every
/// mutation site — only works if *every* site is found, which is
/// audit-dependent; a forgotten one reintroduces exactly that silent staleness.
///
/// So nothing publishes. The record is re-derived here on the ordinary
/// main-loop cadence and written only if it changed, which makes a missed
/// mutation site impossible by construction. See
/// [`register::advert`](cp_mod_bridge::register::advert) for the write-path
/// reasoning (why the dirty check matters given the directory has a live
/// reader).
///
/// # Cost
///
/// The dirty check lives inside the projection, so the steady state is **zero
/// syscalls**: folder, sockets, token and identity simply do not change between
/// ticks, and an identical projection writes nothing.
///
/// Unlike the emitters above there is no boot **seed**, on purpose: the first
/// tick *should* write, because that is what corrects the boot record's
/// placeholder `status: Starting` to `Running`.
///
/// Best-effort, like its siblings — a failed write leaves the previous record
/// in place (still valid, one tick old) and the next tick retries.
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_advert(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    // Resolve both live inputs BEFORE taking the mutable `BridgeState` borrow
    // that reaches the `Boot` — they read `app.state` immutably, so resolving
    // them afterwards would conflict.
    let model = app.state.current_model();
    let identity = wire_identity(&cp_agora::types::AgoraState::get(&app.state).identity);

    if let Some(boot) = app.state.ext_mut::<BridgeState>().boot.as_mut()
        && let Err(e) = boot.republish_advert(model, identity)
    {
        log::debug!("advert republish failed (retried next tick): {e}");
    }
}

/// Convert the agent's Agora self-identity into its wire twin.
///
/// The two structs are field-identical by design (the wire crate is
/// dependency-free, so it cannot borrow the agent's type), and the conversion
/// is written field-by-field rather than via a serde round-trip: it is cheaper,
/// and it turns any future drift between the two definitions into a **compile
/// error** instead of a silently dropped field.
///
/// A never-introduced agent converts to an all-empty value rather than to
/// nothing; deciding that such an identity is not worth advertising belongs to
/// the projection, not here.
fn wire_identity(identity: &cp_agora::types::SelfIdentity) -> cp_wire::types::registry::SelfIdentity {
    cp_wire::types::registry::SelfIdentity {
        identity: identity.identity.clone(),
        values: identity.values.clone(),
        principles: identity.principles.clone(),
        character: identity.character.clone(),
        expertise: identity.expertise.clone(),
        role: identity.role.clone(),
        operational_responsibilities: identity.operational_responsibilities.clone(),
        knowledge_responsibilities: identity.knowledge_responsibilities.clone(),
        organic_responsibilities: identity.organic_responsibilities.clone(),
        direct_management: identity.direct_management.clone(),
    }
}
