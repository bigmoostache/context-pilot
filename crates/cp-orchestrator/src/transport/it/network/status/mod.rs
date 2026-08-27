//! Live uplink status — what the cockpit polls every 5 s.
//!
//! Read from the system, never from what we believe we configured: `/proc/net`,
//! `ip`, `nmcli -t`, `mmcli -J` and `iw`. A human who ran `nmcli` by hand on the
//! box sees their change reflected here until the next apply reverts it — an
//! honest read-out is what makes the box debuggable.
//!
//! **Every *tool-backed* field degrades to `null` rather than erroring when that
//! tool is absent.** A dev machine with no gates set answers `200`; a box whose
//! modem was pulled reports `wwan: null` and keeps serving the cockpit.
//!
//! Two fields deliberately **never** degrade, and a caller must not expect a
//! null from them: `modem_present` is a sysfs hardware fact with a defined
//! answer everywhere, and `active_uplink` is one of four strings — `none` is its
//! spelling of "nothing to report", not `null`. Both need no gate at all,
//! because the default-route half is parsed from `/proc/net/route` and
//! `/proc/net/ipv6_route`, and they are precisely the two fields an admin
//! watches during a failover.
//!
//! `supervisor` is a third kind: `null` when `cp-uplink-watch` has published no
//! state, an object when it has. That is a statement about the supervisor, not
//! about a missing tool.

use std::ffi::OsStr;

use serde_json::{Value, json};

use super::apply::{AP_PROFILE, Tools, WWAN_PROFILE, ap_device, run, wan_iface, wwan_device};
use super::state::NetworkConfig;
use super::uplink::supervisor_status;

/// Build the `status` half of `GET /api/it/network`.
///
/// `config` is passed in — not re-read — so the config and the status in the
/// same response describe the same instant. `has_modem` is threaded in from the
/// transport layer for the same reason: one hardware probe per request,
/// and one answer, so the `modem_present` the cockpit reads is the same one the
/// handlers gated on rather than an independent sysfs read that could disagree.
pub(crate) fn probe(config: &NetworkConfig, has_modem: bool) -> Value {
    // One default-route read per request, threaded into both consumers — it was
    // parsed twice, and the two reads could straddle a failover.
    let route = default_route();
    let default_dev = route.as_ref().map(|found| found.0.as_str());
    // `and_then`, not `map`: an IPv6 winner contributes a device and no gateway,
    // so "no default route at all" and "a default route with no v4 gateway to
    // show" collapse to the same `None` the cockpit already renders as null.
    let gateway = route.as_ref().and_then(|found| found.1.as_deref());
    let tools = Tools::resolve();
    json!({
        "active_uplink": active_uplink(default_dev),
        // A HARDWARE fact, distinct from `wwan` below: this box either is a 5G
        // variant or it is not, and the answer must not flap while ModemManager
        // restarts. It is what tells the cockpit whether to offer the 5G uplink
        // modes at all — and it is readable by any `can_manage_it` caller, since
        // choosing the uplink mode is the client admin's job even though the
        // bearer's configuration is the vendor's.
        "modem_present": has_modem,
        "wan": wan_status(default_dev, gateway),
        "wwan": tools.as_ref().and_then(wwan_status),
        "ap": tools.as_ref().map(|tools| ap_status(tools, config)),
        "supervisor": supervisor_status(),
    })
}

// ── Default route, straight from /proc (no gate) ────────────────────────────

/// `(iface, IPv4 gateway)` of the lowest-metric default route, **across both
/// address families**.
///
/// BOTH TABLES, because which family a mobile bearer gets is not ours to assume.
/// Same SIM, same APN, same operator (Bouygues `20820`), measured a day apart on
/// the test box: attached in UMTS it came up IPv6-only — an IPv6 address, an IPv6
/// default route and no IPv4 anything, identically for every APN tried including
/// a bogus one; attached in LTE/5G-NR it came up dual-stack, IPv4 arriving as the
/// `192.0.0.2/27` CLAT address of 464XLAT over an IPv6 PDN. Reading
/// `/proc/net/route` alone, a box that had failed over in the first of those
/// showed `end0` as its only default route, so the cockpit rendered
/// `active_uplink=wan` while every packet was leaving through the modem — and the
/// supervisor's `classify_dev`, fixed in the same change, would have disagreed
/// with it on the one screen where the two are read side by side.
///
/// The gateway stays IPv4-only on purpose: it is rendered as `wan.gateway`
/// beside `wan.ip`, which `interface_ipv4` also answers in v4. A v6 winner
/// therefore contributes its *device* and no gateway, which is why the second
/// half of the pair is now an `Option`.
fn default_route() -> Option<(String, Option<String>)> {
    // `unwrap_or_default`, not `ok()?`: an absent table is an empty table, and
    // the other family may still have the answer.
    let v4 = std::fs::read_to_string("/proc/net/route").unwrap_or_default();
    let v6 = std::fs::read_to_string("/proc/net/ipv6_route").unwrap_or_default();
    best_default_route(&v4, &v6)
}

