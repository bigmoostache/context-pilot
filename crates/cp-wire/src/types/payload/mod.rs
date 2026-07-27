//! Detail payload message shapes carried over the wire.
//!
//! Grouped under `payload/` to keep `types/` within the 8-entry directory cap.
//! - [`body`] — content-addressed message body reference (write plane).
//! - [`query`] — read-path query/response envelope + the [`Inbound`] socket
//!   discriminator (T671 read plane).
//!
//! [`Inbound`]: query::Inbound

/// Content-addressed body reference.
pub mod body;
/// Read-path query envelope, response, and the `Inbound` socket discriminator.
pub mod query;
