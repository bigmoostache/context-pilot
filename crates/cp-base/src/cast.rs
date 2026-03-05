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

// ── u16: lossless to both f32 and f64 ───────────────────────────────

impl Safe for u16 {
    #[inline]
    fn to_u8(self) -> u8 {
        self.try_into().unwrap_or(u8::MAX)
    }
    #[inline]
    fn to_u16(self) -> u16 {
        self
    }
    #[inline]
    fn to_u32(self) -> u32 {
        u32::from(self)
    }
    #[inline]
    fn to_u64(self) -> u64 {
        u64::from(self)
    }
    #[inline]
    fn to_usize(self) -> usize {
        usize::from(self)
    }
    #[inline]
    fn to_i32(self) -> i32 {
        i32::from(self)
    }
    #[inline]
    fn to_i64(self) -> i64 {
        i64::from(self)
    }
    #[inline]
    fn to_f32(self) -> f32 {
        f32::from(self)
    }
    #[inline]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

/// Lossy int→float: precision loss is inherent.
///
/// Mantissa width (f32: 24 bits, f64: 53 bits) cannot represent all
/// values of wider integer types. This is fundamental, not fixable.
#[expect(
    clippy::cast_precision_loss,
    reason = "lossy int→float: mantissa too narrow — inherent floating-point limitation"
)]
mod lossy_float {
    use super::Safe;

    // ── Integer helpers ──────────────────────────────────────────

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

    // ── Unsigned int→float helpers ───────────────────────────────

    /// Lossless to f64, lossy to f32.
    macro_rules! unsigned_lossless_f64 {
        () => {
            #[inline]
            fn to_f32(self) -> f32 {
                self as f32
            }
            #[inline]
            fn to_f64(self) -> f64 {
                f64::from(self)
            }
        };
    }

    /// Lossy to both f32 and f64.
    macro_rules! unsigned_lossy_both {
        () => {
            #[inline]
            fn to_f32(self) -> f32 {
                self as f32
            }
            #[inline]
            fn to_f64(self) -> f64 {
                self as f64
            }
        };
    }

    // ── Signed int→float helpers ─────────────────────────────────

    /// Lossless to f64, lossy to f32.
    macro_rules! signed_lossless_f64 {
        () => {
            #[inline]
            fn to_f32(self) -> f32 {
                self as f32
            }
            #[inline]
            fn to_f64(self) -> f64 {
                f64::from(self)
            }
        };
    }

    /// Lossy to both f32 and f64.
    macro_rules! signed_lossy_both {
        () => {
            #[inline]
            fn to_f32(self) -> f32 {
                self as f32
            }
            #[inline]
            fn to_f64(self) -> f64 {
                self as f64
            }
        };
    }

    // ── Implementations ──────────────────────────────────────────

    impl Safe for u32 {
        unsigned_int_methods!();
        unsigned_lossless_f64!();
    }

    impl Safe for u64 {
        unsigned_int_methods!();
        unsigned_lossy_both!();
    }

    impl Safe for u128 {
        unsigned_int_methods!();
        unsigned_lossy_both!();
    }

    impl Safe for usize {
        unsigned_int_methods!();
        unsigned_lossy_both!();
    }

    impl Safe for i32 {
        signed_int_methods!();
        signed_lossless_f64!();
    }

    impl Safe for i64 {
        signed_int_methods!();
        signed_lossy_both!();
    }

    impl Safe for isize {
        signed_int_methods!();
        signed_lossy_both!();
    }
}

/// Float→integer: no `TryFrom` in std, raw `as` is the only path.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "saturating float→int: no TryFrom<float> for integers in std"
)]
mod float_to_int {
    use super::Safe;

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
}
