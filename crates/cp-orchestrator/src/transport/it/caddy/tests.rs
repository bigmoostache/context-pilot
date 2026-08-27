use super::*;

fn subjects() -> Vec<String> {
    vec!["192.168.1.116".to_owned(), "pilot.acme.corp".to_owned()]
}

#[test]
fn unprovisioned_render_serves_cockpit_over_cleartext_80() {
    let cfg = render_caddyfile(false, &subjects());
    // Day-0: the cockpit is served on :80 cleartext, proxying the product
    // upstream — no cert, no :443, no redirect.
    assert!(cfg.contains(PRODUCT_UPSTREAM), "cockpit is served on day-0");
    assert!(!cfg.contains("https://"), "no https site until provisioned");
    assert!(!cfg.contains("redir https"), "no :80\u{2192}:443 redirect when unprovisioned");
    // The removed maintenance plane leaves no trace.
    assert!(!cfg.contains("9090"), "no maintenance plane upstream/port");
}

#[test]
fn unprovisioned_render_matches_any_host_so_the_ula_gets_in() {
    // The whole point of day-0: the operator's FIRST connection arrives on the
    // fleet ULA, because the DHCP IPv4 is not known to anyone yet. A host-scoped
    // site address would answer that request with an empty 200 (no site matched),
    // so the day-0 block must be port-only — subjects are deliberately ignored.
    let cfg = render_caddyfile(false, &subjects());
    assert!(cfg.contains("\n:80 {"), "day-0 uses a port-only :80 site address");
    assert!(!cfg.contains("192.168.1.116"), "day-0 does not scope to the detected IPv4");
    assert!(!cfg.contains("pilot.acme.corp"), "day-0 does not scope to the name either");
}

#[test]
fn provisioned_render_serves_cockpit_on_443_and_redirects_80() {
    let cfg = render_caddyfile(true, &subjects());
    assert!(cfg.contains(PRODUCT_UPSTREAM), "cockpit upstream present once provisioned");
    assert!(cfg.contains("tls internal"), "cockpit uses the private CA once provisioned");
    assert!(cfg.contains("https://pilot.acme.corp"), "name is a cert subject");
    assert!(cfg.contains("https://192.168.1.116"), "ip is a cert subject");
    assert!(cfg.contains("redir https://{host}{uri} 308"), ":80 \u{2192} :443 redirect present");
    // The cockpit is no longer reverse-proxied on :80 once provisioned — only
    // the redirect block references :80.
    assert!(!cfg.contains("9090"), "no maintenance plane upstream/port");
}

#[test]
fn empty_subjects_fall_back_to_cleartext_80_cockpit() {
    // No subjects → treated as day-0: cockpit on a port-only :80 site.
    let cfg = render_caddyfile(true, &[]);
    assert!(cfg.contains(":80"), "cockpit falls back to a port-only :80 site");
    assert!(cfg.contains(PRODUCT_UPSTREAM), "cockpit is still served");
    assert!(!cfg.contains("https://"), "no https site without subjects for a trustworthy leaf");
}

#[test]
fn subjects_for_orders_ip_then_name() {
    // No ULAs passed in: the client's identity alone, in order. (Uses the pure
    // half so the result does not depend on the test machine's addresses.)
    let id = Identity { name: "pilot.acme.corp".to_owned(), ip: "10.0.0.9".to_owned() };
    assert_eq!(subjects_with(Some(&id), None, &[]), vec!["10.0.0.9".to_owned(), "pilot.acme.corp".to_owned()]);
    // A blank name is dropped.
    let id2 = Identity { name: "  ".to_owned(), ip: "10.0.0.9".to_owned() };
    assert_eq!(subjects_with(Some(&id2), None, &[]), vec!["10.0.0.9".to_owned()]);
}