/// Pure half of the two `/proc` reads — the lower-metric winner of the two.
///
/// Metrics are compared across families, which is meaningful because both ends
/// are kept in step on this platform: the applier writes `ipv4.route-metric` and
/// `ipv6.route-metric` to the same value, and networkd gives `end0` the same
/// metric in both tables (measured 100/100). A tie goes to IPv4, because that
/// answer carries the gateway the cockpit renders.
fn best_default_route(v4_table: &str, v6_table: &str) -> Option<(String, Option<String>)> {
    let v4 = parse_default_route(v4_table).map(|(metric, dev, gateway)| (metric, dev, Some(gateway)));
    let v6 = parse_default_route6(v6_table).map(|(metric, dev)| (metric, dev, None));
    let best = match (v4, v6) {
        (Some(from_v4), Some(from_v6)) => Some(if from_v6.0 < from_v4.0 { from_v6 } else { from_v4 }),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    };
    best.map(|(_metric, dev, gateway)| (dev, gateway))
}

/// Pure half of the `/proc/net/route` read — `(metric, iface, gateway)` of the
/// lowest-metric IPv4 default route.
///
/// Columns are `Iface Destination Gateway Flags RefCnt Use Metric Mask …`, with
/// the addresses as little-endian hex. A default route is destination
/// `00000000` **and** mask `00000000`.
fn parse_default_route(table: &str) -> Option<(u32, String, String)> {
    let mut best: Option<(u32, String, String)> = None;
    for line in table.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let (Some(iface), Some(dest), Some(gateway), Some(metric), Some(mask)) =
            (cols.first(), cols.get(1), cols.get(2), cols.get(6), cols.get(7))
        else {
            continue;
        };
        if *dest != "00000000" || *mask != "00000000" {
            continue;
        }
        let Ok(metric) = metric.parse::<u32>() else {
            continue;
        };
        if best.as_ref().is_none_or(|(seen, _dev, _gw)| metric < *seen) {
            best = Some((metric, (*iface).to_owned(), hex_le_ipv4(gateway).unwrap_or_default()));
        }
    }
    best
}

/// Pure half of the `/proc/net/ipv6_route` read — `(metric, iface)` of the
/// lowest-metric IPv6 default route.
///
/// A DIFFERENT FILE WITH A DIFFERENT SHAPE, and every difference below is a way
/// to get this wrong. Columns are `dest plen src plen next_hop metric refcnt use
/// flags iface`; there is **no header line** (its IPv4 sibling has one, hence the
/// `skip(1)` there and none here); the metric is **hex**, not decimal; and a
/// default route is a 32-zero destination with prefix length `00`.
///
/// THE FLAG TEST IS LOAD-BEARING. Every box carries this row:
///
/// ```text
/// 000…000 00 000…000 00 000…000 ffffffff 00000001 00000000 00200200 lo
/// ```
///
/// — the kernel's unreachable `::/0`. Its metric is `0xffffffff`, so it loses
/// every comparison and looks harmless, right up until it is the *only* default
/// route in the table: then a parser that merely sorts by metric hands back `lo`,
/// which `classify_device` dutifully reports as `other`, and the cockpit claims
/// the box is routing through something it does not recognise while it in fact
/// has no IPv6 uplink at all. So we require `RTF_UP` and reject `RTF_REJECT`
/// rather than trusting the ordering.
fn parse_default_route6(table: &str) -> Option<(u32, String)> {
    const RTF_UP: u32 = 0x0001;
    const RTF_REJECT: u32 = 0x0200;
    const DEFAULT_DEST: &str = "00000000000000000000000000000000";

    let mut best: Option<(u32, String)> = None;
    for line in table.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let (Some(dest), Some(prefix_len), Some(metric_hex), Some(flags_hex), Some(iface)) =
            (cols.first(), cols.get(1), cols.get(5), cols.get(8), cols.get(9))
        else {
            continue;
        };
        if *dest != DEFAULT_DEST || *prefix_len != "00" {
            continue;
        }
        let (Ok(flags), Ok(metric)) = (u32::from_str_radix(flags_hex, 16), u32::from_str_radix(metric_hex, 16)) else {
            continue;
        };
        if flags & RTF_UP == 0 || flags & RTF_REJECT != 0 {
            continue;
        }
        if best.as_ref().is_none_or(|seen| metric < seen.0) {
            best = Some((metric, (*iface).to_owned()));
        }
    }
    best
}

