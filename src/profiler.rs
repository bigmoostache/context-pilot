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

/// Clear the perf log file
pub fn clear_log() {
    let _ = std::fs::write(LOG_FILE, "");
}

/// Summary stats from perf log
pub fn get_summary() -> String {
    use std::collections::HashMap;

    let content = std::fs::read_to_string(LOG_FILE).unwrap_or_default();
    let mut stats: HashMap<String, (u128, usize)> = HashMap::new(); // (total_ms, count)

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(ms) = parts[0].trim_end_matches("ms").parse::<u128>() {
                let name = parts[1..].join(" ");
                let entry = stats.entry(name).or_insert((0, 0));
                entry.0 += ms;
                entry.1 += 1;
            }
        }
    }

    let mut sorted: Vec<_> = stats.into_iter().collect();
    sorted.sort_by(|a, b| b.1.0.cmp(&a.1.0)); // Sort by total time desc

    let mut output = String::from("=== Performance Summary ===\n");
    output.push_str("Total ms | Count | Avg ms | Operation\n");
    output.push_str("---------+-------+--------+----------\n");

    for (name, (total, count)) in sorted.iter().take(20) {
        let avg = total / (*count as u128);
        output.push_str(&format!("{:>8} | {:>5} | {:>6} | {}\n", total, count, avg, name));
    }

    output
}
