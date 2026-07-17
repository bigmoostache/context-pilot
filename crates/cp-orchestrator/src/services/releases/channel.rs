//! OTA channel selection for the [`ReleaseStore`] — which channel the box
//! follows (`stable`/`nightly`) and the one-shot "crossgrade" flag an explicit
//! admin switch arms so the next check adopts the target channel's head
//! regardless of version ordering. This matters because nightly tags are
//! `v0.1.0-<sha>`, whose `semver_sort_key` sorts *below* a stable `v0.2.x`, so a
//! plain monotonic comparison would refuse the move as a rollback.

use super::ReleaseStore;
use super::updater::UpdateState;

impl ReleaseStore {
    /// The channel this box follows (`stable` or `nightly`).
    #[must_use]
    pub fn channel(&self) -> &str {
        &self.config.channel
    }

    /// Whether an admin channel switch is awaiting its first check — the next
    /// evaluation adopts the new channel's head regardless of version ordering.
    #[must_use]
    pub fn pending_channel_switch(&self) -> bool {
        self.config.pending_channel_switch
    }

    /// Switch the channel this box follows and persist. Arms the crossgrade
    /// flag and drops the now-stale "update available" hint (it pertained to
    /// the old channel) so the pane doesn't offer a foreign version until the
    /// next check on the new channel resolves.
    ///
    /// # Errors
    ///
    /// Returns an error if `channel` is not one of `stable` / `nightly`.
    pub fn set_channel(&mut self, channel: &str) -> Result<(), String> {
        if !matches!(channel, "stable" | "nightly") {
            return Err(format!("unknown channel {channel:?} (expected stable or nightly)"));
        }
        if self.config.channel == channel {
            return Ok(());
        }
        self.config.channel = channel.to_owned();
        self.config.pending_channel_switch = true;
        self.persist();
        let mut st = UpdateState::load(&self.dir);
        st.available = None;
        st.available_notes_url = None;
        st.save(&self.dir);
        Ok(())
    }

    /// Clear the crossgrade flag once a check on the new channel has resolved.
    pub fn clear_pending_switch(&mut self) {
        if self.config.pending_channel_switch {
            self.config.pending_channel_switch = false;
            self.persist();
        }
    }
}
