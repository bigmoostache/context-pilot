//! Simple profiler for identifying slow operations.
//!
//! Usage:
//!   let _guard = `profile!("operation_name`");
//!   // ... code to measure ...
//!   // automatically logs when guard drops if > threshold
//!
//! View results: tail -f .context-pilot/perf.log

use cp_base::cast::Safe as _;
use cp_base::panels::time_arith;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::time::Instant;

/// Minimum duration (ms) before an operation is logged to disk.
const THRESHOLD_MS: u128 = 5;
/// Path to the on-disk performance log file.
const LOG_FILE: &str = ".context-pilot/perf.log";

/// RAII guard that records elapsed time on drop.
pub(crate) struct ProfileGuard {
    /// Name of the profiled operation.
    name: &'static str,
    /// Instant when the guard was created.
    start: Instant,
}

impl ProfileGuard {
    /// Create a new profile guard for the given operation name.
    pub(crate) fn new(name: &'static str) -> Self {
        Self { name, start: Instant::now() }
    }
}

impl Drop for ProfileGuard {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        let us = elapsed.as_micros().to_u64();
        let ms = time_arith::us_to_ms(us);

        // Always record to in-memory perf system
        crate::ui::perf::PERF.record_op(self.name, us);

        // Log to file only for slow operations
        if u128::from(ms) >= THRESHOLD_MS
            && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(LOG_FILE)
        {
            let _r = writeln!(file, "{:>6}ms  {}", ms, self.name);
        }
    }
}

/// Create a profiling guard that logs slow operations on drop.
///
/// Records timing to the in-memory perf system, and writes to `.context-pilot/perf.log`
/// if the operation exceeds 5 ms.
#[macro_export]
macro_rules! profile {
    ($name:expr) => {
        $crate::infra::profiler::ProfileGuard::new($name)
    };
}
