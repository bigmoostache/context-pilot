//! Simple profiler for identifying slow operations.
//!
//! Usage:
//!   let _guard = profile!("operation_name");
//!   // ... code to measure ...
//!   // automatically logs when guard drops if > threshold
//!
//! View results: tail -f .context-pilot/perf.log

use std::fs::OpenOptions;
use std::io::Write;
use std::time::Instant;

const THRESHOLD_MS: u128 = 5; // Only log operations taking > 5ms
const LOG_FILE: &str = ".context-pilot/perf.log";

pub struct ProfileGuard {
    name: &'static str,
    start: Instant,
}

impl ProfileGuard {
    pub fn new(name: &'static str) -> Self {
        Self { name, start: Instant::now() }
    }
}

impl Drop for ProfileGuard {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        let us = elapsed.as_micros() as u64;
        let ms = us / 1000;

        // Always record to in-memory perf system
        crate::perf::PERF.record_op(self.name, us);

        // Log to file only for slow operations
        if ms as u128 >= THRESHOLD_MS
            && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(LOG_FILE)
        {
            let _ = writeln!(file, "{:>6}ms  {}", ms, self.name);
        }
    }
}

#[macro_export]
macro_rules! profile {
    ($name:expr) => {
        $crate::profiler::ProfileGuard::new($name)
    };
}
