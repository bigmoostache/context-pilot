//! Safe numeric casting helpers.
//!
//! Replace raw `as` casts that trigger `clippy::cast_possible_truncation`
//! and `clippy::cast_sign_loss`. All conversions use saturating semantics —
//! values that don't fit clamp to the target type's MIN/MAX.
//!
//! Usage: `use cp_base::cast::Safe;` then `value.to_u16()`, etc.

/// Trait for safe saturating casts between numeric types.
pub trait Safe {
    /// Saturating cast to `u8` — clamps to `0..=255`.
    fn to_u8(self) -> u8;
    /// Saturating cast to `u16` — clamps to `0..=65535`.
    fn to_u16(self) -> u16;
    /// Saturating cast to `u32`.
    fn to_u32(self) -> u32;
    /// Saturating cast to `u64`.
    fn to_u64(self) -> u64;
    /// Saturating cast to `usize`.
    fn to_usize(self) -> usize;
    /// Saturating cast to `i32` — clamps to `i32::MIN..=i32::MAX`.
    fn to_i32(self) -> i32;
    /// Saturating cast to `i64`.
    fn to_i64(self) -> i64;
    /// Lossy cast to `f32` (may lose precision for large integers).
    fn to_f32(self) -> f32;
    /// Lossy cast to `f64` (may lose precision for very large integers).
    fn to_f64(self) -> f64;
}

// ── Integer helpers ──────────────────────────────────────────────────

/// Common body for unsigned integer → integer conversions via `TryInto`.
macro_rules! unsigned_int_methods {
    () => {
        #[inline]
        fn to_u8(self) -> u8 {
            self.try_into().unwrap_or(u8::MAX)
        }
        #[inline]
        fn to_u16(self) -> u16 {
            self.try_into().unwrap_or(u16::MAX)
        }
        #[inline]
        fn to_u32(self) -> u32 {
            self.try_into().unwrap_or(u32::MAX)
        }
        #[inline]
        fn to_u64(self) -> u64 {
            self.try_into().unwrap_or(u64::MAX)
        }
        #[inline]
        fn to_usize(self) -> usize {
            self.try_into().unwrap_or(usize::MAX)
        }
        #[inline]
        fn to_i32(self) -> i32 {
            self.try_into().unwrap_or(i32::MAX)
        }
        #[inline]
        fn to_i64(self) -> i64 {
            self.try_into().unwrap_or(i64::MAX)
        }
    };
}

/// Common body for signed integer → integer conversions via `TryInto`.
macro_rules! signed_int_methods {
    () => {
        #[inline]
        fn to_u8(self) -> u8 {
            self.try_into().unwrap_or(0)
        }
        #[inline]
        fn to_u16(self) -> u16 {
            self.try_into().unwrap_or(0)
        }
        #[inline]
        fn to_u32(self) -> u32 {
            self.try_into().unwrap_or(0)
        }
        #[inline]
        fn to_u64(self) -> u64 {
            self.try_into().unwrap_or(0)
        }
        #[inline]
        fn to_usize(self) -> usize {
            self.try_into().unwrap_or(0)
        }
        #[inline]
        fn to_i32(self) -> i32 {
            self.try_into().unwrap_or(if self < 0 { i32::MIN } else { i32::MAX })
        }
        #[inline]
        fn to_i64(self) -> i64 {
            self.try_into().unwrap_or(if self < 0 { i64::MIN } else { i64::MAX })
        }
    };
}

// ── u16: lossless to both f32 and f64 ───────────────────────────────

impl Safe for u16 {
    unsigned_int_methods!();
    #[inline]
    fn to_f32(self) -> f32 {
        f32::from(self)
    }
    #[inline]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

// ── u32: lossless to f64, lossy to f32 ──────────────────────────────

impl Safe for u32 {
    unsigned_int_methods!();
    #[inline]
    #[expect(clippy::cast_precision_loss, reason = "u32→f32: 24-bit mantissa cannot represent all 32-bit values")]
    fn to_f32(self) -> f32 {
        self as f32
    }
    #[inline]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

// ── u64: lossy to both f32 and f64 ──────────────────────────────────

impl Safe for u64 {
    unsigned_int_methods!();
    #[inline]
    #[expect(clippy::cast_precision_loss, reason = "u64→f32: 24-bit mantissa cannot represent all 64-bit values")]
    fn to_f32(self) -> f32 {
        self as f32
    }
    #[inline]
    #[expect(clippy::cast_precision_loss, reason = "u64→f64: 53-bit mantissa cannot represent all 64-bit values")]
    fn to_f64(self) -> f64 {
        self as f64
    }
}

// ── u128: lossy to both f32 and f64 ─────────────────────────────────

impl Safe for u128 {
    unsigned_int_methods!();
    #[inline]
    #[expect(clippy::cast_precision_loss, reason = "u128→f32: 24-bit mantissa cannot represent all 128-bit values")]
    fn to_f32(self) -> f32 {
        self as f32
    }
    #[inline]
    #[expect(clippy::cast_precision_loss, reason = "u128→f64: 53-bit mantissa cannot represent all 128-bit values")]
    fn to_f64(self) -> f64 {
        self as f64
    }
}

// ── usize: lossy to both f32 and f64 ────────────────────────────────

impl Safe for usize {
    unsigned_int_methods!();
    #[inline]
    #[expect(
        clippy::cast_precision_loss,
        reason = "usize→f32: 24-bit mantissa cannot represent all pointer-width values"
    )]
    fn to_f32(self) -> f32 {
        self as f32
    }
    #[inline]
    #[expect(
        clippy::cast_precision_loss,
        reason = "usize→f64: 53-bit mantissa cannot represent all pointer-width values"
    )]
    fn to_f64(self) -> f64 {
        self as f64
    }
}

