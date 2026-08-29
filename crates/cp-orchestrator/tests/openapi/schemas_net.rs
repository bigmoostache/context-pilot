//! Network `OpenAPI` schemas — uplink mode, 5G bearer, Wi-Fi AP, live status and
//! the failover supervisor's state block.
//!
//! Split out of [`schemas_ext2`](super::schemas_ext2) when the supervisor block
//! and the per-field secret documentation pushed that file past the 500-line
//! budget; merged alongside it in the spec builder.
//!
//! This module owns **both halves of every network shape**: the read projection
//! (`ItNetwork*`) and the request body of the POST that writes it. They are
//! derived from one list of fields per resource ([`ap_properties`],
//! [`wwan_properties`]), with only the secret columns differing — the read half
//! carries the `*_set` booleans, the write half carries the write-only secrets
//! themselves. A field added to the Rust struct therefore lands in the read
//! schema and the write body together, or in neither (review C9). The wire
//! enums below are written once each for the same reason: `wan_5g` was spelled
//! out in three places and a fourth variant would have compiled everywhere.

use serde_json::{Value, json};

use super::{arr, merge, r};

// ── Wire enums, one definition each (review C9) ─────────────────────

/// `UplinkMode`'s wire spellings (`state.rs`, `UplinkMode::as_str`).
fn uplink_mode() -> Value {
    json!({ "type": "string", "enum": ["wan", "wan_5g", "5g"] })
}

/// `Standby`'s wire spellings.
fn standby() -> Value {
    json!({ "type": "string", "enum": ["hot", "cold"] })
}

/// `Band`'s wire spellings — `nmcli`'s, not the human ones.
fn band() -> Value {
    json!({ "type": "string", "enum": ["bg", "a"] })
}

/// How the device carrying the default route is classified. Shared by
/// `status.active_uplink` and the supervisor's own copy, which are produced by
/// two different implementations (`status.rs` and `cp-uplink-watch`) and must
/// agree value for value.
///
/// `other` exists because they used to answer `wwan` for `tun0`, `wg0` or
/// `docker0` — a VPN or a container bridge holding the default route is not a
/// 5G uplink, and saying so during an outage is actively misleading (B17).
fn active_uplink(description: &str) -> Value {
    merge(json!({ "type": "string", "enum": ["wan", "wwan", "other", "none"] }), json!({ "description": description }))
}

/// A write-only secret: `{"type":"string","nullable":true}` plus the tri-state
/// contract, which is the only place a non-TypeScript consumer can learn it
/// (review B13). `description` must spell out all three cases and what the
/// `null` case costs.
fn write_only(description: &str) -> Value {
    json!({ "type": "string", "nullable": true, "description": description })
}

/// `required` as one list: the fields both halves share, plus this half's own.
fn required(shared: &[&str], own: &[&str]) -> Value {
    let mut names: Vec<&str> = shared.to_vec();
    names.extend_from_slice(own);
    json!(names)
}

// ── Access point: one field list, two halves ────────────────────────

/// The AP fields that are neither the secret nor its stand-in — identical in
/// the read projection and in the `POST /api/it/network/ap` body.
fn ap_properties() -> Value {
    json!({
        "enabled": { "type": "boolean" },
        "ssid": { "type": "string", "description": "Broadcast network name, 1\u{2013}32 bytes." },
        "band": band(),
        "channel": { "type": "integer", "description": "Channel number, or 0 for automatic. Must be legal on `band`." },
        "country": { "type": "string", "description": "ISO-3166 two-letter regulatory code, or empty." },
        "hidden": { "type": "boolean" },
        "share_internet": { "type": "boolean" }
    })
}

/// The names in [`ap_properties`], in declaration order.
const AP_SHARED: &[&str] = &["enabled", "ssid", "band", "channel", "country", "hidden", "share_internet"];

/// Request body of `POST /api/it/network/ap` — [`ap_properties`] plus the
/// write-only PSK. `passphrase` is deliberately **not** required: omitting it is
/// how a caller saves the rest of the form without touching the stored key.
pub(super) fn ap_body() -> Value {
    json!({
        "type": "object",
        "properties": merge(ap_properties(), json!({
            "passphrase": write_only(
                "WPA2 pre-shared key, 8\u{2013}63 characters. Write-only and TRI-STATE: **omit the field** to keep \
                 the stored key, send **null** to clear it, send a **string** to replace it. Clearing it is \
                 destructive and quiet: the passphrase prerequisite is only enforced while `enabled` is true, \
                 so `passphrase: null` on a disabled AP returns 200 and the access point can no longer be \
                 enabled until a new key is set. Never returned by any read path \u{2014} see `passphrase_set`."
            )
        })),
        "required": required(AP_SHARED, &[])
    })
}

