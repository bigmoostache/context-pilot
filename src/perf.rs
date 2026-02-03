//! In-memory performance monitoring system.
//!
//! Provides low-overhead profiling with real-time stats collection.
//! Toggle with F12.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::Instant;

/// Number of recent samples to keep for trend analysis
const SAMPLE_RING_SIZE: usize = 64;

/// Frame budget for 60fps (milliseconds)
pub const FRAME_BUDGET_60FPS: f64 = 16.67;

/// Frame budget for 30fps (milliseconds)
pub const FRAME_BUDGET_30FPS: f64 = 33.33;

/// Ring buffer for recent samples
pub struct RingBuffer<T: Copy + Default> {
    data: Vec<T>,
    write_pos: usize,
    len: usize,
}

impl<T: Copy + Default> Default for RingBuffer<T> {
    fn default() -> Self {
        Self {
            data: vec![T::default(); SAMPLE_RING_SIZE],
            write_pos: 0,
            len: 0,
        }
    }
}

impl<T: Copy + Default + Ord> RingBuffer<T> {
    pub fn push(&mut self, value: T) {
        self.data[self.write_pos] = value;
        self.write_pos = (self.write_pos + 1) % SAMPLE_RING_SIZE;
        if self.len < SAMPLE_RING_SIZE {
            self.len += 1;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data[..self.len].iter()
    }

    pub fn recent(&self, count: usize) -> Vec<T> {
        if self.len == 0 {
            return Vec::new();
        }
        let count = count.min(self.len);
        let mut result = Vec::with_capacity(count);
        let start = if self.len < SAMPLE_RING_SIZE {
            0
        } else {
            self.write_pos
        };
        for i in 0..count {
            let idx = (start + self.len - count + i) % SAMPLE_RING_SIZE;
            result.push(self.data[idx]);
        }
        result
    }

    /// Calculate p95 from recent samples
    pub fn percentile_95(&self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let mut sorted: Vec<T> = self.iter().copied().collect();
        sorted.sort();
        let idx = (self.len * 95) / 100;
        Some(sorted[idx.min(self.len - 1)])
    }
}

/// Single operation's accumulated statistics
pub struct OpStats {
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

/// Global performance metrics collector
pub struct PerfMetrics {
    /// Whether performance monitoring is enabled
    pub enabled: AtomicBool,
    /// Per-operation statistics
    ops: RwLock<HashMap<&'static str, OpStats>>,
    /// Frame time ring buffer (microseconds)
    frame_times: RwLock<RingBuffer<u64>>,
    /// Frame start time
    frame_start: RwLock<Option<Instant>>,
    /// Total frames counted
    pub frame_count: AtomicU64,
}

impl Default for PerfMetrics {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            ops: RwLock::new(HashMap::new()),
            frame_times: RwLock::new(RingBuffer::default()),
            frame_start: RwLock::new(None),
            frame_count: AtomicU64::new(0),
        }
    }
}

lazy_static::lazy_static! {
    pub static ref PERF: PerfMetrics = PerfMetrics::default();
}

impl PerfMetrics {
    /// Record operation timing
    pub fn record_op(&self, name: &'static str, duration_us: u64) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }

        let mut ops = self.ops.write().unwrap();
        let stats = ops.entry(name).or_default();
        stats.count.fetch_add(1, Ordering::Relaxed);
        stats.total_us.fetch_add(duration_us, Ordering::Relaxed);
        stats.max_us.fetch_max(duration_us, Ordering::Relaxed);
        if let Ok(mut samples) = stats.samples.write() {
            samples.push(duration_us);
        }
    }

    /// Start a new frame
    pub fn frame_start(&self) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        *self.frame_start.write().unwrap() = Some(Instant::now());
    }

    /// End frame and record frame time
    pub fn frame_end(&self) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        if let Some(start) = self.frame_start.read().unwrap().as_ref() {
            let frame_time = start.elapsed().as_micros() as u64;
            self.frame_times.write().unwrap().push(frame_time);
            self.frame_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get snapshot of metrics for display
    pub fn snapshot(&self) -> PerfSnapshot {
        let ops = self.ops.read().unwrap();
        let frame_times = self.frame_times.read().unwrap();

        let mut op_snapshots: Vec<OpSnapshot> = ops
            .iter()
            .map(|(name, stats)| {
                let samples = stats.samples.read().unwrap();
                OpSnapshot {
                    name,
                    count: stats.count.load(Ordering::Relaxed),
                    total_ms: stats.total_us.load(Ordering::Relaxed) as f64 / 1000.0,
                    max_ms: stats.max_us.load(Ordering::Relaxed) as f64 / 1000.0,
                    p95_ms: samples.percentile_95().map(|us| us as f64 / 1000.0),
                }
            })
            .collect();

        // Sort by total time descending (hotspots first)
        op_snapshots.sort_by(|a, b| b.total_ms.partial_cmp(&a.total_ms).unwrap_or(std::cmp::Ordering::Equal));

        let frame_samples: Vec<f64> = frame_times
            .recent(40)
            .iter()
            .map(|&us| us as f64 / 1000.0)
            .collect();

        let frame_avg_ms = if frame_samples.is_empty() {
            0.0
        } else {
            frame_samples.iter().sum::<f64>() / frame_samples.len() as f64
        };

        PerfSnapshot {
            ops: op_snapshots,
            frame_times_ms: frame_samples.clone(),
            frame_avg_ms,
            frame_max_ms: frame_samples.iter().cloned().fold(0.0, f64::max),
            frame_p95_ms: frame_times
                .percentile_95()
                .map(|us| us as f64 / 1000.0)
                .unwrap_or(0.0),
            frame_count: self.frame_count.load(Ordering::Relaxed),
        }
    }

    /// Reset all metrics
    pub fn reset(&self) {
        *self.ops.write().unwrap() = HashMap::new();
        *self.frame_times.write().unwrap() = RingBuffer::default();
        self.frame_count.store(0, Ordering::Relaxed);
    }

    /// Toggle monitoring on/off, returns new state
    pub fn toggle(&self) -> bool {
        let new_state = !self.enabled.load(Ordering::Relaxed);
        self.enabled.store(new_state, Ordering::Relaxed);
        if new_state {
            self.reset();
        }
        new_state
    }
}

/// Snapshot of operation statistics for display
#[derive(Clone)]
#[allow(dead_code)]
pub struct OpSnapshot {
    pub name: &'static str,
    pub count: u64,
    pub total_ms: f64,
    pub max_ms: f64,
    pub p95_ms: Option<f64>,
}

/// Snapshot of all metrics for display
#[derive(Clone)]
#[allow(dead_code)]
pub struct PerfSnapshot {
    pub ops: Vec<OpSnapshot>,
    pub frame_times_ms: Vec<f64>,
    pub frame_avg_ms: f64,
    pub frame_max_ms: f64,
    pub frame_p95_ms: f64,
    pub frame_count: u64,
}
