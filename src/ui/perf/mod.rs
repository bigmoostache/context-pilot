//! In-memory performance monitoring system.
//!
//! Provides low-overhead profiling with real-time stats collection.
//! Toggle with F12.

/// Performance overlay adapter (F12 panel) — renders from IR snapshot.
mod overlay;
/// One-shot HTML loop-profile report (`--measure N` artifact).
mod report;
/// F12 overlay stacked share-bars (loop substep mean/variance/max).
mod share_bars;
/// Platform-specific process CPU/memory sampling.
mod sys_stat;
/// Plain-text dump of the perf snapshot (Ctrl+R clipboard copy).
pub(crate) mod text;
pub(crate) use overlay::render_perf_overlay_from_ir;
use sys_stat::read_proc_stat;

use crate::infra::constants::PERF_STATS_REFRESH_MS;
use cp_base::cast::Safe as _;
use cp_base::cast::float_math;
use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

/// Number of recent samples for trend analysis / ring buffer size
const SAMPLE_RING_SIZE: usize = 64;

/// Bitmask for power-of-2 ring buffer wrapping (`SIZE - 1`).
const RING_MASK: usize = SAMPLE_RING_SIZE - 1;

/// Ring buffer for recent samples.
pub(crate) struct RingBuffer<T: Copy + Default> {
    /// Backing storage for ring data.
    data: Vec<T>,
    /// Next write position (wraps around).
    write_pos: usize,
    /// Number of valid entries (up to `SAMPLE_RING_SIZE`).
    len: usize,
}

impl<T: Copy + Default> Default for RingBuffer<T> {
    fn default() -> Self {
        Self { data: vec![T::default(); SAMPLE_RING_SIZE], write_pos: 0, len: 0 }
    }
}

impl<T: Copy + Default + Ord> RingBuffer<T> {
    /// Push a new value into the ring buffer.
    pub(crate) fn push(&mut self, value: T) {
        if let Some(slot) = self.data.get_mut(self.write_pos) {
            *slot = value;
        }
        self.write_pos = self.write_pos.saturating_add(1) & RING_MASK;
        if self.len < SAMPLE_RING_SIZE {
            self.len = self.len.saturating_add(1);
        }
    }

    /// Return the `count` most recent values.
    pub(crate) fn recent(&self, count: usize) -> Vec<T> {
        if self.len == 0 {
            return Vec::new();
        }
        let take = count.min(self.len);
        let mut result = Vec::with_capacity(take);
        let start = if self.len < SAMPLE_RING_SIZE { 0 } else { self.write_pos };
        for i in 0..take {
            let idx = start.saturating_add(self.len).saturating_sub(count).saturating_add(i) & RING_MASK;
            if let Some(&val) = self.data.get(idx) {
                result.push(val);
            }
        }
        result
    }
}

/// Single operation's accumulated statistics.
pub(crate) struct OpStats {
    /// Total invocation count
    pub count: AtomicU64,
    /// Total time in microseconds
    pub total_us: AtomicU64,
    /// Sum of squared sample times (µs²) — enables lifetime variance over ALL
    /// samples (not just the 64-entry ring): `var = sumsq/n - mean²`.
    pub sum_sq_us: AtomicU64,
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
            sum_sq_us: AtomicU64::new(0),
            max_us: AtomicU64::new(0),
            samples: RwLock::new(RingBuffer::default()),
        }
    }
}

/// Frame and system stats state (accessed only from the render thread).
pub(crate) struct FrameState {
    /// Timestamp of the current frame start (if any).
    frame_start: Option<Instant>,
    /// Last CPU measurement: (timestamp, `cpu_ticks`).
    last_cpu_measure: (Instant, u64),
    /// Last time system stats were refreshed.
    last_stats_refresh: Instant,
}