/// Decode the little-endian hex IPv4 `/proc/net/route` uses (`0101A8C0` →
/// `192.168.1.1`).
fn hex_le_ipv4(hex: &str) -> Option<String> {
    let raw = u32::from_str_radix(hex, 16).ok()?;
    let [byte0, byte1, byte2, byte3] = raw.to_le_bytes();
    Some(std::net::Ipv4Addr::new(byte0, byte1, byte2, byte3).to_string())
}

/// Classify the default-route device: `wan`, `wwan`, `other` or `none`.
///
/// **`other` is not decoration.** The old fall-through labelled *anything*
/// it did not recognise `wwan`, so a box routing through `tun0`, `wg0`, `docker0`
/// or the second Wi-Fi radio told the cockpit — mid-failover, on the one screen
/// an admin is watching — that the 5G bearer had taken over. `other` says the
/// true thing: the box has a default route and it is not one of ours.
///
/// The sibling `classify_dev` in `deploy/photonicat/network/cp-uplink-watch`
/// implements the same four values against the same devices. They must agree:
/// this one is what the cockpit renders, that one is what the supervisor
/// publishes into `/run/cp-uplink/state`, and the cockpit shows both.
fn active_uplink(default_dev: Option<&str>) -> Value {
    let Some(dev) = default_dev else {
        return json!("none");
    };
    json!(classify_device(dev))
}

/// Pure device-name classifier. Its counterpart for the supervisor's
/// already-classified word is `uplink::normalise_uplink`.
fn classify_device(dev: &str) -> &'static str {
    if dev == wan_iface() || dev.starts_with("en") || dev.starts_with("eth") {
        return "wan";
    }
    // The QMI net port is `wwu…`; `CP_WWAN_DEV` names the control port, which
    // never carries a route but costs nothing to accept.
    if dev == wwan_device() || dev.starts_with("ww") {
        return "wwan";
    }
    "other"
}

// ── Ethernet WAN ────────────────────────────────────────────────────────────

/// Carrier + address + whether this port currently holds the default route.
///
/// `default_dev`/`gateway` come from the single default-route read in [`probe`];
/// re-reading the tables here made a `GET` parse them twice and let the two
/// halves of one response straddle a failover.
///
/// `has_default_route` now also answers `true` for a port holding only an IPv6
/// default route. That is the honest reading — the port IS the box's way out —
/// and it is the same widening `active_uplink` gets, so the two cannot disagree
/// about the same interface within one response.
fn wan_status(default_dev: Option<&str>, gateway: Option<&str>) -> Value {
    let iface = wan_iface();
    let carrier =
        std::fs::read_to_string(format!("/sys/class/net/{iface}/carrier")).is_ok_and(|state| state.trim() == "1");
    let holds_route = default_dev == Some(iface.as_str());
    json!({
        "carrier": carrier,
        "ip": interface_ipv4(&iface),
        "gateway": if holds_route { gateway.map_or(Value::Null, |addr| json!(addr)) } else { Value::Null },
        "has_default_route": holds_route,
    })
}

/// The first global IPv4 on `iface`, via `ip -4 -o addr show dev <iface>`.
///
/// `CP_IP_BIN`-gated like every other tool. `end0` is networkd's, not
/// `NetworkManager`'s, so `nmcli` cannot answer this one.
fn interface_ipv4(iface: &str) -> Value {
    let Some(ip_bin) = std::env::var_os("CP_IP_BIN") else {
        return Value::Null;
    };
    let args =
        ["-4".to_owned(), "-o".to_owned(), "addr".to_owned(), "show".to_owned(), "dev".to_owned(), iface.to_owned()];
    let Ok(out) = run(&ip_bin, &args) else {
        return Value::Null;
    };
    out.split_whitespace()
        .skip_while(|token| *token != "inet")
        .nth(1)
        .and_then(|cidr| cidr.split('/').next())
        .map_or(Value::Null, |addr| json!(addr))
}

// ── 5G bearer ───────────────────────────────────────────────────────────────