// ── i32: lossless to f64, lossy to f32 ──────────────────────────────

impl Safe for i32 {
    signed_int_methods!();
    #[inline]
    #[expect(
        clippy::cast_precision_loss,
        reason = "i32→f32: 24-bit mantissa cannot represent all 32-bit signed values"
    )]
    fn to_f32(self) -> f32 {
        self as f32
    }
    #[inline]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

// ── i64: lossy to both f32 and f64 ──────────────────────────────────

impl Safe for i64 {
    signed_int_methods!();
    #[inline]
    #[expect(
        clippy::cast_precision_loss,
        reason = "i64→f32: 24-bit mantissa cannot represent all 64-bit signed values"
    )]
    fn to_f32(self) -> f32 {
        self as f32
    }
    #[inline]
    #[expect(
        clippy::cast_precision_loss,
        reason = "i64→f64: 53-bit mantissa cannot represent all 64-bit signed values"
    )]
    fn to_f64(self) -> f64 {
        self as f64
    }
}

// ── isize: lossy to both f32 and f64 ────────────────────────────────

impl Safe for isize {
    signed_int_methods!();
    #[inline]
    #[expect(
        clippy::cast_precision_loss,
        reason = "isize→f32: 24-bit mantissa cannot represent all pointer-width signed values"
    )]
    fn to_f32(self) -> f32 {
        self as f32
    }
    #[inline]
    #[expect(
        clippy::cast_precision_loss,
        reason = "isize→f64: 53-bit mantissa cannot represent all pointer-width signed values"
    )]
    fn to_f64(self) -> f64 {
        self as f64
    }
}

// ── Float → integer ──────────────────────────────────────────────────
// No TryFrom path in std — raw `as` with bounds checks is the only
// option. These be the last holdouts where `as` cannot be avoided.

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "f64 Safe impl: saturating float→int casts necessarily use raw `as`"
)]
impl Safe for f64 {
    #[inline]
    fn to_u8(self) -> u8 {
        if self < 0.0 {
            0
        } else if self > Self::from(u8::MAX) {
            u8::MAX
        } else {
            self as u8
        }
    }
    #[inline]
    fn to_u16(self) -> u16 {
        if self < 0.0 {
            0
        } else if self > Self::from(u16::MAX) {
            u16::MAX
        } else {
            self as u16
        }
    }
    #[inline]
    fn to_u32(self) -> u32 {
        if self < 0.0 {
            0
        } else if self > Self::from(u32::MAX) {
            u32::MAX
        } else {
            self as u32
        }
    }
    #[inline]
    fn to_u64(self) -> u64 {
        if self < 0.0 { 0 } else { self as u64 }
    }
    #[inline]
    fn to_usize(self) -> usize {
        if self < 0.0 { 0 } else { self as usize }
    }
    #[inline]
    fn to_i32(self) -> i32 {
        self as i32
    }
    #[inline]
    fn to_i64(self) -> i64 {
        self as i64
    }
    #[inline]
    fn to_f32(self) -> f32 {
        self as f32
    }
    #[inline]
    fn to_f64(self) -> f64 {
        self
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "f32 Safe impl: saturating float→int casts necessarily use raw `as`"
)]
impl Safe for f32 {
    #[inline]
    fn to_u8(self) -> u8 {
        if self < 0.0 {
            0
        } else if self > Self::from(u8::MAX) {
            u8::MAX
        } else {
            self as u8
        }
    }
    #[inline]
    fn to_u16(self) -> u16 {
        if self < 0.0 {
            0
        } else if self > Self::from(u16::MAX) {
            u16::MAX
        } else {
            self as u16
        }
    }
    #[inline]
    fn to_u32(self) -> u32 {
        if self < 0.0 { 0 } else { self as u32 }
    }
    #[inline]
    fn to_u64(self) -> u64 {
        if self < 0.0 { 0 } else { self as u64 }
    }
    #[inline]
    fn to_usize(self) -> usize {
        if self < 0.0 { 0 } else { self as usize }
    }
    #[inline]
    fn to_i32(self) -> i32 {
        self as i32
    }
    #[inline]
    fn to_i64(self) -> i64 {
        self as i64
    }
    #[inline]
    fn to_f32(self) -> f32 {
        self
    }
    #[inline]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}
