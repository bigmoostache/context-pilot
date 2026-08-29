#![recursion_limit = "512"]
//! `OpenAPI` 3.0.3 spec generator (integration test, --ignored).
//!
//! Builds the full spec manually (schemas + paths for all endpoints).
//! Run: `cargo test -p cp-orchestrator --test openapi generate_openapi -- --ignored`
//! Writes `openapi.json` at workspace root.

mod exhaustive;
mod paths;
mod schemas;
mod schemas_ext;
mod schemas_ext2;
mod schemas_net;
mod schemas_sms;

// Acknowledge lib-only deps visible to the integration-test binary.
use argon2 as _;
use base64 as _;
use calamine as _;
use cp_base as _;
use cp_mod_bridge as _;
use cp_mod_utilities as _;
use cp_oplog as _;
use cp_orchestrator as _;
use cp_vault as _;
use cp_wire as _;
use csv as _;
use dotenvy as _;
use minisign_verify as _;
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

use serde_json::{Map, Value, json};

// ── Helpers ─────────────────────────────────────────────────────────

/// `$ref` shorthand.
pub(crate) fn r(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

/// Array-of shorthand.
pub(crate) fn arr(items: Value) -> Value {
    // Build the object by hand so `items` is *moved* into the map rather than
    // borrowed by `json!` — otherwise clippy::needless_pass_by_value fires on
    // the by-value parameter.
    let mut m = Map::new();
    drop(m.insert("type".into(), json!("array")));
    drop(m.insert("items".into(), items));
    Value::Object(m)
}

/// 200 response wrapper.
pub(crate) fn ok(schema: Value) -> Value {
    // Move `schema` into the leaf map so it is consumed by value (json! would
    // only borrow it, tripping clippy::needless_pass_by_value).
    let mut media = Map::new();
    drop(media.insert("schema".into(), schema));
    json!({ "200": { "description": "Success", "content": { "application/json": Value::Object(media) } } })
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
    if let Some(b) = body
        && let Some(obj) = op.as_object_mut()
    {
        drop(obj.insert(
            "requestBody".into(),
            json!({ "required": true, "content": { "application/json": { "schema": b } } }),
        ));
    }
    json!({ "post": op })
}

/// DELETE endpoint.
pub(crate) fn del(tag: &str, summary: &str) -> Value {
    json!({ "delete": { "tags": [tag], "summary": summary, "responses": merge(ok(r("OkResponse")), err()) } })
}

/// Merge two JSON objects.
pub(crate) fn merge(mut a: Value, b: Value) -> Value {
    // Destructure `b` by value so its entries are *moved* into `a` (consuming
    // `b` and avoiding per-entry clones); this also silences
    // clippy::needless_pass_by_value on the by-value parameter.
    if let (Some(ma), Value::Object(mb)) = (a.as_object_mut(), b) {
        for (k, v) in mb {
            drop(ma.insert(k, v));
        }
    }
    a
}

/// Agent path parameter.
fn agent_param() -> Value {
    json!([{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }])
}

/// Prepend the agent path parameter onto one operation's `parameters` array,
/// creating the array if absent. Extracted from [`with_agent`] to keep that
/// function under the cognitive-complexity cap.
fn prepend_agent_param(op: &mut Value) {
    let Some(o) = op.as_object_mut() else { return };
    if let Some(existing) = o.get_mut("parameters") {
        if let Some(arr) = existing.as_array_mut() {
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

/// Inject agent path parameter into every operation.
pub(crate) fn with_agent(mut path_item: Value) -> Value {
    if let Some(obj) = path_item.as_object_mut() {
        for (_, op) in obj.iter_mut() {
            prepend_agent_param(op);
        }
    }
    path_item
}

// ── Build ───────────────────────────────────────────────────────────

fn build_spec() -> Value {
    let all_schemas = merge(
        merge(merge(merge(schemas::core(), schemas_ext::transport()), schemas_ext2::deploy()), schemas_net::network()),
        schemas_sms::sms(),
    );
    json!({
        "openapi": "3.0.3",
        "info": { "title": "Context Pilot Orchestrator", "version": "1.0.0" },
        "servers": [{ "url": "http://localhost:7878" }],
        "components": { "schemas": all_schemas },
        "paths": paths::paths()
    })
}

#[cfg(test)]
mod spec_gen {
    use super::*;

    #[test]
    #[ignore = "spec generator; run explicitly with --ignored to (re)write openapi.json"]
    fn generate_openapi() {
        let spec = serde_json::to_string_pretty(&build_spec()).expect("serialize");
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(|p| p.parent()).expect("workspace root");
        std::fs::write(root.join("openapi.json"), &spec).expect("write openapi.json");
    }
}
