use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::constants::{
    TYPEWRITER_DEFAULT_DELAY_MS, TYPEWRITER_MAX_DELAY_MS, TYPEWRITER_MIN_DELAY_MS, TYPEWRITER_MOVING_AVG_SIZE,
};

pub struct TypewriterBuffer {
    pub pending_chars: VecDeque<char>,
    chunk_intervals: VecDeque<Duration>,
    chunk_sizes: VecDeque<usize>,
    last_chunk_time: Option<Instant>,
    last_char_time: Instant,
    chars_per_ms: f64,
    stream_done: bool,
}

impl TypewriterBuffer {
    pub fn new() -> Self {
        Self {
            pending_chars: VecDeque::new(),
            chunk_intervals: VecDeque::new(),
            chunk_sizes: VecDeque::new(),
            last_chunk_time: None,
            last_char_time: Instant::now(),
            chars_per_ms: 1.0 / TYPEWRITER_DEFAULT_DELAY_MS,
            stream_done: false,
        }
    }

    pub fn reset(&mut self) {
        self.pending_chars.clear();
        self.chunk_intervals.clear();
        self.chunk_sizes.clear();
        self.last_chunk_time = None;
        self.last_char_time = Instant::now();
        self.chars_per_ms = 1.0 / TYPEWRITER_DEFAULT_DELAY_MS;
        self.stream_done = false;
    }

    pub fn add_chunk(&mut self, text: &str) {
        let now = Instant::now();

        if let Some(last_time) = self.last_chunk_time {
            let interval = now.duration_since(last_time);
            if self.chunk_intervals.len() >= TYPEWRITER_MOVING_AVG_SIZE {
                self.chunk_intervals.pop_front();
            }
            self.chunk_intervals.push_back(interval);
        }
        self.last_chunk_time = Some(now);

        let char_count = text.chars().count();
        if self.chunk_sizes.len() >= TYPEWRITER_MOVING_AVG_SIZE {
            self.chunk_sizes.pop_front();
        }
        self.chunk_sizes.push_back(char_count);

        for c in text.chars() {
            self.pending_chars.push_back(c);
        }

        self.recalculate_speed();
    }

    fn recalculate_speed(&mut self) {
        if self.chunk_intervals.is_empty() || self.chunk_sizes.is_empty() {
            return;
        }

        let total_interval_ms: f64 = self.chunk_intervals.iter().map(|d| d.as_secs_f64() * 1000.0).sum();
        let avg_interval_ms = total_interval_ms / self.chunk_intervals.len() as f64;

        let total_chars: usize = self.chunk_sizes.iter().sum();
        let avg_chunk_size = total_chars as f64 / self.chunk_sizes.len() as f64;

        if avg_interval_ms > 0.0 && avg_chunk_size > 0.0 {
            let calculated_delay = avg_interval_ms / avg_chunk_size;
            let clamped_delay = calculated_delay.clamp(TYPEWRITER_MIN_DELAY_MS, TYPEWRITER_MAX_DELAY_MS);
            self.chars_per_ms = 1.0 / clamped_delay;
        }
    }

    pub fn mark_done(&mut self) {
        self.stream_done = true;
    }

    pub fn take_chars(&mut self) -> Option<String> {
        if self.pending_chars.is_empty() {
            return None;
        }

        let now = Instant::now();
        let elapsed_ms = now.duration_since(self.last_char_time).as_secs_f64() * 1000.0;
        let chars_to_release = (elapsed_ms * self.chars_per_ms).floor() as usize;

        if chars_to_release == 0 {
            return None;
        }

        let chars_to_take = if self.stream_done {
            chars_to_release.max(2).min(self.pending_chars.len())
        } else {
            chars_to_release.min(self.pending_chars.len())
        };

        if chars_to_take == 0 {
            return None;
        }

        self.last_char_time = now;

        let mut result = String::with_capacity(chars_to_take);
        for _ in 0..chars_to_take {
            if let Some(c) = self.pending_chars.pop_front() {
                result.push(c);
            }
        }

        Some(result)
    }
}