#[test]
fn ulas_are_appended_after_the_client_identity() {
    // The client's IP/name come first — they are the identity the client's staff
    // and DNS use. Our ULAs ride along so the cockpit is also reachable on the
    // one address that never changes.
    let id = Identity { name: "pilot.acme.corp".to_owned(), ip: "10.0.0.9".to_owned() };
    let port1 = "fd59:ec78:2da4:1:7681:f2a2:27e0:f10d".to_owned();
    let port2 = "fd59:ec78:2da4:2:7681:f2a2:27e0:f10d".to_owned();
    let ulas = vec![port1.clone(), port2.clone()];
    assert_eq!(
        subjects_with(Some(&id), None, &ulas),
        vec!["10.0.0.9".to_owned(), "pilot.acme.corp".to_owned(), port1.clone(), port2]
    );
    // An operator who typed the ULA into the IP field must not get it twice.
    let id_ula = Identity { name: String::new(), ip: port1.clone() };
    assert_eq!(subjects_with(Some(&id_ula), None, std::slice::from_ref(&port1)), vec![port1]);
}

#[test]
fn a_drifted_dhcp_lease_is_added_without_dropping_the_pinned_one() {
    // THE REGRESSION THIS EXISTS FOR. `Identity.ip` is pinned when the operator
    // names the box and never revisited, but it is a lease: the test box
    // rebooted from .38 onto .43 and the render faithfully reproduced .38, so
    // the cockpit had no site — and no cert — for the only address it had.
    let id = Identity { name: "pilot.acme.corp".to_owned(), ip: "192.168.1.38".to_owned() };
    assert_eq!(
        subjects_with(Some(&id), Some("192.168.1.43"), &[]),
        vec!["192.168.1.38".to_owned(), "pilot.acme.corp".to_owned(), "192.168.1.43".to_owned()],
        "the pinned address is kept \u{2014} the client's DNS may point at a reservation that comes back"
    );
}

#[test]
fn an_undrifted_lease_renders_byte_identically() {
    // The other half: on every boot where the lease has NOT moved — which is
    // nearly all of them — the subject list must be exactly what it was, or
    // each boot rewrites the Caddyfile and re-issues the leaf for nothing.
    let id = Identity { name: "pilot.acme.corp".to_owned(), ip: "192.168.1.38".to_owned() };
    let ula = "fd59:ec78:2da4:1:7681:f2a2:27e0:f10d".to_owned();
    assert_eq!(
        subjects_with(Some(&id), Some("192.168.1.38"), std::slice::from_ref(&ula)),
        vec!["192.168.1.38".to_owned(), "pilot.acme.corp".to_owned(), ula],
        "no duplicate, no reordering"
    );
}

#[test]
fn a_drifted_lease_reaches_the_rendered_sites() {
    // End to end: the new address must land in BOTH blocks, because the :80
    // block is what redirects a browser to the https one. Missing there, a
    // request to the box's own address gets Caddy's empty 200 instead.
    let id = Identity { name: "pilot.acme.corp".to_owned(), ip: "192.168.1.38".to_owned() };
    let cfg = render_caddyfile(true, &subjects_with(Some(&id), Some("192.168.1.43"), &[]));
    assert!(cfg.contains("https://192.168.1.43"), "the current lease gets a TLS site + cert subject");
    assert!(cfg.contains("http://192.168.1.43:80"), "and the :80 redirect that sends a browser there");
    assert!(cfg.contains("https://192.168.1.38"), "the pinned one is not evicted");
}

#[test]
fn no_identity_and_no_detectable_ip_stays_empty_even_with_ulas() {
    // Defence-in-depth: a removed identity file must fall back to the cleartext
    // :80 cockpit, NOT to a :443 that only we could reach. So a ULA never turns
    // an empty subject list into a non-empty one.
    assert!(subjects_with(None, None, &["fd00::1".to_owned()]).is_empty());
}

