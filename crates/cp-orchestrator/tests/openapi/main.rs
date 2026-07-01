#![recursion_limit = "512"]
//! OpenAPI 3.0.3 spec generator (integration test, --ignored).
//!
//! Builds the full spec manually (schemas + paths for all endpoints).
//! Run: `cargo test -p cp-orchestrator --test openapi generate_openapi -- --ignored`
//! Writes `openapi.json` at workspace root.

mod exhaustive;
mod paths;
mod schemas;
mod schemas_ext;

// Acknowledge lib-only deps visible to the integration-test binary.
use argon2 as _;
use base64 as _;
use calamine as _;
use cp_mod_bridge as _;
use cp_oplog as _;
use cp_orchestrator as _;
use cp_wire as _;
use csv as _;
use nix as _;
use notify as _;
use openssl as _;
use portable_pty as _;
use reqwest as _;
use rusqlite as _;
use serde as _;
use serde_yaml as _;
use sha2 as _;
use tempfile as _;
use tiny_http as _;
use utoipa as _;

use serde_json::{Value, json};

// ── Helpers ─────────────────────────────────────────────────────────

/// `$ref` shorthand.
pub(crate) fn r(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

/// Array-of shorthand.
pub(crate) fn arr(items: Value) -> Value {
    json!({ "type": "array", "items": items })
}

/// 200 response wrapper.
pub(crate) fn ok(schema: Value) -> Value {
    json!({ "200": { "description": "Success", "content": { "application/json": { "schema": schema } } } })
}

/// Error response (4xx/5xx).
pub(crate) fn err() -> Value {
    json!({ "default": { "description": "Error", "content": { "application/json": { "schema": r("Error") } } } })
}

/// GET endpoint.
pub(crate) fn get(tag: &str, summary: &str, response: Value) -> Value {
    json!({ "get": { "tags": [tag], "summary": summary, "responses": merge(ok(response), err()) } })
}

/// POST endpoint.
pub(crate) fn post(tag: &str, summary: &str, body: Option<Value>, response: Value) -> Value {
    let mut op = json!({ "tags": [tag], "summary": summary, "responses": merge(ok(response), err()) });
    if let Some(b) = body {
        if let Some(obj) = op.as_object_mut() {
            drop(obj.insert(
                "requestBody".into(),
                json!({ "required": true, "content": { "application/json": { "schema": b } } }),
            ));
        }
    }
    json!({ "post": op })
}

/// DELETE endpoint.
pub(crate) fn del(tag: &str, summary: &str) -> Value {
    json!({ "delete": { "tags": [tag], "summary": summary, "responses": merge(ok(r("OkResponse")), err()) } })
}

/// Merge two JSON objects.
pub(crate) fn merge(mut a: Value, b: Value) -> Value {
    if let (Some(ma), Some(mb)) = (a.as_object_mut(), b.as_object()) {
        for (k, v) in mb {
            drop(ma.insert(k.clone(), v.clone()));
        }
    }
    a
}

/// Agent path parameter.
fn agent_param() -> Value {
    json!([{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }])
}

/// Inject agent path parameter into every operation.
pub(crate) fn with_agent(mut path_item: Value) -> Value {
    if let Some(obj) = path_item.as_object_mut() {
        for (_, op) in obj.iter_mut() {
            if let Some(o) = op.as_object_mut() {
                if let Some(existing) = o.get_mut("parameters") {
                    if let Some(arr) = existing.as_array_mut() {
                        // Prepend agent param if not already present.
                        let ap = agent_param();
                        if let Some(items) = ap.as_array() {
                            for item in items.iter().rev() {
                                arr.insert(0, item.clone());
                            }
                        }
                    }
                } else {
                    drop(o.insert("parameters".into(), agent_param()));
                }
            }
        }
    }
    path_item
}

// ── Build ───────────────────────────────────────────────────────────

fn build_spec() -> Value {
    let all_schemas = merge(schemas::core(), schemas_ext::transport());
    json!({
        "openapi": "3.0.3",
        "info": { "title": "Context Pilot Orchestrator", "version": "1.0.0" },
        "servers": [{ "url": "http://localhost:7878" }],
        "components": { "schemas": all_schemas },
        "paths": paths::paths()
    })
}

#[test]
#[ignore]
fn generate_openapi() {
    let spec = serde_json::to_string_pretty(&build_spec()).expect("serialize");
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(|p| p.parent()).expect("workspace root");
    std::fs::write(root.join("openapi.json"), &spec).expect("write openapi.json");
    eprintln!("Wrote openapi.json ({} bytes)", spec.len());
}
