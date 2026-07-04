//! Configuration REST handlers — central settings (`settings`) and the
//! environment-key inspection/editing surface (`env_keys`). Grouped here to keep
//! the `rest` directory within its size budget; both are re-exported from
//! [`super`](super) so callers still use `rest::get_settings`, `rest::env_keys_list`, etc.

pub(crate) mod env_keys;
pub(crate) mod settings;
