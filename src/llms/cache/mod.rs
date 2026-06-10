//! Prompt caching subsystem: breakpoint placement engine, optimizer, and density models.
//!
//! Submodules: `cache_engine` (BP tracking + placement), `cache_optimizer` (DP),
//! `density` (divergence weighting), `prompt_tick_csv` (debug dumper).

pub(crate) mod cache_engine;
pub(crate) mod cache_optimizer;
pub(crate) mod density;
pub(crate) mod prompt_tick_csv;
