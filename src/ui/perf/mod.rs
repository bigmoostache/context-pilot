//! In-memory performance monitoring system.
//!
//! Provides low-overhead profiling with real-time stats collection.
//! Toggle with F12.

mod overlay;
pub(crate) use overlay::render_perf_overlay;

use crate::infra::constants::PERF_STATS_REFRESH_MS;
use cp_base::cast::SafeCast;
use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

/// Number of recent samples for trend analysis / ring buffer size
const SAMPLE_RING_SIZE: usize = 64;

/// Frame budget for 60fps (milliseconds)
pub(crate) const FRAME_BUDGET_60FPS: f64 = 16.67;
/// Frame budget for 30fps (milliseconds)
pub(crate) const FRAME_BUDGET_30FPS: f64 = 33.33;

/// Ring buffer for recent samples
pub(crate) struct RingBuffer<T: Copy + Default> {
    data: Vec<T>,
    write_pos: usize,
    len: usize,
}

impl<T: Copy + Default> Default for RingBuffer<T> {
    fn default() -> Self {
        Self { data: vec![T::default(); SAMPLE_RING_SIZE], write_pos: 0, len: 0 }
    }
}

impl<T: Copy + Default + Ord> RingBuffer<T> {
    pub(crate) fn push(&mut self, value: T) {
        self.data[self.write_pos] = value;
        self.write_pos = (self.write_pos + 1) % SAMPLE_RING_SIZE;
        if self.len < SAMPLE_RING_SIZE {
            self.len += 1;
        }
    }

    pub(crate) fn recent(&self, count: usize) -> Vec<T> {
        if self.len == 0 {
            return Vec::new();
        }
        let count = count.min(self.len);
        let mut result = Vec::with_capacity(count);
        let start = if self.len < SAMPLE_RING_SIZE { 0 } else { self.write_pos };
        for i in 0..count {
            let idx = (start + self.len - count + i) % SAMPLE_RING_SIZE;
            result.push(self.data[idx]);
        }
        result
    }
}

/// Single operation's accumulated statistics
pub(crate) struct OpStats {
    /// Total invocation count
    pub count: AtomicU64,
    /// Total time in microseconds
    pub total_us: AtomicU64,
    /// Maximum single execution time in microseconds
    pub max_us: AtomicU64,
    /// Recent samples ring buffer (microseconds)
    pub samples: RwLock<RingBuffer<u64>>,
}

impl Default for OpStats {
    fn default() -> Self {
        Self {
            count: AtomicU64::new(0),
            total_us: AtomicU64::new(0),
            max_us: AtomicU64::new(0),
            samples: RwLock::new(RingBuffer::default()),
        }
    }
}

/// Frame and system stats state (accessed only from the render thread)
struct FrameState {
    frame_start: Option<Instant>,
    last_cpu_measure: (Instant, u64),
    last_stats_refresh: Instant,
}

/// Global performance metrics collector
pub(crate) struct PerfMetrics {
    /// Whether performance monitoring is enabled
    pub enabled: AtomicBool,
    /// Per-operation statistics
    ops: RwLock<HashMap<&'static str, OpStats>>,
    /// Frame time ring buffer (microseconds)
    frame_times: RwLock<RingBuffer<u64>>,
    /// Frame and system stats state (single lock replaces 3 separate `RwLocks`)
    frame_state: RwLock<FrameState>,
    /// Total frames counted
    pub frame_count: AtomicU64,
    /// CPU usage percentage (0-100), `stored.to_f32()` bits
    cpu_usage: AtomicU32,
    /// Memory usage in bytes
    memory_bytes: AtomicU64,
}

impl Default for PerfMetrics {
    fn default() -> Self {
        let (cpu_ticks, mem_bytes) = read_proc_stat().unwrap_or((0, 0));

        Self {
            enabled: AtomicBool::new(false),
            ops: RwLock::new(HashMap::new()),
            frame_times: RwLock::new(RingBuffer::default()),
            frame_state: RwLock::new(FrameState {
                frame_start: None,
                last_cpu_measure: (Instant::now(), cpu_ticks),
                last_stats_refresh: Instant::now(),
            }),
            frame_count: AtomicU64::new(0),
            cpu_usage: AtomicU32::new(0),
            memory_bytes: AtomicU64::new(mem_bytes),
        }
    }
}