/// Modem facts from `mmcli -J`, plus the bearer's address from `nmcli`.
///
/// `None` when there is no `mmcli` or no modem — a box whose modem was pulled
/// reports `wwan: null` rather than a wall of falsehoods.
fn wwan_status(tools: &Tools) -> Option<Value> {
    let mmcli = std::env::var_os("CP_MMCLI_BIN")?;
    let listed = run(&mmcli, &["-J".to_owned(), "-L".to_owned()]).ok()?;
    let list: Value = serde_json::from_str(&listed).ok()?;
    let path = list.get("modem-list")?.as_array()?.first()?.as_str()?.to_owned();
    let shown = run(&mmcli, &["-J".to_owned(), "-m".to_owned(), path.clone()]).ok()?;
    let modem: Value = serde_json::from_str(&shown).ok()?;
    let generic = modem.get("modem").and_then(|m| m.get("generic"));
    let gpp = modem.get("modem").and_then(|m| m.get("3gpp"));
    let registration = gpp.and_then(|g| g.get("registration-state")).and_then(Value::as_str).unwrap_or("");
    Some(json!({
        "state": generic.and_then(|g| g.get("state")).cloned().unwrap_or(Value::Null),
        "operator": gpp.and_then(|g| g.get("operator-name")).cloned().unwrap_or(Value::Null),
        "tech": generic
            .and_then(|g| g.get("access-technologies"))
            .and_then(Value::as_array)
            .map(|techs| techs.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
            .map_or(Value::Null, |joined| if joined.is_empty() { Value::Null } else { json!(joined) }),
        "signal_dbm": signal_dbm(&mmcli, &path),
        "ip": nmcli_first_address(&tools.nmcli, WWAN_PROFILE),
        "registered": matches!(registration, "home" | "roaming"),
    }))
}

/// Received power in dBm, from `mmcli --signal-get`, best technology first.
///
/// Null unless signal polling has been set up on the modem — deliberately not
/// something an apply turns on behind the admin's back.
///
/// `modem` is the D-Bus path [`wwan_status`] already discovered from `mmcli -L`,
/// not the index `0` this used to hardcode. `ModemManager` increments the
/// index across re-enumerations, so after a modem reset every other bearer field
/// was correct and the signal alone silently read `null`.
fn signal_dbm(mmcli: &OsStr, modem: &str) -> Value {
    let args = ["-J".to_owned(), "-m".to_owned(), modem.to_owned(), "--signal-get".to_owned()];
    let Ok(out) = run(mmcli, &args) else {
        return Value::Null;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&out) else {
        return Value::Null;
    };
    let signal = parsed.get("modem").and_then(|m| m.get("signal"));
    for (tech, field) in [("5g", "rsrp"), ("lte", "rsrp"), ("umts", "rscp"), ("gsm", "rssi")] {
        let reading = signal.and_then(|s| s.get(tech)).and_then(|t| t.get(field)).and_then(Value::as_str);
        if let Some(dbm) = reading.and_then(plausible_dbm) {
            return json!(dbm);
        }
    }
    Value::Null
}

/// Parse an `mmcli` power reading, rejecting the sentinels it uses for "no
/// measurement".
///
/// MEASURED on the test box: with the modem searching, `5g.rsrp` reads
/// `-32768.00` and `5g.snr` reads `-3276.80` while `lte.rsrp` carries the real
/// `-110.00`. Taking the first parseable number would report a signal of
/// −32768 dBm — worse than no reading, because it looks like data. Anything
/// outside the physically plausible band is treated as absent, so the search
/// falls through to the technology that actually has a measurement. `"--"`
/// fails to parse and is rejected on the way in.
fn plausible_dbm(raw: &str) -> Option<i64> {
    // Integer part only: dBm is displayed whole, and going through f64 would
    // need a lossy cast back that the lint config rightly forbids.
    let whole = raw.split_once('.').map_or(raw, |(before, _after)| before);
    let value = whole.trim().parse::<i64>().ok()?;
    (-160..=0).contains(&value).then_some(value)
}

/// The first IPv4 address `NetworkManager` assigned to `profile`, without its
/// prefix length. Null when the profile is not active.
fn nmcli_first_address(nmcli: &OsStr, profile: &str) -> Value {
    let args =
        ["-g".to_owned(), "IP4.ADDRESS".to_owned(), "connection".to_owned(), "show".to_owned(), profile.to_owned()];
    let Ok(out) = run(nmcli, &args) else {
        return Value::Null;
    };
    out.lines()
        .next()
        .and_then(|line| line.split('/').next())
        .filter(|addr| !addr.is_empty())
        .map_or(Value::Null, |addr| json!(addr))
}

// ── Access point ────────────────────────────────────────────────────────────

/// Whether the AP is beaconing, on what channel, under which country, and how
/// many clients are associated.
///
/// `ssid` and `channel` both come from one `iw dev <ap> info` — one call, and
/// **the radio's own answer**, which is what this object claims to be. It used
/// to echo `config.ap.ssid` straight back: labelled live status while
/// actually being the document, so a profile that had not been re-activated
/// since a rename reported the new name off an AP still beaconing the old one.
/// The config value remains the fallback for the honest cases — no `iw`, or the
/// radio down — because the schema says this field is always a string, and "the
/// SSID it is configured to use" beats an empty one.
fn ap_status(tools: &Tools, config: &NetworkConfig) -> Value {
    let running = run(
        &tools.nmcli,
        &[
            "-t".to_owned(),
            "-f".to_owned(),
            "NAME".to_owned(),
            "connection".to_owned(),
            "show".to_owned(),
            "--active".to_owned(),
        ],
    )
    .is_ok_and(|out| out.lines().any(|line| line == AP_PROFILE));
    let info = iw_info(tools);
    json!({
        "running": running,
        "ssid": info.as_deref().and_then(parse_iw_ssid).unwrap_or_else(|| config.ap.ssid.clone()),
        "clients": associated_clients(tools),
        "channel": info.as_deref().and_then(parse_iw_channel).map_or(Value::Null, |channel| json!(channel)),
        "country": iw_country(tools),
    })
}

/// Associated stations, from `iw dev <ap> station dump`. `0` when `iw` is absent
/// — a count is a count, and null would make the UI render "—" for an AP that is
/// demonstrably up.
fn associated_clients(tools: &Tools) -> u32 {
    let Some(iw_bin) = tools.iw.as_ref() else {
        return 0;
    };
    let args = ["dev".to_owned(), ap_device(), "station".to_owned(), "dump".to_owned()];
    run(iw_bin, &args).map_or(0, |out| {
        let count = out.lines().filter(|line| line.starts_with("Station ")).count();
        u32::try_from(count).unwrap_or(u32::MAX)
    })
}

/// `iw dev <ap> info` once per request — it answers both the live SSID and the
/// live channel, and the AP block used to spend two calls on it.
fn iw_info(tools: &Tools) -> Option<String> {
    let iw_bin = tools.iw.as_ref()?;
    run(iw_bin, &["dev".to_owned(), ap_device(), "info".to_owned()]).ok()
}

/// The channel the radio is actually on. Pure half of [`iw_info`].
fn parse_iw_channel(info: &str) -> Option<u32> {
    info.lines()
        .find_map(|line| line.trim().strip_prefix("channel "))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse::<u32>().ok())
}

