//! Simple profiler for identifying slow operations.
//!
//! Usage:
//!   let _guard = profile!("operation_name");
//!   // ... code to measure ...
//!   // automatically logs when guard drops if > threshold
//!
//! View results: tail -f .context-pilot/perf.log

use std::time::Instant;
use std::fs::OpenOptions;
use std::io::Write;

const THRESHOLD_MS: u128 = 5; // Only log operations taking > 5ms
const LOG_FILE: &str = ".context-pilot/perf.log";

pub struct ProfileGuard {
    name: &'static str,
    start: Instant,
}

impl ProfileGuard {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }
}

impl Drop for ProfileGuard {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        let ms = elapsed.as_millis();

        if ms >= THRESHOLD_MS {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(LOG_FILE)
            {
                let _ = writeln!(file, "{:>6}ms  {}", ms, self.name);
            }
        }
    }
}

#[macro_export]
macro_rules! profile {
    ($name:expr) => {
        $crate::profiler::ProfileGuard::new($name)
    };
}