/// Read CPU ticks and memory from /proc/self/stat and /proc/self/statm
fn read_proc_stat() -> Option<(u64, u64)> {
    // Read CPU ticks from /proc/self/stat
    // Format: pid (comm) state ... utime stime ...
    // Fields 14 and 15 (0-indexed: 13, 14) are utime and stime
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let mut fields = stat.split_whitespace();
    let utime: u64 = fields.nth(13)?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    let cpu_ticks = utime + stime;

    // Read memory from /proc/self/statm (in pages)
    // First field is total program size, second is RSS
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let rss_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    let page_size = 4096u64; // Standard page size
    let mem_bytes = rss_pages * page_size;

    Some((cpu_ticks, mem_bytes))
}

pub(crate) static PERF: std::sync::LazyLock<PerfMetrics> = std::sync::LazyLock::new(PerfMetrics::default);

impl PerfMetrics {
    /// Record operation timing
    pub(crate) fn record_op(&self, name: &'static str, duration_us: u64) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        // Ensure the entry exists (write lock), then immediately release.
        // The OpStats fields are independently synchronized (atomics + inner RwLock),
        // so we re-acquire a cheaper read lock for the actual recording.
        {
            let mut ops = self.ops.write().unwrap_or_else(std::sync::PoisonError::into_inner);
            let _r = ops.entry(name).or_default();
        }
        let ops = self.ops.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(stats) = ops.get(name) {
            let _r = stats.count.fetch_add(1, Ordering::Relaxed);
            let _r = stats.total_us.fetch_add(duration_us, Ordering::Relaxed);
            let _r = stats.max_us.fetch_max(duration_us, Ordering::Relaxed);
            if let Ok(mut samples) = stats.samples.write() {
                samples.push(duration_us);
            }
        }
    }

