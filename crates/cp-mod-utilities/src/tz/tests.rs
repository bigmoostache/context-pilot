use super::*;

/// 2026-03-29T01:00:00Z: Paris springs forward.
const PARIS_SPRING: i64 = 1_774_746_000;
/// 2026-10-25T01:00:00Z: Paris falls back.
const PARIS_FALL: i64 = 1_792_890_000;
/// Europe/Paris as a POSIX rule (the `TZif` footer of the real zone).
const PARIS: &str = "CET-1CEST,M3.5.0,M10.5.0/3";

fn paris() -> Zone {
    Zone::from_posix(PARIS).unwrap_or_default()
}

#[test]
fn offset_follows_dst_transitions() {
    let zone = paris();
    let cases: [(i64, i32, &str); 4] = [
        (PARIS_SPRING - 1, 3_600, "CET just before spring-forward"),
        (PARIS_SPRING, 7_200, "CEST at spring-forward"),
        (PARIS_FALL - 1, 7_200, "CEST just before fall-back"),
        (PARIS_FALL, 3_600, "CET at fall-back"),
    ];
    for (at, expected, what) in cases {
        assert_eq!(zone.offset_at(at), expected, "{what}");
    }
}

#[test]
fn southern_hemisphere_rule_wraps_the_year() {
    let zone = Zone::from_posix("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap_or_default();
    // 2026-01-15 and 2026-07-15, noon UTC.
    assert_eq!(zone.offset_at(20_468 * 86_400 + 43_200), 39_600i32, "January = AEDT");
    assert_eq!(zone.offset_at(20_649 * 86_400 + 43_200), 36_000i32, "July = AEST");
}

#[test]
fn fixed_and_negative_offsets() {
    let fixed = Zone::from_posix("<-03>3").unwrap_or_default();
    assert_eq!(fixed.offset_at(PARIS_SPRING), -10_800i32);
    let est = Zone::from_posix("EST5EDT").unwrap_or_default();
    // Default US rule: 2026-07-01 is daylight time.
    assert_eq!(est.offset_at(20_635 * 86_400), -14_400i32);
    assert!(Zone::from_posix("X1").is_none(), "abbreviation shorter than 3 letters");
    assert!(Zone::from_posix("CET-1CEST,M13.5.0,M10.5.0").is_none(), "month 13");
}

#[test]
fn local_to_utc_handles_normal_and_gap_times() {
    let zone = paris();
    // 2026-07-01 12:00 local (CEST) = 10:00Z.
    let noon = 20_635 * 86_400 + 43_200;
    assert_eq!(zone.local_to_utc(noon), noon - 7200);
    // 2026-03-29 02:30 does not exist; it lands one hour later (01:30Z).
    let gap = 20_541 * 86_400 + 9000;
    assert_eq!(zone.local_to_utc(gap), 20_541 * 86_400 + 5400);
}

#[test]
fn numeric_offsets() {
    let cases: [(&str, Option<i32>); 5] = [
        ("+0200", Some(7_200i32)),
        ("-0530", Some(-19_800i32)),
        ("+0000", Some(0i32)),
        ("+02:00", None),
        ("+2500", None),
    ];
    for (input, expected) in cases {
        assert_eq!(parse_numeric_offset(input), expected, "{input}");
    }
}

#[test]
fn host_tzif_parses_when_present() {
    let Ok(data) = std::fs::read("/etc/localtime") else { return };
    let zone = tzif::parse(&data);
    assert!(zone.is_some(), "/etc/localtime should parse as TZif");
    let offset = zone.unwrap_or_default().offset_at(PARIS_SPRING);
    assert!((-50_400i32..=50_400i32).contains(&offset), "{offset}");
}

#[test]
fn tz_var_forms() {
    assert_eq!(from_tz_var("").map(|zone| zone.offset_at(0)), Some(0i32));
    assert_eq!(from_tz_var(PARIS).map(|zone| zone.offset_at(PARIS_SPRING)), Some(7_200i32));
    assert!(from_tz_var("../../etc/passwd").is_none(), "no path traversal through names");
}