// ── 5G bearer: one field list, two halves ───────────────────────────

/// The bearer fields that carry no secret — identical in the read projection
/// and in the `POST /api/it/network/wwan` body.
fn wwan_properties() -> Value {
    json!({
        "apn": { "type": "string", "description": "Carrier access point name; empty lets ModemManager pick." },
        "roaming": { "type": "boolean" },
        "standby": merge(standby(), json!({
            "description": "How much of the 5G stack stays warm while ethernet is the uplink: `hot` keeps the \
                bearer connected at a standby route metric (failover is a metric flip), `cold` keeps the modem \
                registered with no bearer (failover pays the setup, but the SIM is not attached)."
        }))
    })
}

/// The names in [`wwan_properties`], in declaration order.
const WWAN_SHARED: &[&str] = &["apn", "roaming", "standby"];

/// Request body of `POST /api/it/network/wwan` — [`wwan_properties`] plus the
/// three write-only credentials. None of the three is required: omitting one is
/// how a caller leaves the stored value alone.
pub(super) fn wwan_body() -> Value {
    json!({
        "type": "object",
        "properties": merge(wwan_properties(), json!({
            "username": write_only(
                "PAP/CHAP username, when the carrier needs one. TRI-STATE: **omit** to keep the stored value, \
                 **null** to clear it, a **string** to replace it. Unlike the two below it is not a secret \u{2014} \
                 it is returned as `config.wwan.username` \u{2014} it is only tri-state on the way in."
            ),
            "password": write_only(
                "PAP/CHAP password. Write-only: never returned, see `password_set`. TRI-STATE: **omit** to keep \
                 the stored password, **null** to clear it, a **string** to replace it. Clearing it on a carrier \
                 that authenticates leaves the bearer unable to attach."
            ),
            "pin": write_only(
                "SIM PIN, 4\u{2013}8 digits. Write-only: never returned, see `pin_set`. TRI-STATE: **omit** to keep the \
                 stored PIN, **null** to clear it, a **string** to replace it. Sending null on a PIN-locked SIM \
                 costs the box its uplink at the next boot \u{2014} the modem comes up locked and nothing unlocks it. \
                 A wrong value is worse: it burns one of the SIM's three unlock retries."
            )
        })),
        "required": required(WWAN_SHARED, &[])
    })
}

/// Request body of `POST /api/it/network/mode`.
pub(super) fn mode_body() -> Value {
    json!({
        "type": "object",
        "properties": { "mode": uplink_mode() },
        "required": ["mode"]
    })
}

// ── Schemas ─────────────────────────────────────────────────────────

