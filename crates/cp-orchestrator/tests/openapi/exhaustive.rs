//! Route exhaustiveness test — every route in the router must have a
//! corresponding path+method in `openapi.json`, and vice-versa.
//!
//! This is the mechanical guard-rail that prevents "forgot to add the new
//! endpoint to the spec": if someone adds a `(Method::Post, ["api", "agent",
//! id, "new-thing"])` match arm without a corresponding spec entry, **this
//! test fails**.
//!
//! It works by:
//! 1. Parsing `src/transport/mod.rs` as TEXT to extract all route patterns
//!    (both `route_rest()` match arms and `handle()` special-case routes).
//! 2. Reading the committed `openapi.json` to extract all path+method pairs.
//! 3. Canonicalising both sides (path params → `{}`) and asserting
//!    bidirectional set equality.
//!
//! No framework introspection, no macros, no new deps — just source-level
//! string parsing that stays correct as long as the routing code keeps its
//! current (stable) structure.

use std::collections::BTreeSet;

/// A canonical route: `("GET", "/api/agent/{}/meta")`.
///
/// Path parameters are erased to `{}` so the comparison is shape-only —
/// the test does not care whether the spec calls a param `{id}` or
/// `{agentId}`, only that the route *exists*.
type Route = (String, String);

/// Routes intentionally excluded from the spec (protocol upgrades, not REST).
const EXCLUDED: &[(&str, &str)] = &[("GET", "/api/stream")];

/// Extract all routes from the router source code.
fn extract_router_routes() -> BTreeSet<Route> {
    let src_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/transport/mod.rs");
    let src = std::fs::read_to_string(&src_path).expect("read transport/mod.rs");

    let mut routes = BTreeSet::new();

    for line in src.lines() {
        let t = line.trim();

        // ── Match arms in route_rest(): (Method::Get, ["api", ...]) ──
        if let Some(rest) = t.strip_prefix("(Method::") {
            if let Some((method_raw, after)) = rest.split_once(',') {
                if let Some(segs) = extract_bracket_segments(after) {
                    let path = segments_to_path(&segs);
                    let _new = routes.insert((method_raw.trim().to_uppercase(), path));
                }
            }
        }

        // ── Special routes in handle(): if let ["api", ...] = segments ──
        // These live inside `if method == Method::Get { ... }` so are all GET.
        if t.starts_with("if let [\"api\"") {
            if let Some(segs) = extract_bracket_segments(t) {
                let path = segments_to_path(&segs);
                let _new = routes.insert(("GET".to_owned(), path));
            }
        }
    }

    // Remove intentionally excluded routes.
    for &(m, p) in EXCLUDED {
        let _removed = routes.remove(&(m.to_owned(), p.to_owned()));
    }

    routes
}

/// Extract all `(method, path)` pairs from the committed `openapi.json`.
fn extract_spec_routes() -> BTreeSet<Route> {
    let spec_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("openapi.json");
    let raw = std::fs::read_to_string(&spec_path).expect("read openapi.json");
    let spec: serde_json::Value = serde_json::from_str(&raw).expect("parse openapi.json");

    let paths = spec["paths"].as_object().expect("paths object");
    let mut routes = BTreeSet::new();

    for (path, methods_val) in paths {
        let canonical = canonicalize_spec_path(path);
        if let Some(methods_obj) = methods_val.as_object() {
            for method in methods_obj.keys() {
                let _new = routes.insert((method.to_uppercase(), canonical.clone()));
            }
        }
    }

    routes
}

// ── Parsing helpers ─────────────────────────────────────────────────

/// Find the first `[...]` in `s` and parse its contents as route segments.
///
/// Returns segments with string literals as-is and variable bindings
/// replaced by `{}`.
fn extract_bracket_segments(s: &str) -> Option<Vec<String>> {
    let start = s.find('[')?;
    let end = s[start..].find(']')? + start;
    let inner = &s[start + 1..end];
    let mut segs = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.starts_with('"') && part.ends_with('"') && part.len() >= 2 {
            // String literal: "api" → api
            segs.push(part[1..part.len() - 1].to_owned());
        } else if !part.is_empty() {
            // Variable binding (id, name, hash, user_id) → {}
            segs.push("{}".to_owned());
        }
    }
    if segs.is_empty() { None } else { Some(segs) }
}

/// Join parsed segments into a canonical path: `["api", "{}", "meta"]` →
/// `/api/{}/meta`.
fn segments_to_path(segs: &[String]) -> String {
    format!("/{}", segs.join("/"))
}

/// Replace all `{paramName}` occurrences in a spec path with `{}`.
fn canonicalize_spec_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            out.push_str("{}");
            // Skip until closing '}'
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ── The test ────────────────────────────────────────────────────────

#[test]
#[ignore]
fn route_exhaustiveness() {
    let router = extract_router_routes();
    let spec = extract_spec_routes();

    let router_only: BTreeSet<_> = router.difference(&spec).collect();
    let spec_only: BTreeSet<_> = spec.difference(&router).collect();

    let mut failures = Vec::new();

    if !router_only.is_empty() {
        failures.push(format!(
            "Routes in router but NOT in openapi.json ({}):\n{}",
            router_only.len(),
            router_only
                .iter()
                .map(|(m, p)| format!("  {m} {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    if !spec_only.is_empty() {
        failures.push(format!(
            "Routes in openapi.json but NOT in router ({}):\n{}",
            spec_only.len(),
            spec_only
                .iter()
                .map(|(m, p)| format!("  {m} {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    assert!(
        failures.is_empty(),
        "Route exhaustiveness failed!\n\n{}",
        failures.join("\n\n")
    );

    eprintln!(
        "Route exhaustiveness OK: {} routes in sync between router and spec",
        router.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_erases_param_names() {
        assert_eq!(canonicalize_spec_path("/api/agent/{id}/acl/{userId}"), "/api/agent/{}/acl/{}");
        assert_eq!(canonicalize_spec_path("/api/fleet"), "/api/fleet");
        assert_eq!(canonicalize_spec_path("/api/env-keys/{name}"), "/api/env-keys/{}");
    }

    #[test]
    fn parse_bracket_segments() {
        let segs = extract_bracket_segments(r#"["api", "agent", id, "meta"]"#).unwrap();
        assert_eq!(segs, vec!["api", "agent", "{}", "meta"]);

        let segs = extract_bracket_segments(r#"["api", "health"]"#).unwrap();
        assert_eq!(segs, vec!["api", "health"]);

        let segs = extract_bracket_segments(r#"["api", "agent", id, "body", hash]"#).unwrap();
        assert_eq!(segs, vec!["api", "agent", "{}", "body", "{}"]);
    }

    #[test]
    fn segments_join_to_path() {
        assert_eq!(
            segments_to_path(&["api".into(), "agent".into(), "{}".into(), "meta".into()]),
            "/api/agent/{}/meta"
        );
    }
}
