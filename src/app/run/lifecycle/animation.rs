//! Spinner-animation redraw cadence — extracted from the main loop module
//! (`lifecycle/mod.rs`) to keep it under the 500-line cap.
//!
//! A genuinely idle agent produces zero periodic renders; any animated state
//! (streaming, a WAITING watcher badge, a loading panel, a running console)
//! ticks at 10fps. See [`App::has_active_animation`] for the exact conditions.

use crate::app::App;
use crate::app::panels::now_ms;

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Tick the dirty flag at 10fps **only while something on-screen is actually
    /// animating**, so time-based spinners advance without pinning a core when idle.
    ///
    /// Gated on [`has_active_animation`](Self::has_active_animation): a genuinely idle
    /// agent produces **zero** periodic renders (~0% CPU), while any animated state
    /// (streaming/tooling, a WAITING watcher badge, a loading panel, a running console)
    /// ticks at the full 10fps. Event-driven redraws (input, chunks, cache, state
    /// mutations) set `dirty` at their source, so real changes still render instantly.
    /// This gating fixed the "idle yet pinning CPU" pathology (T309). The 100ms throttle
    /// caps the (cheap) animation scan itself to 10Hz.
    pub(super) fn update_spinner_animation(&mut self) {
        let now = now_ms();
        if now.saturating_sub(self.last_spinner_ms) < 100 {
            return;
        }
        self.last_spinner_ms = now;
        if Self::has_active_animation(&self.state) {
            self.state.flags.ui.dirty = true;
        }
    }

    /// Whether any on-screen element is currently animating and therefore needs
    /// the periodic [`update_spinner_animation`](Self::update_spinner_animation)
    /// redraw tick.
    ///
    /// Mirrors *exactly* the conditions under which the renderer draws a moving
    /// spinner, so the forced-redraw cadence is driven by — and only by — real
    /// animation:
    /// - **streaming / tooling** — the primary badge spins;
    /// - a **timed watcher** is pending — the `WAITING` badge (`AccentDim`)
    ///   spins;
    /// - a **panel is still loading** its first cache content — the `LOADING`
    ///   badge and the sidebar entry spin;
    /// - a **console is running** — its sidebar glyph spins.
    ///
    /// When none hold, the screen is static and no periodic redraw is needed.
    fn has_active_animation(state: &crate::state::State) -> bool {
        if state.stream.phase.is_streaming() {
            return true; // STREAMING / TOOLING badge spinner
        }
        // A pending timed watcher renders the animated WAITING badge.
        let has_timed_watcher = state
            .get_ext::<cp_base::state::watchers::WatcherRegistry>()
            .is_some_and(|reg| reg.active_watchers().iter().any(|w| w.fire_at_ms().is_some()))
            || state
                .get_ext::<cp_mod_spine::schedule::CoucouRegistry>()
                .is_some_and(|reg| reg.has_pending_for(state.resident_thread_id.as_deref()));
        if has_timed_watcher {
            return true;
        }
        // A panel still loading its first content (LOADING badge + sidebar
        // spinner) or a running console (animated sidebar glyph).
        state.context.iter().any(|c| {
            (c.cached_content.is_none() && c.context_type.needs_cache())
                || (c.context_type.as_str() == "console"
                    && c.get_meta_str("console_status").is_some_and(|s| s.starts_with("running")))
        })
    }
}
