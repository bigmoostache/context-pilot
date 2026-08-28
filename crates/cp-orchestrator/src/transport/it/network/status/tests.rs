use super::*;

/// Real `/proc/net/route` shape, with two default routes at different
/// metrics — the exact situation `wan_5g` creates.
const ROUTE_TABLE: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
end0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0
end0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
wwu1u1i4\t00000000\t0100A80A\t0003\t0\t0\t700\t00000000\t0\t0\t0
";

/// `/proc/net/ipv6_route` captured from the test box, with the bearer merely
/// STANDING BY at 0x2bc = 700 and `end0` holding the default at 0x64 = 100.
///
/// Verbatim, not approximated: this table has **no header line**, its metric
/// is **hex**, and its last row is the kernel's unreachable `::/0` on `lo` —
/// three separate ways to write a parser that looks right and is not. The
/// on-link rows are kept so the prefix-length filter has something to reject.
const ROUTE_TABLE6: &str = "\
2a01cb088f5d22000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000064 00000002 00000000 08400001 end0
fe800000000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000009 00000000 00000001 end0
00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000001adf26fffea05940 00000064 00000009 00000000 08400003 end0
00000000000000000000000000000000 00 00000000000000000000000000000000 00 2a04cec01052309ecd4134fc1c7f07cf 000002bc 00000007 00000000 00000003 wwu1u1i4
00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo
";

/// The IPv4 half of a box whose bearer is IPv6-only: `end0` is the ONLY
/// default route there, whatever the modem is doing.
const ROUTE_TABLE_WAN_ONLY: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
end0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0
end0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
";

#[test]
fn the_lowest_metric_default_route_wins() {
    let (metric, dev, gateway) = parse_default_route(ROUTE_TABLE).expect("a default route");
    assert_eq!(metric, 100);
    assert_eq!(dev, "end0", "metric 100 beats metric 700");
    assert_eq!(gateway, "192.168.1.1", "little-endian hex decodes to the LAN gateway");
    assert_eq!(active_uplink(Some(&dev)), json!("wan"));
}

