//! Platform-specific process CPU/memory sampling for the perf overlay.
//!
//! Reads this process's cumulative CPU ticks and resident memory. Linux uses
//! `/proc/self/{stat,statm}`; macOS shells out to `ps`. All other platforms
//! return `None` (no data). Split out of `perf/mod.rs` to keep that file under
//! the line cap.

/// Read CPU ticks and memory from /proc/self/stat and /proc/self/statm (Linux).
#[cfg(target_os = "linux")]
pub(super) fn read_proc_stat() -> Option<(u64, u64)> {
    // Read CPU ticks from /proc/self/stat
    // Format: pid (comm) state ... utime stime ...
    // Fields 14 and 15 (0-indexed: 13, 14) are utime and stime
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let mut fields = stat.split_whitespace();
    let utime: u64 = fields.nth(13)?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    let cpu_ticks = utime.saturating_add(stime);

    // Read memory from /proc/self/statm (in pages)
    // First field is total program size, second is RSS
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let rss_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    let page_size = 4096u64; // Standard page size
    let mem_bytes = rss_pages.saturating_mul(page_size);

    Some((cpu_ticks, mem_bytes))
}

/// Read CPU ticks (centiseconds) and memory (bytes) via `ps` (macOS).
#[cfg(target_os = "macos")]
pub(super) fn read_proc_stat() -> Option<(u64, u64)> {
    let pid = std::process::id();
    let output =
        std::process::Command::new("ps").args(["-o", "rss=,cputime=", "-p", &pid.to_string()]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    let mut parts = trimmed.split_whitespace();
    let rss_kb: u64 = parts.next()?.parse().ok()?;
    let mem_bytes = rss_kb.saturating_mul(1024);
    let cpu_centisecs = parse_ps_cputime(parts.next()?)?;
    Some((cpu_centisecs, mem_bytes))
}

/// Parse `ps` cputime format (`H:MM:SS.cc` / `MM:SS.cc`) into centiseconds.
#[cfg(target_os = "macos")]
fn parse_ps_cputime(raw: &str) -> Option<u64> {
    let (main_part, centis_str) = raw.rsplit_once('.')?;
    let centis: u64 = centis_str.parse().ok()?;
    let total_secs = parse_hms_secs(main_part)?;
    Some(total_secs.saturating_mul(100).saturating_add(centis))
}

/// Parse a `SS` / `MM:SS` / `H:MM:SS` colon-separated duration into seconds.
#[cfg(target_os = "macos")]
fn parse_hms_secs(main_part: &str) -> Option<u64> {
    let segments: Vec<&str> = main_part.split(':').collect();
    match segments.len() {
        1 => segments.first()?.parse().ok(),
        2 => parse_ms_secs(&segments),
        3 => parse_hms_triple(&segments),
        _ => None,
    }
}

/// Parse `[MM, SS]` colon segments into total seconds.
#[cfg(target_os = "macos")]
fn parse_ms_secs(segments: &[&str]) -> Option<u64> {
    let mins: u64 = segments.first()?.parse().ok()?;
    let secs: u64 = segments.get(1)?.parse().ok()?;
    Some(mins.saturating_mul(60).saturating_add(secs))
}

/// Parse `[H, MM, SS]` colon segments into total seconds.
#[cfg(target_os = "macos")]
fn parse_hms_triple(segments: &[&str]) -> Option<u64> {
    let hours: u64 = segments.first()?.parse().ok()?;
    let mins: u64 = segments.get(1)?.parse().ok()?;
    let secs: u64 = segments.get(2)?.parse().ok()?;
    Some(hours.saturating_mul(3600).saturating_add(mins.saturating_mul(60)).saturating_add(secs))
}

/// Fallback for unsupported platforms — no CPU/memory data available.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn read_proc_stat() -> Option<(u64, u64)> {
    None
}
