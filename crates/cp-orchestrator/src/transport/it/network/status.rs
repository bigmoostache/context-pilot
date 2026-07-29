//! Live uplink status — what the cockpit polls every 5 s (FR-NET-10/11).
//!
//! Read from the system, never from what we believe we configured: `/proc/net`,
//! `ip`, `nmcli -t`, `mmcli -J` and `iw`. A human who ran `nmcli` by hand on the
//! box (landmine 9) sees their change reflected here until the next apply
//! reverts it — an honest read-out is what makes the box debuggable.
//!
//! **Every field degrades to `null` rather than erroring when a tool is absent.**
//! A dev machine with no gates set answers `200` with a fully-null status; a box
//! whose modem was pulled reports `wwan: null` and keeps serving the cockpit.
//!
//! The default-route half needs no gate at all: it is parsed from
//! `/proc/net/route`, so `active_uplink` and `has_default_route` — the two
//! fields an admin actually watches during a failover — stay truthful even on a
//! box where every optional tool is missing.

use std::ffi::OsStr;

use serde_json::{Value, json};

use super::apply::{AP_PROFILE, Tools, WWAN_PROFILE, ap_device, run, wan_iface};
use super::state::NetworkConfig;

/// Build the `status` half of `GET /api/it/network`.
///
/// `config` is passed in — not re-read — so the config and the status in the
/// same response describe the same instant.
pub(crate) fn probe(config: &NetworkConfig) -> Value {
    let default_dev = default_route_device();
    let tools = Tools::resolve();
    json!({
        "active_uplink": active_uplink(default_dev.as_deref()),
        "wan": wan_status(default_dev.as_deref()),
        "wwan": tools.as_ref().and_then(wwan_status),
        "ap": tools.as_ref().map(|tools| ap_status(tools, config)),
    })
}

// ── Default route, straight from /proc (no gate) ────────────────────────────

/// The interface carrying the lowest-metric IPv4 default route.
///
/// `/proc/net/route` columns are `Iface Destination Gateway Flags RefCnt Use
/// Metric Mask …`, with the addresses as little-endian hex. A default route is
/// destination `00000000` **and** mask `00000000`.
fn default_route_device() -> Option<String> {
    let table = std::fs::read_to_string("/proc/net/route").ok()?;
    parse_default_route(&table).map(|(dev, _gw)| dev)
}

/// Pure half of the `/proc/net/route` read — `(iface, gateway)` of the
/// lowest-metric default route.
fn parse_default_route(table: &str) -> Option<(String, String)> {
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
    best.map(|(_metric, dev, gateway)| (dev, gateway))
}

/// Decode the little-endian hex IPv4 `/proc/net/route` uses (`0101A8C0` →
/// `192.168.1.1`).
fn hex_le_ipv4(hex: &str) -> Option<String> {
    let raw = u32::from_str_radix(hex, 16).ok()?;
    let [byte0, byte1, byte2, byte3] = raw.to_le_bytes();
    Some(std::net::Ipv4Addr::new(byte0, byte1, byte2, byte3).to_string())
}

/// Classify the default-route device as the ethernet WAN, the bearer, or none.
fn active_uplink(default_dev: Option<&str>) -> Value {
    match default_dev {
        None => json!("none"),
        Some(dev) if dev == wan_iface() => json!("wan"),
        Some(dev) if dev.starts_with("en") || dev.starts_with("eth") => json!("wan"),
        Some(_other) => json!("wwan"),
    }
}

// ── Ethernet WAN ────────────────────────────────────────────────────────────

/// Carrier + address + whether this port currently holds the default route.
fn wan_status(default_dev: Option<&str>) -> Value {
    let iface = wan_iface();
    let carrier =
        std::fs::read_to_string(format!("/sys/class/net/{iface}/carrier")).is_ok_and(|state| state.trim() == "1");
    let gateway = std::fs::read_to_string("/proc/net/route")
        .ok()
        .and_then(|table| parse_default_route(&table))
        .filter(|(dev, _gw)| *dev == iface)
        .map(|(_dev, gateway)| gateway);
    json!({
        "carrier": carrier,
        "ip": interface_ipv4(&iface),
        "gateway": gateway,
        "has_default_route": default_dev == Some(iface.as_str()),
    })
}

/// The first global IPv4 on `iface`, via `ip -4 -o addr show dev <iface>`.
///
/// `CP_IP_BIN`-gated like every other tool. `end0` is networkd's, not
/// NetworkManager's, so `nmcli` cannot answer this one.
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
    let shown = run(&mmcli, &["-J".to_owned(), "-m".to_owned(), path]).ok()?;
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
        "signal_dbm": signal_dbm(&mmcli),
        "ip": nmcli_first_address(&tools.nmcli, WWAN_PROFILE),
        "registered": matches!(registration, "home" | "roaming"),
    }))
}

