//! Flame graph telemetry — thin re-export from `cp_base::flame`.
//!
//! The core implementation lives in `cp-base` so all crates can instrument.
//! This re-exports `init()` and `flush()` for `main.rs` startup/shutdown.

pub(crate) use cp_base::flame::{flush, init};