    /// Start a new frame
    pub(crate) fn frame_start(&self) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        self.frame_state.write().unwrap_or_else(std::sync::PoisonError::into_inner).frame_start = Some(Instant::now());
    }

    /// End frame and record frame time
    pub(crate) fn frame_end(&self) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let frame_start = self.frame_state.read().unwrap_or_else(std::sync::PoisonError::into_inner).frame_start;
        if let Some(start) = frame_start {
            let frame_time = start.elapsed().as_micros().to_u64();
            self.frame_times.write().unwrap_or_else(std::sync::PoisonError::into_inner).push(frame_time);
            let _r = self.frame_count.fetch_add(1, Ordering::Relaxed);
        }

        // Check if stats need refresh (time-based, not frame-based)
        let last_refresh =
            self.frame_state.read().unwrap_or_else(std::sync::PoisonError::into_inner).last_stats_refresh;
        if last_refresh.elapsed().as_millis() >= u128::from(PERF_STATS_REFRESH_MS) {
            self.refresh_system_stats();
            self.frame_state.write().unwrap_or_else(std::sync::PoisonError::into_inner).last_stats_refresh =
                Instant::now();
        }
    }

    /// Refresh CPU and memory stats
    fn refresh_system_stats(&self) {
        if let Some((cpu_ticks, mem_bytes)) = read_proc_stat() {
            let cpu_pct = {
                let mut state = self.frame_state.write().unwrap_or_else(std::sync::PoisonError::into_inner);
                let now = Instant::now();
                let elapsed = now.duration_since(state.last_cpu_measure.0).as_secs_f32();

                let pct = if elapsed > 0.0 {
                    let tick_delta = cpu_ticks.saturating_sub(state.last_cpu_measure.1);
                    // Convert ticks to seconds (usually 100 ticks/sec on Linux)
                    let cpu_seconds = tick_delta.to_f32() / 100.0;
                    // CPU percentage = (cpu_time / wall_time) * 100
                    (cpu_seconds / elapsed) * 100.0
                } else {
                    0.0
                };

                state.last_cpu_measure = (now, cpu_ticks);
                pct
            };
            // Atomic stores don't need the lock
            self.cpu_usage.store(cpu_pct.to_bits(), Ordering::Relaxed);
            self.memory_bytes.store(mem_bytes, Ordering::Relaxed);
        }
    }

    /// Get snapshot of metrics for display
    pub(crate) fn snapshot(&self) -> PerfSnapshot {
        // Extract frame data and release lock before processing ops
        let frame_samples: Vec<f64> = {
            let frame_times = self.frame_times.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            frame_times.recent(40).iter().map(|&us| us.to_f64() / 1000.0).collect()
        };

        // Extract op data under lock, then process without holding it
        let raw_ops: Vec<(&'static str, u64, Vec<u64>)> = {
            let ops = self.ops.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            ops.iter()
                .map(|(name, stats)| {
                    let recent = stats
                        .samples
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .recent(SAMPLE_RING_SIZE);
                    (*name, stats.total_us.load(Ordering::Relaxed), recent)
                })
                .collect()
        };

        let mut op_snapshots: Vec<OpSnapshot> = raw_ops
            .iter()
            .map(|(name, total_us, recent)| {
                let count = recent.len();

                // Calculate mean
                let mean_us = if count > 0 { recent.iter().sum::<u64>().to_f64() / count.to_f64() } else { 0.0 };

                // Calculate standard deviation
                let std_us = if count > 1 {
                    let variance = recent
                        .iter()
                        .map(|&x| {
                            let diff = x.to_f64() - mean_us;
                            diff * diff
                        })
                        .sum::<f64>()
                        / (count - 1).to_f64();
                    variance.sqrt()
                } else {
                    0.0
                };

                OpSnapshot {
                    name,
                    total_ms: total_us.to_f64() / 1000.0,
                    mean_ms: mean_us / 1000.0,
                    std_ms: std_us / 1000.0,
                }
            })
            .collect();

        // Sort by total time descending (hotspots first)
        op_snapshots.sort_by(|a, b| b.total_ms.partial_cmp(&a.total_ms).unwrap_or(std::cmp::Ordering::Equal));

        let frame_avg_ms = if frame_samples.is_empty() {
            0.0
        } else {
            frame_samples.iter().sum::<f64>() / frame_samples.len().to_f64()
        };
        let frame_max_ms = frame_samples.iter().copied().fold(0.0, f64::max);

        PerfSnapshot {
            ops: op_snapshots,
            frame_times_ms: frame_samples,
            frame_avg_ms,
            frame_max_ms,
            cpu_usage: f32::from_bits(self.cpu_usage.load(Ordering::Relaxed)),
            memory_mb: self.memory_bytes.load(Ordering::Relaxed).to_f64() / (1024.0 * 1024.0),
        }
    }

    /// Reset all metrics
    pub(crate) fn reset(&self) {
        *self.ops.write().unwrap_or_else(std::sync::PoisonError::into_inner) = HashMap::new();
        *self.frame_times.write().unwrap_or_else(std::sync::PoisonError::into_inner) = RingBuffer::default();
        self.frame_count.store(0, Ordering::Relaxed);
    }

    /// Toggle monitoring on/off, returns new state
    pub(crate) fn toggle(&self) -> bool {
        let new_state = !self.enabled.load(Ordering::Relaxed);
        self.enabled.store(new_state, Ordering::Relaxed);
        if new_state {
            self.reset();
            // Do initial system stats refresh when enabling
            self.refresh_system_stats();
        }
        new_state
    }
}

/// Snapshot of operation statistics for display
#[derive(Clone)]
pub(crate) struct OpSnapshot {
    pub name: &'static str,
    pub total_ms: f64,
    pub mean_ms: f64,
    pub std_ms: f64,
}

/// Snapshot of all metrics for display
#[derive(Clone)]
pub(crate) struct PerfSnapshot {
    pub ops: Vec<OpSnapshot>,
    pub frame_times_ms: Vec<f64>,
    pub frame_avg_ms: f64,
    pub frame_max_ms: f64,
    pub cpu_usage: f32,
    pub memory_mb: f64,
}
