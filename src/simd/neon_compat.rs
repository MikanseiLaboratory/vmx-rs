//! NEON intrinsic compatibility layer shared by the AArch64 and ARMv7 (A32) kernels.
//!
//! The VMX1 kernels were originally written against `core::arch::aarch64`. Most of
//! the NEON intrinsics they use have a 1:1 A32 counterpart, so the same source can
//! be compiled for both architectures. The handful of AArch64-only helpers below are
//! re-implemented in terms of A32 NEON (or a short sequence of A32 intrinsics).
//!
//! Every A32 shim preserves the AArch64 semantics exactly, including the
//! "index out of range yields 0" behaviour of `vqtbl4q_u8`.
//!
//! # Safety
//! All shims require NEON. Callers must have detected NEON at runtime
//! (`is_arm_feature_detected!("neon")`) before reaching them.

#![allow(dead_code)]

#[cfg(target_arch = "aarch64")]
pub use core::arch::aarch64::*;

#[cfg(target_arch = "arm")]
pub use core::arch::arm::*;

#[cfg(target_arch = "arm")]
mod arm32 {
    use core::arch::arm::*;

    /// `vzip1q_s16` — interleave the low halves of `a` and `b`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vzip1q_s16(a: int16x8_t, b: int16x8_t) -> int16x8_t {
        vzipq_s16(a, b).0
    }

    /// `vzip2q_s16` — interleave the high halves of `a` and `b`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vzip2q_s16(a: int16x8_t, b: int16x8_t) -> int16x8_t {
        vzipq_s16(a, b).1
    }

    /// `vzip1q_s32` — interleave the low halves of `a` and `b`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vzip1q_s32(a: int32x4_t, b: int32x4_t) -> int32x4_t {
        vzipq_s32(a, b).0
    }

    /// `vzip1_u8` — interleave the low halves of `a` and `b`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vzip1_u8(a: uint8x8_t, b: uint8x8_t) -> uint8x8_t {
        vzip_u8(a, b).0
    }

    /// `vzip2_u8` — interleave the high halves of `a` and `b`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vzip2_u8(a: uint8x8_t, b: uint8x8_t) -> uint8x8_t {
        vzip_u8(a, b).1
    }

    /// `vzip1_u16` — interleave the low halves of `a` and `b`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vzip1_u16(a: uint16x4_t, b: uint16x4_t) -> uint16x4_t {
        vzip_u16(a, b).0
    }

    /// `vzip2_u16` — interleave the high halves of `a` and `b`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vzip2_u16(a: uint16x4_t, b: uint16x4_t) -> uint16x4_t {
        vzip_u16(a, b).1
    }

    /// `vuzp1q_s16` — even elements of `a` followed by even elements of `b`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vuzp1q_s16(a: int16x8_t, b: int16x8_t) -> int16x8_t {
        vuzpq_s16(a, b).0
    }

    /// `vuzp2q_s16` — odd elements of `a` followed by odd elements of `b`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vuzp2q_s16(a: int16x8_t, b: int16x8_t) -> int16x8_t {
        vuzpq_s16(a, b).1
    }

    /// `vpaddq_s32` — pairwise add of `a` then pairwise add of `b`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vpaddq_s32(a: int32x4_t, b: int32x4_t) -> int32x4_t {
        vcombine_s32(
            vpadd_s32(vget_low_s32(a), vget_high_s32(a)),
            vpadd_s32(vget_low_s32(b), vget_high_s32(b)),
        )
    }

    /// `vaddv_u8` — horizontal add of all eight lanes.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vaddv_u8(a: uint8x8_t) -> u8 {
        let s1 = vpadd_u8(a, a);
        let s2 = vpadd_u8(s1, s1);
        let s3 = vpadd_u8(s2, s2);
        vget_lane_u8(s3, 0)
    }

    /// `vceqzq_s16` — compare against zero.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vceqzq_s16(a: int16x8_t) -> uint16x8_t {
        vceqq_s16(a, vdupq_n_s16(0))
    }

    /// `vcltzq_s16` — compare against zero, true when `a` is negative.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vcltzq_s16(a: int16x8_t) -> uint16x8_t {
        vcltq_s16(a, vdupq_n_s16(0))
    }

    /// `vqtbl4q_u8` — 64-byte table lookup (`a.0..a.3` are consecutive 16-byte tables).
    ///
    /// A32 `vtbl4_u8` covers 32 bytes, so the table is split in half and the two
    /// lookups are blended. Out-of-range indices (>= 64) yield 0, matching AArch64.
    #[inline]
    #[target_feature(enable = "neon")]
    pub unsafe fn vqtbl4q_u8(a: uint8x16x4_t, idx: uint8x16_t) -> uint8x16_t {
        // Table bytes 0..31: [lo(a.0), hi(a.0), lo(a.1), hi(a.1)]
        let table_lo = uint8x8x4_t(
            vget_low_u8(a.0),
            vget_high_u8(a.0),
            vget_low_u8(a.1),
            vget_high_u8(a.1),
        );
        // Table bytes 32..63: [lo(a.2), hi(a.2), lo(a.3), hi(a.3)]
        let table_hi = uint8x8x4_t(
            vget_low_u8(a.2),
            vget_high_u8(a.2),
            vget_low_u8(a.3),
            vget_high_u8(a.3),
        );
        // `vtbl4_u8` yields 0 for indices >= 32, so:
        //   idx < 32  -> table_lo hit, table_hi yields 0
        //   idx >= 32 -> table_lo yields 0, table_hi hit after subtracting 32
        //   idx >= 64 -> both 0 (matches AArch64)
        let idx_shift = vsubq_u8(idx, vdupq_n_u8(32));
        let lo_lo = vtbl4_u8(table_lo, vget_low_u8(idx));
        let lo_hi = vtbl4_u8(table_hi, vget_low_u8(idx_shift));
        let hi_lo = vtbl4_u8(table_lo, vget_high_u8(idx));
        let hi_hi = vtbl4_u8(table_hi, vget_high_u8(idx_shift));

        let sel = vcgeq_u8(idx, vdupq_n_u8(32));
        let lo = vbsl_u8(vget_low_u8(sel), lo_hi, lo_lo);
        let hi = vbsl_u8(vget_high_u8(sel), hi_hi, hi_lo);
        vcombine_u8(lo, hi)
    }
}

#[cfg(target_arch = "arm")]
pub use arm32::*;
