//! Live uplink status — what the cockpit polls every 5 s (FR-NET-10/11).
//!
//! Read from the system, never from what we believe we configured: `ip route`,
//! `nmcli -t`, `mmcli -J`, `iw dev` and the supervisor's `/run/cp-uplink/state`.
//! A human who ran `nmcli` by hand on the box (landmine 9) will see their change
//! reflected here until the next apply reverts it — that is the point, an honest
//! read-out is what makes the box debuggable.
//!
//! **Every field degrades to `null` rather than erroring when a tool is absent.**
//! A dev machine with no gates set answers `200` with a fully-null status; a box
//! whose modem was pulled reports `wwan: null` and keeps serving the cockpit.

use serde_json::{Value, json};

use super::state::NetworkConfig;

/// Build the `status` half of `GET /api/it/network`.
///
/// `config` is passed in — not re-read — so the status and the configuration in
/// the same response describe the same instant.
pub(crate) fn probe(config: &NetworkConfig) -> Value {
    let _unused = config;
    // M3 fills this in from `ip -j route`, `nmcli -t`, `mmcli -J` and `iw dev`.
    json!({
        "active_uplink": Value::Null,
        "wan": Value::Null,
        "wwan": Value::Null,
        "ap": Value::Null,
    })
}
