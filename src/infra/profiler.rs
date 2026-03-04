//! Simple profiler for identifying slow operations.
//!
//! Usage:
//!   let _guard = `profile!("operation_name`");
//!   // ... code to measure ...
//!   // automatically logs when guard drops if > threshold
//!
//! View results: tail -f .context-pilot/perf.log

use cp_base::cast::SafeCast;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Instant;

const THRESHOLD_MS: u128 = 5; // Only log operations taking > 5ms
const LOG_FILE: &str = ".context-pilot/perf.log";

pub(crate) struct ProfileGuard {
    name: &'static str,
    start: Instant,
}

impl ProfileGuard {
    pub(crate) fn new(name: &'static str) -> Self {
        Self { name, start: Instant::now() }
    }
}

impl Drop for ProfileGuard {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        let us = elapsed.as_micros().to_u64();
        let ms = us / 1000;

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