/// Global performance metrics collector.
pub(crate) struct PerfMetrics {
    /// Whether performance monitoring is enabled
    pub enabled: AtomicBool,
    /// Per-operation statistics
    pub ops: RwLock<HashMap<&'static str, OpStats>>,
    /// Frame time ring buffer (microseconds)
    pub frame_times: RwLock<RingBuffer<u64>>,
    /// Frame and system stats state (single lock replaces 3 separate `RwLocks`)
    pub frame_state: RwLock<FrameState>,
    /// Total frames counted
    pub frame_count: AtomicU64,
    /// CPU usage percentage (0-100), `stored.to_f32()` bits
    pub cpu_usage: AtomicU32,
    /// Memory usage in bytes
    pub memory_bytes: AtomicU64,
    /// Number of open file descriptors
    pub open_fds: AtomicU32,
    /// Soft rlimit for NOFILE (set once at init, does not change)
    pub fd_limit_soft: AtomicU64,
    /// Total main-loop iterations since boot (drives `--measure N` dump).
    pub loop_count: AtomicU64,
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
            open_fds: AtomicU32::new(0),
            fd_limit_soft: AtomicU64::new(rlimit::getrlimit(rlimit::Resource::NOFILE).map_or(0, |(soft, _)| soft)),
            loop_count: AtomicU64::new(0),
        }
    }
}