/// Uplink, access-point, probe, status and supervisor schemas.
///
/// Secrets are write-only: the bearer password, the SIM PIN and the Wi-Fi PSK
/// are `POSTed` but never returned, so every read schema carries a `*_set`
/// boolean in their place.
#[expect(
    clippy::too_many_lines,
    reason = "flat OpenAPI schema literal: one linear json! object enumerating the domain schemas; splitting it into sub-builders is churn with no readability gain — the openapi spec-builder twin of the flat State::default initializer"
)]
pub(super) fn network() -> Value {
    json!({
        "ItNetworkWwan": {
            "type": "object",
            "description": "5G bearer settings \u{2014} VENDOR-MANAGED. Writing them needs \
                `can_manage_secrets` (superadmin), not `can_manage_it`: the SIM and the data plan \
                are the vendor's, so the APN that goes with them is a fleet-wide decision. \
                `password_set`/`pin_set` stand in for the write-only credentials.",
            "properties": merge(wwan_properties(), json!({
                "username": { "type": "string", "nullable": true },
                "password_set": { "type": "boolean", "description": "Whether a bearer password is stored." },
                "pin_set": { "type": "boolean", "description": "Whether a SIM PIN is stored." }
            })),
            "required": required(WWAN_SHARED, &["username", "password_set", "pin_set"])
        },
        "ItNetworkAp": {
            "type": "object",
            "description": "Wi-Fi access-point settings. `country` is a functional prerequisite, \
                not a nicety: under the world-default regulatory domain every 5 GHz channel is \
                `no IR` and the radio cannot start, so enabling the AP without a country code is refused.",
            "properties": merge(ap_properties(), json!({
                "passphrase_set": { "type": "boolean", "description": "Whether a PSK is stored. The key itself is never returned." }
            })),
            "required": required(AP_SHARED, &["passphrase_set"])
        },
        "ItNetworkProbe": {
            "type": "object",
            "description": "Failover probe tuning. The probe is interface-bound, so it tests the \
                WAN path itself rather than whatever the default route currently prefers. \
                READ-ONLY throughout: these values are seeded at provisioning time and no endpoint \
                accepts them (see `ItNetworkConfig.probe`).",
            "properties": {
                "targets": arr(json!({ "type": "string" })),
                "fail_threshold": { "type": "integer", "description": "Consecutive failed probes before the WAN is demoted." },
                "ok_threshold": { "type": "integer", "description": "Consecutive successful probes before the WAN is restored." },
                "interval_s": { "type": "integer", "description": "Seconds between probe rounds." },
                "probe_timeout_s": { "type": "integer", "description": "Per-target ping deadline in seconds (`ping -W`). Seeded at 3." },
                "cooldown_s": {
                    "type": "integer",
                    "description": "Minimum seconds between failover transitions \u{2014} the anti-flap floor. Seeded at 60. \
                        It has no effect once `interval_s` exceeds it, since a round can then never come back inside it."
                },
                "nm_wait_s": {
                    "type": "integer",
                    "description": "Cap on any single `nmcli` call (`nmcli --wait`), in seconds. Seeded at 20. \
                        The supervisor is a single loop: an unbounded call against a modem with no coverage would \
                        stop it probing, so it could not notice the WAN coming back."
                }
            },
            "required": ["targets", "fail_threshold", "ok_threshold", "interval_s", "probe_timeout_s", "cooldown_s", "nm_wait_s"]
        },
        "ItNetworkConfig": {
            "type": "object",
            "properties": {
                "mode": uplink_mode(),
                "wwan": {
                    "allOf": [r("ItNetworkWwan")],
                    "nullable": true,
                    "description": "Null for two independent reasons: the caller lacks `can_manage_secrets` (the \
                        bearer's CONFIGURATION is vendor state), or this box is not a 5G variant at all \
                        (`status.modem_present` is false). `status.wwan` is not elided either way \u{2014} registration, \
                        operator and signal are diagnostics the client's own admin needs when the box loses its uplink."
                },
                "ap": r("ItNetworkAp"),
                "probe": {
                    "allOf": [r("ItNetworkProbe")],
                    "readOnly": true,
                    "description": "READ-ONLY. Seeded at provisioning time from `network.json.j2` and not writable \
                        from the cockpit: no endpoint accepts probe tuning, so a generated client must not present \
                        these as editable config. Returned so an admin can see what the supervisor is running with."
                }
            },
            "required": ["mode", "wwan", "ap", "probe"]
        },
        "ItNetworkWanStatus": {
            "type": "object",
            "properties": {
                "carrier": { "type": "boolean" },
                "ip": { "type": "string", "nullable": true },
                "gateway": { "type": "string", "nullable": true },
                "has_default_route": { "type": "boolean" }
            },
            "required": ["carrier", "ip", "gateway", "has_default_route"]
        },
        "ItNetworkWwanStatus": {
            "type": "object",
            "properties": {
                "state": { "type": "string", "nullable": true },
                "operator": { "type": "string", "nullable": true },
                "tech": { "type": "string", "nullable": true },
                "signal_dbm": { "type": "integer", "nullable": true },
                "ip": { "type": "string", "nullable": true },
                "registered": { "type": "boolean" }
            },
            "required": ["state", "operator", "tech", "signal_dbm", "ip", "registered"]
        },
        "ItNetworkApStatus": {
            "type": "object",
            "properties": {
                "running": { "type": "boolean" },
                // Echoes the configured SSID, so it is always present — the
                // whole object is null when there is no `nmcli` to ask.
                "ssid": { "type": "string" },
                "clients": { "type": "integer" },
                "channel": { "type": "integer", "nullable": true },
                "country": { "type": "string", "nullable": true }
            },
            "required": ["running", "ssid", "clients", "channel", "country"]
        },
        "ItNetworkSupervisor": {
            "type": "object",
            "description": "What the failover supervisor itself believes, parsed from the state line the \
                `cp-uplink` daemon writes to `/run/cp-uplink/state` on every probe round. Null when \
                `cp-uplink` is not running or has not yet written its first line. \
                Everything else under `status` is observed from the system; this block is the only place the \
                supervisor's INTENT and its streak counters are visible, and it is what makes a failover that \
                did not happen diagnosable.",
            "properties": {
                "supervising": {
                    "type": "boolean",
                    "description": "The supervisor is ARBITRATING (mode `wan_5g`). False in `wan` and `5g`, where \
                        the routing is static and the applier owns it \u{2014} the daemon then only observes and reports."
                },
                "promoted": {
                    "type": "boolean",
                    "description": "DECIDED: the supervisor has concluded the bearer should be the uplink. This is \
                        its intent, not an observation \u{2014} it says nothing about whether the bearer actually came up. \
                        Read it together with `achieved` and `active_uplink`."
                },
                "achieved": {
                    "type": "boolean",
                    "description": "CARRIED OUT: the decision in `promoted` actually took effect on the bearer. \
                        `promoted: true` with `achieved: false` is the outage signature \u{2014} the supervisor wanted 5G \
                        and could not activate it (no coverage, no SIM, modem still enumerating), so the box has no \
                        uplink at all rather than the 5G one the decision implies."
                },
                "active_uplink": active_uplink(
                    "OBSERVED: what the kernel's routing table actually shows, independent of the two fields above. \
                     `promoted: true` with `active_uplink: \"wan\"` means the promotion has not landed yet; with \
                     `\"none\"` it means it failed outright."
                ),
                "fail_streak": { "type": "integer", "description": "Consecutive failed WAN probes; reset by a success and by a promotion." },
                "ok_streak": { "type": "integer", "description": "Consecutive successful WAN probes; reset by a failure and by a demotion." },
                "last_transition": {
                    "type": "integer",
                    "nullable": true,
                    "description": "Unix seconds of the last promote/demote. Null when there has never been one \
                        since the supervisor started \u{2014} it does not survive a restart (`/run` is wiped)."
                },
                "last_reason": {
                    "type": "string",
                    "nullable": true,
                    "description": "The reason logged with that transition, verbatim from the journal line. Null when there has never been one."
                }
            },
            "required": ["supervising", "promoted", "achieved", "active_uplink", "fail_streak", "ok_streak", "last_transition", "last_reason"]
        },
        "ItNetworkStatus": {
            "type": "object",
            "description": "Live read-back from `/proc/net/route`, sysfs, `nmcli`, `mmcli` and `iw`. The \
                TOOL-BACKED halves \u{2014} `wan.ip`, `wwan`, `ap` \u{2014} degrade to null rather than erroring when the \
                tool is absent, so a dev machine with no env gates set still answers 200. `active_uplink` and \
                `modem_present` deliberately do NOT: they are answered from `/proc/net/route` and from sysfs, \
                which need no gate, so the two fields an admin watches during a failover stay truthful on every box.",
            "properties": {
                "active_uplink": active_uplink(
                    "The device carrying the lowest-metric default route, classified: `wan` (the ethernet port), \
                     `wwan` (the 5G bearer), `other` (some other device holds it \u{2014} a VPN, a container bridge), \
                     `none` (there is no default route). Never null: no default route is spelled `none`."
                ),
                // HARDWARE fact, not a live one: this box either is a 5G variant
                // or it is not. Probed from sysfs so it cannot flap while
                // ModemManager restarts, and readable by any `can_manage_it`
                // caller — choosing the uplink mode is the client admin's job
                // even though the bearer's configuration is the vendor's. False
                // ⇒ the cockpit offers only the ethernet mode.
                "modem_present": { "type": "boolean" },
                // Always an object: carrier and has_default_route come from
                // sysfs and /proc, which are always readable.
                "wan": r("ItNetworkWanStatus"),
                "wwan": { "allOf": [r("ItNetworkWwanStatus")], "nullable": true },
                "ap": { "allOf": [r("ItNetworkApStatus")], "nullable": true },
                "supervisor": {
                    "allOf": [r("ItNetworkSupervisor")],
                    "nullable": true,
                    "description": "Null when the failover supervisor is not running or its state file is absent."
                },
                // Folded in here rather than given a route of its own: the
                // cockpit already polls this response every five seconds, and
                // answering "can this box do SMS" from the SAME `modem_present`
                // probe that gated the 5G modes is what stops the two halves of
                // the panel from ever disagreeing.
                "sms": {
                    "allOf": [r("ItSmsStatus")],
                    "nullable": true,
                    "description": "Null on a box with no 5G module or no `mmcli` \u{2014} the SMS panel is then absent, not disabled."
                }
            },
            "required": ["active_uplink", "modem_present", "wan", "wwan", "ap", "supervisor", "sms"]
        },
        "ItNetworkResponse": {
            "type": "object",
            "properties": {
                "config": r("ItNetworkConfig"),
                "status": r("ItNetworkStatus")
            },
            "required": ["config", "status"]
        },
        "ItNetworkModeResult": {
            "type": "object",
            "properties": {
                "mode": uplink_mode(),
                "applied": { "type": "boolean" }
            },
            "required": ["mode", "applied"]
        },
        "ItNetworkApResult": {
            "type": "object",
            "properties": {
                "ap": r("ItNetworkAp"),
                "applied": { "type": "boolean" }
            },
            "required": ["ap", "applied"]
        },
        "ItNetworkWwanResult": {
            "type": "object",
            "properties": {
                "wwan": r("ItNetworkWwan"),
                "applied": { "type": "boolean" }
            },
            "required": ["wwan", "applied"]
        }
    })
}
