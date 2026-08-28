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

#[cfg(test)]
mod tests {
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
    ///
    /// The routing code is split across two files (the transport `handle` shell
    /// was pushed over the 500-line budget): `router.rs` holds the `route_rest()`
    /// match arms and the raw-bytes `try_raw_route()` dispatcher, while `mod.rs`
    /// still holds the `handle()` special-case routes. Both are parsed as text.
    fn extract_router_routes() -> BTreeSet<Route> {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/transport");
        let mut routes = BTreeSet::new();
        for file in ["router.rs", "mod.rs"] {
            let src = std::fs::read_to_string(base.join(file)).expect("read transport source");
            extract_routes_from_source(&src, &mut routes);
        }

        // Remove intentionally excluded routes.
        for &(m, p) in EXCLUDED {
            let _removed = routes.remove(&(m.to_owned(), p.to_owned()));
        }

        routes
    }

    /// Parse one transport source file for route patterns, inserting each into
    /// `routes`. Recognises three arm shapes:
    ///
    /// * `route_rest()` tuple arms — `(&Method::Get, &["api", …]) =>` (the `&`
    ///   reference patterns `clippy::pattern_type_mismatch` mandates over `&[&str]`);
    /// * `try_raw_route()` slice arms — `["api", …] =>` matched on `*segments`,
    ///   all GET (the raw-bytes dispatcher only runs for GET);
    /// * `handle()` special routes — `if … segments … == ["api", …]`, also GET.
    fn extract_routes_from_source(src: &str, routes: &mut BTreeSet<Route>) {
        for line in src.lines() {
            let t = line.trim();

            // ── route_rest() tuple arms: (&Method::Get, &["api", ...]) ──
            if let Some(rest) = t.strip_prefix("(&Method::").or_else(|| t.strip_prefix("(Method::"))
                && let Some((method_raw, after)) = rest.split_once(',')
                && let Some(segs) = extract_bracket_segments(after)
            {
                let path = segments_to_path(&segs);
                let _new = routes.insert((method_raw.trim().to_uppercase(), path));
            }

            // ── try_raw_route() slice arms: ["api", ...] => (all GET) ──
            if t.starts_with("[\"api\"")
                && t.contains("=>")
                && let Some(segs) = extract_bracket_segments(t)
            {
                let path = segments_to_path(&segs);
                let _new = routes.insert(("GET".to_owned(), path));
            }

            // ── Special routes in handle(): if let ["api", ...] = segments ──
            // These live inside `if method == Method::Get { ... }` so are all GET.
            if t.starts_with("if let [\"api\"")
                && let Some(segs) = extract_bracket_segments(t)
            {
                let path = segments_to_path(&segs);
                let _new = routes.insert(("GET".to_owned(), path));
            }
        }
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

        let paths = spec.get("paths").and_then(serde_json::Value::as_object).expect("paths object");
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

    // ── Parsing helpers ─────────────────────────────────────────────

    /// Find the first `[...]` in `s` and parse its contents as route segments.
    ///
    /// Returns segments with string literals as-is and variable bindings
    /// replaced by `{}`.
    fn extract_bracket_segments(s: &str) -> Option<Vec<String>> {
        let start = s.find('[')?;
        let rest = s.get(start..)?;
        let end = rest.find(']')?.checked_add(start)?;
        let inner = s.get(start.checked_add(1)?..end)?;
        let segs: Vec<String> = inner.split(',').filter_map(parse_segment).collect();
        if segs.is_empty() { None } else { Some(segs) }
    }

    /// Classify one comma-split token: a quoted string literal keeps its inner
    /// text, a bare binding (`id`, `name`, …) becomes `{}`, and an empty token
    /// (trailing comma) is dropped. Extracted from [`extract_bracket_segments`]
    /// to keep that function under the cognitive-complexity cap.
    fn parse_segment(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
            // String literal: "api" → api
            let unquoted = trimmed.get(1..trimmed.len().saturating_sub(1)).unwrap_or(trimmed);
            Some(unquoted.to_owned())
        } else {
            // Variable binding (id, name, hash, user_id) → {}
            Some("{}".to_owned())
        }
    }

    /// Join parsed segments into a canonical path: `["api", "{}", "meta"]` →
    /// `/api/{}/meta`.
    fn segments_to_path(segs: &[String]) -> String {
        format!("/{}", segs.join("/"))
    }

    /// Replace all `{paramName}` occurrences in a spec path with `{}`.
    fn canonicalize_spec_path(path: &str) -> String {
        let mut out = String::with_capacity(path.len());
        let mut chars = path.chars();
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

    // ── The test ────────────────────────────────────────────────────

    #[test]
    #[ignore = "source-scraping guard-rail; run explicitly with --ignored"]
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
                router_only.iter().map(|route| format!("  {} {}", route.0, route.1)).collect::<Vec<_>>().join("\n")
            ));
        }

        if !spec_only.is_empty() {
            failures.push(format!(
                "Routes in openapi.json but NOT in router ({}):\n{}",
                spec_only.len(),
                spec_only.iter().map(|route| format!("  {} {}", route.0, route.1)).collect::<Vec<_>>().join("\n")
            ));
        }

        assert!(failures.is_empty(), "Route exhaustiveness failed!\n\n{}", failures.join("\n\n"));
    }

    #[test]
    fn canonicalize_erases_param_names() {
        assert_eq!(canonicalize_spec_path("/api/agent/{id}/acl/{userId}"), "/api/agent/{}/acl/{}");
        assert_eq!(canonicalize_spec_path("/api/fleet"), "/api/fleet");
        assert_eq!(canonicalize_spec_path("/api/env-keys/{name}"), "/api/env-keys/{}");
    }

    #[test]
    fn parse_bracket_segments() {
        let health = extract_bracket_segments(r#"["api", "health"]"#).unwrap();
        assert_eq!(health, vec!["api", "health"]);

        let meta = extract_bracket_segments(r#"["api", "agent", id, "meta"]"#).unwrap();
        assert_eq!(meta, vec!["api", "agent", "{}", "meta"]);

        let body = extract_bracket_segments(r#"["api", "agent", id, "body", hash]"#).unwrap();
        assert_eq!(body, vec!["api", "agent", "{}", "body", "{}"]);
    }

    #[test]
    fn segments_join_to_path() {
        assert_eq!(segments_to_path(&["api".into(), "agent".into(), "{}".into(), "meta".into()]), "/api/agent/{}/meta");
    }
}
