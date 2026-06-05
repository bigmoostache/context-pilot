//! Headless mode — daemon/client architecture over Unix sockets.
//!
//! The daemon builds IR frames and broadcasts them to connected clients.
//! Each client renders frames locally via the existing IR→ratatui adapters.
//!
//! See `docs/design-headless.md` for the full design.

pub(crate) mod client;
pub(crate) mod launch;
pub(crate) mod protocol;
pub(crate) mod server;
pub(crate) mod session;