#[test]
fn parse_ulas_keeps_global_fd00_only() {
    // Real `/proc/net/if_inet6` shape: 32 hex chars, ifindex, prefixlen, scope,
    // flags, device. Scope 00 = global, 20 = link-local.
    let text = "\
fd59ec782da400017681f2a227e0f10d 02 40 00 80     end0
fd59ec782da400027681f2a227e0f10d 03 40 00 80     end1
fe80000000000000402f19fffe8ff932 02 40 20 80     end0
2a01cb088f5d2200402f19fffe8ff932 02 40 00 80     end0
00000000000000000000000000000001 01 80 10 80       lo
fd00000000000000000000000000abcd 01 80 00 80       lo
";
    assert_eq!(
        parse_ulas(text),
        vec!["fd59:ec78:2da4:1:7681:f2a2:27e0:f10d".to_owned(), "fd59:ec78:2da4:2:7681:f2a2:27e0:f10d".to_owned()],
        "global fd00::/8 on real devices only \u{2014} link-local, GUA and loopback dropped"
    );
}

#[test]
fn parse_ulas_is_canonical_sorted_and_deduped() {
    // Emitted form must match what `ip -6 addr` prints back (RFC 5952), or the
    // Caddyfile would churn against the kernel's spelling of the same address.
    let text = "\
fd0000000000000000000000000000ff 03 40 00 80     end1
fd00000000000000000000000000000a 02 40 00 80     end0
fd00000000000000000000000000000a 04 40 00 80     end2
";
    assert_eq!(parse_ulas(text), vec!["fd00::a".to_owned(), "fd00::ff".to_owned()]);
}

#[test]
fn parse_ulas_ignores_malformed_lines() {
    let text = "not a line\nfd59ec78 02 40 00 80 end0\n\nfd59ec782da400017681f2a227e0f10d 02 40 00 80 end0\n";
    assert_eq!(parse_ulas(text), vec!["fd59:ec78:2da4:1:7681:f2a2:27e0:f10d".to_owned()]);
}

#[test]
fn every_block_binds_all_interfaces() {
    // `bind 0.0.0.0 ::` on every block avoids the IP-vs-hostname conflicting
    // listener problem and survives a DHCP IP change.
    let provisioned = render_caddyfile(true, &subjects());
    assert_eq!(provisioned.matches("bind 0.0.0.0 ::").count(), 2, "cockpit + redirect bind all interfaces");
    // Day-0 renders a single cleartext cockpit block.
    let day0 = render_caddyfile(false, &subjects());
    assert_eq!(day0.matches("bind 0.0.0.0 ::").count(), 1, "the day-0 :80 cockpit binds all interfaces");
}

#[test]
fn ipv6_subjects_are_bracketed() {
    let cfg = render_caddyfile(true, &["fd00::1".to_owned()]);
    assert!(cfg.contains("https://[fd00::1]"), "ipv6 cockpit address is bracketed");
    assert!(cfg.contains("http://[fd00::1]:80"), "ipv6 redirect address is bracketed");
    // IPv4 is never bracketed. Rendered PROVISIONED on purpose: the day-0 block
    // is port-only now, so it would pass this trivially and prove nothing.
    let v4 = render_caddyfile(true, &["10.0.0.9".to_owned()]);
    assert!(!v4.contains('['), "ipv4 is not bracketed");
}

#[test]
fn provisioned_but_no_subjects_keeps_443_gated() {
    // Defence against a removed identity file: provisioned flag set but no
    // subjects → no `:443`/TLS block; fall back to the cleartext `:80` cockpit
    // rather than exposing an untrusted leaf.
    let cfg = render_caddyfile(true, &[]);
    assert!(!cfg.contains("https://"), "no https cockpit without subjects");
    assert!(!cfg.contains("redir https"), "no redirect without a :443 to redirect to");
    assert!(cfg.contains(PRODUCT_UPSTREAM) && cfg.contains(":80"), "cockpit stays reachable on :80");
}

#[test]
fn regenerate_is_skipped_without_caddyfile_env() {
    // No CP_CADDYFILE in the test environment → clean skip, never errors.
    let id = Identity { name: "n".to_owned(), ip: "10.0.0.9".to_owned() };
    assert_eq!(regenerate(true, Some(&id), &[]), Ok(false));
}