/// Received power in dBm, from `mmcli --signal-get`. Null unless signal polling
/// has been set up on the modem, which is deliberately not something an apply
/// turns on behind the admin's back.
fn signal_dbm(mmcli: &OsStr) -> Value {
    let Ok(out) = run(mmcli, &["-J".to_owned(), "-m".to_owned(), "0".to_owned(), "--signal-get".to_owned()]) else {
        return Value::Null;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&out) else {
        return Value::Null;
    };
    let signal = parsed.get("modem").and_then(|m| m.get("signal"));
    for (tech, field) in [("5g", "rsrp"), ("lte", "rsrp"), ("umts", "rscp"), ("gsm", "rssi")] {
        let reading = signal.and_then(|s| s.get(tech)).and_then(|t| t.get(field)).and_then(Value::as_str);
        if let Some(value) = reading.and_then(|raw| raw.parse::<f64>().ok()) {
            return json!(value);
        }
    }
    Value::Null
}

/// The first IPv4 address NetworkManager assigned to `profile`, without its
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
/// many clients are associated (FR-NET-11).
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
    json!({
        "running": running,
        "ssid": config.ap.ssid,
        "clients": associated_clients(tools),
        "channel": iw_channel(tools),
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

/// The channel the radio is actually on, from `iw dev <ap> info`.
fn iw_channel(tools: &Tools) -> Value {
    let Some(iw_bin) = tools.iw.as_ref() else {
        return Value::Null;
    };
    let args = ["dev".to_owned(), ap_device(), "info".to_owned()];
    let Ok(out) = run(iw_bin, &args) else {
        return Value::Null;
    };
    out.lines()
        .find_map(|line| line.trim().strip_prefix("channel "))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse::<u32>().ok())
        .map_or(Value::Null, |channel| json!(channel))
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
        if in_global {
            if let Some(rest) = line.strip_prefix("country ") {
                return rest.split(':').next().map(str::to_owned);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real `/proc/net/route` shape, with two default routes at different
    /// metrics — the exact situation `wan_5g` creates.
    const ROUTE_TABLE: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
end0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0
end0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
wwu1u1i4\t00000000\t0100A80A\t0003\t0\t0\t700\t00000000\t0\t0\t0
";

    #[test]
    fn the_lowest_metric_default_route_wins() {
        let (dev, gateway) = parse_default_route(ROUTE_TABLE).expect("a default route");
        assert_eq!(dev, "end0", "metric 100 beats metric 700");
        assert_eq!(gateway, "192.168.1.1", "little-endian hex decodes to the LAN gateway");
        assert_eq!(active_uplink(Some(&dev)), json!("wan"));
    }

    #[test]
    fn a_promoted_bearer_becomes_the_active_uplink() {
        // What the supervisor produces at failover: the bearer drops to 50.
        let failed_over = ROUTE_TABLE.replace("\t700\t00000000", "\t50\t00000000");
        let (dev, gateway) = parse_default_route(&failed_over).expect("a default route");
        assert_eq!(dev, "wwu1u1i4", "metric 50 beats end0's 100");
        assert_eq!(gateway, "10.168.0.1");
        assert_eq!(active_uplink(Some(&dev)), json!("wwan"));
    }

    #[test]
    fn no_default_route_is_none_not_an_error() {
        let only_link_local = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
end0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_default_route(only_link_local), None);
        assert_eq!(active_uplink(None), json!("none"));
    }

    #[test]
    fn the_global_regulatory_domain_is_read_not_the_first_phy() {
        // Measured shape: the global block and the two self-managed phys
        // disagree, and phy#1 lists FIRST. Reading the first `country` line
        // would report a domain nobody set.
        let output =
            "global\ncountry FR: DFS-ETSI\n\t(2400 - 2483 @ 40)\n\nphy#1 (self-managed)\ncountry 00: DFS-UNSET\n";
        assert_eq!(parse_global_country(output).as_deref(), Some("FR"));
        let unset = "global\ncountry 00: DFS-UNSET\n\nphy#0 (self-managed)\ncountry FR: DFS-ETSI\n";
        assert_eq!(parse_global_country(unset).as_deref(), Some("00"), "the phy's FR must not be mistaken for global");
    }

    #[test]
    fn status_is_well_formed_without_gates() {
        // O3.5's off-box half: a dev machine answers 200 with a well-formed
        // object whose optional halves are null, not an error.
        let status = probe(&NetworkConfig::default());
        for field in ["active_uplink", "wan", "wwan", "ap"] {
            assert!(status.get(field).is_some(), "{field} is present");
        }
        assert!(status.get("wwan").is_some_and(Value::is_null), "no mmcli gate ⇒ null bearer");
        assert!(status.get("ap").is_some_and(Value::is_null), "no nmcli gate ⇒ null AP");
    }
}
