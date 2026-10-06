//! OTA channel selection for the [`ReleaseStore`] — which channel the box
//! follows (`stable`/`nightly`) and the one-shot "crossgrade" flag an explicit
//! admin switch arms so the next check adopts the target channel's head
//! regardless of version ordering. This matters because nightly tags are
//! `v0.1.0-<sha>`, whose `semver_sort_key` sorts *below* a stable `v0.2.x`, so a
//! plain monotonic comparison would refuse the move as a rollback.
//!
//! Exposed as the [`ChannelOps`] trait (not a second inherent `impl`) so
//! `ReleaseStore` keeps a single inherent block while `mod.rs` stays under the
//! 500-line cap. Callers bring it in with `use …::channel::ChannelOps as _;`.

use super::ReleaseStore;
use super::updater::state::UpdateState;

/// OTA-channel operations on a [`ReleaseStore`].
pub(crate) trait ChannelOps {
    /// The channel this box follows (`stable` or `nightly`).
    #[must_use]
    fn channel(&self) -> &str;

    /// Whether an admin channel switch is awaiting its first check — the next
    /// evaluation adopts the new channel's head regardless of version ordering.
    #[must_use]
    fn pending_channel_switch(&self) -> bool;

    /// Switch the channel this box follows and persist. Arms the crossgrade
    /// flag and drops the now-stale "update available" hint (it pertained to
    /// the old channel) so the pane doesn't offer a foreign version until the
    /// next check on the new channel resolves.
    ///
    /// # Errors
    ///
    /// Returns an error if `channel` is not one of `stable` / `nightly`.
    fn set_channel(&mut self, channel: &str) -> Result<(), String>;

    /// Clear the crossgrade flag once a check on the new channel has resolved.
    fn clear_pending_switch(&mut self);
}

impl ChannelOps for ReleaseStore {
    fn channel(&self) -> &str {
        &self.config.channel
    }

    fn pending_channel_switch(&self) -> bool {
        self.config.pending_channel_switch
    }

    fn set_channel(&mut self, channel: &str) -> Result<(), String> {
        if !matches!(channel, "stable" | "nightly") {
            return Err(format!("unknown channel {channel:?} (expected stable or nightly)"));
        }
        if self.config.channel == channel {
            return Ok(());
        }
        channel.clone_into(&mut self.config.channel);
        self.config.pending_channel_switch = true;
        self.persist();
        let mut st = UpdateState::load(&self.dir);
        st.available = None;
        st.available_notes_url = None;
        st.save(&self.dir);
        Ok(())
    }

    fn clear_pending_switch(&mut self) {
        if self.config.pending_channel_switch {
            self.config.pending_channel_switch = false;
            self.persist();
        }
    }
}
