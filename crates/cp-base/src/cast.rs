#![expect(
    clippy::allow_attributes,
    reason = "macro-generated #[allow] can't use #[expect] — some lint triggers depend on which type the macro expands for"
)]

//! Safe numeric casting helpers.
//!
//! Replace raw `as` casts that trigger `clippy::cast_possible_truncation`
//! and `clippy::cast_sign_loss`. All conversions use saturating semantics —
//! values that don't fit clamp to the target type's MIN/MAX.
//!
//! Usage: `use cp_base::cast::SafeCast;` then `value.to_u16()`, etc.

/// Trait for safe saturating casts between numeric types.
#[expect(
    clippy::module_name_repetitions,
    reason = "Trait is re-exported and used as SafeCast::method() — 'Safe' alone is meaningless"
)]
pub trait SafeCast {
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

/// Implement `SafeCast` for an unsigned integer type using saturating semantics.
macro_rules! impl_safe_cast_unsigned {
    ($t:ty) => {
        #[allow(trivial_numeric_casts, trivial_casts, clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_precision_loss, reason = "macro-generated identity casts (e.g. u32 as u32) are unavoidable — expect() would fail on non-identity expansions")]
        impl SafeCast for $t {
            #[inline]
            fn to_u8(self) -> u8 {
                if self > u8::MAX as $t { u8::MAX } else { self as u8 }
            }
            #[inline]
            fn to_u16(self) -> u16 {
                if self > u16::MAX as $t { u16::MAX } else { self as u16 }
            }
            #[inline]
            fn to_u32(self) -> u32 {
                if self > u32::MAX as $t { u32::MAX } else { self as u32 }
            }
            #[inline]
            fn to_u64(self) -> u64 {
                if self > u64::MAX as $t { u64::MAX } else { self as u64 }
            }
            #[inline]
            fn to_usize(self) -> usize {
                self as usize
            }
            #[inline]
            fn to_i32(self) -> i32 {
                if self > i32::MAX as $t { i32::MAX } else { self as i32 }
            }
            #[inline]
            fn to_i64(self) -> i64 {
                if self > i64::MAX as $t { i64::MAX } else { self as i64 }
            }
            #[inline]
            fn to_f32(self) -> f32 {
                self as f32
            }
            #[inline]
            fn to_f64(self) -> f64 {
                self as f64
            }
        }
    };
}

/// Implement `SafeCast` for a signed integer type using saturating/clamping semantics.
macro_rules! impl_safe_cast_signed {
    ($t:ty) => {
        #[allow(
            trivial_numeric_casts,
            trivial_casts,
            clippy::cast_lossless,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_possible_wrap,
            clippy::cast_precision_loss,
            reason = "macro-generated identity casts are unavoidable — expect() would fail on non-identity expansions"
        )]
        impl SafeCast for $t {
            #[inline]
            fn to_u8(self) -> u8 {
                if self < 0 {
                    0
                } else if self > u8::MAX as $t {
                    u8::MAX
                } else {
                    self as u8
                }
            }
            #[inline]
            fn to_u16(self) -> u16 {
                if self < 0 {
                    0
                } else if self > u16::MAX as $t {
                    u16::MAX
                } else {
                    self as u16
                }
            }
            #[inline]
            fn to_u32(self) -> u32 {
                if self < 0 {
                    0
                } else if self > u32::MAX as $t {
                    u32::MAX
                } else {
                    self as u32
                }
            }
            #[inline]
            fn to_u64(self) -> u64 {
                if self < 0 { 0 } else { self as u64 }
            }
            #[inline]
            fn to_usize(self) -> usize {
                if self < 0 { 0 } else { self as usize }
            }
            #[inline]
            fn to_i32(self) -> i32 {
                if self > i32::MAX as $t {
                    i32::MAX
                } else if self < i32::MIN as $t {
                    i32::MIN
                } else {
                    self as i32
                }
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
                self as f64
            }
        }
    };
}

impl_safe_cast_unsigned!(u16);
impl_safe_cast_unsigned!(u32);
impl_safe_cast_unsigned!(u64);
impl_safe_cast_unsigned!(u128);
impl_safe_cast_unsigned!(usize);

impl_safe_cast_signed!(i32);
impl_safe_cast_signed!(i64);
impl_safe_cast_signed!(isize);

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "SafeCast implementations necessarily use raw as casts"
)]
impl SafeCast for f64 {
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

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "SafeCast implementations necessarily use raw as casts"
)]
impl SafeCast for f32 {
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
