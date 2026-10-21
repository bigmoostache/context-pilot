//! `OpenAPI` schemas DERIVED from the Rust wire types via `utoipa::ToSchema`.
//!
//! Hand-written `json!` schemas only prove the TS client matches `openapi.json`;
//! a derived schema is generated from the serde type itself, so renaming a
//! field in Rust changes the spec and trips the contract drift gate.
//! Migrate a schema here by adding `ToSchema` to its type and deleting the
//! hand-written literal — [`NAMES`] is checked against the hand-written maps so
//! a schema can never be defined twice.

use serde_json::{Map, Value};
use utoipa::PartialSchema;

/// Names of every derived schema, in the order of [`derived`].
pub(crate) const NAMES: &[&str] = &["ConvHit", "SqlPage"];

/// Serialize one derived schema to JSON.
fn schema_of<T>() -> Value
where
    T: PartialSchema,
{
    serde_json::to_value(T::schema()).unwrap_or_default()
}

/// Every derived schema, keyed by its component name.
pub(crate) fn derived() -> Value {
    let mut m = Map::new();
    drop(m.insert("ConvHit".into(), schema_of::<cp_wire::types::payload::query::ConvHit>()));
    drop(m.insert("SqlPage".into(), schema_of::<cp_wire::types::payload::query::SqlPage>()));
    Value::Object(m)
}