#[test]
fn a_promoted_bearer_becomes_the_active_uplink() {
    // What the supervisor produces at failover: the bearer drops to 50.
    let failed_over = ROUTE_TABLE.replace("\t700\t00000000", "\t50\t00000000");
    let (_metric, dev, gateway) = parse_default_route(&failed_over).expect("a default route");
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

// ── The IPv6 table, and the failover it used to hide ─────────────────────

#[test]
fn the_ipv6_default_route_is_read_with_its_hex_metric() {
    let (metric, dev) = parse_default_route6(ROUTE_TABLE6).expect("a v6 default route");
    assert_eq!(metric, 100, "0x64, not the decimal 64 a copied v4 parser would produce");
    assert_eq!(dev, "end0", "0x64 = 100 beats the bearer's 0x2bc = 700");
}

#[test]
fn the_unreachable_ipv6_default_on_lo_is_not_an_uplink() {
    // Present on every box. It loses on metric whenever a real route exists,
    // so it stays invisible until it is the only row left — and then a
    // metric-only parser hands back `lo`, which `classify_device` renders as
    // `other`: the cockpit claiming the box routes through something it does
    // not recognise, when it simply has no IPv6 uplink at all.
    let only_the_trap = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo\n";
    assert_eq!(parse_default_route6(only_the_trap), None, "RTF_REJECT is not an uplink");
    assert_eq!(best_default_route("", only_the_trap), None);
}

#[test]
fn an_ipv6_only_bearer_holding_the_default_route_is_seen() {
    // THE REGRESSION THIS WHOLE CHANGE EXISTS FOR. Reading `/proc/net/route`
    // alone, this box answered `end0`: there is no IPv4 default via the
    // bearer and there never will be, because the operator hands out an
    // IPv6-only PDN (measured on Bouygues, every APN tried). So the cockpit
    // reported `active_uplink=wan` throughout a failover that had entirely
    // succeeded — and `cp-uplink-watch`, asking the same question of the
    // same kernel at startup, concluded it had never promoted and could
    // therefore never demote back to a healthy WAN.
    let promoted = ROUTE_TABLE6.replace(" 000002bc ", " 00000032 ");
    let (dev, gateway) = best_default_route(ROUTE_TABLE_WAN_ONLY, &promoted).expect("a default route");
    assert_eq!(dev, "wwu1u1i4", "metric 50 over IPv6 beats metric 100 over IPv4");
    assert_eq!(gateway, None, "an IPv6 winner contributes no IPv4 gateway to render");
    assert_eq!(active_uplink(Some(&dev)), json!("wwan"));
}

#[test]
fn a_standing_by_bearer_does_not_steal_the_default_route() {
    // The other half of the widening, and the one that would turn the fix
    // into a fresh bug: at the standby metric the bearer must lose in BOTH
    // tables, or every healthy box reports a failover that never happened.
    let (dev, gateway) = best_default_route(ROUTE_TABLE_WAN_ONLY, ROUTE_TABLE6).expect("a default route");
    assert_eq!(dev, "end0", "metric 100 beats the 700 the applier parks the bearer at");
    assert_eq!(gateway.as_deref(), Some("192.168.1.1"), "the IPv4 winner still carries its gateway");
    assert_eq!(active_uplink(Some(&dev)), json!("wan"));
}

#[test]
fn a_missing_table_does_not_cost_the_answer_the_other_holds() {
    // `default_route` reads two files and neither is guaranteed to exist —
    // an IPv6-disabled kernel has no `ipv6_route` at all.
    assert_eq!(best_default_route(ROUTE_TABLE_WAN_ONLY, "").map(|(dev, _gw)| dev), Some("end0".to_owned()));
    assert_eq!(best_default_route("", ROUTE_TABLE6).map(|(dev, _gw)| dev), Some("end0".to_owned()));
    assert_eq!(best_default_route("", ""), None);
}

#[test]
fn the_global_regulatory_domain_is_read_not_the_first_phy() {
    // Measured shape: the global block and the two self-managed phys
    // disagree, and phy#1 lists FIRST. Reading the first `country` line
    // would report a domain nobody set.
    let output = "global\ncountry FR: DFS-ETSI\n\t(2400 - 2483 @ 40)\n\nphy#1 (self-managed)\ncountry 00: DFS-UNSET\n";
    assert_eq!(parse_global_country(output).as_deref(), Some("FR"));
    let unset = "global\ncountry 00: DFS-UNSET\n\nphy#0 (self-managed)\ncountry FR: DFS-ETSI\n";
    assert_eq!(parse_global_country(unset).as_deref(), Some("00"), "the phy's FR must not be mistaken for global");
}

#[test]
fn implausible_signal_readings_are_rejected() {
    // MEASURED with the modem searching: the 5g block reads -32768.00 while
    // lte carries the real -110.00. Taking the first parseable number would
    // report −32768 dBm, which is worse than null because it looks like data.
    assert_eq!(plausible_dbm("-110.00"), Some(-110));
    assert_eq!(plausible_dbm("-80"), Some(-80));
    assert_eq!(plausible_dbm("-32768.00"), None, "mmcli's no-measurement sentinel");
    assert_eq!(plausible_dbm("-3276.80"), None, "the SNR sentinel, if ever misread as power");
    assert_eq!(plausible_dbm("--"), None, "mmcli's absent-field spelling");
    assert_eq!(plausible_dbm("42.0"), None, "a positive dBm is not a received power");
}

#[test]
fn status_is_well_formed_without_gates() {
    // Plain-call helpers keep the loop + null checks off the cognitive cap.
    fn assert_present(status: &Value, field: &str) {
        assert!(status.get(field).is_some(), "{field} is present");
    }
    fn assert_null(status: &Value, field: &str, why: &str) {
        assert!(status.get(field).is_some_and(Value::is_null), "{field}: {why}");
    }
    // The off-box half: a dev machine answers 200 with a well-formed
    // object whose optional halves are null, not an error.
    let status = probe(&NetworkConfig::default(), true);
    for field in ["active_uplink", "wan", "wwan", "ap", "supervisor"] {
        assert_present(&status, field);
    }
    assert_null(&status, "wwan", "no mmcli gate \u{21d2} null bearer");
    assert_null(&status, "ap", "no nmcli gate \u{21d2} null AP");
    assert_null(&status, "supervisor", "no state file \u{21d2} null supervisor");
    // The hardware fact is the one the caller threaded in, not a
    // second sysfs read that could disagree with the handler's own gate.
    assert_eq!(status.get("modem_present"), Some(&json!(true)));
    assert_eq!(probe(&NetworkConfig::default(), false).get("modem_present"), Some(&json!(false)));
}

/// The fall-through used to call **everything** it did not recognise
/// `wwan`, so a VPN, a container bridge or the second Wi-Fi radio holding
/// the default route told the cockpit the 5G bearer had taken over. That is
/// the one screen an admin watches during a failover.
#[test]
fn an_unrecognised_default_route_device_is_other_not_wwan() {
    // A plain call keeps the nine cases off the cognitive cap.
    fn assert_uplink(dev: Option<&str>, want: &str) {
        assert_eq!(active_uplink(dev), json!(want), "{dev:?} \u{2192} {want}");
    }
    assert_uplink(Some("end0"), "wan"); // the configured WAN port
    assert_uplink(Some("eth0"), "wan");
    assert_uplink(Some("enp1s0"), "wan");
    assert_uplink(Some("wwu1u1i4"), "wwan"); // the QMI net port
    for stranger in ["tun0", "wg0", "docker0", "wlp1s0", "br-lan"] {
        assert_uplink(Some(stranger), "other");
    }
    assert_uplink(None, "none");
}

/// `ssid` and `channel` come off the radio, from one `iw dev … info`.
#[test]
fn the_live_ssid_and_channel_are_read_from_the_radio() {
    let info =
        "Interface wlp1s0\n\tifindex 4\n\ttype AP\n\tssid ContextPilot f10d\n\tchannel 36 (5180 MHz), width: 80 MHz\n";
    assert_eq!(parse_iw_ssid(info).as_deref(), Some("ContextPilot f10d"), "an SSID may contain spaces");
    assert_eq!(parse_iw_channel(info), Some(36));
    // A radio that is up but not beaconing has neither line — the caller
    // falls back to the configured SSID rather than inventing one.
    let managed = "Interface wlan0\n\tifindex 3\n\ttype managed\n";
    assert_eq!(parse_iw_ssid(managed), None);
    assert_eq!(parse_iw_channel(managed), None);
}
