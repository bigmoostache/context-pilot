//! IT infrastructure — certs, network identity, and provisioning state.
//!
//! These modules were the separate maintenance plane; in the v3 teardown (design
//! §13.4/§13.8) that separate plane is **removed** and its functions are re-homed
//! onto the product REST API (`:443`) under the `can_manage_it` capability. What
//! survives is the shared implementation the product IT handlers
//! ([`crate::transport::rest::config::it`]) delegate to:
//!
//! * [`ca`] — private-CA root download + fingerprint.
//! * [`caddy`] — dynamic Caddyfile generation + reload (`:80`/`:443`, no plane).
//! * [`crypto`] — self-contained SHA-256 / base64 for the CA fingerprint.
//! * [`identity`] — box name/IP + boot-time Caddy apply.
//! * [`state`] — durable `provisioned` flag.
//!
//! This module root carries no request router of its own any more: there is a
//! single product pipeline in [`crate::transport::handle`].

pub(crate) mod ca;
mod caddy;
mod crypto;
pub(crate) mod identity;
pub(crate) mod state;

pub(crate) use identity::apply_caddy_at_boot;
pub(crate) use state::is_provisioned;

// Re-exported for the retained submodules (`identity`, `ca`), which reach the
// backend + reply types through `super::` just as they did under `maint`.
use super::Backend;
use super::rest::HttpReply;
