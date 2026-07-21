//! [`ParamType`] inherent methods — split from `tools/mod.rs` to keep that
//! file under the 500-line structure limit.
//!
//! The [`ParamType`](super::ParamType) enum itself stays in `tools/mod.rs`
//! (so the `cp_base::tools::ParamType` path is unchanged for every downstream
//! crate); only its inherent `impl` block lives here. An inherent impl is not
//! path-scoped, so it applies globally even though this module is private.

use serde_json::{Value, json};

use super::ParamType;

impl ParamType {
    /// Human-readable name for this parameter type (e.g. `"string"`, `"array"`).
    ///
    /// The single in-crate exhaustive match over the (closed) variant set, so
    /// downstream validators name a type without matching `ParamType` themselves.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match *self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }

    /// Whether a JSON `value` satisfies this parameter type.
    ///
    /// Lenient for arrays: a single value matching the inner type is accepted
    /// (a common LLM mistake — sending `"path": "foo.rs"` instead of
    /// `"path": ["foo.rs"]`). The one in-crate exhaustive match over the variant
    /// set, so downstream pre-flight checks a value without matching `ParamType`.
    #[must_use]
    pub fn check_json(&self, value: &Value) -> bool {
        crate::deref_match!(self, {
            Self::String => value.is_string(),
            Self::Integer => value.is_i64() || value.is_u64(),
            Self::Number => value.is_number(),
            Self::Boolean => value.is_boolean(),
            Self::Array(ref inner) => value.is_array() || inner.check_json(value),
            Self::Object(_) => value.is_object(),
        })
    }

    /// Emit the JSON Schema representation (recursive for nested types).
    pub(super) fn to_json_schema(&self) -> Value {
        crate::deref_match!(self, {
            Self::String => json!({"type": "string"}),
            Self::Integer => json!({"type": "integer"}),
            Self::Number => json!({"type": "number"}),
            Self::Boolean => json!({"type": "boolean"}),
            Self::Array(ref inner) => json!({
                "type": "array",
                "items": inner.to_json_schema()
            }),
            Self::Object(ref params) => {
                let mut properties = serde_json::Map::new();
                let mut required = Vec::new();
                for param in params {
                    let mut schema = param.param_type.to_json_schema();
                    if let Some(desc) = param.description.as_ref()
                        && let Some(obj) = schema.as_object_mut()
                    {
                        drop(obj.insert("description".to_owned(), json!(desc)));
                    }
                    if let Some(enum_vals) = param.enum_values.as_ref()
                        && let Some(obj) = schema.as_object_mut()
                    {
                        drop(obj.insert("enum".to_owned(), json!(enum_vals)));
                    }
                    drop(properties.insert(param.name.clone(), schema));
                    if param.required {
                        required.push(param.name.clone());
                    }
                }
                json!({
                    "type": "object",
                    "properties": properties,
                    "required": required
                })
            }
        })
    }
}