/// The SSID the radio is actually beaconing. Pure half of [`iw_info`].
///
/// `iw` prints it as `\tssid ContextPilot-f10d`, one line, verbatim — an SSID
/// may contain spaces, so everything after the keyword is the value. A radio
/// that is up but not in AP mode has no `ssid` line at all, which is `None`.
fn parse_iw_ssid(info: &str) -> Option<String> {
    info.lines()
        .find_map(|line| line.trim().strip_prefix("ssid "))
        .map(str::trim)
        .filter(|ssid| !ssid.is_empty())
        .map(str::to_owned)
}

/// The **global** regulatory domain from `iw reg get`.
///
/// Deliberately not "the first `country` line": `iw reg get` prints one block
/// per authority, and on this hardware they legitimately disagree — phy#0
/// (ath11k) adopts the hint while phy#1 (aic8800) stays at `00` forever
/// (measured). Reading the first line would report a domain we never set.
fn iw_country(tools: &Tools) -> Value {
    let Some(iw_bin) = tools.iw.as_ref() else {
        return Value::Null;
    };
    let Ok(out) = run(iw_bin, &["reg".to_owned(), "get".to_owned()]) else {
        return Value::Null;
    };
    parse_global_country(&out).map_or(Value::Null, |country| json!(country))
}

/// Pure half of [`iw_country`].
fn parse_global_country(output: &str) -> Option<String> {
    let mut in_global = false;
    for line in output.lines() {
        if line.trim() == "global" {
            in_global = true;
            continue;
        }
        if in_global && let Some(rest) = line.strip_prefix("country ") {
            return rest.split(':').next().map(str::to_owned);
        }
    }
    None
}

#[cfg(test)]
mod tests;
