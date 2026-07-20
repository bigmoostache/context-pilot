//! Progress bar animation: smooth fill, color crossfade, and streaming pulse.
//!
//! Provides a `TokenBarAnimator` that interpolates bar segment percentages
//! and token counts toward their targets using ease-out cubic easing.
//! During streaming, a subtle pulse modulates fill brightness.
//!
//! The animator lives as a module-level `LazyLock<Mutex<…>>` — purely
//! visual state with no persistence requirements.

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use cp_render::frame::TokenBar;
use ratatui::style::Color;

use cp_base::cast::Safe as _;
use cp_base::cast::float_math;

/// Default transition duration (ease-out cubic).
const TRANSITION_DURATION: Duration = Duration::from_secs(1);

/// Pulse period during streaming (one full sine cycle).
const PULSE_PERIOD: Duration = Duration::from_secs(2);

/// Pulse amplitude: ±12% brightness swing.
const PULSE_AMPLITUDE: f64 = 0.12;

/// Global animator instance.
static BAR_ANIMATOR: LazyLock<Mutex<TokenBarAnimator>> = LazyLock::new(|| Mutex::new(TokenBarAnimator::new()));

// ── BarTransition ────────────────────────────────────────────────────

/// Animates a single `f64` value from `from` to `to` with ease-out cubic.
struct BarTransition {
    /// Starting value of this transition.
    from: f64,
    /// Target value.
    to: f64,
    /// When the transition began.
    start: Instant,
    /// How long the transition lasts.
    duration: Duration,
}

impl BarTransition {
    /// Create a new transition at the given resting value.
    fn new(value: f64) -> Self {
        Self { from: value, to: value, start: Instant::now(), duration: TRANSITION_DURATION }
    }

    /// Update the target. If it changed, start a new transition from the
    /// current interpolated position.
    fn update(&mut self, target: f64) {
        if float_math::abs_diff(self.to, target) < 0.01f64 {
            return; // no meaningful change
        }
        self.from = self.current();
        self.to = target;
        self.start = Instant::now();
        self.duration = TRANSITION_DURATION;
    }

    /// Current interpolated value (ease-out cubic).
    fn current(&self) -> f64 {
        let elapsed = self.start.elapsed().as_secs_f64();
        let total = self.duration.as_secs_f64();
        if total <= 0.0f64 {
            return self.to;
        }
        let t = float_math::div(elapsed, total).clamp(0.0, 1.0);
        // Ease-out cubic: fast start, smooth deceleration
        let eased = float_math::ease_out_cubic(t);
        float_math::lerp(self.from, self.to, eased)
    }
}

// ── TokenBarAnimator ─────────────────────────────────────────────────

/// Orchestrates animations for all progress bar elements.
struct TokenBarAnimator {
    /// Hit percentage transition.
    hit_pct: BarTransition,
    /// Miss percentage transition.
    miss_pct: BarTransition,
    /// Token count transition.
    used_tokens: BarTransition,
    /// Whether the LLM is currently streaming.
    streaming: bool,
    /// When streaming started (for pulse phase calculation).
    stream_start: Instant,
}

impl TokenBarAnimator {
    /// Create a new animator with all values at zero.
    fn new() -> Self {
        let now = Instant::now();
        Self {
            hit_pct: BarTransition::new(0.0),
            miss_pct: BarTransition::new(0.0),
            used_tokens: BarTransition::new(0.0),
            streaming: false,
            stream_start: now,
        }
    }

    /// Feed new target values from the IR snapshot. Call once per render tick.
    fn update(&mut self, token_bar: &TokenBar) {
        let hit = f64::from(token_bar.segments.first().map_or(0, |s| s.percent));
        let miss = f64::from(token_bar.segments.get(1).map_or(0, |s| s.percent));
        let used = f64::from(token_bar.used);

        self.hit_pct.update(hit);
        self.miss_pct.update(miss);
        self.used_tokens.update(used);

        if token_bar.streaming && !self.streaming {
            self.stream_start = Instant::now();
        }
        self.streaming = token_bar.streaming;
    }

    /// Pulse phase (0.0–1.0) for streaming glow, or `None` if not streaming.
    fn pulse_brightness(&self) -> Option<f64> {
        if !self.streaming {
            return None;
        }
        let elapsed = self.stream_start.elapsed().as_secs_f64();
        let period = PULSE_PERIOD.as_secs_f64();
        let phase = float_math::fract_phase(elapsed, period);
        // Sine wave: 1.0 ± PULSE_AMPLITUDE
        Some(float_math::pulse(PULSE_AMPLITUDE, phase))
    }
}

// ── Public API ───────────────────────────────────────────────────────

/// Animated bar snapshot returned to the renderer.
pub(crate) struct AnimatedBar {
    /// Interpolated hit percentage (0–100).
    pub hit_pct: f64,
    /// Interpolated miss percentage (0–100).
    pub miss_pct: f64,
    /// Interpolated token count.
    pub used_tokens: u32,
    /// Streaming pulse brightness multiplier, or `None` if not streaming.
    pub pulse_brightness: Option<f64>,
}

/// Update the global animator with new targets and return interpolated values.
///
/// Called once per render tick from `render_token_bar_box`.
pub(crate) fn tick(token_bar: &TokenBar) -> AnimatedBar {
    let mut anim = BAR_ANIMATOR.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    anim.update(token_bar);
    AnimatedBar {
        hit_pct: anim.hit_pct.current(),
        miss_pct: anim.miss_pct.current(),
        used_tokens: anim.used_tokens.current().round().to_u32(),
        pulse_brightness: anim.pulse_brightness(),
    }
}

// ── Color helpers ────────────────────────────────────────────────────

/// Linearly interpolate between two RGB colors.
///
/// `t` = 0.0 → returns `a`, `t` = 1.0 → returns `b`.
/// Falls back to `a` or `b` if either is not `Color::Rgb`.
pub(crate) fn lerp_color(from: Color, to: Color, progress: f64) -> Color {
    let clamped = progress.clamp(0.0, 1.0);
    if let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (from, to) {
        Color::Rgb(lerp_u8(r1, r2, clamped), lerp_u8(g1, g2, clamped), lerp_u8(b1, b2, clamped))
    } else if progress < 0.5 {
        from
    } else {
        to
    }
}

/// Apply a brightness multiplier to an RGB color.
///
/// `brightness` = 1.0 → unchanged, >1.0 → brighter, <1.0 → dimmer.
/// Clamps each channel to 0–255.
pub(crate) fn pulse_color(color: Color, brightness: f64) -> Color {
    if let Color::Rgb(red, green, blue) = color {
        Color::Rgb(scale_u8(red, brightness), scale_u8(green, brightness), scale_u8(blue, brightness))
    } else {
        color
    }
}

/// Interpolate a `u8` channel between two values.
fn lerp_u8(from: u8, to: u8, progress: f64) -> u8 {
    let result = float_math::lerp(f64::from(from), f64::from(to), progress);
    result.round().clamp(0.0, 255.0).to_u8()
}

/// Scale a `u8` channel by a brightness factor.
fn scale_u8(value: u8, factor: f64) -> u8 {
    float_math::mul(f64::from(value), factor).round().clamp(0.0, 255.0).to_u8()
}
