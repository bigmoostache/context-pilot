//! Meilisearch HTTP client and server lifecycle management.
//!
//! Groups the Meilisearch-specific code: the HTTP API client, binary
//! download logic, server start/stop/health lifecycle, and init-time
//! helpers (index creation, metrics population).

/// Init-time helpers: index creation, metrics population, project hashing.
pub(crate) mod bootstrap;
/// HTTP API client for Meilisearch: index management, document CRUD, search.
pub(crate) mod client;
/// Binary download and platform detection.
pub(crate) mod download;
/// Ctrl+I overlay data provider (live stats from Meilisearch).
pub(crate) mod overlay;
/// Server lifecycle: start, stop, health check, reconnect.
pub(crate) mod server;
