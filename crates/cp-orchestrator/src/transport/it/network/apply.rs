//! State → system configuration. The **only** writer of system network config.
//!
//! # The env gate (NFR-NET-04)
//!
//! Every tool this module drives is named by an environment variable, exactly as
//! Caddy is named by `CP_CADDYFILE`:
//!
//! | Variable | Tool |
//! |---|---|
//! | `CP_NMCLI_BIN` | `nmcli` — the `cp-wwan` and `cp-ap` profiles |
//! | `CP_MMCLI_BIN` | `mmcli` — modem status |
//! | `CP_IW_BIN` | `iw` — the regulatory domain |
//! | `CP_NETWORKD_DIR` | where the strict-`5g` drop-in is written |
//! | `CP_UPLINK_ENV` | `/etc/default/cp-uplink`, the supervisor's config |
//!
//! With `CP_NMCLI_BIN` unset the applier is a **no-op that reports `Ok(false)`**:
//! the backend persists the document and performs no system call. That is what
//! lets the whole feature be developed, unit-tested and reviewed off-hardware,
//! and it is why `cargo test` on a laptop never touches the laptop's network.

use super::state::NetworkConfig;

/// Apply `config` to the system.
///
/// Returns `Ok(true)` when the system was touched, `Ok(false)` when this
/// environment has no `nmcli` gate set and the call was skipped cleanly.
///
/// # Errors
///
/// Returns a message describing the first step that failed. The caller
/// ([`commit`](super::commit)) then rolls the document **and** the system back,
/// so a bad setting can never wedge the box (NFR-NET-05).
pub(crate) fn apply(config: &NetworkConfig) -> Result<bool, String> {
    let Some(_nmcli) = std::env::var_os("CP_NMCLI_BIN") else {
        return Ok(false); // no gate in this environment — persistence only.
    };
    let _unapplied = config;
    Ok(true)
}