/// Global performance metrics instance.
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
            let _r1 = stats.total_us.fetch_add(duration_us, Ordering::Relaxed);
            // Saturating square keeps lifetime variance well-defined even on a
            // pathological spike; realistic loop substeps never approach u64 range.
            let _rsq = stats.sum_sq_us.fetch_add(duration_us.saturating_mul(duration_us), Ordering::Relaxed);
            let _r2 = stats.max_us.fetch_max(duration_us, Ordering::Relaxed);
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
                let mut frame_st = self.frame_state.write().unwrap_or_else(std::sync::PoisonError::into_inner);
                let now = Instant::now();
                let elapsed = now.duration_since(frame_st.last_cpu_measure.0).as_secs_f32();

                let pct = if elapsed > 0.0 {
                    let tick_delta = cpu_ticks.saturating_sub(frame_st.last_cpu_measure.1);
                    // Convert ticks to seconds (usually 100 ticks/sec on Linux)
                    let cpu_seconds = float_math::div_u64(tick_delta, 100.0).to_f32();
                    // CPU percentage = (cpu_time / wall_time) * 100
                    float_math::percent(cpu_seconds.to_f64(), elapsed.to_f64()).to_f32()
                } else {
                    0.0
                };

                frame_st.last_cpu_measure = (now, cpu_ticks);
                pct
            };
            // Atomic stores don't need the lock
            self.cpu_usage.store(cpu_pct.to_bits(), Ordering::Relaxed);
            self.memory_bytes.store(mem_bytes, Ordering::Relaxed);
        }

        // FD count (works on macOS and Linux via /dev/fd)
        let fd_count = std::fs::read_dir("/dev/fd").map_or(0, Iterator::count);
        self.open_fds.store(fd_count.to_u32(), Ordering::Relaxed);
    }

    /// Get snapshot of metrics for display
    pub(crate) fn snapshot(&self) -> PerfSnapshot {
        /// Type alias for raw operation data extracted under lock:
        /// `(name, total_us, count, sum_sq_us, max_us, recent_samples)`.
        type RawOp = (&'static str, u64, u64, u64, u64, Vec<u64>);

        // Extract frame data and release lock before processing ops
        let frame_samples: Vec<f64> = {
            let frame_times = self.frame_times.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            frame_times.recent(40).iter().map(|&us| float_math::div_u64(us, 1_000.0f64)).collect()
        };

        // Extract op data under lock, then process without holding it
        let raw_ops: Vec<RawOp> = {
            let ops = self.ops.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            ops.iter()
                .map(|(name, stats)| {
                    let recent = stats
                        .samples
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .recent(SAMPLE_RING_SIZE);
                    (
                        *name,
                        stats.total_us.load(Ordering::Relaxed),
                        stats.count.load(Ordering::Relaxed),
                        stats.sum_sq_us.load(Ordering::Relaxed),
                        stats.max_us.load(Ordering::Relaxed),
                        recent,
                    )
                })
                .collect()
        };

        let mut op_snapshots: Vec<OpSnapshot> = raw_ops
            .iter()
            .map(|entry| {
                compute_op_snapshot(&OpRaw {
                    name: entry.0,
                    total_us: entry.1,
                    count: entry.2,
                    sum_sq_us: entry.3,
                    max_us: entry.4,
                    recent: &entry.5,
                })
            })
            .collect();

        // Sort by total time descending (hotspots first)
        op_snapshots.sort_by(|a, b| b.total_ms.partial_cmp(&a.total_ms).unwrap_or(std::cmp::Ordering::Equal));

        let frame_avg_ms = if frame_samples.is_empty() { 0.0f64 } else { float_math::mean(&frame_samples) };
        let frame_max_ms = frame_samples.iter().copied().fold(0.0f64, f64::max);

        PerfSnapshot {
            ops: op_snapshots,
            frame_times_ms: frame_samples,
            frame_avg_ms,
            frame_max_ms,
            cpu_usage: f32::from_bits(self.cpu_usage.load(Ordering::Relaxed)),
            memory_mb: float_math::div_u64(self.memory_bytes.load(Ordering::Relaxed), 1_048_576.0f64),
            open_fds: self.open_fds.load(Ordering::Relaxed),
            fd_limit_soft: self.fd_limit_soft.load(Ordering::Relaxed),
            loop_count: self.loop_count.load(Ordering::Relaxed),
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

    /// `--measure` boot hook: force-enable monitoring when `CP_MEASURE_LOOPS`
    /// is set, bypassing the F12 toggle so substep timings accumulate from the
    /// first loop. No-op otherwise.
    pub(crate) fn enable_if_measuring(&self) {
        if Self::measure_target().is_none() {
            return;
        }
        self.enabled.store(true, Ordering::Relaxed);
        self.refresh_system_stats();
    }

    /// Increment the main-loop iteration counter and return the new count.
    /// Called once per `App::run` loop iteration.
    pub(crate) fn loop_tick(&self) -> u64 {
        self.loop_count.fetch_add(1, Ordering::Relaxed).saturating_add(1)
    }

    /// `--measure N` target loop count, parsed once from `CP_MEASURE_LOOPS`.
    /// `None` when unset/invalid/zero — measurement dump is disabled.
    pub(crate) fn measure_target() -> Option<u64> {
        static TARGET: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
        *TARGET.get_or_init(|| {
            std::env::var("CP_MEASURE_LOOPS").ok().and_then(|v| v.trim().parse::<u64>().ok()).filter(|&n| n > 0)
        })
    }

    /// Per-iteration measurement hook for `--measure N`. Ticks the loop counter
    /// and, exactly when it reaches the target, writes the HTML report. Returns
    /// `true` once (at loop N) so the caller can exit the process cleanly;
    /// otherwise `false`. A no-op when measurement is disabled.
    pub(crate) fn tick_measure(&self) -> bool {
        let Some(target) = Self::measure_target() else {
            return false;
        };
        if self.loop_tick() != target {
            return false;
        }
        match report::write_report(&self.snapshot()) {
            Ok(path) => log::info!("[measure] wrote loop profile over {target} iterations -> {path}"),
            Err(e) => log::error!("[measure] failed to write loop profile: {e}"),
        }
        true
    }
}

/// One operation's raw lifetime counters plus its recent-sample ring, as
/// extracted under lock (bundled to stay under the argument cap).
struct OpRaw<'snap> {
    /// Operation name.
    name: &'static str,
    /// Cumulative time (µs).
    total_us: u64,
    /// Lifetime sample count.
    count: u64,
    /// Sum of squared samples (µs²).
    sum_sq_us: u64,
    /// Lifetime maximum sample (µs).
    max_us: u64,
    /// Recent-sample ring contents (µs).
    recent: &'snap [u64],
}

/// Compute one operation's display snapshot from its accumulated lifetime
/// counters (`total_us`, `count`, `sum_sq_us`, `max_us`) plus the recent-sample
/// ring. Lifetime stats (mean/variance/max, in µs) are exact over ALL samples
/// since reset; the ring only drives the legacy `std_ms` column. Population
/// variance: `E[x²] − E[x]²`, clamped at 0 against float error.
fn compute_op_snapshot(raw: &OpRaw<'_>) -> OpSnapshot {
    let OpRaw { name, total_us, count, sum_sq_us, max_us, recent } = *raw;
    let recent_n = recent.len();

    // Legacy windowed std (ms) over the recent ring — kept for the op table.
    let recent_mean_us =
        if recent_n > 0 { float_math::div(recent.iter().sum::<u64>().to_f64(), recent_n.to_f64()) } else { 0.0f64 };
    let std_us = if recent_n > 1 {
        let variance = float_math::div(
            float_math::sum_iter(recent.iter().map(|&x| {
                let diff = float_math::sub(x.to_f64(), recent_mean_us);
                float_math::mul(diff, diff)
            })),
            recent_n.saturating_sub(1).to_f64(),
        );
        variance.sqrt()
    } else {
        0.0f64
    };

    // Lifetime mean + population variance over every recorded sample.
    let mean_us = if count > 0 { float_math::div(total_us.to_f64(), count.to_f64()) } else { 0.0f64 };
    let variance_us2 = if count > 0 {
        let mean_sq = float_math::div(sum_sq_us.to_f64(), count.to_f64());
        float_math::sub(mean_sq, float_math::mul(mean_us, mean_us)).max(0.0f64)
    } else {
        0.0f64
    };

    OpSnapshot {
        name,
        total_ms: float_math::div_u64(total_us, 1000.0f64),
        mean_ms: float_math::div(mean_us, 1000.0f64),
        std_ms: float_math::div(std_us, 1000.0f64),
        count,
        mean_us,
        variance_us2,
        max_us: max_us.to_f64(),
    }
}

/// Snapshot of operation statistics for display.
#[derive(Clone)]
pub(crate) struct OpSnapshot {
    /// Operation name (static string reference).
    pub name: &'static str,
    /// Total cumulative time in milliseconds.
    pub total_ms: f64,
    /// Mean execution time in milliseconds.
    pub mean_ms: f64,
    /// Standard deviation of execution time in milliseconds (recent ring).
    pub std_ms: f64,
    /// Lifetime sample count.
    pub count: u64,
    /// Lifetime mean execution time in microseconds.
    pub mean_us: f64,
    /// Lifetime population variance in microseconds².
    pub variance_us2: f64,
    /// Lifetime maximum single execution time in microseconds.
    pub max_us: f64,
}

/// Snapshot of all metrics for display.
#[derive(Clone)]
pub(crate) struct PerfSnapshot {
    /// Per-operation snapshots sorted by total time descending.
    pub ops: Vec<OpSnapshot>,
    /// Recent frame times in milliseconds.
    pub frame_times_ms: Vec<f64>,
    /// Average frame time in milliseconds.
    pub frame_avg_ms: f64,
    /// Maximum frame time in milliseconds.
    pub frame_max_ms: f64,
    /// CPU usage percentage (0-100).
    pub cpu_usage: f32,
    /// Memory usage in megabytes.
    pub memory_mb: f64,
    /// Number of open file descriptors.
    pub open_fds: u32,
    /// Soft rlimit for NOFILE.
    pub fd_limit_soft: u64,
    /// Total main-loop iterations since boot.
    pub loop_count: u64,
}
